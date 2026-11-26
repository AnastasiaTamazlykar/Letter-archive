//! The Archive tab: the table of processed PDFs, with search, rescan, sort and filter,
//! editing a file name, category or letter date in place, and the PDF shown next to it.
//! A child module of `app`: its `impl App` block may use `App`'s private fields.
//! `pub(super)` marks what `app.rs` (and the tests under it) may use from here.

use std::error::Error;
use std::path::{Path, PathBuf};

use eframe::egui;
use egui_extras::{Size, StripBuilder};

use super::{App, DIVIDER_WIDTH, file_name};
use crate::archive_folder_manipulations::{path_change::change_category, rename_file::rename_pdf};
use crate::jobs::Job;
use crate::open_file::{open_in_viewer, reveal_in_finder};
use crate::pdf_crud::{DateOrder, Document, PdfCrud, SearchHit};
use crate::rescan;

// The PDF shown right of the table lives in `src/app/archive_tab/pdf_view.rs`.
mod pdf_view;
// The menus that pick the table's order and category live in `src/app/sort_filter.rs`.
mod sort_filter;
// The table of letters lives in `src/app/archive_tab/table.rs`.
mod table;
// The tests live in `src/app/archive_tab/tests.rs`.
#[cfg(test)]
mod tests;

use pdf_view::{PDF_VIEW_SHARE, PDF_VIEW_SHARE_MAX, PDF_VIEW_SHARE_MIN, PdfView, divider, pdf_view_area};
use table::{PdfClick, PdfRow, display_pdfs};

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
    /// The summary: only the database changes, not the file.
    Summary,
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

    /// Editing the summary of the PDF at `path`; the box starts with `summary`, its current one.
    fn of_summary(path: PathBuf, summary: String) -> Self {
        Renaming { path, text: summary, kind: EditKind::Summary, error: String::new(), focused: false }
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
                .add_enabled(!self.jobs.is_filing(), egui::Button::new("Rescan archive folder"))
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
        self.jobs.show(ui);
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
                    close = pdf_view_area(ui, self.archive.pdf_view.as_mut());
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
                // `and_then`: no stored letter, or a letter without a date, both give `None`.
                let date = self.stored_letter(&path).and_then(|letter| letter.letter_date);
                self.archive.renaming = Some(Renaming::of_date(path, date.unwrap_or_default())); // "" if none
                return;
            }
            PdfClick::EditSummary(path) => {
                let summary = self.stored_letter(&path).map(|letter| letter.summary);
                self.archive.renaming = Some(Renaming::of_summary(path, summary.unwrap_or_default()));
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
            EditKind::Summary => change_summary,
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
                // (A date or summary change keeps the file, so there is nothing to show again.)
                if new != old && self.archive.pdf_view.as_ref().is_some_and(|view| view.name == file_name(&old)) {
                    self.show_pdf(ctx, &new);
                }
            }
        }
    }

    /// The stored letter whose PDF is at `path`; `None` if it is not in the archive.
    fn stored_letter(&mut self, path: &Path) -> Option<Document> {
        let id = letter_id(&mut self.crud, &self.config.output_dir, path).ok()?;
        self.crud.document(id).ok()?
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
        self.jobs.add(Job::start_pdf(pdf, output_dir, ctx.clone()));
    }
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

/// Saves `summary` as the summary of the PDF at `path`. The same arguments and result as
/// `change_date`, so `finish_rename` can pick it the same way.
fn change_summary(
    crud: &mut PdfCrud,
    output_dir: &Path,
    path: &Path,
    summary: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let id = letter_id(crud, output_dir, path)?;
    crud.update_summary(id, summary)?;
    Ok(path.to_path_buf())
}

fn rename_cell(ui: &mut egui::Ui, renaming: &mut Renaming) -> Option<PdfClick> {
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
