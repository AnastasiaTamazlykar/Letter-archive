//! Tests for the rescan. The letters are stored with a made-up LLM answer, so no LLM
//! is needed; each test moves, renames or deletes PDFs by hand, like the user in Finder.

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

/// Writes a PDF with `line` as its text and stores it as a letter; its id.
fn store(crud: &mut PdfCrud, pdf: &Path, line: &str) -> i64 {
    write_pdf(pdf, &[line]);
    let answer = LLMAnswer {
        folder: None,
        summary: "Ein Brief.".to_string(),
        letter_date: String::new(),
        file_name: "Brief".to_string(),
    };
    let hash = pdf_crud::file_sha256(pdf).unwrap();
    crud.insert_letter(pdf, &hash, line, &answer).unwrap()
}

/// Where the database says the letter's PDF is.
fn stored_path(crud: &mut PdfCrud, id: i64) -> PathBuf {
    PathBuf::from(crud.document(id).unwrap().unwrap().file_path)
}

#[test]
fn test_rescan_finds_a_moved_file() {
    let (mut crud, output) = setup("rescan_moved");
    let old = output.join("Brief.pdf");
    let id = store(&mut crud, &old, "Finanzamt Berlin");
    let new = output.join("Finanzamt/Brief.pdf");
    fs::create_dir_all(output.join("Finanzamt")).unwrap();
    fs::rename(&old, &new).unwrap();

    let report = rescan(&mut crud, &output).unwrap();

    assert_eq!(report, Rescan { moved: 1, removed: vec![] });
    assert_eq!(stored_path(&mut crud, id), new);
}

#[test]
fn test_rescan_finds_a_renamed_file() {
    let (mut crud, output) = setup("rescan_renamed");
    let old = output.join("Brief.pdf");
    let id = store(&mut crud, &old, "Finanzamt Berlin");
    let new = output.join("Steuerbescheid 2025.pdf");
    fs::rename(&old, &new).unwrap();

    rescan(&mut crud, &output).unwrap();

    assert_eq!(stored_path(&mut crud, id), new);
}

#[test]
fn test_rescan_leaves_files_in_place_alone() {
    let (mut crud, output) = setup("rescan_in_place");
    let pdf = output.join("Brief.pdf");
    let id = store(&mut crud, &pdf, "Finanzamt Berlin");

    let report = rescan(&mut crud, &output).unwrap();

    assert_eq!(report, Rescan::default());
    assert_eq!(report.summary(), "Rescan: all files in place");
    assert_eq!(stored_path(&mut crud, id), pdf);
}

#[test]
fn test_rescan_removes_a_letter_whose_file_is_gone() {
    let (mut crud, output) = setup("rescan_gone");
    let gone = output.join("Gone.pdf");
    let gone_id = store(&mut crud, &gone, "Rundfunkbeitrag");
    let kept_id = store(&mut crud, &output.join("Kept.pdf"), "Finanzamt Berlin");
    fs::remove_file(&gone).unwrap();

    let report = rescan(&mut crud, &output).unwrap();

    let gone_text = gone.to_string_lossy().into_owned();
    assert_eq!(report.removed, [gone_text.clone()]);
    assert_eq!(report.summary(), format!("Rescan: 0 moved, 1 removed: {gone_text}"));
    assert!(crud.document(gone_id).unwrap().is_none());
    assert!(crud.document(kept_id).unwrap().is_some());
}

/// A wrong or missing output folder must never empty the database.
#[test]
fn test_rescan_without_output_folder_fails_and_deletes_nothing() {
    let (mut crud, output) = setup("rescan_no_output");
    let id = store(&mut crud, &output.join("Brief.pdf"), "Finanzamt Berlin");
    fs::remove_dir_all(&output).unwrap();

    let error = rescan(&mut crud, &output).unwrap_err();

    assert!(error.to_string().contains("nothing changed"), "got: {error}");
    assert!(crud.document(id).unwrap().is_some());
}
