use proptest::prelude::*;
use tidyrs_core::{ParseOptions, TidyParser};
use tidyrs_sqlite::SqliteParser;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let parser = SqliteParser::new();
        let _ = parser.parse(&bytes, "fuzz.db", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_bytes_with_the_sqlite_magic_header(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        // The header alone doesn't make a valid database — the rest of
        // the page structure still needs to be well-formed. This forces
        // the magic-header fast path in parse() and exercises rusqlite's
        // own error handling on the corrupt page data that follows.
        let mut forced = b"SQLite format 3\0".to_vec();
        forced.extend(bytes);
        let parser = SqliteParser::new();
        let _ = parser.parse(&forced, "fuzz.db", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_a_mutated_real_database(
        mutations in prop::collection::vec((any::<usize>(), any::<u8>()), 0..64)
    ) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/sqlite/single_table.db");
        let mut bytes = std::fs::read(path).expect("fixture must exist (run gen_fixtures example)");
        for (pos, byte) in mutations {
            if bytes.is_empty() {
                break;
            }
            let idx = pos % bytes.len();
            bytes[idx] = byte;
        }
        let parser = SqliteParser::new();
        let _ = parser.parse(&bytes, "fuzz.db", &ParseOptions::new());
    }
}
