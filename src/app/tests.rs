//! Tests for the window's job handling: starting jobs side by side, unticking the images,
//! and the job list. None of them needs the LLM: the "photos" are missing, so every job
//! fails at once, which is enough to see how the jobs are started and collected.

use super::*;
use crate::test_support::test_folder;
use std::{path::PathBuf, thread::sleep, time::Duration};

/// An app with its own empty database in `name`'s test folder (not the real archive).
/// Built field by field, because `App::new` opens the real database.
pub(super) fn test_app(name: &str) -> App {
    let folder = test_folder(name);
    App {
        crud: PdfCrud::new(&folder.join("archive.db")).unwrap(),
        tab: Tab::default(),
        pdfs: vec![],
        hits: vec![],
        pictures: vec![],
        preview: None,
        pdf_view: None,
        pdf_view_share: PDF_VIEW_SHARE,
        renaming: None,
        conversions: vec![],
        rescan_status: String::new(),
        search: String::new(),
        date_order: DateOrder::default(),
        category: None,
        categories: vec![],
        settings: Settings::default(),
        settings_status: String::new(),
    }
}

/// Asks for news now and then, the way the window does once per frame; gives up after 5 s.
pub(super) fn wait_until_done(app: &mut App) {
    for _ in 0..500 {
        app.check_conversions();
        if !app.is_filing() {
            return;
        }
        sleep(Duration::from_millis(10));
    }
}

#[test]
fn test_a_second_job_starts_while_the_first_is_running() {
    let mut app = test_app("app_two_jobs");
    let ctx = egui::Context::default();

    app.start_conversion(&ctx, vec![PathBuf::from("/no/such/first.HEIC")]);
    // No `check_conversions` in between: the first job has not been collected yet.
    app.start_conversion(&ctx, vec![PathBuf::from("/no/such/second.HEIC")]);

    assert_eq!(app.conversions.len(), 2);
    assert!(app.is_filing());
    wait_until_done(&mut app);
    assert!(!app.is_filing());
    assert!(app.conversions[0].summary().contains("first.HEIC"));
    assert!(app.conversions[1].summary().contains("second.HEIC"));
}

#[test]
fn test_each_job_keeps_its_own_result() {
    let mut app = test_app("app_own_results");
    let ctx = egui::Context::default();
    app.start_conversion(&ctx, vec![PathBuf::from("/no/such/a.HEIC")]);
    app.start_conversion(&ctx, vec![PathBuf::from("/no/such/b.HEIC"), PathBuf::from("/no/such/c.HEIC")]);

    wait_until_done(&mut app);

    assert_eq!(app.conversions[0].label(), "1 image(s)");
    assert_eq!(app.conversions[1].label(), "2 image(s)");
    assert!(app.conversions.iter().all(|job| job.summary().starts_with("Failed: ")));
}

#[test]
fn test_finished_jobs_stay_in_the_list() {
    let mut app = test_app("app_keeps_jobs");
    app.start_conversion(&egui::Context::default(), vec![PathBuf::from("/no/such/a.HEIC")]);

    wait_until_done(&mut app);
    app.check_conversions(); // one more frame changes nothing

    assert_eq!(app.conversions.len(), 1);
    assert!(app.conversions[0].is_finished());
}

#[test]
fn test_no_job_means_not_filing() {
    assert!(!test_app("app_idle").is_filing());
}

/// Draws the job list in a window with no mouse input, and returns what `job_queue` says.
fn draw_job_queue(jobs: &[Conversion]) -> bool {
    let ctx = egui::Context::default();
    let mut clear = true;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        clear = job_queue(ui, jobs);
    });
    // egui panics if the finished frame's texture changes are dropped without being used
    // (a real window uploads them to the graphics card); here nobody needs them.
    output.textures_delta.clear();
    clear
}

#[test]
fn test_job_queue_without_jobs_asks_for_nothing() {
    assert!(!draw_job_queue(&[]));
}

#[test]
fn test_job_queue_draws_running_and_finished_jobs_without_a_click() {
    let mut app = test_app("app_draw_queue");
    let ctx = egui::Context::default();
    app.start_conversion(&ctx, vec![PathBuf::from("/no/such/a.HEIC")]);
    wait_until_done(&mut app);
    // A second job, still running when drawn (it is not collected before drawing).
    app.start_conversion(&ctx, vec![PathBuf::from("/no/such/b.HEIC")]);

    assert!(!draw_job_queue(&app.conversions), "nobody clicked Clear finished");
    wait_until_done(&mut app);
}

// ---- Renaming a file in the table ----

/// An app whose output folder holds one stored letter at `relative` (like "Finanzamt/Brief.pdf");
/// the table is loaded. Also returns the output folder.
fn test_app_with_letter(name: &str, relative: &str) -> (App, PathBuf) {
    let mut app = test_app(name);
    let output = test_folder(&format!("{name}_output"));
    app.settings.output_dir = output.clone();
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
    assert_eq!(app.renaming.as_ref().map(|renaming| &renaming.path), Some(&path));

    app.act_on_pdf_click(&ctx, PdfClick::RenameCancel);
    assert!(app.renaming.is_none());
}

#[test]
fn test_enter_renames_the_file_and_reloads_the_table() {
    let (mut app, output) = test_app_with_letter("app_rename_ok", "Finanzamt/Brief.pdf");
    let old = output.join("Finanzamt/Brief.pdf");
    app.act_on_pdf_click(&egui::Context::default(), PdfClick::Rename(old.clone()));
    app.renaming.as_mut().unwrap().text = "Steuerbescheid".to_string();

    app.act_on_pdf_click(&egui::Context::default(), PdfClick::RenameSubmit);

    assert!(app.renaming.is_none(), "the box is gone after a successful rename");
    assert!(!old.exists());
    assert!(output.join("Finanzamt/Steuerbescheid.pdf").exists());
    assert_eq!(app.pdfs.len(), 1);
    assert_eq!(app.pdfs[0].file_path, "Finanzamt/Steuerbescheid.pdf");
}

#[test]
fn test_a_failed_rename_keeps_the_box_open_with_the_reason() {
    let (mut app, output) = test_app_with_letter("app_rename_clash", "Brief.pdf");
    let old = output.join("Brief.pdf");
    std::fs::write(output.join("Mahnung.pdf"), "other file").unwrap(); // the name is taken
    app.renaming = Some(Renaming::new(old.clone()));
    app.renaming.as_mut().unwrap().text = "Mahnung".to_string();
    app.renaming.as_mut().unwrap().focused = true;

    app.finish_rename(&egui::Context::default());

    let renaming = app.renaming.as_ref().expect("the box stays open");
    assert!(renaming.error.contains("already exists"), "got: {}", renaming.error);
    assert!(!renaming.focused, "the box asks for the focus again, to keep typing");
    assert!(old.exists());
    assert_eq!(app.pdfs[0].file_path, "Brief.pdf");
}

#[test]
fn test_rename_on_a_category_opens_the_box_with_the_current_category() {
    let (mut app, output) = test_app_with_letter("app_cat_open", "Gesundheit/teeth/Brief.pdf");
    let path = output.join("Gesundheit/teeth/Brief.pdf");

    app.act_on_pdf_click(&egui::Context::default(), PdfClick::RenameCategory(path.clone()));

    let renaming = app.renaming.as_ref().unwrap();
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

    assert_eq!(app.renaming.as_ref().unwrap().text, "");
}

#[test]
fn test_enter_in_the_category_box_moves_the_file_and_reloads_the_table() {
    let (mut app, output) = test_app_with_letter("app_cat_move", "Finanzamt/Brief.pdf");
    let old = output.join("Finanzamt/Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, PdfClick::RenameCategory(old.clone()));
    app.renaming.as_mut().unwrap().text = "Steuer/2025".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    assert!(app.renaming.is_none());
    assert!(!old.exists());
    assert!(output.join("Steuer/2025/Brief.pdf").exists());
    assert_eq!(app.pdfs[0].category, "Steuer/2025");
    assert_eq!(app.pdfs[0].file_path, "Steuer/2025/Brief.pdf");
    assert!(app.categories.contains(&"Steuer/2025".to_string()), "the new category is listed");
}

#[test]
fn test_esc_in_the_category_box_moves_nothing() {
    let (mut app, output) = test_app_with_letter("app_cat_esc", "Finanzamt/Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, PdfClick::RenameCategory(output.join("Finanzamt/Brief.pdf")));
    app.renaming.as_mut().unwrap().text = "Steuer".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameCancel);

    assert!(app.renaming.is_none());
    assert!(output.join("Finanzamt/Brief.pdf").exists());
    assert_eq!(app.pdfs[0].category, "Finanzamt");
}

#[test]
fn test_a_category_with_dots_keeps_the_box_open_with_the_reason() {
    let (mut app, output) = test_app_with_letter("app_cat_dots", "Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, PdfClick::RenameCategory(output.join("Brief.pdf")));
    app.renaming.as_mut().unwrap().text = "../Evil".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    let renaming = app.renaming.as_ref().expect("the box stays open");
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
    app.renaming.as_mut().unwrap().text = "Steuer".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    let renaming = app.renaming.as_ref().expect("the box stays open");
    assert!(renaming.error.contains("already exists"), "got: {}", renaming.error);
    assert!(output.join("Brief.pdf").exists());
}

#[test]
fn test_finish_rename_without_a_box_does_nothing() {
    let mut app = test_app("app_rename_none");

    app.finish_rename(&egui::Context::default());

    assert!(app.renaming.is_none());
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

// ---- Cleaning the queue, and the fixed band above the thumbnails ----

#[test]
fn test_a_job_that_worked_leaves_the_queue_at_once() {
    let mut app = test_app("app_queue_success");
    app.conversions.push(Conversion::already_done("2 image(s)", Ok(PathBuf::from("Brief.pdf"))));

    app.check_conversions();

    assert!(app.conversions.is_empty());
}

#[test]
fn test_a_failed_job_stays_in_the_queue_with_its_error() {
    let mut app = test_app("app_queue_failure");
    app.conversions.push(Conversion::already_done("2 image(s)", Err("no LLM".to_string())));

    app.check_conversions();

    assert_eq!(app.conversions.len(), 1);
    assert_eq!(app.conversions[0].summary(), "Failed: no LLM");
}

#[test]
fn test_only_the_job_that_worked_leaves_the_queue() {
    let mut app = test_app("app_queue_mixed");
    app.conversions.push(Conversion::already_done("a", Err("broken".to_string())));
    app.conversions.push(Conversion::already_done("b", Ok(PathBuf::from("B.pdf"))));
    // A job still running: its end of the channel is open and nothing was sent.
    app.start_conversion(&egui::Context::default(), vec![PathBuf::from("/no/such/c.HEIC")]);

    app.check_conversions();

    let labels: Vec<&str> = app.conversions.iter().map(|job| job.label()).collect();
    assert_eq!(labels[0], "a");
    assert!(!labels.contains(&"b"));
    wait_until_done(&mut app);
}

// ---- Editing the letter date in the table ----

#[test]
fn test_edit_date_opens_the_box_with_the_current_date() {
    let (mut app, output) = test_app_with_letter("app_date_open", "Brief.pdf");
    let path = output.join("Brief.pdf");
    let ctx = egui::Context::default();

    // The test letter has no date: the box starts empty, in the date cell of that row.
    app.act_on_pdf_click(&ctx, PdfClick::EditDate(path.clone()));
    let renaming = app.renaming.as_ref().unwrap();
    assert_eq!(renaming.text, "");
    assert!(renaming.is_for(&path, EditKind::Date));
    assert!(!renaming.is_for(&path, EditKind::Name));

    let id = app.crud.find_by_file_path("Brief.pdf").unwrap().unwrap();
    app.crud.update_letter_date(id, "2025-03-04").unwrap();
    app.act_on_pdf_click(&ctx, PdfClick::EditDate(path));
    assert_eq!(app.renaming.as_ref().unwrap().text, "2025-03-04");
}

#[test]
fn test_enter_saves_the_date_and_reloads_the_table() {
    let (mut app, output) = test_app_with_letter("app_date_ok", "Finanzamt/Brief.pdf");
    let path = output.join("Finanzamt/Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, PdfClick::EditDate(path.clone()));
    app.renaming.as_mut().unwrap().text = " 2024-12-31 ".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    assert!(app.renaming.is_none(), "the box is gone after saving");
    assert_eq!(app.pdfs[0].letter_date.as_deref(), Some("2024-12-31"));
    assert!(path.exists(), "the PDF stays where it is");
    assert_eq!(app.pdfs[0].file_path, "Finanzamt/Brief.pdf");
}

#[test]
fn test_an_invalid_date_keeps_the_box_open_with_the_reason() {
    let (mut app, output) = test_app_with_letter("app_date_bad", "Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, PdfClick::EditDate(output.join("Brief.pdf")));
    app.renaming.as_mut().unwrap().text = "25.11.2025".to_string();
    app.renaming.as_mut().unwrap().focused = true;

    app.act_on_pdf_click(&ctx, PdfClick::RenameSubmit);

    let renaming = app.renaming.as_ref().expect("the box stays open");
    assert!(renaming.error.contains("YYYY-MM-DD"), "got: {}", renaming.error);
    assert!(!renaming.focused, "the box asks for the focus again, to keep typing");
    assert_eq!(app.pdfs[0].letter_date, None);
}

#[test]
fn test_esc_in_the_date_box_changes_nothing() {
    let (mut app, output) = test_app_with_letter("app_date_esc", "Brief.pdf");
    let ctx = egui::Context::default();
    app.act_on_pdf_click(&ctx, PdfClick::EditDate(output.join("Brief.pdf")));
    app.renaming.as_mut().unwrap().text = "2024-12-31".to_string();

    app.act_on_pdf_click(&ctx, PdfClick::RenameCancel);

    assert!(app.renaming.is_none());
    assert_eq!(app.pdfs[0].letter_date, None);
}
