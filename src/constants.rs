//! Where the app keeps its files. Relative paths start from the folder the app runs in.
//! Both the window and the background thread need them, so they live in one place.

/// The letter archive's database file.
pub const DB_FILE: &str = "data/archive.db";

/// Where PDFs wait to be analyzed, for example the PDF made from images.
pub const INPUT_DIR: &str = "data/pdfs/input";

/// Where analyzed PDFs are copied to at first, under the name the LLM suggests.
/// The Settings tab can choose another folder (see `settings.rs`).
pub const OUTPUT_DIR: &str = "data/pdfs/output";

/// The user's settings (see `settings.rs`).
pub const CONFIG_FILE: &str = "data/settings.json";
