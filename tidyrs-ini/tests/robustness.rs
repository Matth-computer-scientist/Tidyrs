use proptest::prelude::*;
use tidyrs_core::{ParseOptions, TidyParser};
use tidyrs_ini::IniParser;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let parser = IniParser::new();
        let _ = parser.parse(&bytes, "fuzz.ini", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_random_unicode_text(s in "\\PC*") {
        let parser = IniParser::new();
        let _ = parser.parse(s.as_bytes(), "fuzz.ini", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_mutated_real_ini(
        mutations in prop::collection::vec((any::<usize>(), any::<u8>()), 0..64)
    ) {
        let mut bytes = b"[default]\nhost=localhost\nport=5432\n[prod]\nhost=db.example.com\n".to_vec();
        for (pos, byte) in mutations {
            if bytes.is_empty() {
                break;
            }
            let idx = pos % bytes.len();
            bytes[idx] = byte;
        }
        let parser = IniParser::new();
        let _ = parser.parse(&bytes, "fuzz.ini", &ParseOptions::new());
    }
}
