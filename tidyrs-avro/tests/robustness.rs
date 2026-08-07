use proptest::prelude::*;
use tidyrs_avro::AvroParser;
use tidyrs_core::{ParseOptions, TidyParser};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let parser = AvroParser::new();
        let _ = parser.parse(&bytes, "fuzz.avro", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_bytes_starting_with_the_avro_magic(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let mut forced = b"Obj\x01".to_vec();
        forced.extend(bytes);
        let parser = AvroParser::new();
        let _ = parser.parse(&forced, "fuzz.avro", &ParseOptions::new());
    }

    #[test]
    fn never_panics_on_a_mutated_real_avro_file(
        mutations in prop::collection::vec((any::<usize>(), any::<u8>()), 0..64)
    ) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/avro/users.avro");
        let mut bytes = std::fs::read(path).expect("fixture must exist (run gen_fixtures_avro example)");
        for (pos, byte) in mutations {
            if bytes.is_empty() {
                break;
            }
            let idx = pos % bytes.len();
            bytes[idx] = byte;
        }
        let parser = AvroParser::new();
        let _ = parser.parse(&bytes, "fuzz.avro", &ParseOptions::new());
    }
}
