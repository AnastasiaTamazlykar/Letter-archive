//! Renames a filed PDF: the file in the archive folder gets the new name, and the letter's
//! row in the database follows. The PDF stays in its folder, so its category doesn't change.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use crate::{pdf_crud::PdfCrud, pdf_name::safe_file_name};

// The tests live in `src/rename_pdf/tests.rs`.
#[cfg(test)]
mod tests;

/// Renames the PDF at `old` (a full path inside `output_dir`) to `new_name`, without
/// ".pdf". The name is cleaned like the LLM's names (`safe_file_name`): no `/` and the like.
/// Returns the new full path. Changes nothing, and returns an error, if a file with the
/// new name is already there or the letter is not in the database.
pub fn rename_pdf(
    crud: &mut PdfCrud,
    output_dir: &Path,
    old: &Path,
    new_name: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    // The database keeps the path inside the archive folder, so work with that part.
    let relative = old.strip_prefix(output_dir)?;
    let new_relative = relative.with_file_name(format!("{}.pdf", safe_file_name(new_name)));
    let new_full = output_dir.join(&new_relative);
    if new_full == old {
        return Ok(new_full); // same name: nothing to do
    }
    // macOS treats "Brief.pdf" and "brief.pdf" as one file, so a name that only differs in
    // case is the same file, not a clash. `fs::rename` would silently replace any other file.
    let only_case_differs = new_relative.to_string_lossy().to_lowercase()
        == relative.to_string_lossy().to_lowercase();
    if new_full.exists() && !only_case_differs {
        return Err(format!("{} already exists", new_relative.display()).into());
    }
    let id = crud
        .find_by_file_path(relative.to_str().ok_or("path is not UTF-8")?)?
        .ok_or("this PDF is not in the archive")?;

    fs::rename(old, &new_full)?;
    if let Err(error) = crud.update_file_path_category(id, &new_relative) {
        // The database still has the old name, so put the file back.
        // `let _ =`: if that fails too, the first error is the one worth showing.
        let _ = fs::rename(&new_full, old);
        return Err(error);
    }
    Ok(new_full)
}
