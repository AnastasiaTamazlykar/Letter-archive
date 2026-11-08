//! Starts the app window. The photo → PDF work lives in `pipeline.rs`.

mod app;
// Not called from the window yet, only from its tests; `allow` silences the
// "never used" warnings until it is.
#[allow(dead_code)]
mod archive;
mod convert;
mod language;
mod pipeline;
mod schema;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    let mut app = app::App::default();
    app.set_pdfs();
    eframe::run_native(
        "Paper Archive",
        options,
        // A closure (`|_cc| ...`) that builds the app once the window exists.
        // `Box` puts the app on the heap; eframe only needs to know it implements `App`.
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
