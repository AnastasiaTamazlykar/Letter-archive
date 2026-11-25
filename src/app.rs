//! The app window: drop images on it to see them as thumbnails; click one to see it large;
//! put them in order, tick some and file them as one analyzed PDF.

use crate::pdf_crud::{DateOrder, Document, PdfCrud, SearchHit};
use crate::paths::{DB_FILE, SETTINGS_FILE};
use crate::settings::Settings;
use crate::convert::Conversion;
use eframe::egui;
use std::path::Path;

// The menus that pick the table's order and category live in `src/app/sort_filter.rs`.
mod sort_filter;
// The list of running and finished filing jobs lives in `src/app/job_queue.rs`.
mod job_queue;
use job_queue::job_queue;
// The Archive tab (the table of processed PDFs) lives in `src/app/archive_tab.rs`.
mod archive_tab;
use archive_tab::{PDF_VIEW_SHARE, PdfView, Renaming};
// Only the tests use these here: `use super::*` in `tests.rs` picks them up from `app`.
#[cfg(test)]
use archive_tab::{EditKind, PdfClick, rename_cell};
// The Images → PDF tab lives in `src/app/images_tab.rs`.
mod images_tab;
use images_tab::{PREVIEW_SHARE, Picture, Preview};
// The Settings tab lives in `src/app/settings_tab.rs`.
mod settings_tab;

// The tests live in `src/app/tests.rs`.
#[cfg(test)]
mod tests;
// The tests of the Images → PDF tab live in `src/app/tests_images.rs`.
#[cfg(test)]
mod tests_images;

/// Width of the draggable divider between the table and the PDF area, in points.
const DIVIDER_WIDTH: f32 = 8.0;

/// The pages of the window, picked with the buttons at the top.
/// `PartialEq` lets egui compare tabs, to know which button to show as chosen.
/// `#[default]` marks the tab `Default` gives: the pdf_crud is shown first.
#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Archive,
    Images,
    Settings,
}

/// Everything the window remembers between frames.
pub struct App {
    crud: PdfCrud,
    tab: Tab,
    pdfs: Vec<Document>,
    hits: Vec<SearchHit>,
    /// The images added so far. Their order here is the page order in the PDF.
    pictures: Vec<Picture>,
    /// The image shown large, if one was clicked.
    preview: Option<Preview>,
    /// The PDF shown right of the table, if a file name was clicked.
    pdf_view: Option<PdfView>,
    /// The PDF area's share of the Archive tab's width; changed by dragging the divider.
    pdf_view_share: f32,
    /// The file name being renamed in the table, if any.
    renaming: Option<Renaming>,
    /// The filing jobs, running in the background at the same time or already done, oldest
    /// first. A job that worked leaves the list at once; a failed one stays (to show its
    /// error) until the user clears it.
    conversions: Vec<Conversion>,
    /// What the last rescan did; shown in the Archive tab only.
    rescan_status: String,
    search: String,
    /// How the table is ordered by letter date.
    date_order: DateOrder,
    /// The category the table shows; `None`: all of them.
    category: Option<String>,
    /// The choices of the category menu, from the database.
    categories: Vec<String>,
    /// The user's settings, read from `SETTINGS_FILE` at the start.
    settings: Settings,
    /// Whether the last change in the Settings tab was saved; shown there.
    settings_status: String,
}

impl eframe::App for App {
    /// Called for every frame: egui redraws the whole window from the app's data
    /// ("immediate mode"), so there are no callbacks or widget objects to keep.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let preview_size = ui.available_size() * PREVIEW_SHARE; // before the panel fills it
        self.take_dropped_files(ui.ctx());
        self.check_conversions();

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Paper Archive");
                // `selectable_value` sets `self.tab` to the given tab when clicked.
                ui.selectable_value(&mut self.tab, Tab::Archive, "Archive");
                ui.selectable_value(&mut self.tab, Tab::Images, "Images → PDF");
                ui.selectable_value(&mut self.tab, Tab::Settings, "Settings");
            });
            ui.separator();
            match self.tab {
                Tab::Archive => self.archive_tab(ui),
                Tab::Images => self.images_tab(ui),
                Tab::Settings => self.settings_tab(ui),
            }
        });
        self.show_preview(ui.ctx(), preview_size);
    }
}

// A second `impl` block: the app's own methods, apart from the ones `eframe::App` requires.
impl App {
    pub fn new() -> Self {
        let pdf_crud = match PdfCrud::open(Path::new(DB_FILE)) {
            Ok(pdf_crud) => pdf_crud,
            Err(error) => {
                eprintln!("cannot open the pdf_crud: {error}");
                panic!("cannot open the pdf_crud: {error}");
            }
        };
        App {
            crud: pdf_crud,
            tab: Default::default(),
            pdfs: vec![],
            hits: vec![],
            pictures: vec![],
            preview: None,
            pdf_view: None,
            pdf_view_share: PDF_VIEW_SHARE,
            renaming: None,
            conversions: vec![],
            rescan_status: "".to_string(),
            search: "".to_string(),
            date_order: DateOrder::default(),
            category: None,
            categories: vec![],
            settings: Settings::load(Path::new(SETTINGS_FILE)),
            settings_status: "".to_string(),
        }
    }

    /// True while at least one job has not finished.
    fn is_filing(&self) -> bool {
        self.conversions.iter().any(|job| !job.is_finished())
    }

    /// Draws the list of jobs; removes the finished ones if "Clear finished" was clicked.
    fn show_job_queue(&mut self, ui: &mut egui::Ui) {
        if job_queue(ui, &self.conversions) {
            // `retain` keeps only the jobs for which the closure says true.
            self.conversions.retain(|job| !job.is_finished());
        }
    }

    /// Collects news from every job; reloads the table if one has just filed a letter.
    fn check_conversions(&mut self) {
        let mut filed = false;
        for job in &mut self.conversions {
            let was_finished = job.is_finished();
            job.receive();
            // Only a job that finished in this frame counts; old ones were handled before.
            filed |= !was_finished && job.succeeded();
        }
        if filed {
            self.set_pdfs(); // show the new letters in the Archive tab
        }
        // A job that worked has nothing more to say: the letter is in the table. A failed
        // one stays in the queue, with its error, until the user clears it.
        self.conversions.retain(|job| !job.succeeded());
    }
}

/// The last part of a path, e.g. "IMG_9549.HEIC".
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
