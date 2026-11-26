mod app;
mod jobs;
mod process_document;
mod language;
mod file_system_actions;
mod open_file;
mod constants;
mod document_crud;
mod images_to_pdf;
mod rescan;
mod config;

mod summarize_with_ai;
// Helpers for the tests of several modules; `cfg(test)`: left out of the app itself.
#[cfg(test)]
mod test_support;

/// Size of the window when the app starts, in points (width, height).
const WINDOW_SIZE: [f32; 2] = [1400.0, 900.0];

fn main() -> eframe::Result {
    // `..Default::default()` fills in every other option with its default value.
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size(WINDOW_SIZE),
        ..Default::default()
    };
    let mut app = app::App::new();
    app.rescan(); // finds moved PDFs, removes letters whose PDF is gone, loads the table
    eframe::run_native(
        "Paper Archive",
        options,
        // A closure (`|_cc| ...`) that builds the app once the window exists.
        // `Box` puts the app on the heap; eframe only needs to know it implements `App`.
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
