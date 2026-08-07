use proptest::prelude::*;
use tidyrs_core::{ParseOptions, TidyParser};
use tidyrs_orc::OrcParser;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let parser = OrcParser::new();
        let _ = parser.parse(&bytes, "fuzz.orc", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_bytes_ending_in_the_orc_magic(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        // The magic trailer alone doesn't make a well-formed file — the
        // footer/postscript structure before it still has to parse. This
        // forces the magic-trailer fast path in parse() and exercises
        // orc-rust's own error handling on the corrupt structure that
        // precedes it.
        let mut forced = bytes;
        forced.extend_from_slice(b"ORC");
        let parser = OrcParser::new();
        let _ = parser.parse(&forced, "fuzz.orc", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_a_mutated_real_orc_file(
        mutations in prop::collection::vec((any::<usize>(), any::<u8>()), 0..64)
    ) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/orc/alltypes.snappy.orc");
        let mut bytes = std::fs::read(path).expect("fixture must exist");
        for (pos, byte) in mutations {
            if bytes.is_empty() {
                break;
            }
            let idx = pos % bytes.len();
            bytes[idx] = byte;
        }
        let parser = OrcParser::new();
        let _ = parser.parse(&bytes, "fuzz.orc", &ParseOptions::new());
    }
}
