//! Tests for `file_pdf`. The first two fail before the LLM is asked, so they run
//! without it; the last one needs the local LLM server running.

use crate::pdf_crud::DateOrder;
use super::*;
use crate::test_support::{test_folder, write_pdf};

const LETTER: [&str; 3] = [
    "Finanzamt Berlin, 25.11.2025",
    "Sehr geehrte Frau Müller,",
    "Ihr Einkommensteuerbescheid für 2025 liegt bei.",
];

/// A fresh database file in this test's folder, with its tables.
fn new_crud(folder: &Path) -> PdfCrud {
    PdfCrud::new(&folder.join("archive.db")).unwrap()
}

/// The names of the files in a folder; empty if the folder doesn't exist.
fn files_in(folder: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn test_file_pdf_already_stored_fails_and_copies_nothing() {
    let folder = test_folder("file_pdf_twice");
    let input = folder.join("brief.pdf");
    write_pdf(&input, &LETTER);
    let output = folder.join("output");
    let mut crud = new_crud(&folder);
    // Store the same bytes by hand first, so no LLM is needed.
    let hash = pdf_crud::file_sha256(&input).unwrap();
    let answer = crate::summarize_with_ai::LLMAnswer {
        folder: Some("Finanzamt".to_string()),
        summary: "Steuerbescheid".to_string(),
        letter_date: "2025-11-25".to_string(),
        file_name: "Steuerbescheid".to_string(),
    };
    // Its path inside the output folder doesn't matter here; only its fingerprint does.
    crud.insert_letter(Path::new("brief.pdf"), &hash, "text", &answer).unwrap();

    let error = analyze_place_pdf(&input, &output, &mut crud).unwrap_err();

    assert!(error.to_string().contains("already in the archive"), "got: {error}");
    assert!(files_in(&output).is_empty());
}

#[test]
fn test_file_pdf_without_text_fails_and_copies_nothing() {
    let folder = test_folder("file_pdf_no_text");
    let input = folder.join("empty.pdf");
    write_pdf(&input, &[]);
    let output = folder.join("output");
    let mut crud = new_crud(&folder);

    let error = analyze_place_pdf(&input, &output, &mut crud).unwrap_err();

    assert!(error.to_string().contains("no text"), "got: {error}");
    assert!(files_in(&output).is_empty());
    assert!(crud.find_letters(DateOrder::NewestFirst, None,10).unwrap().is_empty());
}

/// Needs the local LLM server running (like the tests in summarize_with_ai.rs).
#[test]
fn test_file_pdf_copies_and_records() {
    let folder = test_folder("file_pdf_ok");
    let input = folder.join("brief.pdf");
    write_pdf(&input, &LETTER);
    let output = folder.join("output");
    let mut crud = new_crud(&folder);

    let target = analyze_place_pdf(&input, &output, &mut crud).unwrap();

    assert!(target.starts_with(&output), "got: {}", target.display());
    assert!(target.exists());
    assert!(input.exists(), "the input is copied, not moved");
    let stored = crud.find_letters(DateOrder::NewestFirst, None,10).unwrap();
    assert_eq!(stored.len(), 1);
    // The database keeps the path inside the output folder, not the full one.
    assert_eq!(Path::new(&stored[0].file_path), target.strip_prefix(&output).unwrap());
    assert!(stored[0].text.contains("Frau Müller"));
}

/// Needs the local LLM server running. The letter is from the Finanzamt, so the LLM
/// should pick that folder.
#[test]
fn test_file_pdf_puts_the_file_in_the_chosen_folder() {
    let folder = test_folder("file_pdf_folder");
    let input = folder.join("brief.pdf");
    write_pdf(&input, &LETTER);
    let output = folder.join("output");
    fs::create_dir_all(output.join("Finanzamt")).unwrap();
    fs::create_dir_all(output.join("Gesundheit/teeth")).unwrap();
    let mut crud = new_crud(&folder);

    let target = analyze_place_pdf(&input, &output, &mut crud).unwrap();

    assert_eq!(target.parent(), Some(output.join("Finanzamt").as_path()));
}

/// The folder between the output folder and the file name, in two steps:
/// 1. `strip_prefix` cuts the output folder off the front (an error if the path
///    does not start with it): "Gesundheit/teeth/Zahnarzt.pdf".
/// 2. `parent` drops the last part, the file name: "Gesundheit/teeth".
///    For a file right in the output folder, that leaves the empty path "".
/// Both work on whole path parts, not letters, so "data/pdfs/outputX" is no match.
#[test]
fn test_path() {
    let output = Path::new(crate::constants::OUTPUT_DIR); // "data/pdfs/output"

    let nested = Path::new("data/pdfs/output/Gesundheit/teeth/Zahnarzt.pdf");
    let relative = nested.strip_prefix(output).unwrap();
    assert_eq!(relative, Path::new("Gesundheit/teeth/Zahnarzt.pdf"));
    assert_eq!(relative.parent(), Some(Path::new("Gesundheit/teeth")));

    let in_root = Path::new("data/pdfs/output/ARD_ZDF_Deutschlandradio_Beitragsservice.pdf");
    let relative = in_root.strip_prefix(output).unwrap();
    assert_eq!(relative.parent(), Some(Path::new("")));

    // A path outside the output folder: `strip_prefix` says so with an `Err`.
    assert!(Path::new("data/pdfs/input/a.pdf").strip_prefix(output).is_err());
}

/// Needs the local LLM server running. Two near-identical letters are filed at the same
/// time on two threads, so the LLM likely gives both the same name. Thanks to the lock,
/// each still gets its own file; without it, one copy could overwrite the other.
#[test]
fn test_two_letters_filed_at_once_do_not_overwrite_each_other() {
    let folder = test_folder("file_pdf_parallel");
    let output = folder.join("output");
    let db = folder.join("archive.db");
    drop(new_crud(&folder)); // creates the database file and its tables
    let first = folder.join("first.pdf");
    let second = folder.join("second.pdf");
    // The last line differs, so the two files are not the same letter (see the hash check).
    write_pdf(&first, &[LETTER[0], LETTER[1], LETTER[2], "Aktenzeichen 1"]);
    write_pdf(&second, &[LETTER[0], LETTER[1], LETTER[2], "Aktenzeichen 2"]);

    // Each thread opens its own connection, like the app's jobs do. A plain function, not a
    // closure: a closure made inside `scope` would end before the threads it is lent to.
    fn file(input: &Path, output: &Path, db: &Path) -> PathBuf {
        let mut crud = PdfCrud::open(db).unwrap();
        analyze_place_pdf(input, output, &mut crud).unwrap()
    }
    // `scope` lets the threads borrow `output` and `db`: it waits for them all to end.
    let (one, two) = std::thread::scope(|scope| {
        let one = scope.spawn(|| file(&first, &output, &db));
        let two = scope.spawn(|| file(&second, &output, &db));
        (one.join().unwrap(), two.join().unwrap())
    });

    assert_ne!(one, two, "both letters went to the same file");
    assert!(one.exists() && two.exists());
    assert_eq!(PdfCrud::open(&db).unwrap().find_letters(DateOrder::NewestFirst, None,10).unwrap().len(), 2);
}

