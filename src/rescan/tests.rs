//! Tests for the rescan. The letters are stored with a made-up LLM answer, so no LLM
//! is needed; each test moves, renames or deletes PDFs by hand, like the user in Finder.
//! The database keeps each PDF's path inside the output folder, e.g. "Finanzamt/Brief.pdf".

use std::fs;

use super::*;
use crate::{
    summarize_with_ai::LLMAnswer,
    test_support::{test_folder, write_pdf},
};

/// A database in this test's folder and an output folder next to it.
fn setup(name: &str) -> (PdfCrud, PathBuf) {
    let folder = test_folder(name);
    let output = folder.join("output");
    fs::create_dir_all(&output).unwrap();
    (PdfCrud::new(&folder.join("archive.db")).unwrap(), output)
}

/// Writes a PDF with `line` as its text and stores it as a letter, with its path inside
/// `output` (where `pdf` must lie), the way `analyze_place_pdf` does; its id.
fn store(crud: &mut PdfCrud, output: &Path, pdf: &Path, line: &str) -> i64 {
    write_pdf(pdf, &[line]);
    let answer = LLMAnswer {
        folder: None,
        summary: "Ein Brief.".to_string(),
        letter_date: String::new(),
        file_name: "Brief".to_string(),
    };
    let hash = pdf_crud::file_sha256(pdf).unwrap();
    let relative = pdf.strip_prefix(output).unwrap();
    crud.insert_letter(relative, &hash, line, &answer).unwrap()
}

/// Where the database says the letter's PDF is, inside the output folder.
fn stored_path(crud: &mut PdfCrud, id: i64) -> PathBuf {
    PathBuf::from(crud.document(id).unwrap().unwrap().file_path)
}

/// A second letter that stays where it is. The rescan refuses to change anything when not
/// a single letter is in place (it suspects the wrong folder), so the tests that move
/// a PDF need one.
fn store_one_in_place(crud: &mut PdfCrud, output: &Path) {
    store(crud, output, &output.join("Kept.pdf"), "Rundfunkbeitrag");
}

#[test]
fn test_rescan_finds_a_moved_file() {
    let (mut crud, output) = setup("rescan_moved");
    store_one_in_place(&mut crud, &output);
    let old = output.join("Brief.pdf");
    let id = store(&mut crud, &output, &old, "Finanzamt Berlin");
    fs::create_dir_all(output.join("Finanzamt")).unwrap();
    fs::rename(&old, output.join("Finanzamt/Brief.pdf")).unwrap();

    let report = rescan(&mut crud, &output).unwrap();

    assert_eq!(report, Rescan { moved: 1, removed: vec![] });
    assert_eq!(stored_path(&mut crud, id), Path::new("Finanzamt/Brief.pdf"));
    // The folder it is in now is its category.
    assert_eq!(crud.document(id).unwrap().unwrap().category, "Finanzamt");
}

#[test]
fn test_rescan_finds_a_renamed_file() {
    let (mut crud, output) = setup("rescan_renamed");
    store_one_in_place(&mut crud, &output);
    let old = output.join("Brief.pdf");
    let id = store(&mut crud, &output, &old, "Finanzamt Berlin");
    fs::rename(&old, output.join("Steuerbescheid 2025.pdf")).unwrap();

    rescan(&mut crud, &output).unwrap();

    assert_eq!(stored_path(&mut crud, id), Path::new("Steuerbescheid 2025.pdf"));
}

#[test]
fn test_rescan_leaves_files_in_place_alone() {
    let (mut crud, output) = setup("rescan_in_place");
    let id = store(&mut crud, &output, &output.join("Brief.pdf"), "Finanzamt Berlin");

    let report = rescan(&mut crud, &output).unwrap();

    assert_eq!(report, Rescan::default());
    assert_eq!(report.summary(), "Rescan: all files in place");
    assert_eq!(stored_path(&mut crud, id), Path::new("Brief.pdf"));
}

#[test]
fn test_rescan_removes_a_letter_whose_file_is_gone() {
    let (mut crud, output) = setup("rescan_gone");
    let gone = output.join("Gone.pdf");
    let gone_id = store(&mut crud, &output, &gone, "Rundfunkbeitrag");
    let kept_id = store(&mut crud, &output, &output.join("Kept.pdf"), "Finanzamt Berlin");
    fs::remove_file(&gone).unwrap();

    let report = rescan(&mut crud, &output).unwrap();

    assert_eq!(report.removed, ["Gone.pdf"]);
    assert_eq!(report.summary(), "Rescan: 0 moved, 1 removed: Gone.pdf");
    assert!(crud.document(gone_id).unwrap().is_none());
    assert!(crud.document(kept_id).unwrap().is_some());
}

/// A wrong or missing output folder must never empty the database.
#[test]
fn test_rescan_without_output_folder_fails_and_deletes_nothing() {
    let (mut crud, output) = setup("rescan_no_output");
    let id = store(&mut crud, &output, &output.join("Brief.pdf"), "Finanzamt Berlin");
    fs::remove_dir_all(&output).unwrap();

    let error = rescan(&mut crud, &output).unwrap_err();

    assert!(error.to_string().contains("not found"), "got: {error}");
    assert!(crud.document(id).unwrap().is_some());
}
