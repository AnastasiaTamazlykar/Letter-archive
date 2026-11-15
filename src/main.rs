//! Starts the app window. The photo → PDF work lives in `pipeline.rs`.

mod app;
// The window only lists and adds letters so far; search, `new` and `document` are
// used by the tests only. `allow` silences their "never used" warnings until it is.
mod convert;
mod language;
#[allow(dead_code)]
mod pdf_crud;
mod pipeline;
mod schema;

mod summarize_with_ai;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    let mut app = app::App::new();
    app.set_pdfs();
    eframe::run_native(
        "Paper Archive",
        options,
        // A closure (`|_cc| ...`) that builds the app once the window exists.
        // `Box` puts the app on the heap; eframe only needs to know it implements `App`.
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
