//! The Archive tab's table: one row per letter with its file name, category, letter date
//! and summary, the header menus that sort and filter it, and the right-click menus.
//! A child module of `archive_tab`, so it may use its private items (`Renaming`,
//! `rename_cell`, the `sort_filter` menus). `pub(super)` marks what `archive_tab` uses.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui_extras::{Column, TableBuilder, TableRow};

use super::sort_filter::{category_menu, date_order_menu};
use super::{EditKind, Renaming, rename_cell};
use crate::app::file_name;
use crate::pdf_crud::{DateOrder, Document, SearchHit};

/// Height of one row of the PDF table, in points.
const PDF_ROW_HEIGHT: f32 = 60.0;

/// What was clicked on a file name; each holds the PDF's full path.
pub(super) enum PdfClick {
    /// A left click: show it in the area right of the table.
    Show(PathBuf),
    /// "Open" in the right-click menu: open it in the PDF app.
    Open(PathBuf),
    /// "Show in Finder" in the right-click menu.
    Reveal(PathBuf),
    /// "Rename" in the right-click menu: the file name turns into a text box.
    Rename(PathBuf),
    /// "Rename" in the right-click menu of the category: the category turns into a text box.
    RenameCategory(PathBuf),
    /// "Edit" in the right-click menu of the letter date: the date turns into a text box.
    EditDate(PathBuf),
    /// "Edit" in the right-click menu of the summary: the summary turns into a text box.
    EditSummary(PathBuf),
    /// Enter in that text box: rename the file, or move it to the typed category.
    RenameSubmit,
    /// Esc in that text box, or a click somewhere else: keep the old name.
    RenameCancel,
}

/// Draws the table of PDFs and returns what was clicked on a file name, if anything.
/// The Category and Letter date headers hold menus that change `category` and `order`.
pub(super) fn display_pdfs(
    ui: &mut egui::Ui,
    rows: &[PdfRow],
    order: &mut DateOrder,
    category: &mut Option<String>,
    categories: &[String],
    renaming: &mut Option<Renaming>,
) -> Option<PdfClick> {
    // The row closures below assign to `click`; they borrow it mutably one after another.
    let mut click = None;
    TableBuilder::new(ui)
        .striped(true)
        .column(Column::initial(120.0).resizable(true).clip(true))
        .column(Column::initial(120.0).resizable(true).clip(true))
        // Letter date: wide enough for its menu's "Newest first".
        .column(Column::initial(120.0).resizable(true).clip(true))
        // The last column takes the rest of the width, so no empty column is left after it.
        .column(Column::remainder().clip(true))
        .header(PDF_ROW_HEIGHT, |mut header| {
            // Each `col` closure draws one cell; `;` makes it return `()`, as `col` wants.
            header.col(|ui| {
                ui.strong("File");
            });
            // `vertical`: the menu goes under the header's name, not beside it.
            header.col(|ui| {
                ui.vertical(|ui| {
                    ui.strong("Category");
                    category_menu(ui, category, categories);
                });
            });
            header.col(|ui| {
                ui.vertical(|ui| {
                    ui.strong("Letter date");
                    date_order_menu(ui, order);
                });
            });
            header.col(|ui| {
                ui.strong("Summary");
            });
        })
        .body(|mut body| {
            for pdf in rows {
                body.row(PDF_ROW_HEIGHT, |row| {
                    // Only a real click (`Some`) is kept, so a later row cannot erase it.
                    if let Some(row_click) = pdf_row(row, pdf, renaming) {
                        click = Some(row_click);
                    }
                });
            }
        });
    click
}

fn pdf_row(
    mut row: TableRow<'_, '_>,
    pdf: &PdfRow,
    renaming: &mut Option<Renaming>,
) -> Option<PdfClick> {
    let mut click = None;
    row.col(|ui| {
        // The row being renamed shows a text box instead of the file name link.
        // `as_mut` lends the box's data out for changing; `filter` keeps it only for this row.
        click = match renaming.as_mut().filter(|renaming| renaming.is_for(&pdf.path, EditKind::Name)) {
            Some(renaming) => rename_cell(ui, renaming),
            None => file_link(ui, &pdf.path),
        };
    });
    row.col(|ui| {
        // The same for the category: a box in the row being edited, else the category itself.
        let cell_click = match renaming.as_mut().filter(|renaming| renaming.is_for(&pdf.path, EditKind::Category)) {
            Some(renaming) => rename_cell(ui, renaming),
            None => category_label(ui, pdf),
        };
        // Only a real click is kept, so this cell cannot erase the file name's click.
        if cell_click.is_some() {
            click = cell_click;
        }
    });
    row.col(|ui| {
        // The same for the letter date.
        let cell_click = match renaming.as_mut().filter(|renaming| renaming.is_for(&pdf.path, EditKind::Date)) {
            Some(renaming) => rename_cell(ui, renaming),
            None => date_label(ui, pdf),
        };
        if cell_click.is_some() {
            click = cell_click;
        }
    });
    row.col(|ui| {
        // The same for the summary.
        let cell_click = match renaming.as_mut().filter(|renaming| renaming.is_for(&pdf.path, EditKind::Summary)) {
            Some(renaming) => rename_cell(ui, renaming),
            None => summary_label(ui, pdf),
        };
        if cell_click.is_some() {
            click = cell_click;
        }
    });
    click
}


/// The PDF's file name as a link; hovering shows its full `path`. A click shows the PDF,
/// a right click offers to open it or show it in Finder.
fn file_link(ui: &mut egui::Ui, path: &Path) -> Option<PdfClick> {
    let name = file_name(path);
    // A `Label` (not `ui.link`) so long names can still be cut with "…" by `truncate`.
    let text = egui::RichText::new(name)
        .underline()
        .color(ui.visuals().hyperlink_color);
    let link = ui
        .add(
            egui::Label::new(text)
                .truncate()
                .sense(egui::Sense::click()),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(path.display().to_string());

    let mut click = None;
    if link.clicked() {
        click = Some(PdfClick::Show(path.to_path_buf()));
    }
    // `context_menu` shows the closure's buttons in a popup after a right click.
    link.context_menu(|ui| {
        if ui.button("Open").clicked() {
            click = Some(PdfClick::Open(path.to_path_buf()));
            ui.close();
        }
        if ui.button("Show in Finder").clicked() {
            click = Some(PdfClick::Reveal(path.to_path_buf()));
            ui.close();
        }
        if ui.button("Rename").clicked() {
            click = Some(PdfClick::Rename(path.to_path_buf()));
            ui.close();
        }
    });
    click
}

/// The category as text; a right click offers to rename it, which moves the PDF to the
/// folder typed in.
fn category_label(ui: &mut egui::Ui, pdf: &PdfRow) -> Option<PdfClick> {
    // `sense(click)` lets the label notice the right click that opens its menu.
    let label = ui.add(egui::Label::new(pdf.category).sense(egui::Sense::click()));
    let mut click = None;
    label.context_menu(|ui| {
        if ui.button("Rename").clicked() {
            click = Some(PdfClick::RenameCategory(pdf.path.clone()));
            ui.close();
        }
    });
    click
}

/// The letter date as text; a right click offers to edit it.
fn date_label(ui: &mut egui::Ui, pdf: &PdfRow) -> Option<PdfClick> {
    let label = ui.add(egui::Label::new(pdf.letter_date.unwrap_or("-")).sense(egui::Sense::click()));
    let mut click = None;
    label.context_menu(|ui| {
        if ui.button("Edit").clicked() {
            click = Some(PdfClick::EditDate(pdf.path.clone()));
            ui.close();
        }
    });
    click
}

/// The summary, wrapped to the column's width and in full on hover; a right click offers
/// to edit it.
fn summary_label(ui: &mut egui::Ui, pdf: &PdfRow) -> Option<PdfClick> {
    let label = ui
        .add(egui::Label::new(pdf.summary).wrap().sense(egui::Sense::click()))
        .on_hover_text(pdf.summary);
    let mut click = None;
    label.context_menu(|ui| {
        if ui.button("Edit").clicked() {
            click = Some(PdfClick::EditSummary(pdf.path.clone()));
            ui.close();
        }
    });
    click
}

/// One row of the table. The texts are borrowed from the loaded letter (`'a`); the path
/// is new, so the row owns it.
pub(super) struct PdfRow<'a> {
    summary: &'a str,
    letter_date: Option<&'a str>,
    /// The PDF's full path: the archive folder joined with the path the database keeps
    /// ("Gesundheit/Brief.pdf" becomes "data/pdfs/output/Gesundheit/Brief.pdf").
    path: PathBuf,
    category: &'a str,
}

impl<'a> PdfRow<'a> {
    /// The row of a stored letter whose PDF lies in the archive folder `root`.
    pub(super) fn of_document(pdf: &'a Document, root: &Path) -> Self {
        PdfRow {
            summary: &pdf.summary,
            letter_date: pdf.letter_date.as_deref(),
            path: root.join(&pdf.file_path),
            category: &pdf.category,
        }
    }

    /// The row of a search hit whose PDF lies in the archive folder `root`.
    pub(super) fn of_hit(hit: &'a SearchHit, root: &Path) -> Self {
        PdfRow {
            summary: &hit.summary,
            letter_date: hit.letter_date.as_deref(),
            path: root.join(&hit.file_path),
            category: &hit.category,
        }
    }
}
