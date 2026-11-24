//! The list of filing jobs: the ones running now and the ones that are done.

use crate::convert::Conversion;
use eframe::egui;

/// Draws one line per job: a spinner and its progress while it runs, then its result.
/// Draws nothing when there are no jobs. Returns whether "Clear finished" was clicked;
/// the caller removes the finished jobs, because only it owns the list.
pub fn job_queue(ui: &mut egui::Ui, jobs: &[Conversion]) -> bool {
    if jobs.is_empty() {
        return false;
    }
    let running = jobs.iter().filter(|job| !job.is_finished()).count();
    // A job that worked has already left the list, so a finished one here has failed.
    let failed = jobs.len() - running;
    let mut clear = false;
    ui.horizontal(|ui| {
        ui.strong(format!("Jobs: {running} running, {failed} failed"));
        // Nothing to clear while every job is still running.
        clear = ui
            .add_enabled(failed > 0, egui::Button::new("Clear failed"))
            .clicked();
    });
    for job in jobs {
        ui.horizontal(|ui| {
            if job.is_finished() {
                ui.label(format!("{}: {}", job.label(), job.summary()));
            } else {
                ui.spinner();
                ui.label(job.progress());
            }
        });
    }
    clear
}
