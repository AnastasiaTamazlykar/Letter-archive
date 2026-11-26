//! Moves the whole archive folder to a new place, for the Settings tab.
//! The database keeps each letter's path inside the archive folder ("Gesundheit/Brief.pdf"),
//! so after a move only the archive folder setting changes, not a single row.
//!
//! On the same disk the folder is just renamed, which is instant. To another disk every
//! file is copied and checked first, and the old folder is deleted only once all copies
//! are good: if anything fails on the way, the old archive is still complete.

use std::{error::Error, fs, io, path::Path};

use crate::document_crud::file_sha256;

/// The file Finder leaves in a folder it has shown; it doesn't make a folder "not empty".
const FINDER_FILE: &str = ".DS_Store";

/// Moves the archive folder `from` to `to`, with all its subfolders and files.
/// `to` must be an empty folder (or not exist yet) and must not lie inside `from`.
pub fn move_archive(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    check_target(from, to)?;
    clear_target(to)?;
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        // `rename` can't move a folder to another disk; copying can.
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => copy_then_delete(from, to),
        Err(error) => Err(error.into()),
    }
}

/// Refuses a move that would lose or mix up files: `from` must be a folder, and `to` a new
/// or empty folder outside it.
fn check_target(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    if !from.is_dir() {
        return Err(format!("{} not found", from.display()).into());
    }
    // As full paths, so "data/pdfs/output" and "/Users/…/data/pdfs/output" compare equal.
    let from_full = std::path::absolute(from)?;
    if std::path::absolute(to)?.starts_with(&from_full) {
        return Err("the new place is the archive folder itself, or inside it".into());
    }
    if to.exists() && !is_empty(to)? {
        return Err(format!("{} is not empty; choose an empty folder", to.display()).into());
    }
    Ok(())
}

/// True if the folder holds nothing but, perhaps, Finder's `.DS_Store`.
fn is_empty(folder: &Path) -> io::Result<bool> {
    for entry in fs::read_dir(folder)? {
        if entry?.file_name() != FINDER_FILE {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Removes the empty target folder, so `rename` can put the archive folder in its place
/// (it refuses to replace a folder with a file in it, even `.DS_Store`).
fn clear_target(to: &Path) -> io::Result<()> {
    if !to.exists() {
        return Ok(());
    }
    let finder_file = to.join(FINDER_FILE);
    if finder_file.exists() {
        fs::remove_file(finder_file)?;
    }
    fs::remove_dir(to) // only ever removes an empty folder
}

/// For a move to another disk: copies everything, then deletes `from`. If a copy fails or
/// differs from its original, the copies are removed again and `from` stays as it was.
fn copy_then_delete(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    if let Err(error) = copy_checked(from, to) {
        // `let _ =`: if cleaning up fails too, the copy error is the one worth showing.
        let _ = fs::remove_dir_all(to);
        return Err(error);
    }
    fs::remove_dir_all(from)?;
    Ok(())
}

/// Copies the folder `from` to `to` with everything in it, and checks that each copied
/// file has the same fingerprint (SHA-256) as its original.
fn copy_checked(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            // The function calls itself for the folder inside ("recursion").
            copy_checked(&source, &target)?;
        } else {
            fs::copy(&source, &target)?;
            if file_sha256(&source)? != file_sha256(&target)? {
                let error = format!("the copy of {} differs from the original", source.display());
                return Err(error.into());
            }
        }
    }
    Ok(())
}
