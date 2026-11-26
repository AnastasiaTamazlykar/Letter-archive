//! The one step every new letter goes through, whether it was made from images or
//! picked as a PDF: read its text, ask the LLM about it, copy it to the archive folder
//! under the name the LLM suggests, and record it in the database.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::file_system_actions::utils::{scan_folders, safe_file_name, free_pdf_path};
use crate::{
    pdf_crud::{self, PdfCrud},
    summarize_with_ai::ask_model,
};

// The tests live in `src/file_pdf/tests.rs`.
#[cfg(test)]
mod tests;

/// Held while a letter is copied and recorded. Letters are filed on several threads at once;
/// without it, two letters the LLM gives the same name would both pick "Brief.pdf", and the
/// second copy would overwrite the first. A `Mutex` lets one thread in at a time.
/// `()`: it guards no data, only the order of the steps.
pub(crate) static FILING: Mutex<()> = Mutex::new(());

/// Files the PDF at `input`: copies it into the folder of `output_dir` the LLM chose
/// (or `output_dir` itself if none fits) and adds it to the database.
pub fn analyze_place_pdf(
    input: &Path,
    output_dir: &Path,
    crud: &mut PdfCrud,
) -> Result<PathBuf, Box<dyn Error>> {
    let hash = pdf_crud::file_sha256(input)?;
    if let Some(id) = crud.find_by_hash(&hash)? {
        return Err(format!("already in the archive (#{id})").into());
    }
    let text = pdf_crud::extract_text(input)?;
    let folders = scan_folders(output_dir)?;
    let answer = ask_model(&text, &folders)?; // nothing is written before the LLM has answered

    // `answer.folder` is always one of `folders`, so `dir` stays inside `output_dir`.
    let dir = match &answer.folder {
        Some(folder) => output_dir.join(folder),
        None => output_dir.to_path_buf(),
    };
    // `lock` waits until no other thread is filing; the guard `_filing` unlocks the moment
    // this function ends. A "poisoned" lock (a thread crashed holding it) is still safe to use.
    let _filing = FILING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    fs::create_dir_all(&dir)?;
    let name = safe_file_name(&answer.file_name);
    let target = free_pdf_path(&dir, &name);
    // The database keeps only the path inside the archive folder ("Finanzamt/Brief.pdf"),
    // so the archive folder may move. `target` starts with `output_dir`, so this can't
    // fail; it comes before the copy anyway, so a failure would leave no file behind.
    let relative = target.strip_prefix(output_dir)?;
    fs::copy(input, &target)?;
    if let Err(error) = crud.insert_letter(relative, &hash, &text, &answer) {
        // `let _ =`: if removing fails too, the first error is the one worth showing.
        let _ = fs::remove_file(&target);
        return Err(error);
    }
    Ok(target)
}
