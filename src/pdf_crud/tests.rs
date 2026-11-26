//! Tests for the letter archive. The test PDFs are written here with real text, so the
//! tests need no photos and no Vision; most use a database that lives only in memory.

// `super::*` brings in everything `archive.rs` can see, private items included.
use super::*;
use crate::test_support::{letter_by_id, test_folder, write_pdf};

// A method added to `PdfCrud` here, in the tests file: Rust allows an `impl` block in any
// module of the same crate, and as a child module this one may set the private `db`
// field. So `pdf_crud.rs` itself carries no test-only code.
impl PdfCrud {
    /// A database that lives only in memory, with its tables, and is gone when dropped.
    /// `:memory:` is SQLite's special name for that.
    fn in_memory() -> Result<Self, Box<dyn Error>> {
        let mut db = SqliteConnection::establish(":memory:")?;
        db.batch_execute(CREATE_DOCUMENTS)?;
        create_search_index(&mut db)?;
        Ok(Self { db })
    }
}

// The helpers `test_folder` and `write_pdf` live in `src/test_support.rs`.

/// A made-up LLM answer, so these tests need no LLM.
fn answer(folder: Option<&str>) -> LLMAnswer {
    LLMAnswer {
        folder: folder.map(String::from),
        summary: "Ein Brief an Frau Müller.".to_string(),
        letter_date: "2025-11-25".to_string(),
        file_name: "Brief".to_string(),
    }
}

/// Stores the PDF the way `analyze_place_pdf` does, with a made-up answer; its new id.
/// The PDF's own folder plays the output folder, so the stored path is its file name.
fn store(archive: &mut PdfCrud, pdf: &Path) -> i64 {
    let hash = file_sha256(pdf).unwrap();
    let text = extract_text(pdf).unwrap();
    archive
        .insert_letter(in_own_folder(pdf), &hash, &text, &answer(Some("Finanzamt")))
        .unwrap()
}

/// The path of `pdf` inside its own folder: just its file name, "brief.pdf".
fn in_own_folder(pdf: &Path) -> &Path {
    Path::new(pdf.file_name().unwrap())
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
fn test_insert_letter_stores_path_text_and_answer() {
    let pdf = test_folder("store").join("brief.pdf");
    write_pdf(&pdf, &LETTER);
    let mut archive = PdfCrud::in_memory().unwrap();

    let id = store(&mut archive, &pdf);

    let document = letter_by_id(&mut archive, id).unwrap();
    assert_eq!(document.id, id);
    assert_eq!(document.file_path, "brief.pdf"); // the path inside the output folder
    assert_eq!(document.category, ""); // right in the output folder, no subfolder
    assert_eq!(document.sha256.len(), 64);
    assert!(document.text.contains("Frau Müller"));
    assert_eq!(document.language, "de");
    assert_eq!(document.summary, "Ein Brief an Frau Müller.");
    assert_eq!(document.letter_date.as_deref(), Some("2025-11-25"));
}

/// `sha256` is `UNIQUE`: the database itself refuses the same file twice.
#[test]
fn test_insert_letter_refuses_the_same_file_twice() {
    let pdf = test_folder("twice").join("brief.pdf");
    write_pdf(&pdf, &LETTER);
    let mut archive = PdfCrud::in_memory().unwrap();
    store(&mut archive, &pdf);

    let hash = file_sha256(&pdf).unwrap();
    let relative = in_own_folder(&pdf);
    assert!(archive.insert_letter(relative, &hash, "text", &answer(None)).is_err());
}

/// The same bytes are recognized, even when the file has another name.
#[test]
fn test_find_by_hash_knows_the_same_bytes_under_another_name() {
    let folder = test_folder("by_hash");
    let pdf = folder.join("brief.pdf");
    write_pdf(&pdf, &LETTER);
    let copy = folder.join("copy.pdf");
    std::fs::copy(&pdf, &copy).unwrap();
    let other = folder.join("other.pdf");
    write_pdf(&other, &["Ein anderer Brief"]);
    let mut archive = PdfCrud::in_memory().unwrap();

    let id = store(&mut archive, &pdf);

    let copy_hash = file_sha256(&copy).unwrap();
    assert_eq!(archive.find_by_hash(&copy_hash).unwrap(), Some(id));
    let other_hash = file_sha256(&other).unwrap();
    assert_eq!(archive.find_by_hash(&other_hash).unwrap(), None);
}

#[test]
fn test_stored_files_returns_every_row() {
    let folder = test_folder("stored_files");
    let first = folder.join("first.pdf");
    write_pdf(&first, &LETTER);
    let second = folder.join("second.pdf");
    write_pdf(&second, &["Ein anderer Brief"]);
    let mut archive = PdfCrud::in_memory().unwrap();
    let first_id = store(&mut archive, &first);
    let second_id = store(&mut archive, &second);

    let stored = archive.stored_files().unwrap();

    let ids: Vec<i64> = stored.iter().map(|file| file.id).collect();
    assert_eq!(ids, [first_id, second_id]);
    assert_eq!(stored[0].file_path, "first.pdf");
    assert_eq!(stored[0].sha256, file_sha256(&first).unwrap());
}

#[test]
fn test_find_by_file_path_finds_only_that_letter() {
    let folder = test_folder("find_by_path");
    let first = folder.join("first.pdf");
    write_pdf(&first, &LETTER);
    let second = folder.join("second.pdf");
    write_pdf(&second, &["Ein anderer Brief"]);
    let mut archive = PdfCrud::in_memory().unwrap();
    let first_id = store(&mut archive, &first);
    let second_id = store(&mut archive, &second);

    assert_eq!(archive.find_by_file_path("first.pdf").unwrap(), Some(first_id));
    assert_eq!(archive.find_by_file_path("second.pdf").unwrap(), Some(second_id));
    assert_eq!(archive.find_by_file_path("third.pdf").unwrap(), None);
}

#[test]
fn test_update_file_path_changes_only_that_row() {
    let folder = test_folder("update_path");
    let moved = folder.join("moved.pdf");
    write_pdf(&moved, &LETTER);
    let other = folder.join("other.pdf");
    write_pdf(&other, &["Ein anderer Brief"]);
    let mut archive = PdfCrud::in_memory().unwrap();
    let moved_id = store(&mut archive, &moved);
    let other_id = store(&mut archive, &other);

    let new_path = Path::new("Finanzamt/moved.pdf"); // inside the output folder
    archive.update_file_path_category(moved_id, new_path).unwrap();

    let moved_row = letter_by_id(&mut archive, moved_id).unwrap();
    assert_eq!(moved_row.file_path, "Finanzamt/moved.pdf");
    assert_eq!(moved_row.category, "Finanzamt");
    assert_eq!(letter_by_id(&mut archive, other_id).unwrap().file_path, "other.pdf");
}

/// Two stored letters, both dated 2025-11-25; their ids.
fn two_letters(archive: &mut PdfCrud, name: &str) -> (i64, i64) {
    let folder = test_folder(name);
    let (first, second) = (folder.join("first.pdf"), folder.join("second.pdf"));
    write_pdf(&first, &LETTER);
    write_pdf(&second, &["Ein anderer Brief"]);
    (store(archive, &first), store(archive, &second))
}

/// The stored date of letter `id`.
fn date_of(archive: &mut PdfCrud, id: i64) -> Option<String> {
    letter_by_id(archive, id).unwrap().letter_date
}

#[test]
fn test_update_letter_date_overwrites_the_stored_date() {
    let mut archive = PdfCrud::in_memory().unwrap();
    let (id, _) = two_letters(&mut archive, "date_overwrite");

    archive.update_letter_date(id, "2024-01-31").unwrap();

    assert_eq!(date_of(&mut archive, id).as_deref(), Some("2024-01-31"));
}

#[test]
fn test_update_letter_date_ignores_spaces_around_the_date() {
    let mut archive = PdfCrud::in_memory().unwrap();
    let (id, _) = two_letters(&mut archive, "date_trim");

    archive.update_letter_date(id, "  2024-01-31 ").unwrap();

    assert_eq!(date_of(&mut archive, id).as_deref(), Some("2024-01-31"));
}

#[test]
fn test_update_letter_date_with_empty_text_removes_the_date() {
    let mut archive = PdfCrud::in_memory().unwrap();
    let (id, _) = two_letters(&mut archive, "date_clear");

    archive.update_letter_date(id, "  ").unwrap();

    assert_eq!(date_of(&mut archive, id), None);
}

#[test]
fn test_update_letter_date_refuses_a_bad_date_and_keeps_the_old_one() {
    let mut archive = PdfCrud::in_memory().unwrap();
    let (id, _) = two_letters(&mut archive, "date_bad");

    for bad in ["31.12.2025", "2025-02-30", "tomorrow", "2025-13-01"] {
        let error = archive.update_letter_date(id, bad).unwrap_err();
        assert!(error.to_string().contains("YYYY-MM-DD"), "{bad}: {error}");
    }

    assert_eq!(date_of(&mut archive, id).as_deref(), Some("2025-11-25"));
}

#[test]
fn test_update_letter_date_changes_only_that_row() {
    let mut archive = PdfCrud::in_memory().unwrap();
    let (id, other) = two_letters(&mut archive, "date_one_row");

    archive.update_letter_date(id, "2020-05-06").unwrap();

    assert_eq!(date_of(&mut archive, other).as_deref(), Some("2025-11-25"));
}

#[test]
fn test_update_letter_date_fails_for_an_unknown_letter() {
    let mut archive = PdfCrud::in_memory().unwrap();

    let error = archive.update_letter_date(42, "2025-01-01").unwrap_err();

    assert!(error.to_string().contains("not in the archive"), "got: {error}");
}

/// What is typed never becomes SQL: it is refused as "not a date", and the table and its
/// rows are untouched.
#[test]
fn test_update_letter_date_cannot_be_used_for_sql_injection() {
    let mut archive = PdfCrud::in_memory().unwrap();
    let (id, _) = two_letters(&mut archive, "date_injection");

    for attack in [
        "2025-01-01'; DROP TABLE documents;--",
        "'; UPDATE documents SET letter_date = '1999-01-01';--",
        "2025-01-01' OR '1'='1",
    ] {
        assert!(archive.update_letter_date(id, attack).is_err(), "accepted: {attack}");
    }

    // The table still exists, and both rows still have their date.
    let rows = archive.find_letters(DateOrder::NewestFirst, None,10).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.letter_date.as_deref() == Some("2025-11-25")));
}

#[test]
fn test_delete_letter_removes_the_row_and_its_search_hits() {
    let pdf = test_folder("delete").join("brief.pdf");
    write_pdf(&pdf, &LETTER);
    let mut archive = PdfCrud::in_memory().unwrap();
    let id = store(&mut archive, &pdf);

    archive.delete_letter(id).unwrap();

    // The letter is gone, and so is its text from the search index.
    assert!(letter_by_id(&mut archive, id).is_none());
    assert!(archive.search_letters("Müller", DateOrder::default(), None).unwrap().is_empty());
}

/// A letter stored in a database file is still there after opening it again.
#[test]
fn test_open_keeps_letters_on_disk() {
    let folder = test_folder("on_disk");
    let pdf = folder.join("brief.pdf");
    write_pdf(&pdf, &LETTER);
    let db_file = folder.join("archive.db");

    // `new` creates the file; the archive is dropped at the end of the line,
    // which closes the database.
    let id = store(&mut PdfCrud::new(&db_file).unwrap(), &pdf);

    let mut archive = PdfCrud::open(&db_file).unwrap();
    assert!(letter_by_id(&mut archive, id).is_some());
}

// ── Searching ──

const TAX_LETTER: [&str; 4] = [
    "Finanzamt Berlin",
    "Kundennummer: 4711-23",
    "Ihr Einkommensteuerbescheid für 2025",
    "Sehr geehrte Frau Müller,",
];

/// An archive in memory holding these letters, each as its own PDF; and their ids.
fn archive_with(name: &str, letters: &[&[&str]]) -> (PdfCrud, Vec<i64>) {
    let folder = test_folder(name);
    let mut archive = PdfCrud::in_memory().unwrap();
    let mut ids = Vec::new();
    for (number, lines) in letters.iter().enumerate() {
        let pdf = folder.join(format!("letter_{number}.pdf"));
        write_pdf(&pdf, lines);
        ids.push(store(&mut archive, &pdf));
    }
    (archive, ids)
}

/// The ids of the letters found, in the order `search` gives them.
fn found(archive: &mut PdfCrud, words: &str) -> Vec<i64> {
    let hits = archive.search_letters(words, DateOrder::default(), None).unwrap();
    hits.iter().map(|hit| hit.id).collect()
}

#[test]
fn test_search_finds_part_of_a_word() {
    let (mut archive, ids) = archive_with("search_part", &[&TAX_LETTER]);
    let hits = archive.search_letters("steuer", DateOrder::default(), None).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, ids[0]);
    assert!(hits[0].file_path.ends_with("letter_0.pdf"));
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


#[test]
#[ignore]
fn test_new() {
    PdfCrud::new(Path::new("data/archive.db")).unwrap();
}
