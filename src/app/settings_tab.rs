//! The Settings tab: the output folder, with buttons to point to its new place or move it.
//! A child module of `app`: its `impl App` block may use `App`'s private fields.
//! `pub(super)` marks what `app.rs` may use from here.

use std::path::Path;

use eframe::egui;

use super::App;
use crate::archive_folder_manipulations::move_archive::move_archive;
use crate::paths::CONFIG_FILE;

// The Settings tab's part of `App`; the rest of its methods are in `app.rs` and the other tabs.
impl App {
    /// The Settings tab: the output folder, with a button to choose another one.
    pub(super) fn settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Output folder:");
            ui.monospace(self.config.output_dir.display().to_string())
                .on_hover_text("Where analyzed PDFs are copied to; its subfolders are the categories");
        });
        ui.horizontal(|ui| {
            if ui
                .button("Locate…")
                .on_hover_text("You moved or renamed the folder yourself: point to its new place. Nothing is moved.")
                .clicked()
            {
                self.pick_output_dir();
            }
            // Not while letters are being filed: they would be copied into the old place.
            let move_button = egui::Button::new("Move archive to…");
            if ui
                .add_enabled(!self.is_filing(), move_button)
                .on_hover_text("Moves the folder with all its PDFs into an empty folder you choose")
                .on_disabled_hover_text("Wait until the letter being filed is done")
                .clicked()
            {
                self.move_archive_to();
            }
        });
        ui.label(&self.settings_status);
    }

    /// Asks for an empty folder with the macOS Open panel and moves the archive folder
    /// there (see `move_archive.rs`), then keeps that place as the output folder and
    /// rescans, to check every letter is found there. Does nothing on Cancel.
    fn move_archive_to(&mut self) {
        let Some(target) = rfd::FileDialog::new()
            .set_title("Choose an empty folder for the archive (New Folder makes one)")
            .pick_folder()
        else {
            return;
        };
        if let Err(error) = move_archive(&self.config.output_dir, &target) {
            self.settings_status = format!("Not moved: {error}");
            return;
        }
        let place = target.display().to_string();
        self.config.output_dir = target;
        self.settings_status = match self.config.save(Path::new(CONFIG_FILE)) {
            Ok(()) => format!("Moved to {place}."),
            // Without the saved setting, the next start would look in the old place.
            Err(error) => format!("Moved to {place}, but the setting was not saved: {error}"),
        };
        self.rescan(); // its result shows in the Archive tab
    }

    /// Asks for a folder with the macOS Open panel and keeps it as the output folder,
    /// saved for the next start; does nothing on Cancel. PDFs filed so far stay where they are.
    fn pick_output_dir(&mut self) {
        // The panel opens in the current output folder; it needs a full path for that.
        let current = std::path::absolute(&self.config.output_dir).unwrap_or_default();
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Choose the output folder")
            .set_directory(current)
            .pick_folder()
        else {
            return;
        };
        self.config.output_dir = folder;
        self.settings_status = match self.config.save(Path::new(CONFIG_FILE)) {
            Ok(()) => "Saved."
                .to_string(),
            Err(error) => format!("Could not save the settings: {error}"),
        };
    }
}
