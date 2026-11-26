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

/// One language and its names.
pub struct Language {
    /// Three-letter code (ISO 639-2), as the pipeline takes it, e.g. "deu".
    pub code: &'static str,
    /// Two-letter code (ISO 639-1), as the archive stores it, e.g. "de".
    pub short: &'static str,
    /// The name Apple Vision expects, e.g. "de-DE".
    pub vision: &'static str,
}

pub const GERMAN: Language = Language {
    code: "deu",
    short: "de",
    vision: "de-DE",
};
pub const ENGLISH: Language = Language {
    code: "eng",
    short: "en",
    vision: "en-US",
};

/// The language of the letters; for now always German.
pub const LETTERS: Language = GERMAN;

/// The languages the pipeline knows. A `static` (unlike a `const`) has one fixed place
/// in memory, so `by_code` can hand out references to its entries.
static KNOWN: [Language; 2] = [GERMAN, ENGLISH];

/// The language with this three-letter code, or `None` if we don't know it.
pub fn language_by_code(code: &str) -> Option<&'static Language> {
    KNOWN.iter().find(|language| language.code == code)
}
