use proptest::prelude::*;
use tidyrs_core::{ParseOptions, TidyParser};
use tidyrs_json::JsonXmlParser;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn never_panics_on_arbitrary_bytes_as_json(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        // Force JSON detection regardless of content so the parser body
        // itself (not just sniff()) gets exercised on garbage.
        let mut forced = b"{".to_vec();
        forced.extend(bytes);
        let parser = JsonXmlParser::new();
        let _ = parser.parse(&forced, "fuzz.json", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_arbitrary_bytes_as_xml(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let mut forced = b"<".to_vec();
        forced.extend(bytes);
        let parser = JsonXmlParser::new();
        let _ = parser.parse(&forced, "fuzz.xml", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_mutated_real_json(
        mutations in prop::collection::vec((any::<usize>(), any::<u8>()), 0..64)
    ) {
        let mut bytes = br#"[{"id":1,"tags":["a","b"]},{"id":2,"tags":{"x":"y"}}]"#.to_vec();
        for (pos, byte) in mutations {
            if bytes.is_empty() {
                break;
            }
            let idx = pos % bytes.len();
            bytes[idx] = byte;
        }
        let parser = JsonXmlParser::new();
        let _ = parser.parse(&bytes, "fuzz.json", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_deeply_nested_arrays(depth in 0usize..200) {
        let mut s = String::new();
        for _ in 0..depth {
            s.push('[');
        }
        s.push('1');
        for _ in 0..depth {
            s.push(']');
        }
        let parser = JsonXmlParser::new();
        let _ = parser.parse(s.as_bytes(), "fuzz.json", &ParseOptions::new());
    }
}
