//! Tests for `rename_pdf`. The letters are stored with a made-up LLM answer, so no LLM is
//! needed. The database keeps each PDF's path inside the output folder, e.g. "Finanzamt/Brief.pdf".

use std::fs;

use super::*;
use crate::{
    pdf_crud,
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
    fs::create_dir_all(pdf.parent().unwrap()).unwrap();
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

/// The path the database holds for the letter.
fn stored_path(crud: &mut PdfCrud, id: i64) -> String {
    crud.document(id).unwrap().unwrap().file_path
}

/// The names of the files in a folder, sorted. Unlike `Path::exists`, this tells "Brief.pdf"
/// from "brief.pdf" even on macOS, whose file system ignores case.
fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn test_rename_changes_the_file_and_the_database() {
    let (mut crud, output) = setup("rename_basic");
    let old = output.join("Brief.pdf");
    let id = store(&mut crud, &output, &old, "Finanzamt Berlin");

    let new = rename_pdf(&mut crud, &output, &old, "Steuerbescheid 2025").unwrap();

    assert_eq!(new, output.join("Steuerbescheid 2025.pdf"));
    assert!(new.exists());
    assert!(!old.exists());
    assert_eq!(stored_path(&mut crud, id), "Steuerbescheid 2025.pdf");
}

#[test]
fn test_rename_keeps_the_folder_and_the_category() {
    let (mut crud, output) = setup("rename_category");
    let old = output.join("Gesundheit/teeth/Zahnarzt.pdf");
    let id = store(&mut crud, &output, &old, "Zahnarzt Berlin");

    let new = rename_pdf(&mut crud, &output, &old, "Kontrolle").unwrap();

    assert_eq!(new, output.join("Gesundheit/teeth/Kontrolle.pdf"));
    let letter = crud.document(id).unwrap().unwrap();
    assert_eq!(letter.file_path, "Gesundheit/teeth/Kontrolle.pdf");
    assert_eq!(letter.category, "Gesundheit/teeth"); // the category is not the full path
}

#[test]
fn test_rename_to_the_same_name_changes_nothing() {
    let (mut crud, output) = setup("rename_same");
    let old = output.join("Brief.pdf");
    let id = store(&mut crud, &output, &old, "Finanzamt Berlin");

    let new = rename_pdf(&mut crud, &output, &old, "Brief").unwrap();

    assert_eq!(new, old);
    assert_eq!(names_in(&output), ["Brief.pdf"]);
    assert_eq!(stored_path(&mut crud, id), "Brief.pdf");
}

#[test]
fn test_rename_to_an_existing_name_fails_and_changes_nothing() {
    let (mut crud, output) = setup("rename_clash");
    let first = output.join("Brief.pdf");
    let second = output.join("Mahnung.pdf");
    let first_id = store(&mut crud, &output, &first, "Finanzamt Berlin");
    let second_id = store(&mut crud, &output, &second, "Rundfunkbeitrag");

    let error = rename_pdf(&mut crud, &output, &first, "Mahnung").unwrap_err();

    assert!(error.to_string().contains("already exists"), "got: {error}");
    assert_eq!(names_in(&output), ["Brief.pdf", "Mahnung.pdf"]);
    assert_eq!(stored_path(&mut crud, first_id), "Brief.pdf");
    assert_eq!(stored_path(&mut crud, second_id), "Mahnung.pdf");
    // The other file was not overwritten: it still holds its own text.
    assert!(pdf_crud::extract_text(&second).unwrap().contains("Rundfunkbeitrag"));
}

#[test]
fn test_rename_that_only_changes_the_case_is_allowed() {
    let (mut crud, output) = setup("rename_case");
    let old = output.join("Brief.pdf");
    let id = store(&mut crud, &output, &old, "Finanzamt Berlin");

    rename_pdf(&mut crud, &output, &old, "brief").unwrap();

    assert_eq!(names_in(&output), ["brief.pdf"]);
    assert_eq!(stored_path(&mut crud, id), "brief.pdf");
}

#[test]
fn test_rename_cleans_the_name_so_it_stays_in_its_folder() {
    let (mut crud, output) = setup("rename_clean");
    let old = output.join("Finanzamt/Brief.pdf");
    let id = store(&mut crud, &output, &old, "Finanzamt Berlin");

    let new = rename_pdf(&mut crud, &output, &old, "../../evil").unwrap();

    assert_eq!(new, output.join("Finanzamt/evil.pdf"));
    assert_eq!(stored_path(&mut crud, id), "Finanzamt/evil.pdf");
    assert_eq!(names_in(&output), ["Finanzamt"]); // nothing was put beside or above it
}

#[test]
fn test_rename_an_empty_name_gives_the_fallback_name() {
    let (mut crud, output) = setup("rename_empty");
    let old = output.join("Brief.pdf");
    store(&mut crud, &output, &old, "Finanzamt Berlin");

    let new = rename_pdf(&mut crud, &output, &old, "  ").unwrap();

    assert_eq!(new, output.join("Letter.pdf"));
}

#[test]
fn test_rename_a_pdf_not_in_the_database_fails_and_keeps_the_file() {
    let (mut crud, output) = setup("rename_unknown");
    let stray = output.join("Stray.pdf");
    write_pdf(&stray, &["nicht gespeichert"]);

    let error = rename_pdf(&mut crud, &output, &stray, "Neu").unwrap_err();

    assert!(error.to_string().contains("not in the archive"), "got: {error}");
    assert_eq!(names_in(&output), ["Stray.pdf"]);
}

#[test]
fn test_rename_a_pdf_outside_the_output_folder_fails() {
    let (mut crud, output) = setup("rename_outside");
    let outside = output.parent().unwrap().join("Elsewhere.pdf");
    write_pdf(&outside, &["irgendwo"]);

    assert!(rename_pdf(&mut crud, &output, &outside, "Neu").is_err());
    assert!(outside.exists());
}
