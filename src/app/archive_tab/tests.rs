//! Tests for the Archive tab: renaming a file, changing its category or its letter date in
//! the table. A child module of `archive_tab`, so `use super::*` brings in its private items
//! too. `test_app` is shared with the other window tests in `src/app/tests.rs`.

use super::*;
use crate::app::tests::test_app;
use crate::test_support::test_folder;

// ---- Renaming a file in the table ----

/// An app whose output folder holds one stored letter at `relative` (like "Finanzamt/Brief.pdf");
/// the table is loaded. Also returns the output folder.
fn test_app_with_letter(name: &str, relative: &str) -> (App, PathBuf) {
    let mut app = test_app(name);
    let output = test_folder(&format!("{name}_output"));
    app.config.output_dir = output.clone();
    let pdf = output.join(relative);
    std::fs::create_dir_all(pdf.parent().unwrap()).unwrap();
    crate::test_support::write_pdf(&pdf, &["Finanzamt Berlin"]);
    let answer = crate::summarize_with_ai::LLMAnswer {
        folder: None,
        summary: "Ein Brief.".to_string(),
        letter_date: String::new(),
        file_name: "Brief".to_string(),
    };
    let hash = crate::pdf_crud::file_sha256(&pdf).unwrap();
    app.crud
        .insert_letter(Path::new(relative), &hash, "Finanzamt Berlin", &answer)
        .unwrap();
    app.set_pdfs();
    (app, output)
}

#[test]
fn test_renaming_starts_with_the_current_name_without_pdf() {
    let renaming = Renaming::new(PathBuf::from("/archive/Finanzamt/Steuerbescheid 2025.pdf"));

    assert_eq!(renaming.text, "Steuerbescheid 2025");
    assert!(renaming.error.is_empty());
    assert!(!renaming.focused, "the box takes the focus when it is first drawn");
}

#[test]
fn test_rename_in_the_menu_opens_the_box_and_esc_closes_it() {
    let mut app = test_app("app_rename_open");
    let ctx = egui::Context::default();
    let path = PathBuf::from("/archive/Brief.pdf");

    app.act_on_pdf_click(&ctx, PdfClick::Rename(path.clone()));
    assert_eq!(app.archive.renaming.as_ref().map(|renaming| &renaming.path), Some(&path));

    app.act_on_pdf_click(&ctx, PdfClick::RenameCancel);
    assert!(app.archive.renaming.is_none());
}

#[test]
fn test_enter_renames_the_file_and_reloads_the_table() {
    let (mut app, output) = test_app_with_letter("app_rename_ok", "Finanzamt/Brief.pdf");
    let old = output.join("Finanzamt/Brief.pdf");
    app.act_on_pdf_click(&egui::Context::default(), PdfClick::Rename(old.clone()));
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
    app.archive.renaming = Some(Renaming::new(old.clone()));
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

    app.act_on_pdf_click(&egui::Context::default(), PdfClick::RenameCategory(path.clone()));

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
        PdfClick::RenameCategory(output.join("Brief.pdf")),
    );

    assert_eq!(app.archive.renaming.as_ref().unwrap().text, "");
}

#[test]
fn test_enter_in_the_category_box_moves_the_file_and_reloads_the_table() {
    let (mut app, output) = test_app_with_letter("app_cat_move", "Finanzamt/Brief.pdf");
    let old = output.join("Finanzamt/Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, PdfClick::RenameCategory(old.clone()));
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
    app.act_on_pdf_click(&ctx, PdfClick::RenameCategory(output.join("Finanzamt/Brief.pdf")));
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
    app.act_on_pdf_click(&ctx, PdfClick::RenameCategory(output.join("Brief.pdf")));
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
    app.act_on_pdf_click(&ctx, PdfClick::RenameCategory(output.join("Brief.pdf")));
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

#[test]
fn test_the_rename_box_takes_the_focus_once_and_does_not_end_by_itself() {
    let mut renaming = Renaming::new(PathBuf::from("/archive/Brief.pdf"));
    let ctx = egui::Context::default();
    let mut click = None;

    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        click = rename_cell(ui, &mut renaming);
    });
    output.textures_delta.clear(); // see `draw_job_queue`

    assert!(click.is_none(), "no key was pressed, so neither Enter nor Esc");
    assert!(renaming.focused);
    assert_eq!(renaming.text, "Brief");
}

// ---- Editing the letter date in the table ----

#[test]
fn test_edit_date_opens_the_box_with_the_current_date() {
    let (mut app, output) = test_app_with_letter("app_date_open", "Brief.pdf");
    let path = output.join("Brief.pdf");
    let ctx = egui::Context::default();

    // The test letter has no date: the box starts empty, in the date cell of that row.
    app.act_on_pdf_click(&ctx, PdfClick::EditDate(path.clone()));
    let renaming = app.archive.renaming.as_ref().unwrap();
    assert_eq!(renaming.text, "");
    assert!(renaming.is_for(&path, EditKind::Date));
    assert!(!renaming.is_for(&path, EditKind::Name));

    let id = app.crud.find_by_file_path("Brief.pdf").unwrap().unwrap();
    app.crud.update_letter_date(id, "2025-03-04").unwrap();
    app.act_on_pdf_click(&ctx, PdfClick::EditDate(path));
    assert_eq!(app.archive.renaming.as_ref().unwrap().text, "2025-03-04");
}

#[test]
fn test_enter_saves_the_date_and_reloads_the_table() {
    let (mut app, output) = test_app_with_letter("app_date_ok", "Finanzamt/Brief.pdf");
    let path = output.join("Finanzamt/Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, PdfClick::EditDate(path.clone()));
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
    app.act_on_pdf_click(&ctx, PdfClick::EditDate(output.join("Brief.pdf")));
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
    app.act_on_pdf_click(&ctx, PdfClick::EditDate(output.join("Brief.pdf")));
    app.archive.renaming.as_mut().unwrap().text = "2024-12-31".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameCancel);

    assert!(app.archive.renaming.is_none());
    assert_eq!(app.archive.pdfs[0].letter_date, None);
}
