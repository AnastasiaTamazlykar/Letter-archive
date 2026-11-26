//! Tests for the Archive tab: renaming a file, changing its category or its letter date in
//! the table. A child module of `archive_tab`, so `use super::*` brings in its private items
//! too. `test_app` is shared with the other window tests in `src/app/tests.rs`.

use super::*;
use super::{EditKind, Renaming};
use super::table::PdfClick;
use crate::app::tests::test_app;
use crate::test_support::{letter_by_id, test_folder};
use std::path::{Path, PathBuf};

// ---- Renaming a file in the table ----

/// An app whose output folder holds one stored letter at `relative` (like "Finanzamt/Brief.pdf");
/// the table is loaded. Also returns the output folder.
fn test_app_with_letter(name: &str, relative: &str) -> (App, PathBuf) {
    let mut app = test_app(name);
    let output = test_folder(&format!("{name}_output"));
    app.config.output_dir = output.clone();
    add_letter(&mut app, relative, "Finanzamt Berlin");
    app.set_pdfs();
    (app, output)
}

/// Stores one more letter at `relative` in the app's output folder: a PDF saying `text`,
/// with the summary "Ein Brief." and no date. Different texts give different fingerprints,
/// so several letters can be stored.
fn add_letter(app: &mut App, relative: &str, text: &str) {
    let pdf = app.config.output_dir.join(relative);
    std::fs::create_dir_all(pdf.parent().unwrap()).unwrap();
    crate::test_support::write_pdf(&pdf, &[text]);
    let answer = crate::summarize_with_ai::LLMAnswer {
        folder: None,
        summary: "Ein Brief.".to_string(),
        letter_date: String::new(),
        file_name: "Brief".to_string(),
    };
    let hash = crate::document_crud::file_sha256(&pdf).unwrap();
    app.crud
        .insert_letter(Path::new(relative), &hash, text, &answer)
        .unwrap();
}

fn click_rename_category(app: &App, path: PathBuf) -> PdfClick {
    PdfClick::RenameCategory { id: app.archive.pdfs[0].id, path }
}

fn click_edit_date(app: &App, path: PathBuf) -> PdfClick {
    let letter = &app.archive.pdfs[0];
    PdfClick::EditDate { id: letter.id, path, date: letter.letter_date.clone() }
}

fn click_edit_summary(app: &App, path: PathBuf) -> PdfClick {
    let letter = &app.archive.pdfs[0];
    PdfClick::EditSummary { id: letter.id, path, summary: letter.summary.clone() }
}

#[test]
fn test_renaming_starts_with_the_current_name_without_pdf() {
    let renaming = Renaming::new(1, PathBuf::from("/archive/Finanzamt/Steuerbescheid 2025.pdf"));

    assert_eq!(renaming.text, "Steuerbescheid 2025");
    assert!(renaming.error.is_empty());
    assert!(!renaming.focused, "the box takes the focus when it is first drawn");
}

#[test]
fn test_rename_in_the_menu_opens_the_box_and_esc_closes_it() {
    let mut app = test_app("app_rename_open");
    let ctx = egui::Context::default();
    let path = PathBuf::from("/archive/Brief.pdf");

    app.act_on_pdf_click(&ctx, PdfClick::Rename { id: 1, path: path.clone() });
    assert_eq!(app.archive.renaming.as_ref().map(|renaming| &renaming.path), Some(&path));

    app.act_on_pdf_click(&ctx, PdfClick::RenameCancel);
    assert!(app.archive.renaming.is_none());
}

#[test]
fn test_enter_renames_the_file_and_reloads_the_table() {
    let (mut app, output) = test_app_with_letter("app_rename_ok", "Finanzamt/Brief.pdf");
    let old = output.join("Finanzamt/Brief.pdf");
    let id = app.archive.pdfs[0].id;
    app.act_on_pdf_click(&egui::Context::default(), PdfClick::Rename { id, path: old.clone() });
    app.archive.renaming.as_mut().unwrap().text = "Steuerbescheid".to_string();

    app.act_on_pdf_click(&egui::Context::default(), PdfClick::RenameSubmit);

    assert!(app.archive.renaming.is_none(), "the box is gone after a successful rename");
    assert!(!old.exists());
    assert!(output.join("Finanzamt/Steuerbescheid.pdf").exists());
    assert_eq!(app.archive.pdfs.len(), 1);
    assert_eq!(app.archive.pdfs[0].file_path, "Finanzamt/Steuerbescheid.pdf");
}

#[test]
fn test_a_failed_rename_keeps_the_box_open_with_the_reason() {
    let (mut app, output) = test_app_with_letter("app_rename_clash", "Brief.pdf");
    let old = output.join("Brief.pdf");
    std::fs::write(output.join("Mahnung.pdf"), "other file").unwrap(); // the name is taken
    app.archive.renaming = Some(Renaming::new(app.archive.pdfs[0].id, old.clone()));
    app.archive.renaming.as_mut().unwrap().text = "Mahnung".to_string();
    app.archive.renaming.as_mut().unwrap().focused = true;

    app.finish_rename(&egui::Context::default());

    let renaming = app.archive.renaming.as_ref().expect("the box stays open");
    assert!(renaming.error.contains("already exists"), "got: {}", renaming.error);
    assert!(!renaming.focused, "the box asks for the focus again, to keep typing");
    assert!(old.exists());
    assert_eq!(app.archive.pdfs[0].file_path, "Brief.pdf");
}

#[test]
fn test_rename_on_a_category_opens_the_box_with_the_current_category() {
    let (mut app, output) = test_app_with_letter("app_cat_open", "Gesundheit/teeth/Brief.pdf");
    let path = output.join("Gesundheit/teeth/Brief.pdf");

    app.act_on_pdf_click(&egui::Context::default(), click_rename_category(&app, path.clone()));

    let renaming = app.archive.renaming.as_ref().unwrap();
    assert!(renaming.kind == EditKind::Category);
    assert_eq!(renaming.text, "Gesundheit/teeth");
    // The box belongs to the category cell of that row, not to its file name cell.
    assert!(renaming.is_for(&path, EditKind::Category));
    assert!(!renaming.is_for(&path, EditKind::Name));
}

#[test]
fn test_a_pdf_in_the_output_folder_starts_with_an_empty_category() {
    let (mut app, output) = test_app_with_letter("app_cat_root", "Brief.pdf");

    app.act_on_pdf_click(
        &egui::Context::default(),
        click_rename_category(&app, output.join("Brief.pdf")),
    );

    assert_eq!(app.archive.renaming.as_ref().unwrap().text, "");
}

#[test]
fn test_enter_in_the_category_box_moves_the_file_and_reloads_the_table() {
    let (mut app, output) = test_app_with_letter("app_cat_move", "Finanzamt/Brief.pdf");
    let old = output.join("Finanzamt/Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, click_rename_category(&app, old.clone()));
    app.archive.renaming.as_mut().unwrap().text = "Steuer/2025".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    assert!(app.archive.renaming.is_none());
    assert!(!old.exists());
    assert!(output.join("Steuer/2025/Brief.pdf").exists());
    assert_eq!(app.archive.pdfs[0].category, "Steuer/2025");
    assert_eq!(app.archive.pdfs[0].file_path, "Steuer/2025/Brief.pdf");
    assert!(app.archive.categories.contains(&"Steuer/2025".to_string()), "the new category is listed");
}

#[test]
fn test_esc_in_the_category_box_moves_nothing() {
    let (mut app, output) = test_app_with_letter("app_cat_esc", "Finanzamt/Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, click_rename_category(&app, output.join("Finanzamt/Brief.pdf")));
    app.archive.renaming.as_mut().unwrap().text = "Steuer".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameCancel);

    assert!(app.archive.renaming.is_none());
    assert!(output.join("Finanzamt/Brief.pdf").exists());
    assert_eq!(app.archive.pdfs[0].category, "Finanzamt");
}

#[test]
fn test_a_category_with_dots_keeps_the_box_open_with_the_reason() {
    let (mut app, output) = test_app_with_letter("app_cat_dots", "Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, click_rename_category(&app, output.join("Brief.pdf")));
    app.archive.renaming.as_mut().unwrap().text = "../Evil".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    let renaming = app.archive.renaming.as_ref().expect("the box stays open");
    assert!(renaming.error.contains("not a valid folder name"), "got: {}", renaming.error);
    assert!(output.join("Brief.pdf").exists());
}

#[test]
fn test_a_category_change_that_clashes_keeps_the_box_open_with_the_reason() {
    let (mut app, output) = test_app_with_letter("app_cat_clash", "Brief.pdf");
    std::fs::create_dir_all(output.join("Steuer")).unwrap();
    std::fs::write(output.join("Steuer/Brief.pdf"), "other file").unwrap(); // the name is taken
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, click_rename_category(&app, output.join("Brief.pdf")));
    app.archive.renaming.as_mut().unwrap().text = "Steuer".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    let renaming = app.archive.renaming.as_ref().expect("the box stays open");
    assert!(renaming.error.contains("already exists"), "got: {}", renaming.error);
    assert!(output.join("Brief.pdf").exists());
}

#[test]
fn test_finish_rename_without_a_box_does_nothing() {
    let mut app = test_app("app_rename_none");

    app.finish_rename(&egui::Context::default());

    assert!(app.archive.renaming.is_none());
}



// ---- Editing the letter date in the table ----

#[test]
fn test_edit_date_opens_the_box_with_the_current_date() {
    let (mut app, output) = test_app_with_letter("app_date_open", "Brief.pdf");
    let path = output.join("Brief.pdf");
    let ctx = egui::Context::default();

    // The test letter has no date: the box starts empty, in the date cell of that row.
    app.act_on_pdf_click(&ctx, click_edit_date(&app, path.clone()));
    let renaming = app.archive.renaming.as_ref().unwrap();
    assert_eq!(renaming.text, "");
    assert!(renaming.is_for(&path, EditKind::Date));
    assert!(!renaming.is_for(&path, EditKind::Name));

    let id = app.crud.find_by_file_path("Brief.pdf").unwrap().unwrap();
    app.crud.update_letter_date(id, "2025-03-04").unwrap();
    app.set_pdfs();
    app.act_on_pdf_click(&ctx, click_edit_date(&app, path));
    assert_eq!(app.archive.renaming.as_ref().unwrap().text, "2025-03-04");
}

#[test]
fn test_enter_saves_the_date_and_reloads_the_table() {
    let (mut app, output) = test_app_with_letter("app_date_ok", "Finanzamt/Brief.pdf");
    let path = output.join("Finanzamt/Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, click_edit_date(&app, path.clone()));
    app.archive.renaming.as_mut().unwrap().text = " 2024-12-31 ".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    assert!(app.archive.renaming.is_none(), "the box is gone after saving");
    assert_eq!(app.archive.pdfs[0].letter_date.as_deref(), Some("2024-12-31"));
    assert!(path.exists(), "the PDF stays where it is");
    assert_eq!(app.archive.pdfs[0].file_path, "Finanzamt/Brief.pdf");
}

#[test]
fn test_an_invalid_date_keeps_the_box_open_with_the_reason() {
    let (mut app, output) = test_app_with_letter("app_date_bad", "Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, click_edit_date(&app, output.join("Brief.pdf")));
    app.archive.renaming.as_mut().unwrap().text = "25.11.2025".to_string();
    app.archive.renaming.as_mut().unwrap().focused = true;

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    let renaming = app.archive.renaming.as_ref().expect("the box stays open");
    assert!(renaming.error.contains("YYYY-MM-DD"), "got: {}", renaming.error);
    assert!(!renaming.focused, "the box asks for the focus again, to keep typing");
    assert_eq!(app.archive.pdfs[0].letter_date, None);
}

#[test]
fn test_esc_in_the_date_box_changes_nothing() {
    let (mut app, output) = test_app_with_letter("app_date_esc", "Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, click_edit_date(&app, output.join("Brief.pdf")));
    app.archive.renaming.as_mut().unwrap().text = "2024-12-31".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameCancel);

    assert!(app.archive.renaming.is_none());
    assert_eq!(app.archive.pdfs[0].letter_date, None);
}

// ---- Editing the summary in the table ----

/// The id of the letter stored at `relative`.
fn id_of(app: &mut App, relative: &str) -> i64 {
    app.crud.find_by_file_path(relative).unwrap().unwrap()
}

/// The summary stored for the letter at `relative`.
fn summary_of(app: &mut App, relative: &str) -> String {
    let id = id_of(app, relative);
    letter_by_id(&mut app.crud, id).unwrap().summary
}

#[test]
fn test_update_summary_overwrites_the_stored_summary() {
    let (mut app, _) = test_app_with_letter("app_summary_overwrite", "Brief.pdf");
    let id = id_of(&mut app, "Brief.pdf");

    app.crud.update_summary(id, "Mahnung vom Finanzamt, Frist 1.12.").unwrap();

    assert_eq!(summary_of(&mut app, "Brief.pdf"), "Mahnung vom Finanzamt, Frist 1.12.");
}

#[test]
fn test_update_summary_ignores_spaces_around_it() {
    let (mut app, _) = test_app_with_letter("app_summary_trim", "Brief.pdf");
    let id = id_of(&mut app, "Brief.pdf");

    app.crud.update_summary(id, "  Neue Zusammenfassung ").unwrap();

    assert_eq!(summary_of(&mut app, "Brief.pdf"), "Neue Zusammenfassung");
}

#[test]
fn test_update_summary_changes_only_that_row() {
    let (mut app, _) = test_app_with_letter("app_summary_one_row", "Brief.pdf");
    add_letter(&mut app, "Andere.pdf", "Ein anderer Brief");
    let id = id_of(&mut app, "Brief.pdf");

    app.crud.update_summary(id, "Neu").unwrap();

    assert_eq!(summary_of(&mut app, "Andere.pdf"), "Ein Brief.");
}

#[test]
fn test_update_summary_fails_for_an_unknown_letter() {
    let mut app = test_app("app_summary_unknown");

    let error = app.crud.update_summary(42, "Neu").unwrap_err();

    assert!(error.to_string().contains("not in the archive"), "got: {error}");
}

/// A summary is free text, so quotes and `;` can't be refused. They are stored exactly as
/// typed and never become SQL: the table and both rows are still there.
#[test]
fn test_update_summary_stores_quotes_and_sql_as_plain_text() {
    let (mut app, _) = test_app_with_letter("app_summary_sql", "Brief.pdf");
    add_letter(&mut app, "Andere.pdf", "Ein anderer Brief");
    let id = id_of(&mut app, "Brief.pdf");
    let attack = "O'Brien'; DROP TABLE documents;--";

    app.crud.update_summary(id, attack).unwrap();

    assert_eq!(summary_of(&mut app, "Brief.pdf"), attack);
    assert_eq!(app.crud.find_letters(DateOrder::NewestFirst, None,10).unwrap().len(), 2);
}

#[test]
fn test_edit_summary_opens_the_box_with_the_current_summary() {
    let (mut app, output) = test_app_with_letter("app_summary_open", "Brief.pdf");
    let path = output.join("Brief.pdf");

    app.act_on_pdf_click(&egui::Context::default(), click_edit_summary(&app, path.clone()));

    let renaming = app.archive.renaming.as_ref().unwrap();
    assert_eq!(renaming.text, "Ein Brief.");
    // The box belongs to the summary cell of that row, not to its date cell.
    assert!(renaming.is_for(&path, EditKind::Summary));
    assert!(!renaming.is_for(&path, EditKind::Date));
}

#[test]
fn test_enter_saves_the_summary_and_reloads_the_table() {
    let (mut app, output) = test_app_with_letter("app_summary_ok", "Finanzamt/Brief.pdf");
    let path = output.join("Finanzamt/Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, click_edit_summary(&app, path.clone()));
    app.archive.renaming.as_mut().unwrap().text = " Neue Zusammenfassung ".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    assert!(app.archive.renaming.is_none(), "the box is gone after saving");
    assert_eq!(app.archive.pdfs[0].summary, "Neue Zusammenfassung");
    assert!(path.exists(), "the PDF stays where it is");
    assert_eq!(app.archive.pdfs[0].file_path, "Finanzamt/Brief.pdf");
}

#[test]
fn test_esc_in_the_summary_box_changes_nothing() {
    let (mut app, output) = test_app_with_letter("app_summary_esc", "Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, click_edit_summary(&app, output.join("Brief.pdf")));
    app.archive.renaming.as_mut().unwrap().text = "Neue Zusammenfassung".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameCancel);

    assert!(app.archive.renaming.is_none());
    assert_eq!(app.archive.pdfs[0].summary, "Ein Brief.");
}
