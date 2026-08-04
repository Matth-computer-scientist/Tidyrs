//! Proves format detection works from file *content* alone, with no
//! filename/extension hint at all — the spec's "identify the real type of
//! a file beyond its extension" requirement. Every `sniff()` implementation
//! takes `filename: Option<&str>`; this exercises the `None` path across
//! every stable-ish format so an extension-less file (e.g. piped stdin, or
//! a file a user renamed) still gets classified correctly.

use tidyrs_core::FormatRegistry;

fn build_registry() -> FormatRegistry {
    let mut reg = FormatRegistry::new();
    reg.register(Box::new(tidyrs_csv::CsvParser::new()));
    reg.register(Box::new(tidyrs_xlsx::XlsxParser::new()));
    reg.register(Box::new(tidyrs_json::JsonXmlParser::new()));
    reg.register(Box::new(tidyrs_fixed::FixedWidthParser::new()));
    reg.register(Box::new(tidyrs_pdf::PdfParser::new()));
    reg
}

fn fixture(dir: &str, name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures").join(dir).join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {dir}/{name}: {e}"))
}

#[test]
fn csv_is_detected_from_content_with_no_filename() {
    let registry = build_registry();
    let bytes = fixture("csv", "pipe_delimited.csv");
    let detection = registry.detect(&bytes, None).expect("should detect a format with no filename hint");
    assert_eq!(detection.parser.format_name(), "csv");
}

#[test]
fn xlsx_is_detected_from_magic_bytes_with_no_filename() {
    let registry = build_registry();
    let bytes = fixture("xlsx", "multi_sheet_different_shapes.xlsx");
    let detection = registry.detect(&bytes, None).expect("should detect a format with no filename hint");
    assert_eq!(detection.parser.format_name(), "xlsx");
}

#[test]
fn json_is_detected_from_content_with_no_filename() {
    let registry = build_registry();
    let bytes = fixture("json", "single_object.json");
    let detection = registry.detect(&bytes, None).expect("should detect a format with no filename hint");
    assert_eq!(detection.parser.format_name(), "json");
}

#[test]
fn xml_is_detected_from_content_with_no_filename() {
    let registry = build_registry();
    let bytes = fixture("xml", "products.xml");
    let detection = registry.detect(&bytes, None).expect("should detect a format with no filename hint");
    assert_eq!(detection.parser.format_name(), "json"); // JsonXmlParser handles both
}

#[test]
fn pdf_is_detected_from_magic_bytes_with_no_filename() {
    let registry = build_registry();
    let bytes = fixture("pdf", "simple_table.pdf");
    let detection = registry.detect(&bytes, None).expect("should detect a format with no filename hint");
    assert_eq!(detection.parser.format_name(), "pdf");
}

#[test]
fn a_misleading_extension_does_not_override_real_content() {
    // A .txt-named file that's actually CSV content should still be
    // detected as CSV: the extension hint only nudges the score, it
    // never overrides what the content itself says.
    let registry = build_registry();
    let bytes = fixture("csv", "pipe_delimited.csv");
    let detection = registry.detect(&bytes, Some("data.txt")).expect("should still detect csv");
    assert_eq!(detection.parser.format_name(), "csv");
}
