use crate::jobs::JobQueue;
use crate::paths::{DB_FILE, CONFIG_FILE};
use crate::pdf_crud::PdfCrud;
use crate::config::Config;
use eframe::egui;
use std::path::Path;

// The Archive tab (the table of processed PDFs) lives in `src/app/archive_tab.rs`.
mod archive_tab;
use archive_tab::ArchiveTab;
// The Images → PDF tab lives in `src/app/images_tab.rs`.
mod images_tab;
use images_tab::{ImagesTab, PREVIEW_SHARE};
// The Settings tab lives in `src/app/settings_tab.rs`.
mod settings_tab;
use settings_tab::SettingsTab;

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
    tab: Tab,

    // ---- Shared by several tabs ----
    crud: PdfCrud,
    config: Config,
    jobs: JobQueue,

    archive: ArchiveTab,
    images: ImagesTab,
    settings: SettingsTab,
}

impl eframe::App for App {
    /// Called for every frame: egui redraws the whole window from the app's data
    /// ("immediate mode"), so there are no callbacks or widget objects to keep.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let preview_size = ui.available_size() * PREVIEW_SHARE; // before the panel fills it
        self.take_dropped_files(ui.ctx());
        self.check_jobs();

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
            tab: Default::default(),
            crud: pdf_crud,
            config: Config::load(Path::new(CONFIG_FILE)),
            jobs: JobQueue::default(),
            archive: ArchiveTab::default(),
            images: ImagesTab::default(),
            settings: SettingsTab::default(),
        }
    }

    /// Collects news from the jobs once per frame; a letter just filed shows in the
    /// Archive tab. The queue only reports it; showing it is the window's part.
    fn check_jobs(&mut self) {
        if self.jobs.are_freshly_finished_jobs() {
            self.set_pdfs();
        }
    }
}

/// The last part of a path, e.g. "IMG_9549.HEIC".
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
