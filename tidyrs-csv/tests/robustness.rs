//! Property-based "never panics" checks. This isn't full-blown fuzzing
//! (no coverage-guided corpus, no cargo-fuzz/libFuzzer harness) but the
//! same underlying idea: throw adversarial input at the parser and assert
//! it always returns a `Result` — never panics — regardless of what junk
//! it's handed. A CLI tool meant to run unattended in a pipeline must
//! never crash the process on a malformed file; an `Err` is fine, a
//! panic is not.

use proptest::prelude::*;
use tidyrs_core::{ParseOptions, TidyParser};
use tidyrs_csv::CsvParser;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let parser = CsvParser::new();
        let _ = parser.parse(&bytes, "fuzz.csv", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_mutated_real_csv(
        mutations in prop::collection::vec((any::<usize>(), any::<u8>()), 0..64)
    ) {
        let mut bytes = b"name;age;city;notes\nAlice;30;Paris;likes tea\nBob;;Lyon\nCharlotte;25;Marseille;\"loves; markets\";extra\n".to_vec();
        for (pos, byte) in mutations {
            if bytes.is_empty() {
                break;
            }
            let idx = pos % bytes.len();
            bytes[idx] = byte;
        }
        let parser = CsvParser::new();
        let _ = parser.parse(&bytes, "fuzz.csv", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_arbitrary_delimiter_option(delim in "\\PC") {
        let bytes = b"a,b,c\n1,2,3\n".to_vec();
        let parser = CsvParser::new();
        let opts = ParseOptions::new().set("delimiter", delim);
        let _ = parser.parse(&bytes, "fuzz.csv", &opts);
    }
}
