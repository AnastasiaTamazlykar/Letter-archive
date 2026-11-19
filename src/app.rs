//! The app window: drop images on it to see them as thumbnails; click one to see it large;
//! put them in order, tick some and file them as one analyzed PDF.

use crate::open_file::{open_in_viewer, reveal_in_finder};
use crate::pdf_crud::{Document, PdfCrud, SearchHit};
use crate::pdf_pages::render_pages;
use crate::paths::{DB_FILE, OUTPUT_DIR};
use crate::{convert::Conversion, pipeline};
use eframe::egui;
use egui_extras::{Column, Size, StripBuilder, TableBuilder, TableRow};
use std::error::Error;
use std::path::{Path, PathBuf};

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

enum PdfClick {
    /// A left click: show it in the area right of the table.
    Show(String),
    /// "Open" in the right-click menu: open it in the PDF app.
    Open(String),
    /// "Show in Finder" in the right-click menu.
    Reveal(String),
}

/// The pages of the window, picked with the buttons at the top.
/// `PartialEq` lets egui compare tabs, to know which button to show as chosen.
/// `#[default]` marks the tab `Default` gives: the pdf_crud is shown first.
#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Archive,
    Images,
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
    /// The conversion running in the background, if any.
    conversion: Option<Conversion>,
    /// What the last conversion did, shown next to the buttons that start one.
    status: String,
    search: String,
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
            conversion: None,
            status: "".to_string(),
            search: "".to_string(),
        }
    }

    fn load_pdfs(&mut self, limit: i64) -> Result<Vec<Document>, Box<dyn Error>> {
        Ok(self.crud.find_all(limit)?)
    }

    pub fn set_pdfs(&mut self) {
        match self.load_pdfs(50) {
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
            // One letter at a time: greyed out while a conversion runs.
            let button = egui::Button::new("Analyze & file PDF…");
            if ui
                .add_enabled(self.conversion.is_none(), button)
                .on_hover_text(format!("Pick a PDF; a copy goes to {OUTPUT_DIR}"))
                .clicked()
            {
                self.pick_pdf(ui.ctx());
            }
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
                            .hint_text("Search… enter at least 3 symbols"),
                    )
                })
                .inner;
            if search_box.lost_focus() {
                self.hits = self.crud.search(self.search.as_str()).unwrap();
            }
            //  once search fields is cleared, results are also cleared
            if self.search.is_empty() {
                self.hits = vec![];
            }
            self.conversion_status(ui);
        });
        let rows: Vec<PdfRow> = if !self.search.is_empty() {
            self.hits.iter().map(PdfRow::from).collect()
        } else {
            self.pdfs.iter().map(PdfRow::from).collect()
        };

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
                    click = display_pdfs(ui, &rows);
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
        if let Some(click) = click {
            self.act_on_pdf_click(ui.ctx(), click);
        }
    }


    /// Acts on a click on a file name: shows the PDF right of the table, opens it in the
    /// PDF app, or shows it in Finder; prints why if that fails.
    fn act_on_pdf_click(&mut self, ctx: &egui::Context, click: PdfClick) {
        let (path, result) = match click {
            PdfClick::Show(path) => {
                self.show_pdf(ctx, Path::new(&path));
                return;
            }
            PdfClick::Open(path) => {
                let result = open_in_viewer(Path::new(&path));
                (path, result)
            }
            PdfClick::Reveal(path) => {
                let result = reveal_in_finder(Path::new(&path));
                (path, result)
            }
        };
        if let Err(error) = result {
            eprintln!("cannot open {path}: {error}");
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

    /// Asks for a PDF with the macOS Open panel, then analyzes and files it in the
    /// background; does nothing on Cancel.
    fn pick_pdf(&mut self, ctx: &egui::Context) {
        let Some(pdf) = rfd::FileDialog::new()
            .set_title("Analyze & file PDF")
            .add_filter("PDF", &["pdf"])
            .pick_file()
        else {
            return;
        };
        self.status.clear();
        self.conversion = Some(Conversion::start_pdf(pdf, ctx.clone()));
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

    /// The Create button and where the PDF goes, with the progress or result next to it.
    fn convert_bar(&mut self, ui: &mut egui::Ui) {
        let selected = self.selected_paths();
        let ready = !selected.is_empty() && self.conversion.is_none();
        let button = egui::Button::new("Create, analyze & file PDF");

        ui.horizontal(|ui| {
            let response = ui
                .add_enabled(ready, button)
                .on_disabled_hover_text("Tick one or more images first");
            if response.clicked() {
                self.start_conversion(ui.ctx(), selected);
            }
            ui.weak(format!("→ {OUTPUT_DIR}"));
            self.conversion_status(ui);
        });
    }

    /// A spinner and what the running conversion does, or else what the last one did.
    fn conversion_status(&self, ui: &mut egui::Ui) {
        match &self.conversion {
            Some(conversion) => {
                ui.spinner();
                ui.label(conversion.progress());
            }
            None => {
                ui.label(&self.status);
            }
        }
    }

    /// Makes a PDF from the images, analyzes and files it, all in the background.
    fn start_conversion(&mut self, ctx: &egui::Context, images: Vec<PathBuf>) {
        self.status.clear();
        // `clone` gives the thread its own handle to the window; both point to the same one.
        self.conversion = Some(Conversion::start_images(images, ctx.clone()));
    }

    /// Collects news from the running conversion; when it is done, keeps its summary.
    fn check_conversion(&mut self) {
        let Some(conversion) = &mut self.conversion else {
            return;
        };
        conversion.receive();
        if conversion.is_finished() {
            self.status = conversion.summary();
            let filed = conversion.succeeded();
            self.conversion = None;
            // Untick all images, so the same ones aren't filed twice by accident.
            for picture in &mut self.pictures {
                picture.selected = false;
            }
            if filed {
                self.set_pdfs(); // show the new letter in the Archive tab
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
fn display_pdfs(ui: &mut egui::Ui, rows: &[PdfRow]) -> Option<PdfClick> {
    // The row closures below assign to `click`; they borrow it mutably one after another.
    let mut click = None;
    TableBuilder::new(ui)
        .striped(true)
        .column(Column::initial(120.0).resizable(true).clip(true))
        .column(Column::initial(120.0).resizable(true).clip(true))
        .column(Column::initial(100.0).resizable(true).clip(true))
        // The last column takes the rest of the width, so no empty column is left after it.
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
                ui.strong("Letter date");
            });
            header.col(|ui| {
                ui.strong("Summary");
            });
        })
        .body(|mut body| {
            for pdf in rows {
                body.row(PDF_ROW_HEIGHT, |row| {
                    // Only a real click (`Some`) is kept, so a later row cannot erase it.
                    if let Some(row_click) = pdf_row(row, pdf) {
                        click = Some(row_click);
                    }
                });
            }
        });
    click
}

fn pdf_row(mut row: TableRow<'_, '_>, pdf: &PdfRow) -> Option<PdfClick> {
    let mut click = None;
    row.col(|ui| {
        click = file_link(ui, pdf.file_path);
    });
    row.col(|ui| {
        ui.label(pdf.category);
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

fn file_link(ui: &mut egui::Ui, file_path: &str) -> Option<PdfClick> {
    let name = file_name(Path::new(file_path));
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
        .on_hover_text(file_path);

    let mut click = None;
    if link.clicked() {
        click = Some(PdfClick::Show(file_path.to_string()));
    }
    // `context_menu` shows the closure's buttons in a popup after a right click.
    link.context_menu(|ui| {
        if ui.button("Open").clicked() {
            click = Some(PdfClick::Open(file_path.to_string()));
            ui.close();
        }
        if ui.button("Show in Finder").clicked() {
            click = Some(PdfClick::Reveal(file_path.to_string()));
            ui.close();
        }
    });
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

struct PdfRow<'a> {
    category: &'a str,
    summary: &'a str,
    letter_date: Option<&'a str>,
    file_path: &'a str,
}

impl<'a> From<&'a Document> for PdfRow<'a> {
    fn from(pdf: &'a Document) -> Self {
        PdfRow {
            category: &pdf.category,
            summary: &pdf.summary,
            letter_date: pdf.letter_date.as_deref(),
            file_path: &pdf.file_path,
        }
    }
}

impl<'a> From<&'a SearchHit> for PdfRow<'a> {
    fn from(hit: &'a SearchHit) -> Self {
        PdfRow {
            category: &hit.category,
            summary: &hit.summary,
            letter_date: hit.letter_date.as_deref(),
            file_path: &hit.file_path,
        }
    }
}
