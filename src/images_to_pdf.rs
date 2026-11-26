//! Photo of a letter → clean A4 PDF: find and flatten the page, recognize the text,
//! rebuild the background, and write every line at its original position.

use std::{
    error::Error,
    path::{Path, PathBuf},
};

use image::{RgbImage, RgbaImage};

use objc2::{AnyThread, rc::Retained, runtime::AnyObject};
use objc2_core_image::{CIImage, CIVector};
use objc2_foundation::{NSArray, NSDictionary, NSString, NSURL};
use objc2_vision::{
    VNDetectDocumentSegmentationRequest, VNImageRequestHandler, VNRecognizeTextRequest,
    VNRecognizedTextObservation, VNRectangleObservation, VNRequest, VNRequestTextRecognitionLevel,
};

use crate::language;

// Child modules of `pipeline`: Rust looks for them in `src/pipeline/`.
// Children may use their parent's private items through `super::`.
mod clean_paper;
mod orientation;
mod pdf;
mod render;

// `cfg(test)`: compiled only for `cargo test`. The tests live in `src/pipeline/tests.rs`.
#[cfg(test)]
mod tests;

/// Font used for the text in the PDFs; covers German (ä, ö, ü, ß).
/// `pub(crate)`: the archive's tests write their test PDFs with it too.
pub(crate) const FONT: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";
/// A4 page size in millimetres.
const PAGE_WIDTH_MM: f32 = 210.0;
const PAGE_HEIGHT_MM: f32 = 297.0;
/// Top part of the page (as a share of its height) holding the letterhead with logos.
/// Text there is kept as in the photo, not erased and redrawn.
const LETTERHEAD_SHARE: f32 = 0.12;

/// One recognized line of text. Position and size are fractions of the page (0.0 to 1.0),
/// measured from the bottom-left corner, the same way a PDF measures.
struct Line {
    text: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    /// Text colour as red, green, blue (0 to 255).
    colour: [u8; 3],
}

/// True if the line starts in the letterhead at the top of the page (logos live there).
fn is_letterhead(line: &Line) -> bool {
    line.y + line.height > 1.0 - LETTERHEAD_SHARE
}

/// One finished page: the cleaned background picture and the text lines to draw on it.
struct Page {
    paper: RgbImage,
    lines: Vec<Line>,
}

/// Turns photos of letter pages into one clean PDF: one page per photo, in the order
/// given. Each page keeps its letter's layout: every line is placed where it was.
/// `&[PathBuf]` is a slice: a borrowed view of the caller's list, so nothing is copied.
/// `pub` so the app can call it from outside this module.
pub fn images_to_pdf(images: &[PathBuf], pdf: &Path, lang: &str) -> Result<(), Box<dyn Error>> {
    if images.is_empty() {
        return Err("no images to convert".into());
    }
    let mut pages = Vec::new();
    for image in images {
        // Name the photo in the error, so the user knows which one failed.
        let page = make_page(image, lang).map_err(|e| format!("{}: {e}", image.display()))?;
        pages.push(page);
    }
    pdf::write_pdf(&pages, pdf)
}

/// Steps 0 to 2 and the background for one photo: a page ready to be written.
/// To look at the background picture, run the test `test_make_page_background_template`.
fn make_page(image: &Path, lang: &str) -> Result<Page, Box<dyn Error>> {
    let photo = load_image(image)?;
    let flat = flatten_page(photo)?;
    let mut lines = recognize_lines(&flat, lang)?;

    let mut paper = render::render_page(&flat)?;
    clean_paper::whiten_paper(&mut paper);
    clean_paper::set_text_colours(&paper, &mut lines); // before the text is erased
    clean_paper::erase_text(&mut paper, &lines);

    Ok(Page { paper, lines })
}

/// Step 0: loads an image file (HEIC, JPEG, PNG) and turns it upright if its tag says so.
fn load_image(path: &Path) -> Result<Retained<CIImage>, Box<dyn Error>> {
    let image = open_image(path)?;
    Ok(orientation::make_upright(image))
}

/// A small upright preview of an image file, at most `max_side` pixels wide and high.
/// Keeps the photo's proportions and never enlarges it.
pub fn thumbnail(path: &Path, max_side: u32) -> Result<RgbaImage, Box<dyn Error>> {
    let image = load_image(path)?;
    let size = unsafe { image.extent() }.size;
    let (width, height) = fit_within(size.width, size.height, max_side);
    render::render_rgba(&image, width, height)
}

/// Scales a size down so its longer side is at most `max_side`; never below 1 pixel.
fn fit_within(width: f64, height: f64, max_side: u32) -> (u32, u32) {
    let scale = (max_side as f64 / width.max(height)).min(1.0);
    let to_pixels = |side: f64| ((side * scale).round() as u32).max(1);
    (to_pixels(width), to_pixels(height))
}

/// Reads an image file exactly as it is stored, without turning it.
fn open_image(path: &Path) -> Result<Retained<CIImage>, Box<dyn Error>> {
    let path = path.to_str().ok_or("path is not UTF-8")?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    // `unsafe`: Rust can't check Apple's code, so the bindings ask us to confirm the call.
    let image = unsafe { CIImage::imageWithContentsOfURL(&url) };
    Ok(image.ok_or("cannot read image")?)
}

/// Step 1: finds the sheet of paper in the photo and straightens it into a flat rectangle.
/// If no paper is found, the photo is returned unchanged.
fn flatten_page(photo: Retained<CIImage>) -> Result<Retained<CIImage>, Box<dyn Error>> {
    let Some(corners) = find_page_corners(&photo)? else {
        return Ok(photo);
    };

    // Vision gives corners as fractions of the photo; Core Image wants pixels.
    // `extent` is where the picture lies: its start point (origin) and its size.
    let extent = unsafe { photo.extent() };
    let (start, size) = (extent.origin, extent.size);
    let to_pixels = |p: objc2_core_foundation::CGPoint| unsafe {
        CIVector::vectorWithX_Y(start.x + p.x * size.width, start.y + p.y * size.height)
    };
    let corners = unsafe {
        [
            to_pixels(corners.topLeft()),
            to_pixels(corners.topRight()),
            to_pixels(corners.bottomLeft()),
            to_pixels(corners.bottomRight()),
        ]
    };

    // Settings for Core Image's built-in "perspective correction" filter.
    let names = [
        "inputTopLeft",
        "inputTopRight",
        "inputBottomLeft",
        "inputBottomRight",
    ]
    .map(NSString::from_str);
    let keys: Vec<&NSString> = names.iter().map(|k| &**k).collect();
    let values: Vec<&AnyObject> = corners.iter().map(|v| -> &AnyObject { v }).collect();
    let settings = NSDictionary::from_slices(&keys, &values);

    let filter = NSString::from_str("CIPerspectiveCorrection");
    Ok(unsafe { photo.imageByApplyingFilter_withInputParameters(&filter, &settings) })
}

/// Asks Vision where the four corners of the document are, if it sees one.
fn find_page_corners(
    image: &CIImage,
) -> Result<Option<Retained<VNRectangleObservation>>, Box<dyn Error>> {
    let request = unsafe { VNDetectDocumentSegmentationRequest::new() };
    run_vision(&request, image)?;
    Ok(unsafe { request.results() }.and_then(|found| found.firstObject()))
}

/// Step 2: recognizes every line of text together with its position on the page.
/// `lang` uses three-letter codes ("deu", "eng", "deu+eng"); with no known code,
/// Vision detects the language itself.
fn recognize_lines(page: &CIImage, lang: &str) -> Result<Vec<Line>, Box<dyn Error>> {
    let request = text_request(lang);
    run_vision(&request, page)?;
    let found = request.results().unwrap_or_default();
    // `filter_map` keeps only the lines `to_line` could turn into a `Line`.
    Ok(found.iter().filter_map(|line| to_line(&line)).collect())
}

/// A Vision request for accurate text recognition in the given languages.
fn text_request(lang: &str) -> Retained<VNRecognizeTextRequest> {
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(true);
    let languages = vision_languages(lang);
    if languages.is_empty() {
        request.setAutomaticallyDetectsLanguage(true);
    } else {
        request.setRecognitionLanguages(&NSArray::from_retained_slice(&languages));
    }
    request
}

/// Turns one line Vision found into our `Line`, using Vision's best guess for its text.
/// `None` if Vision has no guess at all.
fn to_line(found: &VNRecognizedTextObservation) -> Option<Line> {
    // Vision may offer several guesses per line; take the best one.
    let best = found.topCandidates(1).firstObject()?;
    let b = unsafe { found.boundingBox() };
    Some(Line {
        text: best.string().to_string(),
        x: b.origin.x as f32,
        y: b.origin.y as f32,
        width: b.size.width as f32,
        height: b.size.height as f32,
        colour: [0, 0, 0], // black until the real colour is measured
    })
}

/// Turns codes like "deu+eng" into the names Vision expects, like "de-DE" and "en-US".
/// Unknown codes are skipped.
fn vision_languages(lang: &str) -> Vec<Retained<NSString>> {
    lang.split('+')
        .filter_map(language::by_code)
        .map(|language| NSString::from_str(language.vision))
        .collect()
}

/// Runs one Vision request on an image. The results end up inside `request`.
fn run_vision(request: &VNRequest, image: &CIImage) -> Result<(), Box<dyn Error>> {
    let handler = unsafe {
        VNImageRequestHandler::initWithCIImage_options(
            VNImageRequestHandler::alloc(),
            image,
            &NSDictionary::new(),
        )
    };
    handler
        .performRequests_error(&NSArray::from_slice(&[request]))
        .map_err(|e| e.localizedDescription().to_string())?;
    Ok(())
}
