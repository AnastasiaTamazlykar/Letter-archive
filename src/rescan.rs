//! Keeps the database in step with the output folder: a letter whose PDF was moved or
//! renamed inside the output folder gets its new path; a letter whose PDF is gone is
//! removed. PDFs are recognized by their fingerprint (SHA-256), not by their name.

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    path::{Path, PathBuf},
};

use crate::file_system_actions::utils::collect_pdf_files;
use crate::
document_crud::{self, PdfCrud, StoredFile};

// The tests live in `src/rescan/tests.rs`.
#[cfg(test)]
mod tests;

/// What a rescan did.
#[derive(Debug, Default, PartialEq)]
pub struct Rescan {
    /// How many letters were found again under a new path.
    pub moved: usize,
    /// The old paths of the letters whose PDF was not found; they were removed.
    pub removed: Vec<String>,
}

impl Rescan {
    /// For the status line, e.g. "Rescan: 2 moved, 1 removed: data/pdfs/output/Brief.pdf".
    pub fn summary(&self) -> String {
        if self.moved == 0 && self.removed.is_empty() {
            return "Rescan: all files in place".to_string();
        }
        let mut text = format!("Rescan: {} moved, {} removed", self.moved, self.removed.len());
        if !self.removed.is_empty() {
            text += &format!(": {}", self.removed.join(", "));
        }
        text
    }
}

/// Checks every stored letter's PDF. A missing one is looked for in `output_dir` by its
/// fingerprint: found, the path is updated; not found, the letter is removed.
/// If `output_dir` doesn't exist, nothing is changed and an error is returned, so a
/// wrong folder can never empty the database.
pub fn rescan(crud: &mut PdfCrud, output_dir: &Path) -> Result<Rescan, Box<dyn Error>> {
    if !output_dir.is_dir() {
        return Err(format!("{} not found", output_dir.display()).into());
    }
    // `partition` splits the rows in two lists: PDFs still in place, and missing ones.
    // A row keeps the path inside the archive folder, so the folder goes in front.
    let (in_place, missing): (Vec<StoredFile>, Vec<StoredFile>) = crud
        .stored_files()?
        .into_iter()
        .partition(|file| output_dir.join(&file.file_path).exists());

    let mut report = Rescan::default();
    if in_place.is_empty() {
        return Err(format!("Not a single file found in {}", output_dir.display()).into());
    }
    if missing.is_empty() {
        return Ok(report); // the usual case: nothing is hashed at all
    }
    let by_hash = hash_unknown_pdfs(output_dir, &in_place)?;
    for file in missing {
        match by_hash.get(&file.sha256) {
            Some(new_path) => {
                // `new_path` is a full path, "<output_dir>/Finanzamt/Brief.pdf"; the database
                // keeps only "Finanzamt/Brief.pdf". Its category comes from that same path,
                // so with the full path it would be the whole "<output_dir>/Finanzamt".
                // `?` turns a `StripPrefixError` into the `Box<dyn Error>` we return.
                let relative = new_path.strip_prefix(output_dir)?;
                crud.update_file_path_category(file.id, relative)?;
                report.moved += 1;
            }
            None => {
                crud.delete_letter(file.id)?;
                report.removed.push(file.file_path);
            }
        }
    }
    Ok(report)
}

/// The fingerprints of the PDFs in `output_dir` that no stored letter points to, each
/// with its path. PDFs a letter already points to are skipped: they can't be a moved
/// one, and skipping them keeps a rescan fast however big the archive grows.
/// If two PDFs have the same bytes, the first one (in sorted order) is kept.
fn hash_unknown_pdfs(
    output_dir: &Path,
    in_place: &[StoredFile],
) -> Result<HashMap<String, PathBuf>, Box<dyn Error>> {
    // A `HashSet` answers "is this path in it?" at once, however many paths it holds.
    // Full paths, the archive folder in front, to compare with what `pdf_files` finds.
    let known: HashSet<PathBuf> = in_place
        .iter()
        .map(|file| output_dir.join(&file.file_path))
        .collect();
    let mut by_hash = HashMap::new();
    for pdf in collect_pdf_files(output_dir)? {
        if known.contains(pdf.as_path()) {
            continue;
        }
        // `entry(..).or_insert(..)` adds the path only if this fingerprint is new.
        by_hash.entry(document_crud::file_sha256(&pdf)?).or_insert(pdf);
    }
    Ok(by_hash)
}
