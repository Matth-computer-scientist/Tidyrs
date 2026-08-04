use proptest::prelude::*;
use tidyrs_core::{ParseOptions, TidyParser};
use tidyrs_pdf::PdfParser;

fn real_pdf_bytes() -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/pdf/simple_table.pdf")).expect("fixture must exist")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let parser = PdfParser::new();
        let _ = parser.parse(&bytes, "fuzz.pdf", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_bytes_with_pdf_magic_header(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let mut forced = b"%PDF-1.4\n".to_vec();
        forced.extend(bytes);
        let parser = PdfParser::new();
        let _ = parser.parse(&forced, "fuzz.pdf", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_mutated_real_pdf(
        mutations in prop::collection::vec((any::<usize>(), any::<u8>()), 0..32)
    ) {
        let mut bytes = real_pdf_bytes();
        for (pos, byte) in mutations {
            if bytes.is_empty() {
                break;
            }
            let idx = pos % bytes.len();
            bytes[idx] = byte;
        }
        let parser = PdfParser::new();
        let _ = parser.parse(&bytes, "fuzz.pdf", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_truncated_real_pdf(cut_at in 0usize..3000) {
        let bytes = real_pdf_bytes();
        let cut = cut_at.min(bytes.len());
        let parser = PdfParser::new();
        let _ = parser.parse(&bytes[..cut], "fuzz.pdf", &ParseOptions::new());
    }
}
