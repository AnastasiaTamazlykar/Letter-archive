//! Tests for the Images → PDF tab: ticking images, creating a PDF from them, and the fixed
//! band above the thumbnails. The helpers `test_app` and `wait_until_done` are shared with
//! the other window tests in `src/app/tests.rs`.

use super::*;
use super::images_tab::{IMAGES_TOP_HEIGHT, Picture};
use super::tests::{test_app, wait_until_done};
use std::path::PathBuf;

/// A ticked picture of a file that doesn't exist (so a job made from it fails at once).
fn ticked_picture(name: &str) -> Picture {
    Picture {
        path: PathBuf::from(format!("/no/such/{name}.HEIC")),
        thumbnail: None,
        selected: true,
    }
}

#[test]
fn test_create_unticks_all_images_at_once() {
    let mut app = test_app("app_unticks");
    app.images.pictures = vec![ticked_picture("a"), ticked_picture("b")];
    app.images.pictures[1].selected = false;
    let images = app.selected_paths();
    assert_eq!(images.len(), 1, "one picture is ticked before Create");

    app.start_conversion(&egui::Context::default(), images);

    // Right after the click, with the job still running.
    assert!(app.images.pictures.iter().all(|picture| !picture.selected));
    assert!(app.is_filing());
    wait_until_done(&mut app);
}

#[test]
fn test_images_stay_in_the_list_after_create() {
    let mut app = test_app("app_keeps_images");
    app.images.pictures = vec![ticked_picture("a"), ticked_picture("b")];

    app.start_conversion(&egui::Context::default(), app.selected_paths());

    assert_eq!(app.images.pictures.len(), 2, "only the ticks go, not the images");
    wait_until_done(&mut app);
}

/// How high the band above the thumbnails is, drawn in a window of its own.
fn height_of_images_top(app: &mut App) -> f32 {
    let ctx = egui::Context::default();
    let mut height = 0.0;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let before = ui.cursor().min.y;
        app.images_top(ui);
        height = ui.cursor().min.y - before;
    });
    output.textures_delta.clear(); // see `draw_job_queue`
    height
}

#[test]
fn test_the_band_above_the_thumbnails_is_as_high_with_many_jobs_as_with_none() {
    let mut app = test_app("app_band_height");
    let empty = height_of_images_top(&mut app);

    for number in 0..12 {
        let error = Err(format!("job {number} failed"));
        app.conversions.push(Conversion::already_done("2 image(s)", error));
    }
    app.check_conversions(); // collects the 12 results; failed jobs stay in the list
    assert_eq!(app.conversions.len(), 12);
    let crowded = height_of_images_top(&mut app);

    // The same height, so the thumbnails below never move.
    assert_eq!(empty, crowded);
    assert!(empty >= IMAGES_TOP_HEIGHT && empty < IMAGES_TOP_HEIGHT + 20.0, "got: {empty}");
}
