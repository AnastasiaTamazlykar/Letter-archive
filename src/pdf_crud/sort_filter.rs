//! The archive table's sort and filter, done by the database: letters ordered by letter
//! date, and only those of one category if one is chosen.
//! A child module of `pdf_crud` (`pdf_crud.rs` declares `mod sort_filter;`), so it may
//! use `PdfCrud`'s private connection (`self.db`) and the private `search_query`.

use diesel::{
    prelude::*,
    sql_query,
    sql_types::{Nullable, Text},
};

use super::{Document, PdfCrud, SearchHit, search_query};
use crate::schema::documents;

/// How letters are ordered by letter date. Letters without a date always come last.
/// `Clone, Copy`: a tiny value, so it is copied instead of moved when passed on.
/// `PartialEq`: lets the app compare two orders, to see whether the user picked another.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum DateOrder {
    /// The table starts with the newest letters on top.
    #[default]
    NewestFirst,
    OldestFirst,
}

impl DateOrder {
    /// The SQL word for this order, for the raw SQL of `search_letters`.
    fn sql(self) -> &'static str {
        match self {
            DateOrder::NewestFirst => "DESC",
            DateOrder::OldestFirst => "ASC",
        }
    }
}

impl PdfCrud {
    /// Up to `limit` letters, ordered by letter date. With a `category`, only the letters
    /// in it or in one of its subfolders: "Gesundheit" finds "Gesundheit/teeth" too.
    pub fn find_letters(
        &mut self,
        order: DateOrder,
        category: Option<&str>,
        limit: i64,
    ) -> QueryResult<Vec<Document>> {
        // `into_boxed`: the query gets one fixed type, so the `if` and `match` below can
        // add to it step by step. Without it, each added part would change its type.
        let mut query = documents::table
            .select(Document::as_select())
            .limit(limit)
            .into_boxed();
        if let Some(category) = category {
            // WHERE category = ? OR category LIKE ? ESCAPE '\'
            query = query.filter(
                documents::category.eq(category).or(documents::category
                    .like(subfolders_pattern(category))
                    .escape('\\')),
            );
        }
        query = match order {
            DateOrder::NewestFirst => query.order((
                documents::letter_date.is_null(),
                documents::letter_date.desc(),
            )),
            DateOrder::OldestFirst => query.order((
                documents::letter_date.is_null(),
                documents::letter_date.asc(),
            )),
        };
        query
            .then_order_by(documents::id)
            .load(&mut self.db)
    }

    /// Every category that has letters, sorted, each once: the choices of the category
    /// menu. Letters lying right in the output folder have the category "" and add none.
    pub fn categories(&mut self) -> QueryResult<Vec<String>> {
        documents::table
            .select(documents::category)
            .filter(documents::category.ne(""))
            .distinct()
            .order(documents::category)
            .load(&mut self.db)
    }

    /// Like `search`, but ordered by letter date instead of best match, and with a
    /// `category` only the letters in it or in one of its subfolders.
    pub fn search_letters(
        &mut self,
        words: &str,
        order: DateOrder,
        category: Option<&str>,
    ) -> QueryResult<Vec<SearchHit>> {
        let Some(query) = search_query(words) else {
            return Ok(Vec::new()); // nothing searchable was typed
        };
        // FTS5's `MATCH`, `snippet` and `bm25` are unknown to Diesel, so this stays raw SQL
        // (see `search`). A `?` can only stand for a value, not for a word like DESC, so the
        // order goes into the text with `format!`. That is safe: it is always "ASC" or
        // "DESC", never something typed. Everything typed goes in through `bind`.
        // `? IS NULL OR …`: with no category (`None` is bound as NULL) every letter passes.
        // Same date: the better match comes first (`bm25`, lower is better).
        let sql = format!(
            "SELECT documents.id AS id, documents.summary AS summary,
                    documents.letter_date AS letter_date, documents.file_path AS file_path,
                    documents.category AS category,
                    snippet(documents_fts, 0, '[', ']', '…', 64) AS snippet
             FROM documents_fts JOIN documents ON documents.id = documents_fts.rowid
             WHERE documents_fts MATCH ?
               AND (? IS NULL OR documents.category = ?
                    OR documents.category LIKE ? ESCAPE '\\')
             ORDER BY documents.letter_date IS NULL, documents.letter_date {},
                      bm25(documents_fts)",
            order.sql()
        );
        // The `bind`s fill in the `?` from top to bottom.
        sql_query(sql)
            .bind::<Text, _>(query)
            .bind::<Nullable<Text>, _>(category)
            .bind::<Nullable<Text>, _>(category)
            .bind::<Nullable<Text>, _>(category.map(subfolders_pattern))
            .load(&mut self.db)
    }
}

/// The LIKE pattern for every subfolder of `category`: "Gesundheit/%" finds
/// "Gesundheit/teeth". In LIKE, `%` and `_` are wildcards; a `\` in front of one in a
/// folder's name (the `ESCAPE` character) makes it count as a plain letter.
fn subfolders_pattern(category: &str) -> String {
    let plain = category
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("{plain}/%")
}
