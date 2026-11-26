//! The user's settings, kept in a small JSON file so they last from one run to the next.
//! For now there is one: the output folder, where filed PDFs are copied to.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::constants::OUTPUT_DIR;

/// `Serialize, Deserialize`: serde writes it as JSON and reads it back, field by field,
/// e.g. `{ "output_dir": "data/pdfs/output" }`.
/// `#[serde(default)]`: a key missing from the file takes its value from `Default`, so an
/// older settings file still loads after a new setting is added here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Where analyzed PDFs are copied to; its subfolders are the categories.
    pub output_dir: PathBuf,
}

/// The settings before the user changed anything.
impl Default for Config {
    fn default() -> Self {
        Config {
            output_dir: PathBuf::from(OUTPUT_DIR),
        }
    }
}

impl Config {
    /// Reads the settings from `file`. With no file yet (the first run) or one that can't
    /// be read, it gives the defaults, so the app always starts.
    pub fn load(file: &Path) -> Config {
        let Ok(text) = fs::read_to_string(file) else {
            return Config::default();
        };
        match serde_json::from_str(&text) {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!("cannot read {}: {error}; using the defaults", file.display());
                Config::default()
            }
        }
    }

    /// Writes the settings to `file`, creating its folder if missing.
    pub fn save(&self, file: &Path) -> Result<(), Box<dyn Error>> {
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder)?;
        }
        fs::write(file, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}
