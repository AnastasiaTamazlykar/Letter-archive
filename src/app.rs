//! The app window: drop images on it to see them as thumbnails; click one to see it large;
//! put them in order, tick some and file them as one analyzed PDF.

use crate::open_file::{open_in_viewer, reveal_in_finder};
use crate::pdf_crud::{DateOrder, Document, PdfCrud, SearchHit};
use crate::pdf_pages::render_pages;
use crate::paths::{DB_FILE, SETTINGS_FILE};
use crate::archive_folder_manipulations::{
    move_archive::move_archive, path_change::change_category, rename_file::rename_pdf,
};
use crate::settings::Settings;
use crate::{convert::Conversion, pipeline, rescan};
use eframe::egui;
use egui_extras::{Column, Size, StripBuilder, TableBuilder, TableRow};
use std::error::Error;
use std::path::{Path, PathBuf};

// The menus that pick the table's order and category live in `src/app/sort_filter.rs`.
mod sort_filter;
use sort_filter::{category_menu, date_order_menu};
// The list of running and finished filing jobs lives in `src/app/job_queue.rs`.
mod job_queue;
use job_queue::job_queue;

// The tests live in `src/app/tests.rs`.
#[cfg(test)]
mod tests;

/// File endings the pipeline can read, in lowercase.
const IMAGE_EXTENSIONS: [&str; 4] = ["heic", "png", "jpg", "jpeg"];
/// Height of one row of the PDF table, in points.
const PDF_ROW_HEIGHT: f32 = 60.0;
/// Height of the drop area, in points.
const DROP_AREA_HEIGHT: f32 = 80.0;
/// Size of a thumbnail on screen, in points (the longer side).
const THUMBNAIL_POINTS: f32 = 150.0;
/// Size of a thumbnail in pixels: twice the points, so it stays sharp on Retina screens.
const THUMBNAIL_PIXELS: u32 = 300;
/// Size of the large preview in pixels (the longer side). Phone photos are about 4032
/// pixels, so this keeps them at full resolution (smaller images are never enlarged).
const PREVIEW_PIXELS: u32 = 4096;
/// Share of the window the large preview may cover.
const PREVIEW_SHARE: f32 = 0.9;
/// How far the preview can be zoomed in: 2.0 shows each image pixel as 2 × 2 points.
const MAX_ZOOM: f32 = 2.0;
/// Width of a PDF page drawn for the area right of the table, in pixels.
const PDF_VIEW_PIXELS: u32 = 1200;
/// Share of the Archive tab's width the PDF area right of the table takes at the start.
const PDF_VIEW_SHARE: f32 = 0.5;
/// How small and how large the PDF area's share can be dragged.
const PDF_VIEW_SHARE_MIN: f32 = 0.2;
const PDF_VIEW_SHARE_MAX: f32 = 0.8;
/// Width of the draggable divider between the table and the PDF area, in points.
const DIVIDER_WIDTH: f32 = 8.0;
/// Width of the search box, in points: wide enough to show its whole hint text.
const SEARCH_WIDTH: f32 = 400.0;
/// Height of the band above the thumbnails in the Images tab, in points: the drop area and
/// the Create button next to the job list. Fixed, so the thumbnails below never move.
const IMAGES_TOP_HEIGHT: f32 = 140.0;
/// Room left for ".pdf" after the rename box, in points.
const RENAME_SUFFIX_WIDTH: f32 = 36.0;
/// Empty space above and below the search box, in points.
const SEARCH_GAP: f32 = 8.0;

/// One added image and its thumbnail.
struct Picture {
    path: PathBuf,
    /// The thumbnail, uploaded to the graphics card; `None` if the file could not be read.
    thumbnail: Option<egui::TextureHandle>,
    /// Ticked for converting to PDF.
    selected: bool,
}

/// The image shown large on top of the window.
struct Preview {
    name: String,
    texture: egui::TextureHandle,
    /// The part of the image that is visible, changed by zooming and dragging.
    /// `Rect::ZERO` means "not set yet": the whole image is fitted in.
    view: egui::Rect,
}

/// The PDF shown in the area right of the table: its name and its pages as textures.
struct PdfView {
    name: String,
    pages: Vec<egui::TextureHandle>,
}

/// What the user clicked on a card in this frame.
enum CardClick {
    Nothing,
    /// The thumbnail: show it large.
    Open,
    /// The × button: take the image out of the list.
    Remove,
    /// The ← button: swap with the image before it.
    MoveEarlier,
    /// The → button: swap with the image after it.
    MoveLater,
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
    /// Enter in that text box: rename the file, or move it to the typed category.
    RenameSubmit,
    /// Esc in that text box, or a click somewhere else: keep the old name.
    RenameCancel,
}

/// The rename in progress: the file name in the table is a text box while this exists.
struct Renaming {
    /// The PDF's full path; tells the table which row to draw the box in.
    path: PathBuf,
    /// The new name without ".pdf", or the new category if `category` is true; starts as
    /// the current one.
    text: String,
    /// True when the category is edited (the file moves to that folder), false for the name.
    category: bool,
    /// Why the last try failed, shown under the box.
    error: String,
    /// False until the box has been given the keyboard focus (once at the start, and again
    /// after an error, so the user can keep typing).
    focused: bool,
}

impl Renaming {
    fn new(path: PathBuf) -> Self {
        let text = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        Renaming { path, text, category: false, error: String::new(), focused: false }
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
        Renaming { path, text, category: true, error: String::new(), focused: false }
    }

    /// True if this edit is for the name (or category) cell of the row of `path`.
    fn is_for(&self, path: &Path, category: bool) -> bool {
        self.path == path && self.category == category
    }
}

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

    /// Up to `limit` letters, in the chosen order and category; the database sorts and filters.
    fn load_pdfs(&mut self, limit: i64) -> Result<Vec<Document>, Box<dyn Error>> {
        let category = self.category.as_deref();
        Ok(self.crud.find_letters(self.date_order, category, limit)?)
    }

    /// Fixes the paths of PDFs moved inside the output folder and removes letters whose
    /// PDF is gone (see `rescan.rs`), shows what it did, then reloads the table.
    /// Runs at startup and from the Rescan button.
    pub fn rescan(&mut self) {
        self.rescan_status = match rescan::rescan(&mut self.crud, &self.settings.output_dir) {
            Ok(report) => report.summary(),
            Err(error) => format!("Rescan failed: {error}"),
        };
        self.set_pdfs(); // also after a failed rescan, so the table is never empty
    }

    pub fn set_pdfs(&mut self) {
        match self.load_pdfs(50) {
            Ok(pdfs) => {
                self.pdfs = pdfs;
            }
            Err(err) => eprintln!("Pdfs were not loaded {}", err),
        }
        match self.crud.categories() {
            Ok(categories) => self.categories = categories,
            Err(error) => eprintln!("categories were not loaded: {error}"),
        }
    }

    /// Searches the letters for what is typed, in the chosen order and category.
    fn run_search(&mut self) {
        let category = self.category.as_deref();
        self.hits = match self.crud.search_letters(&self.search, self.date_order, category) {
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
        if !self.search.is_empty() {
            self.run_search();
        }
    }

    /// Adds the files dropped on the window in this frame.
    fn take_dropped_files(&mut self, ctx: &egui::Context) {
        // `input` lends us this frame's input only inside the closure (a borrow),
        // so we copy each path (`&Path`, borrowed) into a `PathBuf` (owned) to keep it.
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        for path in dropped {
            self.add_file(ctx, path);
        }
    }

    /// The main page: a + button to add a PDF, and the table of stored PDFs.
    fn archive_tab(&mut self, ui: &mut egui::Ui) {
        // `display()`: a path as text to show; it may not be valid UTF-8, so no plain `{}`.
        let output_dir = self.settings.output_dir.display().to_string();
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
            ui.label(&self.rescan_status);
        });
        // The title of the table, on a line of its own.
        ui.label("Processed PDFs");
        self.show_job_queue(ui);
        // The search box gets a row of its own, below the buttons, with space around it.
        ui.add_space(SEARCH_GAP);
        ui.horizontal(|ui| {
            let too_short = !self.search.is_empty() && self.search.chars().count() < 3;
            let search_box = ui
                .scope(|ui| {
                    if too_short {
                        // Red border while not focused, and red text.
                        ui.visuals_mut().widgets.inactive.bg_stroke =
                            egui::Stroke::new(1.0, egui::Color32::RED);
                        ui.visuals_mut().override_text_color = Some(egui::Color32::RED);
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search document by text… enter at least 3 symbols")
                            .desired_width(SEARCH_WIDTH),
                    )
                })
                .inner;
            if search_box.lost_focus() {
                self.run_search();
            }
            //  once search fields is cleared, results are also cleared
            if self.search.is_empty() {
                self.hits = vec![];
            }
        });
        ui.add_space(SEARCH_GAP);
        // Already sorted and filtered by the database.
        let root = &self.settings.output_dir;
        let rows: Vec<PdfRow> = if !self.search.is_empty() {
            self.hits.iter().map(|hit| PdfRow::of_hit(hit, root)).collect()
        } else {
            self.pdfs.iter().map(|pdf| PdfRow::of_document(pdf, root)).collect()
        };
        // Kept to see after the table whether its menus changed the order or category.
        let old_order = self.date_order;
        let old_category = self.category.clone();

        // A strip splits the space into cells side by side: the table on the left, the
        // divider, the PDF on the right. `clip`: no cell can draw over another.
        // The cells only note what happened; we act on it after the strip.
        let width = ui.available_width();
        let mut click = None;
        let mut close = false;
        let mut dragged = 0.0;
        StripBuilder::new(ui)
            .clip(true)
            .size(Size::relative(1.0 - self.pdf_view_share))
            .size(Size::exact(DIVIDER_WIDTH))
            .size(Size::remainder())
            .horizontal(|mut strip| {
                strip.cell(|ui| {
                    // `rows` borrows `self.pdfs` (or `self.hits`) while the header's menus
                    // change `self.date_order` and `self.category`. Rust allows this: they
                    // are different fields of `self`.
                    click = display_pdfs(
                        ui,
                        &rows,
                        &mut self.date_order,
                        &mut self.category,
                        &self.categories,
                        &mut self.renaming,
                    );
                });
                strip.cell(|ui| {
                    dragged = divider(ui);
                });
                strip.cell(|ui| {
                    close = pdf_view_area(ui, self.pdf_view.as_ref());
                });
            });

        // Dragging right makes the table wider, so the PDF area's share gets smaller.
        // Only when dragged: with a width of 0, `0.0 / 0.0` would give NaN ("not a number").
        if dragged != 0.0 {
            self.pdf_view_share = (self.pdf_view_share - dragged / width)
                .clamp(PDF_VIEW_SHARE_MIN, PDF_VIEW_SHARE_MAX);
        }

        if close {
            self.pdf_view = None;
        }
        // A new choice in a header menu: ask the database again, and draw the next frame
        // right away so the new rows show without waiting for the mouse to move.
        if self.date_order != old_order || self.category != old_category {
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
                self.renaming = Some(Renaming::new(path));
                return;
            }
            PdfClick::RenameCategory(path) => {
                self.renaming = Some(Renaming::of_category(path, &self.settings.output_dir));
                return;
            }
            PdfClick::RenameSubmit => {
                self.finish_rename(ctx);
                return;
            }
            PdfClick::RenameCancel => {
                self.renaming = None;
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

    /// Draws the PDF's pages and keeps them, to show right of the table.
    /// Prints the error and keeps the PDF shown so far if this one can't be read.
    fn show_pdf(&mut self, ctx: &egui::Context, path: &Path) {
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
        self.pdf_view = Some(PdfView { name, pages });
    }

    /// Renames the PDF being renamed to the typed name (see `rename_pdf.rs`). On success the
    /// table is loaded again; on failure the box stays, with the reason under it.
    fn finish_rename(&mut self, ctx: &egui::Context) {
        let Some(renaming) = &mut self.renaming else {
            return;
        };
        let old = renaming.path.clone();
        // `self.crud`, `self.settings` and `self.renaming` are different fields, so Rust
        // lets us borrow all three at once.
        // Both functions take the same arguments: pick the one for what is being edited.
        let change = if renaming.category { change_category } else { rename_pdf };
        match change(&mut self.crud, &self.settings.output_dir, &old, &renaming.text) {
            Err(error) => {
                renaming.error = error.to_string();
                renaming.focused = false; // give the box the focus again
            }
            Ok(new) => {
                self.renaming = None;
                self.reload_table();
                // The renamed PDF was shown right of the table: show it under its new name.
                if self.pdf_view.as_ref().is_some_and(|view| view.name == file_name(&old)) {
                    self.show_pdf(ctx, &new);
                }
            }
        }
    }

    fn images_tab(&mut self, ui: &mut egui::Ui) {
        self.images_top(ui);
        ui.add_space(8.0);
        self.picture_grid(ui);
    }

    /// The band above the thumbnails, always `IMAGES_TOP_HEIGHT` high, so what is below it
    /// never moves: the drop area and the Create button on the left half, the job list on
    /// the right half. The list scrolls inside its space instead of pushing things down.
    fn images_top(&mut self, ui: &mut egui::Ui) {
        let size = egui::vec2(ui.available_width(), IMAGES_TOP_HEIGHT);
        ui.allocate_ui(size, |ui| {
            ui.set_height(IMAGES_TOP_HEIGHT); // exactly this high, whatever is inside
            StripBuilder::new(ui)
                .clip(true)
                .size(Size::relative(0.5))
                .size(Size::exact(DIVIDER_WIDTH)) // a gap between the halves
                .size(Size::remainder())
                .horizontal(|mut strip| {
                    strip.cell(|ui| {
                        drop_area(
                            ui,
                            "Drop images here (HEIC, PNG, JPEG)",
                            "Let go to add the images",
                        );
                        ui.add_space(8.0);
                        self.convert_bar(ui);
                    });
                    strip.empty();
                    strip.cell(|ui| {
                        // `auto_shrink([false, false])`: the area keeps its size, even if empty.
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if self.conversions.is_empty() {
                                    ui.weak("Jobs show here while they run");
                                }
                                self.show_job_queue(ui);
                            });
                    });
                });
        });
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
        let output_dir = self.settings.output_dir.clone(); // the thread gets its own copy
        self.conversions.push(Conversion::start_pdf(pdf, output_dir, ctx.clone()));
    }

    /// Adds one file if it is an image and not in the list yet, and makes its thumbnail.
    fn add_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        let known = self.pictures.iter().any(|picture| picture.path == path);
        if !is_image(&path) || known {
            return;
        }
        let thumbnail = make_texture(ctx, &path, THUMBNAIL_PIXELS);
        self.pictures.push(Picture {
            path,
            thumbnail,
            selected: false,
        });
    }

    /// The paths of the ticked images, in list order (the order of the PDF's pages).
    fn selected_paths(&self) -> Vec<PathBuf> {
        self.pictures
            .iter()
            .filter(|picture| picture.selected)
            .map(|picture| picture.path.clone())
            .collect()
    }

    /// The Create button and where the PDF goes. Always usable once images are ticked:
    /// jobs already running don't block a new one.
    fn convert_bar(&mut self, ui: &mut egui::Ui) {
        let selected = self.selected_paths();
        let button = egui::Button::new("Create, analyze & file PDF");

        ui.horizontal(|ui| {
            let response = ui
                .add_enabled(!selected.is_empty(), button)
                .on_disabled_hover_text("Tick one or more images first");
            if response.clicked() {
                self.start_conversion(ui.ctx(), selected);
            }
            ui.weak(format!("→ {}", self.settings.output_dir.display()));
        });
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

    /// Makes a PDF from the images, analyzes and files it, all in the background.
    /// The ticks are removed at once, so the next PDF can be picked right away and the
    /// same images are not filed twice by accident.
    fn start_conversion(&mut self, ctx: &egui::Context, images: Vec<PathBuf>) {
        for picture in &mut self.pictures {
            picture.selected = false;
        }
        // `clone` gives the thread its own handle to the window; both point to the same one.
        let output_dir = self.settings.output_dir.clone(); // the thread gets its own copy
        self.conversions.push(Conversion::start_images(images, output_dir, ctx.clone()));
    }

    /// The Settings tab: the output folder, with a button to choose another one.
    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Output folder:");
            ui.monospace(self.settings.output_dir.display().to_string())
                .on_hover_text("Where analyzed PDFs are copied to; its subfolders are the categories");
        });
        ui.horizontal(|ui| {
            if ui
                .button("Locate…")
                .on_hover_text("You moved or renamed the folder yourself: point to its new place. Nothing is moved.")
                .clicked()
            {
                self.pick_output_dir();
            }
            // Not while letters are being filed: they would be copied into the old place.
            let move_button = egui::Button::new("Move archive to…");
            if ui
                .add_enabled(!self.is_filing(), move_button)
                .on_hover_text("Moves the folder with all its PDFs into an empty folder you choose")
                .on_disabled_hover_text("Wait until the letter being filed is done")
                .clicked()
            {
                self.move_archive_to();
            }
        });
        ui.label(&self.settings_status);
    }

    /// Asks for an empty folder with the macOS Open panel and moves the archive folder
    /// there (see `move_archive.rs`), then keeps that place as the output folder and
    /// rescans, to check every letter is found there. Does nothing on Cancel.
    fn move_archive_to(&mut self) {
        let Some(target) = rfd::FileDialog::new()
            .set_title("Choose an empty folder for the archive (New Folder makes one)")
            .pick_folder()
        else {
            return;
        };
        if let Err(error) = move_archive(&self.settings.output_dir, &target) {
            self.settings_status = format!("Not moved: {error}");
            return;
        }
        let place = target.display().to_string();
        self.settings.output_dir = target;
        self.settings_status = match self.settings.save(Path::new(SETTINGS_FILE)) {
            Ok(()) => format!("Moved to {place}."),
            // Without the saved setting, the next start would look in the old place.
            Err(error) => format!("Moved to {place}, but the setting was not saved: {error}"),
        };
        self.rescan(); // its result shows in the Archive tab
    }

    /// Asks for a folder with the macOS Open panel and keeps it as the output folder,
    /// saved for the next start; does nothing on Cancel. PDFs filed so far stay where they are.
    fn pick_output_dir(&mut self) {
        // The panel opens in the current output folder; it needs a full path for that.
        let current = std::path::absolute(&self.settings.output_dir).unwrap_or_default();
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Choose the output folder")
            .set_directory(current)
            .pick_folder()
        else {
            return;
        };
        self.settings.output_dir = folder;
        self.settings_status = match self.settings.save(Path::new(SETTINGS_FILE)) {
            Ok(()) => "Saved."
                .to_string(),
            Err(error) => format!("Could not save the settings: {error}"),
        };
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

    /// Shows how many images there are, then their thumbnails in rows that wrap.
    fn picture_grid(&mut self, ui: &mut egui::Ui) {
        ui.label(format!("{} images", self.pictures.len()));

        // The list can't change while we walk through it (Rust forbids changing what is
        // borrowed), so we only note the click here and act on it after the loop.
        let mut click = None;
        let count = self.pictures.len();
        // Left to right, wrapping to a new line when full. `Align::Min` lines the cards up
        // at the top; `horizontal_wrapped` would center them, which makes them drift down.
        let rows = egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.with_layout(rows, |ui| {
                // `iter_mut`: each card may change its picture (the tick box).
                for (index, picture) in self.pictures.iter_mut().enumerate() {
                    match picture_card(ui, picture, index, count) {
                        CardClick::Nothing => {}
                        action => click = Some((index, action)),
                    }
                }
            });
        });

        match click {
            Some((index, CardClick::Open)) => self.open_preview(ui.ctx(), index),
            Some((index, CardClick::Remove)) => {
                self.pictures.remove(index);
            }
            // `swap` exchanges two neighbours. The `if` guards make sure the neighbour exists.
            Some((index, CardClick::MoveEarlier)) if index > 0 => {
                self.pictures.swap(index - 1, index);
            }
            Some((index, CardClick::MoveLater)) if index + 1 < count => {
                self.pictures.swap(index, index + 1);
            }
            _ => {}
        }
    }

    /// Loads the image at `index` in a large size and shows it on top of the window.
    fn open_preview(&mut self, ctx: &egui::Context, index: usize) {
        let path = &self.pictures[index].path;
        self.preview = make_texture(ctx, path, PREVIEW_PIXELS).map(|texture| Preview {
            name: file_name(path),
            texture,
            view: egui::Rect::ZERO,
        });
    }

    /// Draws the large preview, if there is one. Closes on the Close button,
    /// a click beside it, or the Esc key.
    fn show_preview(&mut self, ctx: &egui::Context, max_size: egui::Vec2) {
        // `&mut`: zooming and dragging change `preview.view`.
        let Some(preview) = &mut self.preview else {
            return;
        };
        // A modal sits on top of the window and dims everything behind it.
        // The closure returns whether Close was clicked; we get it back as `modal.inner`.
        let modal = egui::Modal::new(egui::Id::unique("preview")).show(ctx, |ui| {
            let close = preview_header(ui, preview);
            zoomable_image(ui, preview, max_size);
            close
        });
        if modal.inner || modal.should_close() {
            self.preview = None;
        }
    }
}

/// The preview's top row: the name, a hint, and the Fit and Close buttons.
/// Returns whether Close was clicked.
fn preview_header(ui: &mut egui::Ui, preview: &mut Preview) -> bool {
    ui.horizontal(|ui| {
        ui.strong(&preview.name);
        ui.weak("Pinch or ⌘ + scroll to zoom, drag to move");
        if ui.button("Fit").clicked() {
            preview.view = egui::Rect::ZERO; // the scene fits the whole image in again
        }
        ui.button("Close").clicked()
    })
    .inner
}

/// The image inside a `Scene`, an area that can be zoomed and moved, filling `size`.
/// `preview.view` remembers the visible part, so it stays put from frame to frame.
fn zoomable_image(ui: &mut egui::Ui, preview: &mut Preview, size: egui::Vec2) {
    let scene = egui::Scene::new().zoom_range(0.0..=MAX_ZOOM);
    ui.allocate_ui(size, |ui| {
        ui.set_min_size(size);
        // The closure only reads `preview.texture` while `show` changes `preview.view`:
        // Rust allows that, because they are different fields of `preview`.
        scene.show(ui, &mut preview.view, |ui| {
            // One unit inside the scene per image pixel; the scene scales it on screen.
            let full_size = preview.texture.size_vec2();
            ui.add(egui::Image::new(&preview.texture).fit_to_exact_size(full_size));
        });
    });
}

/// Makes a picture of the file, at most `pixels` on its longer side, and hands it to egui
/// as a texture (a picture on the graphics card). Prints the error and returns `None`
/// if the file can't be read.
fn make_texture(ctx: &egui::Context, path: &Path, pixels: u32) -> Option<egui::TextureHandle> {
    let picture = match pipeline::thumbnail(path, pixels) {
        Ok(picture) => picture,
        Err(error) => {
            eprintln!("cannot show {}: {error}", path.display());
            return None;
        }
    };
    let size = [picture.width() as usize, picture.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw());
    // The name only labels the texture for debugging; it does not need to be unique.
    Some(ctx.load_texture(file_name(path), image, egui::TextureOptions::LINEAR))
}

/// One cell of the grid: the thumbnail; below it a tick box with the position and file
/// name, then buttons to move or remove the image. `index` is the card's place in the
/// list of `count`. Returns what was clicked; the tick box changes `picture` directly.
fn picture_card(ui: &mut egui::Ui, picture: &mut Picture, index: usize, count: usize) -> CardClick {
    // `.inner` is the value the closure returns; egui hands it back next to its layout info.
    // We give the card's width up front (unlike `ui.vertical`), so the wrapping row in
    // `picture_grid` can tell when a card won't fit and start a new line for it.
    let card_size = egui::vec2(THUMBNAIL_POINTS, THUMBNAIL_POINTS);
    let top_down = egui::Layout::top_down(egui::Align::Min);
    ui.allocate_ui_with_layout(card_size, top_down, |ui| {
        ui.set_width(THUMBNAIL_POINTS);
        let opened = match &picture.thumbnail {
            Some(texture) => thumbnail_button(ui, texture).clicked(),
            None => {
                let square = egui::vec2(THUMBNAIL_POINTS, THUMBNAIL_POINTS);
                ui.add_sized(square, egui::Label::new("no preview"));
                false
            }
        };
        ui.horizontal(|ui| {
            ui.checkbox(&mut picture.selected, "")
                .on_hover_text("Put in the PDF");
            // `truncate` cuts long names with "…" instead of widening the cell.
            let name = format!("{}. {}", index + 1, file_name(&picture.path));
            ui.add(egui::Label::new(name).truncate());
        });
        let button = ui.horizontal(|ui| card_buttons(ui, index, count)).inner;

        if opened { CardClick::Open } else { button }
    })
    .inner
}

/// The ←, → and × buttons of a card. The arrows are greyed out at the ends of the list.
fn card_buttons(ui: &mut egui::Ui, index: usize, count: usize) -> CardClick {
    let earlier = egui::Button::new("←").small();
    let later = egui::Button::new("→").small();
    if ui
        .add_enabled(index > 0, earlier)
        .on_hover_text("Move earlier")
        .clicked()
    {
        return CardClick::MoveEarlier;
    }
    if ui
        .add_enabled(index + 1 < count, later)
        .on_hover_text("Move later")
        .clicked()
    {
        return CardClick::MoveLater;
    }
    if ui.small_button("×").on_hover_text("Remove").clicked() {
        return CardClick::Remove;
    }
    CardClick::Nothing
}

/// The thumbnail as a clickable picture, shrunk to fit the card while keeping its proportions.
fn thumbnail_button(ui: &mut egui::Ui, texture: &egui::TextureHandle) -> egui::Response {
    let square = egui::vec2(THUMBNAIL_POINTS, THUMBNAIL_POINTS);
    let image = egui::Image::new(texture)
        .max_size(square)
        .sense(egui::Sense::click());
    ui.add(image)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
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
        click = match renaming.as_mut().filter(|renaming| renaming.is_for(&pdf.path, false)) {
            Some(renaming) => rename_cell(ui, renaming),
            None => file_link(ui, &pdf.path),
        };
    });
    row.col(|ui| {
        // The same for the category: a box in the row being edited, else the category itself.
        let cell_click = match renaming.as_mut().filter(|renaming| renaming.is_for(&pdf.path, true)) {
            Some(renaming) => rename_cell(ui, renaming),
            None => category_label(ui, pdf),
        };
        // Only a real click is kept, so this cell cannot erase the file name's click.
        if cell_click.is_some() {
            click = cell_click;
        }
    });
    row.col(|ui| {
        ui.label(pdf.letter_date.unwrap_or("-"));
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

fn rename_cell(ui: &mut egui::Ui, renaming: &mut Renaming) -> Option<PdfClick> {
    // The text box that replaces the file name or the category while it is edited (".pdf"
    // follows a file name), with the reason under it if the last try failed. Enter saves,
    // Esc or a click elsewhere cancels.
    let mut click = None;
    ui.horizontal(|ui| {
        // ".pdf" only follows a file name, not a category.
        let suffix_width = if renaming.category { 0.0 } else { RENAME_SUFFIX_WIDTH };
        let width = (ui.available_width() - suffix_width).max(0.0);
        let name = ui.add(egui::TextEdit::singleline(&mut renaming.text).desired_width(width));
        if !renaming.focused {
            name.request_focus(); // so the user can type at once
            renaming.focused = true;
        }
        if !renaming.category {
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
fn pdf_view_area(ui: &mut egui::Ui, view: Option<&PdfView>) -> bool {
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
fn divider(ui: &mut egui::Ui) -> f32 {
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
/// A box asking for files with `text`; while files are dragged over the window it
/// lights up and shows `hover_text`.
fn drop_area(ui: &mut egui::Ui, text: &str, hover_text: &str) {
    let hovering = ui.ctx().input(|input| !input.raw.hovered_files.is_empty());
    let (text, fill) = if hovering {
        (hover_text, ui.visuals().selection.bg_fill)
    } else {
        (text, egui::Color32::TRANSPARENT)
    };

    egui::Frame::group(ui.style()).fill(fill).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_height(DROP_AREA_HEIGHT);
        ui.centered_and_justified(|ui| ui.label(text));
    });
}

/// True if the file ends in one of `IMAGE_EXTENSIONS`, in any case (".HEIC" too).
fn is_image(path: &Path) -> bool {
    // `extension` is an `Option`: a file may have no ending. `is_some_and` is false then.
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}

/// The last part of a path, e.g. "IMG_9549.HEIC".
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
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
