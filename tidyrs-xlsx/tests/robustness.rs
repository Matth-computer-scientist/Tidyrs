use proptest::prelude::*;
use tidyrs_core::{ParseOptions, TidyParser};
use tidyrs_xlsx::XlsxParser;

fn real_xlsx_bytes() -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/xlsx/multi_sheet_different_shapes.xlsx"))
        .expect("fixture must exist")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let parser = XlsxParser::new();
        let _ = parser.parse(&bytes, "fuzz.xlsx", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_mutated_real_xlsx(
        mutations in prop::collection::vec((any::<usize>(), any::<u8>()), 0..32)
    ) {
        let mut bytes = real_xlsx_bytes();
        for (pos, byte) in mutations {
            if bytes.is_empty() {
                break;
            }
            let idx = pos % bytes.len();
            bytes[idx] = byte;
        }
        let parser = XlsxParser::new();
        let _ = parser.parse(&bytes, "fuzz.xlsx", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_truncated_real_xlsx(cut_at in 0usize..2000) {
        let bytes = real_xlsx_bytes();
        let cut = cut_at.min(bytes.len());
        let parser = XlsxParser::new();
        let _ = parser.parse(&bytes[..cut], "fuzz.xlsx", &ParseOptions::new());
    }
}
