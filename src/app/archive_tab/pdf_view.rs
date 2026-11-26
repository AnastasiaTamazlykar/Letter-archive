//! The PDF shown right of the Archive tab's table: its pages, a Close button, and the
//! divider dragged to share the width between the table and the PDF.
//! A child module of `archive_tab`. `pub(super)` makes an item visible to `archive_tab`;
//! `pub(in crate::app)` to the whole `app` module, for what `app.rs` itself needs.

use objc2::AnyThread;
use std::error::Error;
use std::path::Path;

use eframe::egui;
use image::RgbaImage;
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo};
use objc2_foundation::{NSString, NSURL};
use objc2_pdf_kit::{PDFDisplayBox, PDFDocument, PDFPage};
use crate::app::{App, file_name};

/// Width of a PDF page drawn for the area right of the table, in pixels.
const PDF_VIEW_PIXELS: u32 = 1200;
/// Share of the Archive tab's width the PDF area right of the table takes at the start.
pub(in crate::app) const PDF_VIEW_SHARE: f32 = 0.5;
/// How small and how large the PDF area's share can be dragged.
pub(super) const PDF_VIEW_SHARE_MIN: f32 = 0.2;
pub(super) const PDF_VIEW_SHARE_MAX: f32 = 0.8;

/// The PDF shown in the area right of the table: its name and its pages as textures.
pub(super) struct PdfView {
    pub(super) name: String,
    pages: Vec<egui::TextureHandle>,
    /// The letter's text, as stored in the database; shown instead of the pages on "Text".
    text: String,
    /// True: show `text`, which can be selected and copied. False: show the pages.
    show_text: bool,
}

// The PDF view's part of `App`.
impl App {
    /// Draws the PDF's pages and keeps them, to show right of the table.
    /// Prints the error and keeps the PDF shown so far if this one can't be read.
    pub(super) fn show_pdf(&mut self, ctx: &egui::Context, path: &Path) {
        let pages = match render_pages(path, PDF_VIEW_PIXELS) {
            Ok(pages) => pages,
            Err(error) => {
                eprintln!("cannot show {}: {error}", path.display());
                return;
            }
        };
        let name = file_name(path);
        let pages = pages
            .iter()
            .map(|page| {
                let size = [page.width() as usize, page.height() as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, page.as_raw());
                ctx.load_texture(&name, image, egui::TextureOptions::LINEAR)
            })
            .collect();
        // The text read when the letter was filed; empty if it is not in the archive.
        let text = self.stored_letter(path).map(|letter| letter.text).unwrap_or_default();
        self.archive.pdf_view = Some(PdfView { name, pages, text, show_text: false });
    }
}

/// The shown PDF: its name and a Close button, then its pages one under the other,
/// scrollable and as wide as the area. Returns whether Close was clicked.
fn pdf_pages_view(ui: &mut egui::Ui, view: &mut PdfView) -> bool {
    let close = ui
        .horizontal(|ui| {
            ui.strong(&view.name);
            // `selectable_value` sets `show_text` to the given value when clicked, like the tabs.
            ui.selectable_value(&mut view.show_text, false, "Pages");
            ui.selectable_value(&mut view.show_text, true, "Text");
            ui.button("Close").clicked()
        })
        .inner;
    egui::ScrollArea::vertical().show(ui, |ui| {
        if view.show_text {
            // `selectable`: drag over the text to select it, ⌘C to copy. `wrap`: the strip
            // around this area clips its cells, which makes text one truncated row by
            // default; `wrap` asks for every line, wrapped to the area's width.
            ui.add(egui::Label::new(&view.text).selectable(true).wrap());
        } else {
            for page in &view.pages {
                // `max_width` shrinks the page to the area's width, keeping its proportions.
                ui.add(egui::Image::new(page).max_width(ui.available_width()));
            }
        }
    });
    close
}

/// The area right of the table: the clicked PDF's pages, or a hint if none is shown.
/// Returns whether Close was clicked. `&mut`: the Pages/Text switch changes the view.
pub(super) fn pdf_view_area(ui: &mut egui::Ui, view: Option<&mut PdfView>) -> bool {
    match view {
        Some(view) => pdf_pages_view(ui, view),
        None => {
            ui.weak("Click a file name to see the letter here");
            false
        }
    }
}

/// The line between the table and the PDF area. Returns how far it was dragged in this
/// frame, in points (right is positive); 0 when it was not dragged.
pub(super) fn divider(ui: &mut egui::Ui) -> f32 {
    // The whole cell reacts to dragging, not only the thin line drawn in its middle.
    let response = ui
        .allocate_response(ui.available_size(), egui::Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    let rect = response.rect;
    // Brighter while the mouse is on it or dragging, like other egui widgets.
    let stroke = ui.style().interact(&response).fg_stroke;
    ui.painter().vline(rect.center().x, rect.y_range(), stroke);
    response.drag_delta().x
}

fn render_pages(pdf: &Path, width: u32) -> Result<Vec<RgbaImage>, Box<dyn Error>> {
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
