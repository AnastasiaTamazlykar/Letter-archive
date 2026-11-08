//! The language of the letters, kept in one place. The pipeline, Apple Vision and the
//! archive each name a language in their own way; all of those names come from here.

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
pub fn by_code(code: &str) -> Option<&'static Language> {
    KNOWN.iter().find(|language| language.code == code)
}
