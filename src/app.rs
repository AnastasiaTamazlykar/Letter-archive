//! The app window: drop images on it to see them as thumbnails; click one to see it large;
//! put them in order, tick some and file them as one analyzed PDF.

use crate::pdf_crud::{DateOrder, Document, PdfCrud, SearchHit};
use crate::paths::{DB_FILE, SETTINGS_FILE};
use crate::archive_folder_manipulations::move_archive::move_archive;
use crate::settings::Settings;
use crate::{convert::Conversion, pipeline};
use eframe::egui;
use egui_extras::{Size, StripBuilder};
use std::path::{Path, PathBuf};

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

// The tests live in `src/app/tests.rs`.
#[cfg(test)]
mod tests;

/// File endings the pipeline can read, in lowercase.
const IMAGE_EXTENSIONS: [&str; 4] = ["heic", "png", "jpg", "jpeg"];
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
/// Width of the draggable divider between the table and the PDF area, in points.
const DIVIDER_WIDTH: f32 = 8.0;
/// Height of the band above the thumbnails in the Images tab, in points: the drop area and
/// the Create button next to the job list. Fixed, so the thumbnails below never move.
const IMAGES_TOP_HEIGHT: f32 = 140.0;

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
