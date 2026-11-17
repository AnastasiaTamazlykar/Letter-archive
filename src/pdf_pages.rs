//! Draws the pages of a PDF into pixels with Apple's PDFKit, to show them in the window.

use std::{error::Error, path::Path};

use image::RgbaImage;
use objc2::AnyThread;
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo};
use objc2_foundation::{NSString, NSURL};
use objc2_pdf_kit::{PDFDisplayBox, PDFDocument, PDFPage};

/// Draws every page of the PDF, each `width` pixels wide; the height follows the page.
pub fn render_pages(pdf: &Path, width: u32) -> Result<Vec<RgbaImage>, Box<dyn Error>> {
    let document = open_pdf(pdf)?;
    let count = unsafe { document.pageCount() };
    // `collect` into a `Result`: all pages if every one worked, or the first error.
    (0..count)
        .map(|index| {
            let page = unsafe { document.pageAtIndex(index) }.ok_or("missing page")?;
            render_page(&page, width)
        })
        .collect()
}

/// Opens the PDF with PDFKit; fails if it is not a readable PDF.
fn open_pdf(pdf: &Path) -> Result<objc2::rc::Retained<PDFDocument>, Box<dyn Error>> {
    let path = pdf.to_str().ok_or("path is not UTF-8")?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    // `unsafe`: Rust can't check Apple's code, so the bindings ask us to confirm the call.
    let document = unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) };
    Ok(document.ok_or("cannot open PDF")?)
}

/// Draws one page on white, `width` pixels wide, keeping the page's proportions.
fn render_page(page: &PDFPage, width: u32) -> Result<RgbaImage, Box<dyn Error>> {
    // The page's size in PDF points (1/72 inch).
    let bounds = unsafe { page.boundsForBox(PDFDisplayBox::MediaBox) };
    if bounds.size.width <= 0.0 {
        return Err("page has no width".into());
    }
    let scale = width as f64 / bounds.size.width;
    let height = ((bounds.size.height * scale).round() as u32).max(1);

    // 4 bytes per pixel (red, green, blue, alpha), all 255: the page starts out white.
    // With alpha always 255, Apple's "premultiplied" pixels equal the plain ones egui reads.
    let mut pixels = vec![255u8; (width * height * 4) as usize];
    let color_space = CGColorSpace::new_device_rgb().ok_or("no RGB colour space")?;
    // A drawing context that paints straight into `pixels`. It gets a raw pointer, which
    // Rust does not track, so we must make sure `pixels` outlives the context (see below).
    let context = unsafe {
        CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            width as usize,
            height as usize,
            8,                    // bits per colour
            (width * 4) as usize, // bytes per row
            Some(&color_space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }
    .ok_or("cannot make a drawing context")?;

    // Scale PDF points to pixels, then let PDFKit draw the page.
    CGContext::scale_ctm(Some(&context), scale, scale);
    unsafe { page.drawWithBox_toContext(PDFDisplayBox::MediaBox, &context) };
    // The context must be gone before `pixels` moves into the picture below.
    drop(context);

    Ok(RgbaImage::from_raw(width, height, pixels).ok_or("wrong number of pixels")?)
}
