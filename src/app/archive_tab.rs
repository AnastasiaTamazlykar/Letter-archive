use eframe::egui;
use egui_extras::{Size, StripBuilder};
use std::error::Error;
use std::path::{Path, PathBuf};

use super::{App, DIVIDER_WIDTH, file_name};
use crate::jobs::Job;
use crate::pdf_crud::{DateOrder, Document, SearchHit};
use crate::rescan;

// Acting on clicks in the table and editing a cell live in `src/app/archive_tab/editing.rs`.

// The PDF shown right of the table lives in `src/app/archive_tab/pdf_view.rs`.
mod pdf_view;
// The menus that pick the table's order and category live in `src/app/sort_filter.rs`.
mod sort_filter;
// The table of letters lives in `src/app/archive_tab/table.rs`.
mod table;
// The tests live in `src/app/archive_tab/tests.rs`.
#[cfg(test)]
mod tests;

use pdf_view::{
    PDF_VIEW_SHARE, PDF_VIEW_SHARE_MAX, PDF_VIEW_SHARE_MIN, PdfView, divider, pdf_view_area,
};

use crate::app::archive_tab::table::PdfClick;
use crate::file_system_actions::move_file_within_archive::{change_category, rename_pdf};
use crate::open_file::{open_in_viewer, reveal_in_finder};
use table::{PdfRow, display_pdfs};


#[derive(Clone, Copy, PartialEq)]
pub(super) enum EditKind {
    Name,
    Category,
    Date,
    Summary,
}

/// Width of the search box, in points: wide enough to show its whole hint text.
const SEARCH_WIDTH: f32 = 400.0;
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

// The Archive tab's part of `App`; the rest of its methods are in `app.rs`.
impl App {
    /// Up to `limit` letters, in the chosen order and category; the database sorts and filters.
    fn load_pdfs(&mut self, limit: i64) -> Result<Vec<Document>, Box<dyn Error>> {
        let category = self.archive.category.as_deref();
        Ok(self
            .crud
            .find_letters(self.archive.date_order, category, limit)?)
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
            Err(err) => eprintln!("Documents were not loaded {}", err),
        }
        match self.crud.categories() {
            Ok(categories) => self.archive.categories = categories,
            Err(error) => eprintln!("Categories were not loaded: {error}"),
        }
    }

    /// Searches the letters for what is typed, in the chosen order and category.
    fn run_search(&mut self) {
        let category = self.archive.category.as_deref();
        self.archive.hits = self
            .crud
            .search_letters(&self.archive.search, self.archive.date_order, category)
            .unwrap_or_else(|error| {
                eprintln!("search failed: {error}");
                vec![]
            });
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
                .add_enabled(
                    !self.jobs.is_filing(),
                    egui::Button::new("Rescan archive folder"),
                )
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
            let too_short =
                !self.archive.search.is_empty() && self.archive.search.chars().count() < 3;
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
            self.archive
                .hits
                .iter()
                .map(|hit| PdfRow::of_hit(hit, root))
                .collect()
        } else {
            self.archive
                .pdfs
                .iter()
                .map(|pdf| PdfRow::of_document(pdf, root))
                .collect()
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

    fn act_on_pdf_click(&mut self, ctx: &egui::Context, click: PdfClick) {
        let (path, result) = match click {
            PdfClick::Show { id, path } => {
                self.show_pdf(ctx, id, &path);
                return;
            }
            PdfClick::Rename { id, path } => {
                self.archive.renaming = Some(Renaming::new(id, path));
                return;
            }
            PdfClick::RenameCategory { id, path } => {
                self.archive.renaming =
                    Some(Renaming::of_category(id, path, &self.config.output_dir));
                return;
            }
            PdfClick::EditDate { id, path, date } => {
                self.archive.renaming = Some(Renaming::of_date(id, path, date.unwrap_or_default()));
                return;
            }
            PdfClick::EditSummary { id, path, summary } => {
                self.archive.renaming = Some(Renaming::of_summary(id, path, summary));
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

    fn finish_rename(&mut self, ctx: &egui::Context) {
        let Some(renaming) = &mut self.archive.renaming else {
            return;
        };
        let id = renaming.id;
        let old = renaming.path.clone();
        let output_dir = &self.config.output_dir;
        let result = match renaming.kind {
            EditKind::Name => rename_pdf(&mut self.crud, output_dir, &old, &renaming.text),
            EditKind::Category => change_category(&mut self.crud, output_dir, &old, &renaming.text),
            EditKind::Date => self
                .crud
                .update_letter_date(id, &renaming.text)
                .map(|()| old.clone()),
            EditKind::Summary => self
                .crud
                .update_summary(id, &renaming.text)
                .map(|()| old.clone()),
        };
        match result {
            Err(error) => {
                renaming.error = error.to_string();
                renaming.focused = false; // give the box the focus again
            }
            Ok(new) => {
                self.archive.renaming = None;
                self.reload_table();
                // The renamed PDF was shown right of the table: show it under its new name.
                // (A date or summary change keeps the file, so there is nothing to show again.)
                if new != old
                    && self
                        .archive
                        .pdf_view
                        .as_ref()
                        .is_some_and(|view| view.name == file_name(&old))
                {
                    self.show_pdf(ctx, id, &new);
                }
            }
        }
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

struct Renaming {
    id: i64,
    path: PathBuf,
    text: String,
    kind: EditKind,
    error: String,
    focused: bool,
}

impl Renaming {
    fn new(id: i64, path: PathBuf) -> Self {
        let text = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        Renaming {
            id,
            path,
            text,
            kind: EditKind::Name,
            error: String::new(),
            focused: false,
        }
    }

    /// Editing the letter date of the PDF at `path`; the box starts with `date`, its
    /// current one ("" if it has none).
    fn of_date(id: i64, path: PathBuf, date: String) -> Self {
        Renaming {
            id,
            path,
            text: date,
            kind: EditKind::Date,
            error: String::new(),
            focused: false,
        }
    }

    /// Editing the summary of the PDF at `path`; the box starts with `summary`, its current one.
    fn of_summary(id: i64, path: PathBuf, summary: String) -> Self {
        Renaming {
            id,
            path,
            text: summary,
            kind: EditKind::Summary,
            error: String::new(),
            focused: false,
        }
    }

    /// Editing the category of the PDF at `path`; the box starts with its current category,
    /// the folder part of its path inside `output_dir` ("" for a PDF right in it).
    fn of_category(id: i64, path: PathBuf, output_dir: &Path) -> Self {
        let text = path
            .strip_prefix(output_dir)
            .ok()
            .and_then(|relative| relative.parent())
            .map(|folder| folder.to_string_lossy().into_owned())
            .unwrap_or_default();
        Renaming {
            id,
            path,
            text,
            kind: EditKind::Category,
            error: String::new(),
            focused: false,
        }
    }

    /// True if this edit is for the `kind` cell of the row of `path`.
    fn is_for(&self, path: &Path, kind: EditKind) -> bool {
        self.path == path && self.kind == kind
    }
}
