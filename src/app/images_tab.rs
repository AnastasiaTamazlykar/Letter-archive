//! The Images → PDF tab: drop images to see them as thumbnails; click one to see it large;
//! put them in order, tick some and file them as one analyzed PDF.
//! A child module of `app`: its `impl App` block may use `App`'s private fields.
//! `pub(super)` marks what `app.rs` (and the tests under it) may use from here.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui_extras::{Size, StripBuilder};

use super::{App, DIVIDER_WIDTH, file_name};
use crate::{jobs::Job, images_to_pdf};

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
pub(super) const PREVIEW_SHARE: f32 = 0.9;
/// How far the preview can be zoomed in: 2.0 shows each image pixel as 2 × 2 points.
const MAX_ZOOM: f32 = 2.0;
/// Height of the band above the thumbnails in the Images tab, in points: the drop area and
/// the Create button next to the job list. Fixed, so the thumbnails below never move.
pub(super) const IMAGES_TOP_HEIGHT: f32 = 140.0;

/// What the Images tab remembers between frames; it is `App.images`.
/// `Default`: no images and no preview at the start.
#[derive(Default)]
pub(super) struct ImagesTab {
    /// The images added so far. Their order here is the page order in the PDF.
    pub(super) pictures: Vec<Picture>,
    /// The image shown large, if one was clicked.
    preview: Option<Preview>,
}

/// One added image and its thumbnail.
pub(super) struct Picture {
    pub(super) path: PathBuf,
    /// The thumbnail, uploaded to the graphics card; `None` if the file could not be read.
    pub(super) thumbnail: Option<egui::TextureHandle>,
    /// Ticked for converting to PDF.
    pub(super) selected: bool,
}

/// The image shown large on top of the window.
pub(super) struct Preview {
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

// The Images tab's part of `App`; the rest of its methods are in `app.rs` and `archive_tab.rs`.
impl App {
    /// Adds the files dropped on the window in this frame.
    pub(super) fn take_dropped_files(&mut self, ctx: &egui::Context) {
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

    pub(super) fn images_tab(&mut self, ui: &mut egui::Ui) {
        self.images_top(ui);
        ui.add_space(8.0);
        self.picture_grid(ui);
    }

    /// The band above the thumbnails, always `IMAGES_TOP_HEIGHT` high, so what is below it
    /// never moves: the drop area and the Create button on the left half, the job list on
    /// the right half. The list scrolls inside its space instead of pushing things down.
    pub(super) fn images_top(&mut self, ui: &mut egui::Ui) {
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
                                if self.jobs.is_empty() {
                                    ui.weak("Jobs show here while they run");
                                }
                                self.jobs.show(ui);
                            });
                    });
                });
        });
    }

    /// Adds one file if it is an image and not in the list yet, and makes its thumbnail.
    fn add_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        let known = self.images.pictures.iter().any(|picture| picture.path == path);
        if !is_image(&path) || known {
            return;
        }
        let thumbnail = make_texture(ctx, &path, THUMBNAIL_PIXELS);
        self.images.pictures.push(Picture {
            path,
            thumbnail,
            selected: false,
        });
    }

    /// The paths of the ticked images, in list order (the order of the PDF's pages).
    pub(super) fn selected_paths(&self) -> Vec<PathBuf> {
        self.images.pictures
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
                self.start_jobs(ui.ctx(), selected);
            }
            ui.weak(format!("→ {}", self.config.output_dir.display()));
        });
    }

    /// Makes a PDF from the images, analyzes and files it, all in the background.
    /// The ticks are removed at once, so the next PDF can be picked right away and the
    /// same images are not filed twice by accident.
    pub(super) fn start_jobs(&mut self, ctx: &egui::Context, images: Vec<PathBuf>) {
        for picture in &mut self.images.pictures {
            picture.selected = false;
        }
        // `clone` gives the thread its own handle to the window; both point to the same one.
        let output_dir = self.config.output_dir.clone(); // the thread gets its own copy
        self.jobs.add(Job::start_images(images, output_dir, ctx.clone()));
    }

    /// Shows how many images there are, then their thumbnails in rows that wrap.
    fn picture_grid(&mut self, ui: &mut egui::Ui) {
        ui.label(format!("{} images", self.images.pictures.len()));

        // The list can't change while we walk through it (Rust forbids changing what is
        // borrowed), so we only note the click here and act on it after the loop.
        let mut click = None;
        let count = self.images.pictures.len();
        // Left to right, wrapping to a new line when full. `Align::Min` lines the cards up
        // at the top; `horizontal_wrapped` would center them, which makes them drift down.
        let rows = egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.with_layout(rows, |ui| {
                // `iter_mut`: each card may change its picture (the tick box).
                for (index, picture) in self.images.pictures.iter_mut().enumerate() {
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
                self.images.pictures.remove(index);
            }
            // `swap` exchanges two neighbours. The `if` guards make sure the neighbour exists.
            Some((index, CardClick::MoveEarlier)) if index > 0 => {
                self.images.pictures.swap(index - 1, index);
            }
            Some((index, CardClick::MoveLater)) if index + 1 < count => {
                self.images.pictures.swap(index, index + 1);
            }
            _ => {}
        }
    }

    /// Loads the image at `index` in a large size and shows it on top of the window.
    fn open_preview(&mut self, ctx: &egui::Context, index: usize) {
        let path = &self.images.pictures[index].path;
        self.images.preview = make_texture(ctx, path, PREVIEW_PIXELS).map(|texture| Preview {
            name: file_name(path),
            texture,
            view: egui::Rect::ZERO,
        });
    }

    /// Draws the large preview, if there is one. Closes on the Close button,
    /// a click beside it, or the Esc key.
    pub(super) fn show_preview(&mut self, ctx: &egui::Context, max_size: egui::Vec2) {
        // `&mut`: zooming and dragging change `preview.view`.
        let Some(preview) = &mut self.images.preview else {
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
            self.images.preview = None;
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
    let picture = match images_to_pdf::thumbnail(path, pixels) {
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
