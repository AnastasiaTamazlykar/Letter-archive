//! The PDF shown right of the Archive tab's table: its pages, a Close button, and the
//! divider dragged to share the width between the table and the PDF.
//! A child module of `archive_tab`. `pub(super)` makes an item visible to `archive_tab`;
//! `pub(in crate::app)` to the whole `app` module, for what `app.rs` itself needs.

use std::path::Path;

use eframe::egui;

use crate::app::{App, file_name};
use crate::pdf_pages::render_pages;

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
        self.archive.pdf_view = Some(PdfView { name, pages });
    }
}

/// The shown PDF: its name and a Close button, then its pages one under the other,
/// scrollable and as wide as the area. Returns whether Close was clicked.
fn pdf_pages_view(ui: &mut egui::Ui, view: &PdfView) -> bool {
    let close = ui
        .horizontal(|ui| {
            ui.strong(&view.name);
            ui.button("Close").clicked()
        })
        .inner;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for page in &view.pages {
            // `max_width` shrinks the page to the area's width, keeping its proportions.
            ui.add(egui::Image::new(page).max_width(ui.available_width()));
        }
    });
    close
}

/// The area right of the table: the clicked PDF's pages, or a hint if none is shown.
/// Returns whether Close was clicked.
pub(super) fn pdf_view_area(ui: &mut egui::Ui, view: Option<&PdfView>) -> bool {
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
