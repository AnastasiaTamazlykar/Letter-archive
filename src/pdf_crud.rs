use std::{error::Error, path::Path};

use diesel::{
    prelude::*,
    sql_types::{BigInt, Text, Nullable},
};
#[cfg(test)]
use diesel::{connection::SimpleConnection, sql_query, sql_types::Bool};
use objc2::AnyThread;
use objc2_foundation::{NSString, NSURL};
use objc2_pdf_kit::PDFDocument;
use sha2::{Digest, Sha256};

use crate::summarize_with_ai::LLMAnswer;
use crate::constants;


table! {
    documents (id) {
        id -> BigInt,        // BigInt = i64, matching your `id: i64`
        file_path -> Text,
        category -> Text,
        sha256 -> Text,
        text -> Text,
        summary -> Text,
        language -> Text,
        created_at -> Text,
        letter_date -> Nullable<Text>,
    }
}

// The tests live in `src/pdf_crud/tests.rs`.
#[cfg(test)]
mod tests;

// Sorting and filtering the letters, in `src/pdf_crud/sort_filter.rs`.
mod sort_filter;
// `pub use`: others can write `pdf_crud::DateOrder`, not knowing the module inside.
pub use sort_filter::DateOrder;

/// One row per letter. `sha256` is `UNIQUE`: the same file can never be stored twice.
/// `IF NOT EXISTS` makes it safe to run every time the database is opened.
/// `created_at` is filled in by SQLite itself (in UTC).
/// It must match `schema.rs`, which tells Diesel what this table looks like.
#[cfg(test)]
const CREATE_DOCUMENTS: &str = "
    CREATE TABLE IF NOT EXISTS documents (
        id          INTEGER PRIMARY KEY,
        file_path   TEXT NOT NULL,
        category    TEXT NOT NULL DEFAULT '',
        sha256      TEXT NOT NULL UNIQUE,
        text        TEXT NOT NULL,
        summary TEXT NOT NULL,
        language    TEXT NOT NULL,
        created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        letter_date  TEXT
    )";

/// The search index over `documents.text`, built by SQLite's full-text engine FTS5.
/// - A "virtual table": it looks like a table, but FTS5 runs it.
/// - `content = 'documents'`: the index reads the text from `documents` instead of
///   keeping a second copy; `content_rowid = 'id'` says which column links the two.
/// - `trigram`: the text is indexed in pieces of three letters, so a search finds any part
///   of a word ("steuer" finds "Einkommensteuerbescheid"), which German compounds need.
///   Upper and lower case count the same; `remove_diacritics 1` lets "muller" find "Müller".
///
/// The triggers are small rules inside SQLite: whenever a row of `documents` is added,
/// changed or deleted, they update the index too, so the Rust code can't forget it.
#[cfg(test)]
const CREATE_SEARCH: &str = "
    CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
        text,
        content = 'documents',
        content_rowid = 'id',
        tokenize = 'trigram remove_diacritics 1'
    );
    CREATE TRIGGER IF NOT EXISTS documents_fts_insert AFTER INSERT ON documents BEGIN
        INSERT INTO documents_fts (rowid, text) VALUES (new.id, new.text);
    END;
    CREATE TRIGGER IF NOT EXISTS documents_fts_delete AFTER DELETE ON documents BEGIN
        INSERT INTO documents_fts (documents_fts, rowid, text) VALUES ('delete', old.id, old.text);
    END;
    CREATE TRIGGER IF NOT EXISTS documents_fts_update AFTER UPDATE ON documents BEGIN
        INSERT INTO documents_fts (documents_fts, rowid, text) VALUES ('delete', old.id, old.text);
        INSERT INTO documents_fts (rowid, text) VALUES (new.id, new.text);
    END;
";

/// The trigram index can't find anything shorter than three letters.
const MIN_WORD_LEN: usize = 3;


#[derive(Debug, Queryable, Selectable)]
#[diesel(table_name = documents)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Document {
    pub id: i64,
    pub file_path: String,
    #[allow(dead_code)]
    pub sha256: String,
    #[allow(dead_code)]
    pub text: String,
    pub summary: String,
    #[allow(dead_code)]
    pub language: String,
    pub letter_date: Option<String>,
    pub category: String,
}

/// Where a stored letter's PDF should be, and its fingerprint; what the rescan needs.
/// `Queryable` fills the fields in order from the columns `stored_files` selects.
#[derive(Debug, Queryable)]
pub struct StoredFile {
    pub id: i64,
    pub file_path: String,
    pub sha256: String,
}

#[derive(Debug, QueryableByName)]
pub struct SearchHit {
    #[diesel(sql_type = BigInt)]
    pub id: i64,
    #[diesel(sql_type = Text)]
    pub file_path: String,
    #[diesel(sql_type = Text)]
    pub category: String,
    /// A short piece of the text around the match, with the match in [brackets].
    #[diesel(sql_type = Text)]
    #[allow(dead_code)]
    pub snippet: String,
    #[diesel(sql_type = Nullable<Text>)]
    pub letter_date: Option<String>,
    #[diesel(sql_type = Text)]
    pub summary: String,
}

/// The open database.
pub struct PdfCrud {
    db: SqliteConnection,
}

// `impl` holds the functions that belong to `PdfCrud`. Those taking `self` are called
// on one (`pdf_crud.insert_letter(..)`); the others build one (`PdfCrud::open(..)`).
// Most take `&mut self`: Diesel needs the connection to itself (`&mut`) while a query
// runs, so the borrow checker guarantees no two queries share it at once.
impl PdfCrud {
    /// Opens the database file, creating it if missing, and makes sure the table exists.
    /// The connection closes by itself when the `PdfCrud` goes out of scope (Rust's `Drop`).
    #[cfg(test)]
    pub fn new(db_file: &Path) -> Result<Self, Box<dyn Error>> {
        let db_file = std::path::absolute(db_file)?;
        let url = db_file.to_str().ok_or("database path is not UTF-8")?;
        let db = SqliteConnection::establish(url)?;
        let mut arch = PdfCrud { db };
        arch.db.batch_execute(CREATE_DOCUMENTS)?;
        create_search_index(&mut arch.db)?;
        Ok(arch)
    }
    pub fn open(db_file: &Path) -> Result<Self, Box<dyn Error>> {
        let db_file = std::path::absolute(db_file)?;
        if !db_file.exists() {
            panic!("no '{}' db found", db_file.to_str().unwrap_or(""));
        }
        let url = db_file.to_str().ok_or("database path is not UTF-8")?;
        let db = SqliteConnection::establish(url)?;
        Ok(PdfCrud { db })
    }

    fn verify_date(date: &str)->Option<String>{
        use jiff::civil::Date;
        date.parse::<Date>().ok().map(|d| d.to_string())
    }

    pub fn insert_letter(
        &mut self,
        relative: &Path,
        hash: &str,
        text: &str,
        answer: &LLMAnswer,
    ) -> Result<i64, Box<dyn Error>> {
        // let store date as text in iso format YYYY-MM-DD
        let iso_letter_date = Self::verify_date(answer.letter_date.as_str());
        let id = diesel::insert_into(documents::table)
            .values((
                documents::file_path.eq(relative.to_str().ok_or("path is not UTF-8")?),
                documents::sha256.eq(hash),
                documents::text.eq(text),
                documents::summary.eq(&answer.summary),
                documents::language.eq(constants::LETTERS.short),
                documents::letter_date.eq(iso_letter_date),
                documents::category.eq(category_of(relative)),
            ))
            .returning(documents::id)
            .get_result(&mut self.db)?;
        Ok(id)
    }

    /// Every stored letter's id, path and fingerprint, with no limit.
    pub fn stored_files(&mut self) -> QueryResult<Vec<StoredFile>> {
        documents::table
            .select((documents::id, documents::file_path, documents::sha256))
            .load(&mut self.db)
    }
    /// Changes where a letter's PDF is, for one letter, and so its category: the subfolder
    /// it is in now. `relative_path` is the path inside the archive folder, like
    /// "Gesundheit/teeth/Zahnarzt.pdf", not the full one: the database only keeps that part.
    pub fn update_file_path_category(
        &mut self,
        id: i64,
        relative_path: &Path,
    ) -> Result<(), Box<dyn Error>> {
        let path = relative_path.to_str().ok_or("path is not UTF-8")?;
        diesel::update(documents::table.find(id)) // UPDATE documents WHERE id = ?
            .set((
                documents::file_path.eq(path),
                documents::category.eq(category_of(relative_path)),
            ))
            .execute(&mut self.db)?;
        Ok(())
    }
    /// Overwrites the letter date of letter `id` with `date`, written `YYYY-MM-DD`.
    /// Spaces around it are ignored; an empty text removes the date. Anything else that is
    /// not a real date is an error and the old date stays.
    pub fn update_letter_date(&mut self, id: i64, date: &str) -> Result<(), Box<dyn Error>> {
        let date = date.trim();
        // `None` is stored as NULL, "no date".
        let iso = match date {
            "" => None,
            _ => Some(Self::verify_date(date).ok_or("use the format YYYY-MM-DD, like 2025-11-25")?),
        };
        // UPDATE documents SET letter_date = ? WHERE id = ?. The values are bound
        // parameters, never pasted into the SQL text, so what is typed cannot change the query.
        let changed = diesel::update(documents::table.find(id))
            .set(documents::letter_date.eq(iso))
            .execute(&mut self.db)?;
        if changed == 0 {
            return Err("this letter is not in the archive".into()); // no row had this id
        }
        Ok(())
    }

    /// Overwrites the summary of letter `id` with `summary`; spaces around it are ignored.
    /// Any text is allowed, even an empty one: quotes or `;` stay plain text, because the
    /// value is a bound parameter and never becomes part of the SQL.
    pub fn update_summary(&mut self, id: i64, summary: &str) -> Result<(), Box<dyn Error>> {
        // UPDATE documents SET summary = ? WHERE id = ?
        let changed = diesel::update(documents::table.find(id))
            .set(documents::summary.eq(summary.trim()))
            .execute(&mut self.db)?;
        if changed == 0 {
            return Err("this letter is not in the archive".into()); // no row had this id
        }
        Ok(())
    }


    pub fn delete_letter(&mut self, id: i64) -> QueryResult<()> {
        diesel::delete(documents::table.find(id)).execute(&mut self.db)?;
        Ok(())
    }

    /// The id of the letter with this fingerprint, if there is one.
    pub fn find_by_hash(&mut self, hash: &str) -> QueryResult<Option<i64>> {
        documents::table
            .filter(documents::sha256.eq(hash)) // WHERE sha256 = ?
            .select(documents::id) // SELECT id
            .first(&mut self.db) // LIMIT 1, and run it
            // `first` fails when there is no row; `optional` turns that into `None`.
            .optional()
    }

    /// The id of the letter whose PDF is at this path inside the archive folder
    /// ("Finanzamt/Brief.pdf"), if there is one.
    pub fn find_by_file_path(&mut self, relative_path: &str) -> QueryResult<Option<i64>> {
        documents::table
            .filter(documents::file_path.eq(relative_path))
            .select(documents::id)
            .first(&mut self.db)
            .optional()
    }



    pub fn text_of(&mut self, id: i64) -> QueryResult<Option<String>> {
        documents::table
            .find(id)
            .select(documents::text)
            .first(&mut self.db)
            .optional()
    }
}

/// Creates the search index if it is missing. A database made before the index existed
/// already has letters, so the new index is filled with them once (`rebuild`).
#[cfg(test)]
fn create_search_index(db: &mut SqliteConnection) -> QueryResult<()> {
    let existed = table_exists(db, "documents_fts")?;
    db.batch_execute(CREATE_SEARCH)?; // `batch_execute`: several statements at once
    if !existed {
        sql_query("INSERT INTO documents_fts (documents_fts) VALUES ('rebuild')").execute(db)?;
    }
    Ok(())
}

/// The one column `table_exists` reads back. Even a single value needs a struct
/// when it comes from raw SQL, so Diesel knows its name and type.
#[cfg(test)]
#[derive(QueryableByName)]
struct Exists {
    #[diesel(sql_type = Bool)]
    found: bool,
}

/// True if the database has a table with this name (`sqlite_master` lists them all).
#[cfg(test)]
fn table_exists(db: &mut SqliteConnection, name: &str) -> QueryResult<bool> {
    sql_query("SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = ?) AS found")
        .bind::<Text, _>(name)
        .get_result::<Exists>(db)
        .map(|row| row.found)
}

/// Turns what the user typed into an FTS5 query in which every word must appear.
/// Each word goes in double quotes, so characters like `-`, `:` or `*` count as plain
/// text and not as FTS5 operators (unquoted, "4711-23" would be a syntax error).
/// A `"` inside a word is written twice, the way FTS5 escapes it.
/// Words under three letters are left out: the trigram index can't find them, and one
/// of them would make the whole search find nothing. `None` if no word is left.
fn search_query(words: &str) -> Option<String> {
    let quoted: Vec<String> = words
        .split_whitespace()
        .filter(|word| word.chars().count() >= MIN_WORD_LEN)
        .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
        .collect();
    if quoted.is_empty() {
        None
    } else {
        Some(quoted.join(" ")) // words side by side mean AND in FTS5
    }
}

/// Reads all the text of a PDF with Apple's PDFKit, page after page.
/// Fails if there is no text (for example a scan without a text layer):
/// such a letter would be useless for analysis, so it should not slip in unnoticed.
pub fn extract_text(pdf: &Path) -> Result<String, Box<dyn Error>> {
    let path = pdf.to_str().ok_or("path is not UTF-8")?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    // `alloc` reserves memory for the object, `initWithURL` fills it from the file;
    // `None` means PDFKit could not open it. `unsafe`: Rust can't check Apple's code.
    let document =
        unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) }.ok_or("cannot open PDF")?;
    let text = unsafe { document.string() }
        .map(|text| text.to_string())
        .unwrap_or_default();
    let text = text.trim();
    if text.is_empty() {
        return Err("PDF has no text (maybe a scan without a text layer)".into());
    }
    Ok(text.to_string())
}

/// The SHA-256 fingerprint of a file's bytes, as 64 hex characters.
/// Same bytes give the same fingerprint, whatever the file is called.
pub fn file_sha256(path: &Path) -> Result<String, Box<dyn Error>> {
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(&bytes);
    // Each byte becomes two hex digits, e.g. 10 -> "0a".
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}



/// The category of a letter: the folder part of its path inside the archive folder,
/// "Gesundheit/teeth" for "Gesundheit/teeth/Zahnarzt.pdf"; "" for a PDF lying right in
/// the archive folder.
fn category_of(relative: &Path) -> String {
    relative
        .parent()
        .map(|folder| folder.to_string_lossy().into_owned())
        .unwrap_or_default() // `None` becomes ""
}
