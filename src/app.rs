//! The app window: drop images on it to see them as thumbnails; click one to see it large;
//! put them in order, tick some and save them as one PDF.

use crate::pdf_crud::{Document, PdfCrud};
use crate::{convert::Conversion, pipeline};
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use std::error::Error;
use std::path::{Path, PathBuf};

/// File endings the pipeline can read, in lowercase.
const IMAGE_EXTENSIONS: [&str; 4] = ["heic", "png", "jpg", "jpeg"];
/// The letter archive's database file.
const DB_FILE: &str = "data/archive.db";
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
/// `#[default]` marks the tab `Default` gives: the archive is shown first.
#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Archive,
    Images,
}

/// Everything the window remembers between frames.
#[derive(Default)]
pub struct App {
    /// The page shown now.
    tab: Tab,
    pdfs: Vec<Document>,
    /// The images added so far. Their order here is the page order in the PDF.
    pictures: Vec<Picture>,
    /// The image shown large, if one was clicked.
    preview: Option<Preview>,
    /// The conversion running in the background, if any.
    conversion: Option<Conversion>,
    /// What the last conversion did, shown next to the Convert button.
    status: String,
}

impl eframe::App for App {
    /// Called for every frame: egui redraws the whole window from the app's data
    /// ("immediate mode"), so there are no callbacks or widget objects to keep.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let preview_size = ui.available_size() * PREVIEW_SHARE; // before the panel fills it
        self.take_dropped_files(ui.ctx());
        self.check_conversion();

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Paper Archive");
                // `selectable_value` sets `self.tab` to the given tab when clicked.
                ui.selectable_value(&mut self.tab, Tab::Archive, "Archive");
                ui.selectable_value(&mut self.tab, Tab::Images, "Images → PDF");
            });
            ui.separator();
            match self.tab {
                Tab::Archive => self.archive_tab(ui),
                Tab::Images => self.images_tab(ui),
            }
        });
        self.show_preview(ui.ctx(), preview_size);
    }
}

// A second `impl` block: the app's own methods, apart from the ones `eframe::App` requires.
impl App {
    fn load_pdfs(limit: i64) -> Result<Vec<Document>, Box<dyn Error>> {
        let mut pdf_crud = PdfCrud::open(Path::new(DB_FILE))?;
        Ok(pdf_crud.find_all(limit)?)
    }

    pub fn set_pdfs(&mut self) {
        match Self::load_pdfs(50) {
            Ok(pdfs) => {
                self.pdfs = pdfs;
            }
            Err(err) => eprintln!("Pdfs were not loaded {}", err),
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
        ui.horizontal(|ui| {
            ui.label("Processed PDFs");
            if ui
                .button("+")
                .on_hover_text("Add a PDF to the archive")
                .clicked()
            {
                self.pick_pdf();
            }
        });
        display_pdfs(ui, &self.pdfs);
    }

    /// The images page: drop images, tick them and save them as one PDF.
    fn images_tab(&mut self, ui: &mut egui::Ui) {
        drop_area(
            ui,
            "Drop images here (HEIC, PNG, JPEG)",
            "Let go to add the images",
        );
        ui.add_space(8.0);
        self.convert_bar(ui);
        ui.add_space(8.0);
        self.picture_grid(ui);
    }

    /// Asks for a PDF with the macOS Open panel and stores it; does nothing on Cancel.
    fn pick_pdf(&mut self) {
        let Some(pdf) = rfd::FileDialog::new()
            .set_title("Add PDF to archive")
            .add_filter("PDF", &["pdf"])
            .pick_file()
        else {
            return;
        };
        // `&[pdf]`: a list of one, borrowed as a slice, which is what `store_pdfs` takes.
        self.store_pdfs(&[pdf]);
    }

    /// Stores the files in the archive, then reloads the list of PDFs.
    /// A file stored before is skipped by `add_pdf`; failures (not a PDF, no text) are printed.
    fn store_pdfs(&mut self, pdfs: &[PathBuf]) {
        let mut archive = match PdfCrud::open(Path::new(DB_FILE)) {
            Ok(archive) => archive,
            Err(error) => {
                eprintln!("cannot open the archive: {error}");
                return;
            }
        };
        for pdf in pdfs {
            if let Err(error) = archive.add_pdf(pdf) {
                eprintln!("cannot store {error}"); // the error already names the file
            }
        }
        // `archive` is only a local variable, so `self` is free to be borrowed again here.
        self.set_pdfs();
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

    /// The Convert button, with the conversion's progress or result next to it.
    fn convert_bar(&mut self, ui: &mut egui::Ui) {
        let selected = self.selected_paths();
        let ready = !selected.is_empty() && self.conversion.is_none();
        let button = egui::Button::new(format!("Save {} as PDF…", selected.len()));

        ui.horizontal(|ui| {
            let response = ui
                .add_enabled(ready, button)
                .on_disabled_hover_text("Tick one or more images first");
            if response.clicked() {
                self.start_conversion(ui.ctx(), selected);
            }
            match &self.conversion {
                Some(conversion) => {
                    ui.spinner();
                    ui.label(conversion.progress());
                }
                None => {
                    ui.label(&self.status);
                }
            }
        });
    }

    /// Asks where to save the PDF, then makes it from the images in the background.
    fn start_conversion(&mut self, ctx: &egui::Context, images: Vec<PathBuf>) {
        // Opens the macOS "Save As" panel and waits for the user; `None` if they cancel.
        // The panel itself asks before replacing a file that already exists.
        let Some(pdf) = rfd::FileDialog::new()
            .set_title("Save as PDF")
            .set_file_name(suggested_name(&images))
            .add_filter("PDF", &["pdf"])
            .save_file()
        else {
            return;
        };
        self.status.clear();
        // `clone` gives the thread its own handle to the window; both point to the same one.
        self.conversion = Some(Conversion::start(images, with_pdf_ending(pdf), ctx.clone()));
    }

    /// Collects news from the running conversion; when it is done, keeps its summary.
    fn check_conversion(&mut self) {
        let Some(conversion) = &mut self.conversion else {
            return;
        };
        conversion.receive();
        if conversion.is_finished() {
            self.status = conversion.summary();
            self.conversion = None;
            // Untick all images, so the button goes back to "Save 0 as PDF…".
            for picture in &mut self.pictures {
                picture.selected = false;
            }
        }
    }

    /// Shows how many images there are, then their thumbnails in rows that wrap.
    fn picture_grid(&mut self, ui: &mut egui::Ui) {
        ui.label(format!("{} images", self.pictures.len()));

        // The list can't change while we walk through it (Rust forbids changing what is
        // borrowed), so we only note the click here and act on it after the loop.
        let mut click = None;
        let count = self.pictures.len();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
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
    ui.vertical(|ui| {
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

/// The stored letters as a table that fits the window's width: the file name, the
/// category and the summary. Hovering the name shows the path; the summary wraps.
fn display_pdfs(ui: &mut egui::Ui, pdfs: &[Document]) {
    // Unlike `Grid`, which grows with its longest text, `TableBuilder` gives each column
    // a width, and `clip` cuts what doesn't fit. Rows scroll inside the table.
    TableBuilder::new(ui)
        .striped(true)
        .column(Column::initial(120.0).resizable(true).clip(true))
        .column(Column::initial(120.0).resizable(true).clip(true))
        .column(Column::remainder().clip(true))
        .header(PDF_ROW_HEIGHT, |mut header| {
            // Each `col` closure draws one cell; `;` makes it return `()`, as `col` wants.
            header.col(|ui| {
                ui.strong("File");
            });
            header.col(|ui| {
                ui.strong("Category");
            });
            header.col(|ui| {
                ui.strong("Summary");
            });
        })
        .body(|mut body| {
            for pdf in pdfs {
                body.row(PDF_ROW_HEIGHT, |mut row| {
                    row.col(|ui| {
                        let name = file_name(Path::new(&pdf.file_path));
                        ui.add(egui::Label::new(name).truncate())
                            .on_hover_text(&pdf.file_path);
                    });
                    row.col(|ui| {
                        ui.label(&pdf.category);
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(&pdf.summary).wrap())
                            .on_hover_text(&pdf.summary);
                    });
                });
            }
        });
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

/// The name the Save panel suggests: the first image's name, e.g. "IMG_9549.pdf".
fn suggested_name(images: &[PathBuf]) -> String {
    let stem = images
        .first()
        .and_then(|image| image.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Letter".to_string());
    format!("{stem}.pdf")
}

/// Adds ".pdf" if the user typed a name without an ending.
fn with_pdf_ending(mut pdf: PathBuf) -> PathBuf {
    if pdf.extension().is_none() {
        pdf.set_extension("pdf");
    }
    pdf
}

/// The last part of a path, e.g. "IMG_9549.HEIC".
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
