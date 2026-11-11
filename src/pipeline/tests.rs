//! Tests for the photo side of the pipeline, in its order: load, flatten, recognize,
//! build a page, and the whole run. Writing the PDF is tested in `pdf/tests.rs`.
//!
//! Tests that run Apple Vision take seconds, so only `test_make_page_gives_clean_a4_page`
//! runs every time; the others are `#[ignore]`d. Run them with `cargo test -- --ignored`.

// `super::*` brings in everything `pipeline.rs` can see, private items included.
use super::*;
use crate::pdf_crud::extract_text;

/// The two test photos in `data/`, and a heading Vision reads on each one.
const LETTER_A: &str = "IMG_9549.HEIC";
const TITLE_A: &str = "BEITRAGSSERVICE";
const LETTER_B: &str = "IMG_7280.HEIC";
const TITLE_B: &str = "Haushaltsscheck-Verfahren";

/// Vision's boxes may stick out of the page by a rounding error; this much is allowed.
const ROUNDING: f32 = 0.001;

// ── Helpers ──

/// The path of a test photo.
fn photo(name: &str) -> PathBuf {
    Path::new("data").join(name)
}

/// Width and height of a Core Image picture, in pixels.
fn size_of(image: &CIImage) -> (f64, f64) {
    let size = unsafe { image.extent() }.size;
    (size.width, size.height)
}

/// A file in the system's temp folder, so `data/` stays untouched.
fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("paper_archive_test_{name}"))
}

// ── Step 0: loading ──

/// Phone photos are stored on their side, with a tag saying how to turn them.
/// `open_image` reads them as stored; `load_image` turns them upright.
#[test]
fn test_load_image_turns_photo_upright() {
    let (stored_width, stored_height) = size_of(&open_image(&photo(LETTER_A)).unwrap());
    let upright = size_of(&load_image(&photo(LETTER_A)).unwrap());

    assert!(stored_width > stored_height, "stored on its side");
    assert_eq!(upright, (stored_height, stored_width));
}

// ── Thumbnails ──

/// Writes a thumbnail to look at: data/IMG_9549.thumbnail.png. It is upright,
/// so the longer side is the height.
#[test]
fn test_thumbnail_writes_png() {
    let picture = thumbnail(&photo(LETTER_A), 200).unwrap();

    assert_eq!(picture.height(), 200);
    assert!(picture.width() < picture.height());
    picture.save("data/IMG_9549.thumbnail.png").unwrap();
}

/// A big picture shrinks to fit; a small one keeps its size.
#[test]
fn test_fit_within_scales_down_only() {
    assert_eq!(fit_within(4000.0, 3000.0, 200), (200, 150));
    assert_eq!(fit_within(100.0, 50.0, 200), (100, 50));
}

// ── Step 1: flattening ──

/// The letter is cut out of the photo and straightened: smaller than the photo,
/// and shaped like an A4 sheet (height ≈ 1.41 × width).
#[test]
#[ignore = "runs Vision; run with --ignored"]
fn test_flatten_page_crops_to_the_paper() {
    let picture = load_image(&photo(LETTER_A)).unwrap();
    let (photo_width, photo_height) = size_of(&picture);

    let (page_width, page_height) = size_of(&flatten_page(picture).unwrap());

    assert!(page_width < photo_width && page_height < photo_height);
    let shape = page_height / page_width;
    assert!(
        (shape - 2f64.sqrt()).abs() < 0.1,
        "height / width = {shape}"
    );
}

/// With no sheet of paper in sight (a plain grey picture), the picture comes back as it was.
#[test]
#[ignore = "runs Vision; run with --ignored"]
fn test_flatten_page_without_paper_keeps_picture() {
    let file = temp_file("grey.png");
    RgbImage::from_pixel(600, 800, image::Rgb([128, 128, 128]))
        .save(&file)
        .unwrap();
    let grey = open_image(&file).unwrap();

    assert_eq!(size_of(&flatten_page(grey).unwrap()), (600.0, 800.0));
}

// ── Step 2: recognizing text ──

/// Our three-letter codes become the names Vision expects. An unknown code gives
/// no names, and then Vision guesses the language itself.
#[test]
fn test_vision_languages_maps_codes() {
    let names = |lang: &str| -> Vec<String> {
        vision_languages(lang)
            .iter()
            .map(|name| name.to_string())
            .collect()
    };

    assert_eq!(names("deu"), ["de-DE"]);
    assert_eq!(names("deu+eng"), ["de-DE", "en-US"]);
    assert!(names("xyz").is_empty());
}

/// Vision finds the letter's heading, and every line's box lies inside the page.
#[test]
#[ignore = "runs Vision; run with --ignored"]
fn test_recognize_lines_finds_the_title() {
    let page = flatten_page(load_image(&photo(LETTER_A)).unwrap()).unwrap();

    let lines = recognize_lines(&page, "deu").unwrap();

    assert!(
        lines.iter().any(|line| line.text.contains(TITLE_A)),
        "{TITLE_A} not found"
    );
    for line in &lines {
        let inside = line.x >= -ROUNDING
            && line.y >= -ROUNDING
            && line.x + line.width <= 1.0 + ROUNDING
            && line.y + line.height <= 1.0 + ROUNDING;
        assert!(inside, "box outside the page: {}", line.text);
    }
}

/// Only lines at the very top of the page count as letterhead.
#[test]
fn test_is_letterhead_only_at_top() {
    let line_at = |y| Line {
        text: String::new(),
        x: 0.1,
        y,
        width: 0.5,
        height: 0.02,
        colour: [0, 0, 0],
    };

    assert!(is_letterhead(&line_at(0.95)));
    assert!(!is_letterhead(&line_at(0.5)));
}

// ── One page ──

/// The one Vision test that runs every time: a photo goes through all steps.
/// The background is A4 at 150 DPI, the page has lines, and after cleaning most of
/// the paper is pure white.
#[test]
fn test_make_page_gives_clean_a4_page() {
    let page = make_page(&photo(LETTER_A), "deu").unwrap();

    assert_eq!(page.paper.dimensions(), (1240, 1754));
    assert!(!page.lines.is_empty());
    let white = page
        .paper
        .pixels()
        .filter(|pixel| pixel.0 == [255, 255, 255])
        .count();
    let share = white as f64 / (page.paper.width() * page.paper.height()) as f64;
    assert!(share > 0.8, "only {share:.2} of the paper is white");
}

/// Template: saves the cleaned background of a photo to look at, as
/// data/IMG_9549.background.png. Run with `cargo test make_page_background -- --ignored`.
#[test]
#[ignore = "template; writes into data/"]
fn test_make_page_background_template() {
    let page = make_page(&photo(LETTER_A), "deu").unwrap();
    page.paper.save("data/IMG_9549.background.png").unwrap();
}

// ── The whole pipeline ──

/// Two photos become one PDF, with the pages in the order of the photos.
#[test]
#[ignore = "runs Vision; run with --ignored"]
fn test_images_to_pdf_puts_photos_in_order() {
    let pdf = temp_file("two_letters.pdf");

    images_to_pdf(&[photo(LETTER_A), photo(LETTER_B)], &pdf, "deu").unwrap();

    let text = extract_text(&pdf).unwrap();
    let a = text.find(TITLE_A).expect("first letter missing");
    let b = text.find(TITLE_B).expect("second letter missing");
    assert!(a < b, "the first photo should be the first page");
}

#[test]
fn test_images_to_pdf_needs_an_image() {
    assert!(images_to_pdf(&[], Path::new("unused.pdf"), "deu").is_err());
}

/// When a photo can't be read, the error says which one.
#[test]
fn test_images_to_pdf_names_the_failing_image() {
    let missing = PathBuf::from("/no/such/photo.HEIC");

    let error = images_to_pdf(&[missing], &temp_file("never.pdf"), "deu").unwrap_err();

    assert!(
        error.to_string().contains("/no/such/photo.HEIC"),
        "got: {error}"
    );
}

/// Template: fill in the paths and languages, then run with `cargo test -- --ignored`.
/// The pages come out in the order of `inputs`.
#[test]
#[ignore = "template; writes into data/"]
fn test_images_to_pdf_template() {
    let inputs = [photo(LETTER_A), photo(LETTER_B)];
    let output = Path::new("data/letters.pdf");
    images_to_pdf(&inputs, output, "deu").unwrap();
    assert!(output.exists());
}
