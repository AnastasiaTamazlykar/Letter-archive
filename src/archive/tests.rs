//! Tests for the letter archive. The test PDFs are written here with real text, so the
//! tests need no photos and no Vision; most use a database that lives only in memory.

use std::{fs::File, io::BufWriter};

use printpdf::{Mm, PdfDocument as NewPdf};

// `super::*` brings in everything `archive.rs` can see, private items included.
use super::*;
use crate::pipeline::FONT;

// A method added to `Archive` here, in the tests file: Rust allows an `impl` block in any
// module of the same crate, and as a child module this one may call the private
// `with_connection`. So `archive.rs` itself carries no test-only code.
impl Archive {
    /// A database that lives only in memory and is gone when dropped.
    /// `:memory:` is SQLite's special name for that.
    fn in_memory() -> Result<Self, Box<dyn Error>> {
        Ok(Self{db:SqliteConnection::establish(":memory:")?})
    }
}

// ── Helpers ──

/// An empty folder of its own for one test, so tests running at the same time
/// don't touch each other's files.
fn test_folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("paper_archive_test_{name}"));
    let _ = std::fs::remove_dir_all(&folder); // left over from an earlier run
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

/// Writes a one-page PDF with these lines of real text, top to bottom.
fn write_pdf(path: &Path, lines: &[&str]) {
    let (doc, page, layer) = NewPdf::new("test", Mm(210.0), Mm(297.0), "text");
    let font_data = std::fs::read(FONT).unwrap();
    let font = doc.add_external_font(font_data.as_slice()).unwrap();
    let layer = doc.get_page(page).get_layer(layer);
    for (number, line) in lines.iter().enumerate() {
        let y = Mm(270.0 - 10.0 * number as f32);
        layer.use_text(*line, 12.0, Mm(20.0), y, &font);
    }
    doc.save(&mut BufWriter::new(File::create(path).unwrap()))
        .unwrap();
}

const LETTER: [&str; 2] = ["Sehr geehrte Frau Müller,", "Größe der Straße"];

// ── Reading text from a PDF ──

#[test]
fn test_extract_text_reads_german_text() {
    let pdf = test_folder("extract").join("brief.pdf");
    write_pdf(&pdf, &LETTER);
    let text = extract_text(&pdf).unwrap();
    assert!(text.contains("Sehr geehrte Frau Müller,"), "got: {text}");
    assert!(text.contains("Größe der Straße"), "got: {text}");
}

#[test]
fn test_extract_text_fails_without_text() {
    let pdf = test_folder("no_text").join("empty.pdf");
    write_pdf(&pdf, &[]);
    assert!(extract_text(&pdf).is_err());
}

#[test]
fn test_extract_text_fails_for_missing_file() {
    assert!(extract_text(Path::new("/no/such/letter.pdf")).is_err());
}

// ── Storing letters ──

#[test]
fn test_add_pdf_stores_text_and_relative_path() {
    let folder = test_folder("store");
    let pdf = folder.join("brief.pdf");
    write_pdf(&pdf, &LETTER);
    let mut archive = Archive::in_memory().unwrap();

    let Added::New(id) = archive.add_pdf(&pdf).unwrap() else {
        panic!("expected a new letter");
    };
    let document = archive.document(id).unwrap().unwrap();
    assert_eq!(document.id, id);
    assert_eq!(document.file_path, "brief.pdf");
    assert_eq!(document.sha256.len(), 64);
    assert!(document.text.contains("Frau Müller"));
    assert_eq!(document.text_source, "pdfkit");
    assert_eq!(document.language, "de");
}

#[test]
fn test_add_pdf_keeps_full_path_outside_folder() {
    let pdf = test_folder("outside_pdf").join("brief.pdf");
    write_pdf(&pdf, &LETTER);
    let mut archive = Archive::in_memory().unwrap();

    let Added::New(id) = archive.add_pdf(&pdf).unwrap() else {
        panic!("expected a new letter");
    };
    let stored = archive.document(id).unwrap().unwrap().file_path;
    assert_eq!(Path::new(&stored), pdf.canonicalize().unwrap());
}

/// The same bytes are recognized, even when the file has another name.
#[test]
fn test_add_pdf_skips_a_letter_stored_before() {
    let folder = test_folder("twice");
    let pdf = folder.join("brief.pdf");
    write_pdf(&pdf, &LETTER);
    let copy = folder.join("copy.pdf");
    std::fs::copy(&pdf, &copy).unwrap();
    let mut archive = Archive::in_memory().unwrap();

    let Added::New(id) = archive.add_pdf(&pdf).unwrap() else {
        panic!("expected a new letter");
    };
    assert_eq!(archive.add_pdf(&pdf).unwrap(), Added::AlreadyStored(id));
    assert_eq!(archive.add_pdf(&copy).unwrap(), Added::AlreadyStored(id));
}

#[test]
fn test_add_pdf_names_the_missing_file() {
    let mut archive = Archive::in_memory().unwrap();
    let error = archive
        .add_pdf(Path::new("/no/such/letter.pdf"))
        .unwrap_err();
    assert!(error.to_string().contains("/no/such/letter.pdf"));
}

/// A letter stored in a database file is still there after opening it again.
#[test]
fn test_open_keeps_letters_on_disk() {
    let folder = test_folder("on_disk");
    let pdf = folder.join("brief.pdf");
    write_pdf(&pdf, &LETTER);
    let db_file = folder.join("archive.db");

    let id = match Archive::open(&db_file).unwrap().add_pdf(&pdf).unwrap() {
        Added::New(id) => id,
        other => panic!("expected a new letter, got {other:?}"),
    }; // the first archive is dropped here, which closes the database

    let mut archive = Archive::open(&db_file).unwrap();
    assert!(archive.document(id).unwrap().is_some());
}

// ── Searching ──

const TAX_LETTER: [&str; 4] = [
    "Finanzamt Berlin",
    "Kundennummer: 4711-23",
    "Ihr Einkommensteuerbescheid für 2025",
    "Sehr geehrte Frau Müller,",
];

/// An archive in memory holding these letters, each as its own PDF; and their ids.
fn archive_with(name: &str, letters: &[&[&str]]) -> (Archive, Vec<i64>) {
    let folder = test_folder(name);
    let mut archive = Archive::in_memory().unwrap();
    let mut ids = Vec::new();
    for (number, lines) in letters.iter().enumerate() {
        let pdf = folder.join(format!("letter_{number}.pdf"));
        write_pdf(&pdf, lines);
        let Added::New(id) = archive.add_pdf(&pdf).unwrap() else {
            panic!("expected a new letter");
        };
        ids.push(id);
    }
    (archive, ids)
}

/// The ids of the letters found, in the order `search` gives them.
fn found(archive: &mut Archive, words: &str) -> Vec<i64> {
    let hits = archive.search(words).unwrap();
    hits.iter().map(|hit| hit.id).collect()
}

#[test]
fn test_search_finds_part_of_a_word() {
    let (mut archive, ids) = archive_with("search_part", &[&TAX_LETTER]);
    let hits = archive.search("steuer").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, ids[0]);
    assert_eq!(hits[0].file_path, "letter_0.pdf");
    let snippet = &hits[0].snippet;
    assert!(
        snippet.contains("Einkommen[steuer]bescheid"),
        "got: {snippet}"
    );
}

#[test]
fn test_search_ignores_case_and_umlauts() {
    let (mut archive, ids) = archive_with("search_case", &[&TAX_LETTER]);
    assert_eq!(found(&mut archive, "FINANZAMT"), ids);
    assert_eq!(found(&mut archive, "muller"), ids);
}

#[test]
fn test_search_needs_every_word() {
    let (mut archive, ids) = archive_with("search_every", &[&TAX_LETTER, &LETTER]);
    assert_eq!(found(&mut archive, "Müller").len(), 2); // both letters
    assert_eq!(found(&mut archive, "Müller Finanzamt"), vec![ids[0]]);
    assert!(found(&mut archive, "Finanzamt Katze").is_empty());
}

/// `-`, `:`, `*` and `"` are FTS5 operators; typed in a search they are just text.
#[test]
fn test_search_treats_symbols_as_text() {
    let (mut archive, ids) = archive_with("search_symbols", &[&TAX_LETTER]);
    assert_eq!(found(&mut archive, "4711-23"), ids);
    assert_eq!(found(&mut archive, "Kundennummer:"), ids);
    assert!(found(&mut archive, "4711-99").is_empty());
    assert!(found(&mut archive, "\"Kunden* -OR:").is_empty()); // no syntax error
}

#[test]
fn test_search_skips_short_words() {
    let (mut archive, ids) = archive_with("search_short", &[&TAX_LETTER]);
    assert_eq!(found(&mut archive, "Nr 4711"), ids); // "Nr" is skipped
    assert!(found(&mut archive, "ab").is_empty());
    assert!(found(&mut archive, "").is_empty());
}

#[test]
fn test_search_query_quotes_every_word() {
    assert_eq!(
        search_query("Nr 4711-23 a\"b").as_deref(),
        Some(r#""4711-23" "a""b""#)
    );
    assert_eq!(search_query("ab cd"), None);
}

/// The triggers keep the index in step when a letter's text changes or it is deleted.
#[test]
fn test_search_follows_changes_and_deletes() {
    let (mut archive, ids) = archive_with("search_changes", &[&TAX_LETTER]);
    diesel::update(documents::table.find(ids[0]))
        .set(documents::text.eq("Mahnung"))
        .execute(&mut archive.db)
        .unwrap();
    assert_eq!(found(&mut archive, "Mahnung"), ids);
    assert!(found(&mut archive, "Finanzamt").is_empty());

    diesel::delete(documents::table.find(ids[0]))
        .execute(&mut archive.db)
        .unwrap();
    assert!(found(&mut archive, "Mahnung").is_empty());
}

/// A database made before the index existed: its letters become searchable on opening.
#[test]
fn test_open_indexes_letters_stored_before() {
    let db_file = test_folder("search_old_db").join("archive.db");
    let mut old = SqliteConnection::establish(db_file.to_str().unwrap()).unwrap();
    old.batch_execute(CREATE_DOCUMENTS).unwrap();
    diesel::insert_into(documents::table)
        .values((
            documents::file_path.eq("alt.pdf"),
            documents::sha256.eq("abc"),
            documents::text.eq("Brief vom Finanzamt"),
            documents::text_source.eq("pdfkit"),
            documents::language.eq("de"),
        ))
        .execute(&mut old)
        .unwrap();
    drop(old); // closes the database, like the app quitting

    let mut archive = Archive::open(&db_file).unwrap();
    assert_eq!(found(&mut archive, "Finanzamt").len(), 1);
}

/// Template: stores a real letter in data/archive.db and prints its text.
/// Run with `cargo test -- --ignored --nocapture`.
#[test]
#[ignore]
fn test_add_pdf_template() {
    let mut archive = Archive::open(Path::new("data/archive.db")).unwrap();
    let added = archive.add_pdf(Path::new("data/IMG_9549.pdf")).unwrap();
    archive.add_pdf(Path::new("data/IMG_7280.pdf")).unwrap();
    let (Added::New(id) | Added::AlreadyStored(id)) = added;
    let documents = archive.find_all(45).unwrap();
    let document = archive.document(id).unwrap().unwrap();
    println!("{added:?}: {}\n\n{}", document.file_path, document.text);
}
#[test]
#[ignore]
fn test_add_pdfs() {
    let mut archive = Archive::open(Path::new("data/archive.db")).unwrap();
    archive.add_pdf(Path::new("data/IMG_9549.pdf")).unwrap();
    archive.add_pdf(Path::new("data/IMG_7280.pdf")).unwrap();
    let documents = archive.find_all(45).unwrap();
    assert!(found(&mut archive, "Mahnung").is_empty());
    assert!(documents.len() >= 2);
}


#[test]
#[ignore]
fn test_new() {
    let mut archive = Archive::new(Path::new("data/archive.db")).unwrap();
  
}
