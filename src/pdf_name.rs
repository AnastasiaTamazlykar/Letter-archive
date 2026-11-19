//! File names for the PDFs we write: a safe name from what the LLM suggests, and a path
//! that doesn't overwrite a file already there.

use std::path::{Path, PathBuf};

/// Longest name we keep, in characters (not counting ".pdf").
const MAX_NAME_CHARS: usize = 60;
/// The name used when nothing usable is left.
const FALLBACK_NAME: &str = "Letter";

/// Turns a suggested name into one that is safe as a file name: letters, digits, spaces,
/// `-` and `_` stay; anything else (like `/`, `:` or `.`) becomes a space. Runs of spaces
/// become one, and the name is cut to `MAX_NAME_CHARS`. "Letter" if nothing is left.
pub fn safe_file_name(suggested: &str) -> String {
    let cleaned: String = suggested
        .chars()
        // `matches!` checks a value against a pattern; `|` means "or".
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_') { c } else { ' ' })
        .collect();
    // `split_whitespace` drops all spaces, `join` puts single ones back between the words.
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let short: String = words.join(" ").chars().take(MAX_NAME_CHARS).collect();
    let name = short.trim_end(); // cutting may leave a space at the end
    if name.is_empty() {
        FALLBACK_NAME.to_string()
    } else {
        name.to_string()
    }
}

/// `dir/name.pdf`, or `dir/name 2.pdf`, `dir/name 3.pdf`, … if that file already exists.
pub fn free_pdf_path(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join(format!("{name}.pdf"));
    let mut number = 2;
    while path.exists() {
        path = dir.join(format!("{name} {number}.pdf"));
        number += 1;
    }
    path
}

// Small file, so the tests stay here.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_folder;

    #[test]
    fn test_safe_file_name_keeps_a_plain_name() {
        let name = "2025-11-25 Rundfunkbeitrag Beitragsservice_Köln";
        assert_eq!(safe_file_name(name), name);
    }

    #[test]
    fn test_safe_file_name_removes_slashes_and_odd_characters() {
        assert_eq!(safe_file_name("Brief: 25/11/2025"), "Brief 25 11 2025");
        assert_eq!(safe_file_name("../../etc/passwd"), "etc passwd");
        assert_eq!(safe_file_name("\"Mahnung\".pdf"), "Mahnung pdf");
    }

    #[test]
    fn test_safe_file_name_empty_becomes_letter() {
        assert_eq!(safe_file_name(""), "Letter");
        assert_eq!(safe_file_name(" /:. "), "Letter");
    }

    #[test]
    fn test_safe_file_name_cuts_long_names() {
        let long = "Wort ".repeat(30);
        let name = safe_file_name(&long);
        assert!(name.chars().count() <= MAX_NAME_CHARS, "got: {name}");
        assert!(!name.ends_with(' '), "got: {name:?}");
    }

    #[test]
    fn test_free_pdf_path_keeps_a_free_name() {
        let dir = test_folder("free_name");
        assert_eq!(free_pdf_path(&dir, "Brief"), dir.join("Brief.pdf"));
    }

    #[test]
    fn test_free_pdf_path_adds_a_number_when_taken() {
        let dir = test_folder("taken_name");
        std::fs::write(dir.join("Brief.pdf"), "").unwrap();
        assert_eq!(free_pdf_path(&dir, "Brief"), dir.join("Brief 2.pdf"));

        std::fs::write(dir.join("Brief 2.pdf"), "").unwrap();
        assert_eq!(free_pdf_path(&dir, "Brief"), dir.join("Brief 3.pdf"));
    }
}
