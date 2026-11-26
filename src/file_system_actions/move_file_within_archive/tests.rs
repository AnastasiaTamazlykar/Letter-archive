//! Tests for `path_change`. The letters are stored with a made-up LLM answer, so no LLM is
//! needed. The database keeps each PDF's path inside the output folder, e.g. "Finanzamt/Brief.pdf".
//! Renaming only (`rename_pdf`) is tested in its own file; this one is about folders.

use std::fs;

use super::*;
use crate::{
    document_crud,
    test_support::{letter_by_id, setup_archive, store_letter, write_pdf},
};
use crate::summarize_with_ai::LLMAnswer;
use crate::test_support::test_folder;

/// The path and the category the database holds for the letter.
fn stored(crud: &mut PdfCrud, id: i64) -> (String, String) {
    let letter = letter_by_id(crud, id).unwrap();
    (letter.file_path, letter.category)
}

// ---- change_path ----

#[test]
fn test_change_path_moves_the_file_and_updates_the_row() {
    let (mut crud, output) = setup_archive("path_move");
    let old = output.join("Brief.pdf");
    let id = store_letter(&mut crud, &output, &old, "Finanzamt Berlin");

    let new = change_path(&mut crud, &output, &old, Path::new("Finanzamt/Steuer.pdf")).unwrap();

    assert_eq!(new, output.join("Finanzamt/Steuer.pdf"));
    assert!(new.exists() && !old.exists());
    assert_eq!(stored(&mut crud, id), ("Finanzamt/Steuer.pdf".into(), "Finanzamt".into()));
}

#[test]
fn test_change_path_to_a_taken_path_fails_and_changes_nothing() {
    let (mut crud, output) = setup_archive("path_clash");
    let first = output.join("Brief.pdf");
    let taken = output.join("Finanzamt/Brief.pdf");
    let id = store_letter(&mut crud, &output, &first, "Finanzamt Berlin");
    store_letter(&mut crud, &output, &taken, "Rundfunkbeitrag");

    let error = change_path(&mut crud, &output, &first, Path::new("Finanzamt/Brief.pdf")).unwrap_err();

    assert!(error.to_string().contains("already exists"), "got: {error}");
    assert!(first.exists());
    assert_eq!(stored(&mut crud, id).0, "Brief.pdf");
    // The other file was not overwritten: it still holds its own text.
    assert!(document_crud::extract_text(&taken).unwrap().contains("Rundfunkbeitrag"));
}

#[test]
fn test_change_path_of_a_pdf_not_in_the_database_fails_and_keeps_the_file() {
    let (mut crud, output) = setup_archive("path_unknown");
    let stray = output.join("Stray.pdf");
    crate::test_support::write_pdf(&stray, &["nicht gespeichert"]);

    let error = change_path(&mut crud, &output, &stray, Path::new("A/Stray.pdf")).unwrap_err();

    assert!(error.to_string().contains("not in the archive"), "got: {error}");
    assert!(stray.exists());
    assert!(!output.join("A").exists(), "no folder is made for a move that fails");
}

#[test]
fn test_change_path_touches_only_that_letter() {
    let (mut crud, output) = setup_archive("path_only_one");
    let moved = output.join("Brief.pdf");
    let other = output.join("Mahnung.pdf");
    store_letter(&mut crud, &output, &moved, "Finanzamt Berlin");
    let other_id = store_letter(&mut crud, &output, &other, "Rundfunkbeitrag");

    change_path(&mut crud, &output, &moved, Path::new("A/Brief.pdf")).unwrap();

    assert_eq!(stored(&mut crud, other_id), ("Mahnung.pdf".into(), "".into()));
    assert!(other.exists());
}

// ---- change_category ----

#[test]
fn test_change_category_makes_the_new_folder_and_moves_the_file_in() {
    let (mut crud, output) = setup_archive("cat_new");
    let old = output.join("Brief.pdf");
    let id = store_letter(&mut crud, &output, &old, "Finanzamt Berlin");

    let new = change_category(&mut crud, &output, &old, "Steuer").unwrap();

    assert_eq!(new, output.join("Steuer/Brief.pdf")); // the file name stays
    assert!(new.exists() && !old.exists());
    assert_eq!(stored(&mut crud, id), ("Steuer/Brief.pdf".into(), "Steuer".into()));
}

#[test]
fn test_change_category_accepts_a_nested_category() {
    let (mut crud, output) = setup_archive("cat_nested");
    let old = output.join("Brief.pdf");
    let id = store_letter(&mut crud, &output, &old, "Zahnarzt Berlin");

    change_category(&mut crud, &output, &old, "Gesundheit/teeth").unwrap();

    assert_eq!(stored(&mut crud, id).1, "Gesundheit/teeth");
    assert!(output.join("Gesundheit/teeth/Brief.pdf").exists());
}

#[test]
fn test_change_category_to_nothing_moves_the_file_to_the_output_folder() {
    let (mut crud, output) = setup_archive("cat_root");
    let old = output.join("Finanzamt/Brief.pdf");
    let id = store_letter(&mut crud, &output, &old, "Finanzamt Berlin");

    change_category(&mut crud, &output, &old, "").unwrap();

    assert_eq!(stored(&mut crud, id), ("Brief.pdf".into(), "".into()));
    assert!(output.join("Brief.pdf").exists());
}

#[test]
fn test_change_category_to_the_same_category_changes_nothing() {
    let (mut crud, output) = setup_archive("cat_same");
    let old = output.join("Finanzamt/Brief.pdf");
    let id = store_letter(&mut crud, &output, &old, "Finanzamt Berlin");

    let new = change_category(&mut crud, &output, &old, "Finanzamt").unwrap();

    assert_eq!(new, old);
    assert_eq!(stored(&mut crud, id).1, "Finanzamt");
}

#[test]
fn test_change_category_cleans_slashes_spaces_and_odd_characters() {
    let (mut crud, output) = setup_archive("cat_clean");
    let old = output.join("Brief.pdf");
    let id = store_letter(&mut crud, &output, &old, "Finanzamt Berlin");

    change_category(&mut crud, &output, &old, " //Gesundheit / teeth: neu// ").unwrap();

    assert_eq!(stored(&mut crud, id).1, "Gesundheit/teeth neu");
}

#[test]
fn test_change_category_refuses_dots_and_keeps_everything_where_it_is() {
    let (mut crud, output) = setup_archive("cat_dots");
    let old = output.join("Brief.pdf");
    let id = store_letter(&mut crud, &output, &old, "Finanzamt Berlin");

    for bad in ["..", "../Evil", "A/../../Evil", ".hidden", "A/.hidden"] {
        let error = change_category(&mut crud, &output, &old, bad).unwrap_err();
        assert!(error.to_string().contains("not a valid folder name"), "{bad}: {error}");
    }

    assert!(old.exists());
    assert_eq!(stored(&mut crud, id).0, "Brief.pdf");
    // Nothing was created beside or above the output folder.
    assert_eq!(fs::read_dir(output.parent().unwrap()).unwrap().count(), 2); // archive.db, output
}

#[test]
fn test_change_category_with_an_absolute_path_stays_inside_the_output_folder() {
    let (mut crud, output) = setup_archive("cat_absolute");
    let old = output.join("Brief.pdf");
    store_letter(&mut crud, &output, &old, "Finanzamt Berlin");

    let new = change_category(&mut crud, &output, &old, "/etc/Evil").unwrap();

    assert_eq!(new, output.join("etc/Evil/Brief.pdf"));
}

#[test]
fn test_change_category_to_a_folder_with_that_file_name_fails() {
    let (mut crud, output) = setup_archive("cat_clash");
    let old = output.join("Brief.pdf");
    let taken = output.join("Steuer/Brief.pdf");
    let id = store_letter(&mut crud, &output, &old, "Finanzamt Berlin");
    store_letter(&mut crud, &output, &taken, "Rundfunkbeitrag");

    let error = change_category(&mut crud, &output, &old, "Steuer").unwrap_err();

    assert!(error.to_string().contains("already exists"), "got: {error}");
    assert!(old.exists());
    assert_eq!(stored(&mut crud, id).0, "Brief.pdf");
}

#[test]
fn test_rescan_finds_everything_in_place_after_a_category_change() {
    let (mut crud, output) = setup_archive("cat_rescan");
    let kept = output.join("Kept.pdf");
    let old = output.join("Brief.pdf");
    store_letter(&mut crud, &output, &kept, "Rundfunkbeitrag");
    store_letter(&mut crud, &output, &old, "Finanzamt Berlin");
    change_category(&mut crud, &output, &old, "Steuer").unwrap();

    let report = crate::rescan::rescan(&mut crud, &output).unwrap();

    assert_eq!(report, crate::rescan::Rescan::default());
}


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
    let hash = document_crud::file_sha256(pdf).unwrap();
    let relative = pdf.strip_prefix(output).unwrap();
    crud.insert_letter(relative, &hash, line, &answer).unwrap()
}

/// The path the database holds for the letter.
fn stored_path(crud: &mut PdfCrud, id: i64) -> String {
    letter_by_id(crud, id).unwrap().file_path
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
    let letter = letter_by_id(&mut crud, id).unwrap();
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
    assert!(document_crud::extract_text(&second).unwrap().contains("Rundfunkbeitrag"));
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
