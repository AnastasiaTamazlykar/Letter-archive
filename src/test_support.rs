//! Helpers shared by the tests of several modules. Built only for `cargo test`
//! (see `#[cfg(test)]` in main.rs).

use std::{
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
};

use printpdf::{Mm, PdfDocument as NewPdf};

use crate::pipeline::FONT;

/// An empty folder of its own for one test, so tests running at the same time
/// don't touch each other's files.
pub fn test_folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("paper_archive_test_{name}"));
    let _ = std::fs::remove_dir_all(&folder); // left over from an earlier run
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

/// Writes a one-page PDF with these lines of real text, top to bottom.
pub fn write_pdf(path: &Path, lines: &[&str]) {
    let (doc, page, layer) = NewPdf::new("test", Mm(210.0), Mm(297.0), "text");
    let font_data = std::fs::read(FONT).unwrap();
    let font = doc.add_external_font(font_data.as_slice()).unwrap();
    let layer = doc.get_page(page).get_layer(layer);
    for (number, line) in lines.iter().enumerate() {
        let y = Mm(270.0 - 10.0 * number as f32);
        layer.use_text(*line, 12.0, Mm(20.0), y, &font);
    }
    doc.save(&mut BufWriter::new(File::create(path).unwrap()))
        .unwrap();
}
