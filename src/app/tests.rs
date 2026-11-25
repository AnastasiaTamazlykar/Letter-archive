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
        tab: Tab::default(),
        crud: PdfCrud::new(&folder.join("archive.db")).unwrap(),
        config: Config::default(),
        conversions: vec![],
        archive: ArchiveTab::default(),
        images: ImagesTab::default(),
        settings: SettingsTab::default(),
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
