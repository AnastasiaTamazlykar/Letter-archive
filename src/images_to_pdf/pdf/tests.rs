//! Tests for writing the PDF. The pages are built by hand, so no photo and no Vision
//! are needed: these tests are fast and always give the same result.

use std::path::PathBuf;

use objc2::AnyThread;
use objc2_foundation::{NSString, NSURL};
use objc2_pdf_kit::PDFDocument;

// `super::*` brings in everything `pdf.rs` can see, private items included.
use super::*;
use crate::document_crud::extract_text;

// ── Helpers ──

/// A black line of text in the body of the page (below the letterhead).
/// `width` and `height` are fractions of the page, like Vision gives them.
fn body_line(text: &str, width: f32, height: f32) -> Line {
    Line {
        text: text.to_string(),
        x: 0.1,
        y: 0.5,
        width,
        height,
        colour: [0, 0, 0],
    }
}

/// A page with one body line on a small white background.
fn page_with_text(text: &str) -> Page {
    let paper = RgbImage::from_pixel(21, 30, image::Rgb([255, 255, 255]));
    Page {
        paper,
        lines: vec![body_line(text, 0.8, 0.02)],
    }
}

/// A PDF path in the system's temp folder, so `data/` stays untouched.
fn temp_pdf(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("paper_archive_test_{name}.pdf"))
}

/// How many pages a PDF has, read with Apple's PDFKit.
fn page_count(pdf: &Path) -> usize {
    let url = NSURL::fileURLWithPath(&NSString::from_str(pdf.to_str().unwrap()));
    let document = unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) }.unwrap();
    unsafe { document.pageCount() }
}

/// The font file's bytes. A `Face` (the font, ready to measure) only borrows them,
/// so each test keeps the bytes in a variable that lives as long as the `Face`.
fn font_bytes() -> Vec<u8> {
    std::fs::read(FONT).unwrap()
}

// ── Pages ──

/// Each page becomes one PDF page, in the order given.
#[test]
fn test_write_pdf_keeps_page_order() {
    let texts = ["page one", "page two", "page three"];
    let pdf = temp_pdf("page_order");

    write_pdf(&texts.map(page_with_text), &pdf).unwrap();

    assert_eq!(page_count(&pdf), 3);
    let text = extract_text(&pdf).unwrap();
    let positions: Vec<usize> = texts
        .iter()
        .map(|t| {
            text.find(t)
                .unwrap_or_else(|| panic!("{t:?} missing in: {text}"))
        })
        .collect();
    assert!(positions.is_sorted(), "pages out of order in: {text}");
}

// ── Font size ──

/// The font size comes from the height of the line's box: twice as tall, twice as big.
#[test]
fn test_font_size_follows_line_height() {
    let bytes = font_bytes();
    let face = Face::parse(&bytes, 0).unwrap();

    let short = font_size(&face, &body_line("Hallo", 0.8, 0.01));
    let tall = font_size(&face, &body_line("Hallo", 0.8, 0.02));

    assert!(
        (tall / short - 2.0).abs() < 0.001,
        "short {short}, tall {tall}"
    );
}

/// Text too long for its box gets a smaller font: just small enough to fill the box.
#[test]
fn test_font_size_shrinks_long_text_to_fit() {
    let bytes = font_bytes();
    let face = Face::parse(&bytes, 0).unwrap();
    let text = "Sehr geehrte Damen und Herren, vielen Dank für Ihr Schreiben";
    let narrow = body_line(text, 0.1, 0.02);
    let wide = body_line(text, 0.9, 0.02);

    let size = font_size(&face, &narrow);

    assert!(
        size < font_size(&face, &wide),
        "the narrow box needs smaller text"
    );
    let box_width_pt = mm_to_pt(narrow.width * PAGE_WIDTH_MM);
    let text_width = text_width_pt(&face, text, size);
    assert!(
        (text_width - box_width_pt).abs() < 0.01,
        "{text_width} pt in a {box_width_pt} pt box"
    );
}
