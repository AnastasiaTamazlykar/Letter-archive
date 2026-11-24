//! Renames a filed PDF: the file in the archive folder gets the new name, and the letter's
//! row in the database follows. The PDF stays in its folder, so its category doesn't change.

use std::{
    error::Error,
    path::{Path, PathBuf},
};

// `super` is the parent module, `archive_folder_manipulations`; `path_change` is its sibling.
use super::path_change::change_path;
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
    let relative = old.strip_prefix(output_dir)?;
    let new_relative = relative.with_file_name(format!("{}.pdf", safe_file_name(new_name)));
    change_path(crud, output_dir, old, &new_relative)
}
