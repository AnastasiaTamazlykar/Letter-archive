//! Moves a filed PDF to another path inside the archive folder: a new name, a new folder
//! (its category), or both. The file moves and the letter's row in the database follows.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use crate::{pdf_crud::PdfCrud, process_document::FILING};
use crate::file_system_actions::utils::safe_file_name;

// The tests live in `src/path_change/tests.rs`.
#[cfg(test)]
mod tests;

/// Moves the PDF at `old` (a full path inside `output_dir`) to `new_relative`, a path inside
/// `output_dir` too, like "Finanzamt/Brief.pdf". Missing folders are created. Returns the new
/// full path. If a file is already there, or the letter is not in the database, it returns an
/// error and changes nothing.
fn change_path(
    crud: &mut PdfCrud,
    output_dir: &Path,
    old: &Path,
    new_relative: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    // The database keeps the path inside the archive folder, so work with that part.
    let relative = old.strip_prefix(output_dir)?;
    let new_full = output_dir.join(new_relative);
    if new_full == old {
        return Ok(new_full); // same path: nothing to do
    }
    // Filing a new letter takes this lock too, so it can't pick the same name meanwhile.
    let _filing = FILING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // macOS treats "Brief.pdf" and "brief.pdf" as one file, so a path that only differs in
    // case is the same file, not a clash. `fs::rename` would silently replace any other file.
    let only_case_differs = new_relative.to_string_lossy().to_lowercase()
        == relative.to_string_lossy().to_lowercase();
    if new_full.exists() && !only_case_differs {
        return Err(format!("{} already exists", new_relative.display()).into());
    }
    let id = crud
        .find_by_file_path(relative.to_str().ok_or("path is not UTF-8")?)?
        .ok_or("this PDF is not in the archive")?;

    if let Some(folder) = new_full.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::rename(old, &new_full)?;
    if let Err(error) = crud.update_file_path_category(id, new_relative) {
        // The database still has the old path, so put the file back.
        // `let _ =`: if that fails too, the first error is the one worth showing.
        let _ = fs::rename(&new_full, old);
        return Err(error);
    }
    Ok(new_full)
}

/// Moves the PDF at `old` into the folder `category`, like "Gesundheit/teeth" ("" is the
/// archive folder itself), keeping its file name. The folder is created if it is new.
/// Each part is cleaned like a file name; `.`, `..` and hidden folders are refused, so the
/// file can never leave the archive folder.
pub fn change_category(
    crud: &mut PdfCrud,
    output_dir: &Path,
    old: &Path,
    category: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let mut new_relative = PathBuf::new();
    for part in category.split('/').map(str::trim).filter(|part| !part.is_empty()) {
        if part.starts_with('.') {
            return Err(format!("\"{part}\" is not a valid folder name").into());
        }
        new_relative.push(safe_file_name(part));
    }
    new_relative.push(old.file_name().ok_or("the PDF has no file name")?);
    change_path(crud, output_dir, old, &new_relative)
}


pub fn rename_pdf(
    crud: &mut PdfCrud,
    output_dir: &Path,
    old: &Path,
    new_name: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let relative = old.strip_prefix(output_dir)?;
    let new_relative = relative.with_file_name(format!("{}.pdf", safe_file_name(new_name)));
    change_path(crud, output_dir, old, &new_relative)
}
