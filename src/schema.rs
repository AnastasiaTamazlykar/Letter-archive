diesel::table! {
    documents (id) {
        id -> BigInt,        // BigInt = i64, matching your `id: i64`
        file_path -> Text,
        sha256 -> Text,
        text -> Text,
        summary -> Text,
        language -> Text,
        created_at -> Text,
        category -> Text,
    }
}
