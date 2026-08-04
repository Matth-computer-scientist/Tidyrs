use proptest::prelude::*;
use tidyrs_core::{ParseOptions, TidyParser};
use tidyrs_fixed::FixedWidthParser;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn never_panics_on_arbitrary_bytes_fixed_mode(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let parser = FixedWidthParser::new();
        let _ = parser.parse(&bytes, "fuzz.txt", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_arbitrary_bytes_whitespace_mode(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let parser = FixedWidthParser::new();
        let opts = ParseOptions::new().set("mode", "whitespace");
        let _ = parser.parse(&bytes, "fuzz.log", &opts);
    }

    #[test]
    fn never_panics_on_random_unicode_text(text in "\\PC{0,500}") {
        let parser = FixedWidthParser::new();
        let _ = parser.parse(text.as_bytes(), "fuzz.txt", &ParseOptions::new());
    }
}
