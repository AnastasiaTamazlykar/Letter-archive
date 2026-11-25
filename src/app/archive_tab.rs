//! The Archive tab: the table of processed PDFs, with search, rescan, sort and filter,
//! editing a file name, category or letter date in place, and the PDF shown next to it.
//! A child module of `app`: its `impl App` block may use `App`'s private fields.
//! `pub(super)` marks what `app.rs` (and the tests under it) may use from here.

use std::error::Error;
use std::path::{Path, PathBuf};

use eframe::egui;
use egui_extras::{Column, Size, StripBuilder, TableBuilder, TableRow};

use sort_filter::{category_menu, date_order_menu};
use super::{App, DIVIDER_WIDTH, file_name};
use crate::archive_folder_manipulations::{path_change::change_category, rename_file::rename_pdf};
use crate::convert::Conversion;
use crate::open_file::{open_in_viewer, reveal_in_finder};
use crate::pdf_crud::{DateOrder, Document, PdfCrud, SearchHit};
use crate::rescan;

// The PDF shown right of the table lives in `src/app/archive_tab/pdf_view.rs`.
mod pdf_view;
// The menus that pick the table's order and category live in `src/app/sort_filter.rs`.
mod sort_filter;
// The tests live in `src/app/archive_tab/tests.rs`.
#[cfg(test)]
mod tests;

use pdf_view::{PDF_VIEW_SHARE, PDF_VIEW_SHARE_MAX, PDF_VIEW_SHARE_MIN, PdfView, divider, pdf_view_area};

/// Height of one row of the PDF table, in points.
const PDF_ROW_HEIGHT: f32 = 60.0;
/// Width of the search box, in points: wide enough to show its whole hint text.
const SEARCH_WIDTH: f32 = 400.0;
/// Room left for ".pdf" after the rename box, in points.
const RENAME_SUFFIX_WIDTH: f32 = 36.0;
/// Empty space above and below the search box, in points.
const SEARCH_GAP: f32 = 8.0;

/// What the Archive tab remembers between frames; it is `App.archive`.
/// The fields are private: only this module and its children (`pdf_view`, `tests`) use them.
pub(super) struct ArchiveTab {
    /// The letters in the table, as the database sorted and filtered them.
    pdfs: Vec<Document>,
    /// The letters found by the search; shown instead of `pdfs` while a search is typed.
    hits: Vec<SearchHit>,
    search: String,
    /// How the table is ordered by letter date.
    date_order: DateOrder,
    /// The category the table shows; `None`: all of them.
    category: Option<String>,
    /// The choices of the category menu, from the database.
    categories: Vec<String>,
    /// The cell being edited in the table, if any.
    renaming: Option<Renaming>,
    /// The PDF shown right of the table, if a file name was clicked.
    pdf_view: Option<PdfView>,
    /// The PDF area's share of the Archive tab's width; changed by dragging the divider.
    pdf_view_share: f32,
    /// What the last rescan did; shown in the Archive tab only.
    rescan_status: String,
}

// Written by hand, not `#[derive(Default)]`: the derived one would start the PDF area's
// share at 0.0, so the PDF area would be invisible.
impl Default for ArchiveTab {
    fn default() -> Self {
        ArchiveTab {
            pdfs: vec![],
            hits: vec![],
            search: String::new(),
            date_order: DateOrder::default(),
            category: None,
            categories: vec![],
            renaming: None,
            pdf_view: None,
            pdf_view_share: PDF_VIEW_SHARE,
            rescan_status: String::new(),
        }
    }
}

/// What was clicked on a file name; each holds the PDF's full path.
enum PdfClick {
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
    /// Enter in that text box: rename the file, or move it to the typed category.
    RenameSubmit,
    /// Esc in that text box, or a click somewhere else: keep the old name.
    RenameCancel,
}

/// Which cell of a row is a text box.
/// `Copy` + `PartialEq`: a tiny value that is copied and compared like a number.
#[derive(Clone, Copy, PartialEq)]
enum EditKind {
    /// The file name.
    Name,
    /// The category: the file moves to that folder.
    Category,
    /// The letter date: only the database changes, not the file.
    Date,
}

/// The edit in progress: a cell of the table (file name, category or letter date) is a text
/// box while this exists.
struct Renaming {
    /// The PDF's full path; tells the table which row to draw the box in.
    pub(super) path: PathBuf,
    /// The new name without ".pdf", the new category or the new letter date, depending on
    /// `kind`; starts as the current one.
    pub(super) text: String,
    /// What is edited.
    pub(super) kind: EditKind,
    /// Why the last try failed, shown under the box.
    pub(super) error: String,
    /// False until the box has been given the keyboard focus (once at the start, and again
    /// after an error, so the user can keep typing).
    pub(super) focused: bool,
}

impl Renaming {
    pub(super) fn new(path: PathBuf) -> Self {
        let text = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        Renaming { path, text, kind: EditKind::Name, error: String::new(), focused: false }
    }

    /// Editing the letter date of the PDF at `path`; the box starts with `date`, its
    /// current one ("" if it has none).
    fn of_date(path: PathBuf, date: String) -> Self {
        Renaming { path, text: date, kind: EditKind::Date, error: String::new(), focused: false }
    }

    /// Editing the category of the PDF at `path`; the box starts with its current category,
    /// the folder part of its path inside `output_dir` ("" for a PDF right in it).
    fn of_category(path: PathBuf, output_dir: &Path) -> Self {
        let text = path
            .strip_prefix(output_dir)
            .ok()
            .and_then(|relative| relative.parent())
            .map(|folder| folder.to_string_lossy().into_owned())
            .unwrap_or_default();
        Renaming { path, text, kind: EditKind::Category, error: String::new(), focused: false }
    }

    /// True if this edit is for the `kind` cell of the row of `path`.
    pub(super) fn is_for(&self, path: &Path, kind: EditKind) -> bool {
        self.path == path && self.kind == kind
    }
}

// The Archive tab's part of `App`; the rest of its methods are in `app.rs`.
impl App {
    /// Up to `limit` letters, in the chosen order and category; the database sorts and filters.
    fn load_pdfs(&mut self, limit: i64) -> Result<Vec<Document>, Box<dyn Error>> {
        let category = self.archive.category.as_deref();
        Ok(self.crud.find_letters(self.archive.date_order, category, limit)?)
    }

    /// Fixes the paths of PDFs moved inside the output folder and removes letters whose
    /// PDF is gone (see `rescan.rs`), shows what it did, then reloads the table.
    /// Runs at startup and from the Rescan button.
    pub fn rescan(&mut self) {
        self.archive.rescan_status = match rescan::rescan(&mut self.crud, &self.config.output_dir) {
            Ok(report) => report.summary(),
            Err(error) => format!("Rescan failed: {error}"),
        };
        self.set_pdfs(); // also after a failed rescan, so the table is never empty
    }

    pub fn set_pdfs(&mut self) {
        match self.load_pdfs(50) {
            Ok(pdfs) => {
                self.archive.pdfs = pdfs;
            }
            Err(err) => eprintln!("Pdfs were not loaded {}", err),
        }
        match self.crud.categories() {
            Ok(categories) => self.archive.categories = categories,
            Err(error) => eprintln!("categories were not loaded: {error}"),
        }
    }

    /// Searches the letters for what is typed, in the chosen order and category.
    fn run_search(&mut self) {
        let category = self.archive.category.as_deref();
        self.archive.hits = match self.crud.search_letters(&self.archive.search, self.archive.date_order, category) {
            Ok(hits) => hits,
            Err(error) => {
                eprintln!("search failed: {error}");
                vec![]
            }
        };
    }

    /// Loads the table again after the order or category changed: the letters, and the
    /// search results if a search is shown.
    fn reload_table(&mut self) {
        self.set_pdfs();
        if !self.archive.search.is_empty() {
            self.run_search();
        }
    }

    /// The main page: a + button to add a PDF, and the table of stored PDFs.
    pub(super) fn archive_tab(&mut self, ui: &mut egui::Ui) {
        // `display()`: a path as text to show; it may not be valid UTF-8, so no plain `{}`.
        let output_dir = self.config.output_dir.display().to_string();
        // The Analyze button has a row of its own. Jobs run side by side, so it is always available.
        if ui
            .button("Process new PDF file…")
            .on_hover_text(format!("Pick a PDF; a copy goes to {output_dir}"))
            .clicked()
        {
            self.pick_pdf(ui.ctx());
        }
        ui.horizontal(|ui| {
            // Not while letters are being filed: a rescan could remove a letter just added.
            if ui
                .add_enabled(!self.is_filing(), egui::Button::new("Rescan archive folder"))
                .on_hover_text(format!(
                    "Find moved PDFs in {output_dir}; remove letters whose PDF is gone"
                ))
                .on_disabled_hover_text("Wait until the letters being filed are done")
                .clicked()
            {
                self.rescan();
            }
            ui.label(&self.archive.rescan_status);
        });
        // The title of the table, on a line of its own.
        ui.label("Processed PDFs");
        self.show_job_queue(ui);
        // The search box gets a row of its own, below the buttons, with space around it.
        ui.add_space(SEARCH_GAP);
        ui.horizontal(|ui| {
            let too_short = !self.archive.search.is_empty() && self.archive.search.chars().count() < 3;
            let search_box = ui
                .scope(|ui| {
                    if too_short {
                        // Red border while not focused, and red text.
                        ui.visuals_mut().widgets.inactive.bg_stroke =
                            egui::Stroke::new(1.0, egui::Color32::RED);
                        ui.visuals_mut().override_text_color = Some(egui::Color32::RED);
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.archive.search)
                            .hint_text("Search document by text… enter at least 3 symbols")
                            .desired_width(SEARCH_WIDTH),
                    )
                })
                .inner;
            if search_box.lost_focus() {
                self.run_search();
            }
            //  once search fields is cleared, results are also cleared
            if self.archive.search.is_empty() {
                self.archive.hits = vec![];
            }
        });
        ui.add_space(SEARCH_GAP);
        // Already sorted and filtered by the database.
        let root = &self.config.output_dir;
        let rows: Vec<PdfRow> = if !self.archive.search.is_empty() {
            self.archive.hits.iter().map(|hit| PdfRow::of_hit(hit, root)).collect()
        } else {
            self.archive.pdfs.iter().map(|pdf| PdfRow::of_document(pdf, root)).collect()
        };
        // Kept to see after the table whether its menus changed the order or category.
        let old_order = self.archive.date_order;
        let old_category = self.archive.category.clone();

        // A strip splits the space into cells side by side: the table on the left, the
        // divider, the PDF on the right. `clip`: no cell can draw over another.
        // The cells only note what happened; we act on it after the strip.
        let width = ui.available_width();
        let mut click = None;
        let mut close = false;
        let mut dragged = 0.0;
        StripBuilder::new(ui)
            .clip(true)
            .size(Size::relative(1.0 - self.archive.pdf_view_share))
            .size(Size::exact(DIVIDER_WIDTH))
            .size(Size::remainder())
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    // `rows` borrows `self.archive.pdfs` (or `self.archive.hits`) while the header's menus
                    // change `self.archive.date_order` and `self.archive.category`. Rust allows this: they
                    // are different fields of `self`.
                    click = display_pdfs(
                        ui,
                        &rows,
                        &mut self.archive.date_order,
                        &mut self.archive.category,
                        &self.archive.categories,
                        &mut self.archive.renaming,
                    );
                });
                strip.cell(|ui| {
                    dragged = divider(ui);
                });
                strip.cell(|ui| {
                    close = pdf_view_area(ui, self.archive.pdf_view.as_ref());
                });
            });

        // Dragging right makes the table wider, so the PDF area's share gets smaller.
        // Only when dragged: with a width of 0, `0.0 / 0.0` would give NaN ("not a number").
        if dragged != 0.0 {
            self.archive.pdf_view_share = (self.archive.pdf_view_share - dragged / width)
                .clamp(PDF_VIEW_SHARE_MIN, PDF_VIEW_SHARE_MAX);
        }

        if close {
            self.archive.pdf_view = None;
        }
        // A new choice in a header menu: ask the database again, and draw the next frame
        // right away so the new rows show without waiting for the mouse to move.
        if self.archive.date_order != old_order || self.archive.category != old_category {
            self.reload_table();
            ui.ctx().request_repaint();
        }
        if let Some(click) = click {
            self.act_on_pdf_click(ui.ctx(), click);
        }
    }


    /// Acts on a click on a file name: shows the PDF right of the table, opens it in the
    /// PDF app, or shows it in Finder; prints why if that fails.
    fn act_on_pdf_click(&mut self, ctx: &egui::Context, click: PdfClick) {
        let (path, result) = match click {
            PdfClick::Show(path) => {
                self.show_pdf(ctx, &path);
                return;
            }
            PdfClick::Rename(path) => {
                self.archive.renaming = Some(Renaming::new(path));
                return;
            }
            PdfClick::RenameCategory(path) => {
                self.archive.renaming = Some(Renaming::of_category(path, &self.config.output_dir));
                return;
            }
            PdfClick::EditDate(path) => {
                let date = self.stored_date(&path).unwrap_or_default(); // "" if none
                self.archive.renaming = Some(Renaming::of_date(path, date));
                return;
            }
            PdfClick::RenameSubmit => {
                self.finish_rename(ctx);
                return;
            }
            PdfClick::RenameCancel => {
                self.archive.renaming = None;
                return;
            }
            PdfClick::Open(path) => {
                let result = open_in_viewer(&path);
                (path, result)
            }
            PdfClick::Reveal(path) => {
                let result = reveal_in_finder(&path);
                (path, result)
            }
        };
        if let Err(error) = result {
            eprintln!("cannot open {}: {error}", path.display());
        }
    }

    /// Renames the PDF being renamed to the typed name (see `rename_pdf.rs`). On success the
    /// table is loaded again; on failure the box stays, with the reason under it.
    fn finish_rename(&mut self, ctx: &egui::Context) {
        let Some(renaming) = &mut self.archive.renaming else {
            return;
        };
        let old = renaming.path.clone();
        // `self.crud`, `self.settings` and `self.archive.renaming` are different fields, so Rust
        // lets us borrow all three at once.
        // Both functions take the same arguments: pick the one for what is being edited.
        let change = match renaming.kind {
            EditKind::Name => rename_pdf,
            EditKind::Category => change_category,
            EditKind::Date => change_date,
        };
        match change(&mut self.crud, &self.config.output_dir, &old, &renaming.text) {
            Err(error) => {
                renaming.error = error.to_string();
                renaming.focused = false; // give the box the focus again
            }
            Ok(new) => {
                self.archive.renaming = None;
                self.reload_table();
                // The renamed PDF was shown right of the table: show it under its new name.
                // (A date change keeps the file, so there is nothing to show again.)
                if new != old && self.archive.pdf_view.as_ref().is_some_and(|view| view.name == file_name(&old)) {
                    self.show_pdf(ctx, &new);
                }
            }
        }
    }

    /// The letter date stored for the PDF at `path`; `None` if it has none or is not stored.
    fn stored_date(&mut self, path: &Path) -> Option<String> {
        let id = letter_id(&mut self.crud, &self.config.output_dir, path).ok()?;
        self.crud.document(id).ok()??.letter_date
    }

    /// Asks for a PDF with the macOS Open panel, then analyzes and files it in the
    /// background; does nothing on Cancel.
    fn pick_pdf(&mut self, ctx: &egui::Context) {
        let Some(pdf) = rfd::FileDialog::new()
            .set_title("Process new PDF file")
            .add_filter("PDF", &["pdf"])
            .pick_file()
        else {
            return;
        };
        let output_dir = self.config.output_dir.clone(); // the thread gets its own copy
        self.conversions.push(Conversion::start_pdf(pdf, output_dir, ctx.clone()));
    }
}

/// Draws the table of PDFs and returns what was clicked on a file name, if anything.
/// The Category and Letter date headers hold menus that change `category` and `order`.
fn display_pdfs(
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
        ui.add(egui::Label::new(pdf.summary).wrap())
            .on_hover_text(pdf.summary);
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

/// The id of the stored letter whose PDF is at `path`, a full path inside `output_dir`.
fn letter_id(crud: &mut PdfCrud, output_dir: &Path, path: &Path) -> Result<i64, Box<dyn Error>> {
    // The database keeps the path inside the archive folder only.
    let relative = path.strip_prefix(output_dir)?.to_str().ok_or("path is not UTF-8")?;
    Ok(crud.find_by_file_path(relative)?.ok_or("this PDF is not in the archive")?)
}

/// Saves `date` as the letter date of the PDF at `path`. Takes the same arguments and
/// returns the same as `rename_pdf` and `change_category`, so `finish_rename` can use any
/// of the three; the PDF stays where it is, so it returns `path` itself.
fn change_date(
    crud: &mut PdfCrud,
    output_dir: &Path,
    path: &Path,
    date: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let id = letter_id(crud, output_dir, path)?;
    crud.update_letter_date(id, date)?;
    Ok(path.to_path_buf())
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

pub(super) fn rename_cell(ui: &mut egui::Ui, renaming: &mut Renaming) -> Option<PdfClick> {
    // The text box that replaces the file name or the category while it is edited (".pdf"
    // follows a file name), with the reason under it if the last try failed. Enter saves,
    // Esc or a click elsewhere cancels.
    let mut click = None;
    ui.horizontal(|ui| {
        // ".pdf" only follows a file name, not a category.
        let is_name = renaming.kind == EditKind::Name;
        let suffix_width = if is_name { RENAME_SUFFIX_WIDTH } else { 0.0 };
        let width = (ui.available_width() - suffix_width).max(0.0);
        let name = ui.add(egui::TextEdit::singleline(&mut renaming.text).desired_width(width));
        if !renaming.focused {
            name.request_focus(); // so the user can type at once
            renaming.focused = true;
        }
        if is_name {
            ui.label(".pdf");
        }
        // Enter, Esc and a click elsewhere all take the focus away from the box;
        // the key tells them apart.
        if name.lost_focus() {
            click = Some(if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                PdfClick::RenameSubmit
            } else {
                PdfClick::RenameCancel
            });
        }
    });
    if !renaming.error.is_empty() {
        ui.colored_label(egui::Color32::RED, &renaming.error);
    }
    click
}

/// One row of the table. The texts are borrowed from the loaded letter (`'a`); the path
/// is new, so the row owns it.
struct PdfRow<'a> {
    summary: &'a str,
    letter_date: Option<&'a str>,
    /// The PDF's full path: the archive folder joined with the path the database keeps
    /// ("Gesundheit/Brief.pdf" becomes "data/pdfs/output/Gesundheit/Brief.pdf").
    path: PathBuf,
    category: &'a str,
}

impl<'a> PdfRow<'a> {
    /// The row of a stored letter whose PDF lies in the archive folder `root`.
    fn of_document(pdf: &'a Document, root: &Path) -> Self {
        PdfRow {
            summary: &pdf.summary,
            letter_date: pdf.letter_date.as_deref(),
            path: root.join(&pdf.file_path),
            category: &pdf.category,
        }
    }

    /// The row of a search hit whose PDF lies in the archive folder `root`.
    fn of_hit(hit: &'a SearchHit, root: &Path) -> Self {
        PdfRow {
            summary: &hit.summary,
            letter_date: hit.letter_date.as_deref(),
            path: root.join(&hit.file_path),
            category: &hit.category,
        }
    }
}
