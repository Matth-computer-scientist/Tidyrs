//! Apache Avro reading for tidyloom — **experimental**.
//!
//! Reads Avro Object Container Files (the `.avro` files real pipelines
//! actually pass around — a self-describing container embedding its own
//! writer schema, as opposed to bare Avro-encoded bytes needing an
//! external schema). One writer schema applies to the *whole* file
//! (unlike JSON, where each record can independently vary in shape), so
//! the header list comes straight from the first record's field order
//! rather than a union-of-all-records scan.
//!
//! `apache_avro::types::Value` is considerably richer than `TidyValue`.
//! `Union` (Avro's usual `["null", T]` encoding for an optional field) is
//! transparently unwrapped rather than surfaced as-is, since otherwise
//! *every* nullable field in a real-world schema would show up as an
//! opaque wrapper instead of its actual value. Logical date/time types
//! are converted to real calendar dates/timestamps via `chrono`. Nested
//! `Record`/`Array`/`Map` values, and `Decimal`/`Duration`/`Fixed`
//! (types with no natural short text form), fall back to Rust's `Debug`
//! formatting — readable and lossless, but not flattened into
//! sub-columns the way `tidyrs-json`'s JSON/YAML flattening is, and not
//! independently round-trippable the way a hand-written decimal-to-
//! string conversion would be. This mirrors the same "native type where
//! unambiguous, readable text where not" policy `tidyrs-orc` and
//! `tidyrs-parquet` already use for their own rich schemas.
//!
//! ## An allocation-crash vulnerability in `apache_avro` 0.21.0
//!
//! Found via proptest fuzzing (`tests/robustness.rs`): a malformed or
//! adversarial `.avro` file could crash the whole process with `memory
//! allocation of N bytes failed` — not a panic, an actual allocator
//! abort, which `catch_unwind` (already wrapping this crate's decoding,
//! same as every other parser here) cannot intercept, since it only
//! catches unwinding panics.
//!
//! `apache_avro` 0.21.0 funnels essentially every length read from
//! untrusted file bytes (string/bytes lengths, array/map counts, block
//! byte counts) through its own internal `safe_len` guard, capped at
//! [`apache_avro::util::DEFAULT_MAX_ALLOCATION_BYTES`] (512MiB) — except
//! two places, both confirmed by reading the crate's source directly:
//!
//! 1. **A `fixed` schema's declared `size`.** This comes from the file's
//!    own embedded schema JSON (itself attacker-controlled, decoded
//!    safely via the guarded path above) but the `size` value is then
//!    used directly as `vec![0u8; size]` the moment a value of that type
//!    is decoded — no upper bound at all. [`find_oversized_fixed_type`]
//!    closes this: after `Reader::new` succeeds (safe — the header is
//!    small and itself fully `safe_len`-guarded), we walk
//!    `Reader::writer_schema()` for any `Fixed` node whose size exceeds
//!    the crate's own 512MiB default and refuse to decode before ever
//!    reaching the vulnerable path. Zero cost on any legitimate file,
//!    since no real schema declares a multi-gigabyte fixed-width field.
//! 2. **Snappy's declared decompressed length**, read straight from the
//!    compressed stream itself (`snap::raw::decompress_len`) and used as
//!    `vec![0; decompressed_size]` with the same lack of a bound. This
//!    one can't be intercepted the same way — it fires mid-decode,
//!    inside a `Reader` iteration step we don't get a hook into, and
//!    checking it ourselves would mean re-implementing a chunk of
//!    `apache_avro`'s own block/codec reading. Since nothing in this
//!    project uses Snappy-compressed Avro files, the pragmatic fix is
//!    simpler: don't compile that code path in at all. `Cargo.toml`
//!    deliberately does *not* enable the `snappy` feature (see its own
//!    comment) — `Null` and `Deflate`, the two codecs covering the
//!    overwhelming majority of real-world `.avro` files, are always
//!    compiled in regardless of feature flags, and `Deflate`'s
//!    decompression grows its output buffer incrementally rather than
//!    trusting a single untrusted declared size, so it doesn't share this
//!    vulnerability class.
//!
//! Neither fix above turned out to be what the fuzz suite was actually
//! finding — confirmed by capturing the exact crashing byte sequence (see
//! `tests/robustness.rs`'s history) and hand-decoding it against
//! `apache_avro`'s own zigzag-varint format. **A third, more serious
//! issue**, present unconditionally in `apache_avro` 0.21.0's `decode.rs`
//! and reachable from nothing more than the 4-byte magic plus a handful
//! of adversarial bytes (no valid schema JSON required at all): decoding
//! any `Schema::Array`/`Schema::Map` reads a declared entry count via
//! `decode_seq_len`, which *does* run through `safe_len` — but `safe_len`
//! validates it as if it were a **byte length** (its 512MiB default is
//! sized for "this many raw bytes"), then that same number is passed
//! directly to `Vec::reserve`/`HashMap::reserve` as an **element count**.
//! For `Schema::Array`, one element can be many bytes; for `Schema::Map`
//! specifically, one `(String, Value)` `HashMap` entry is on the order of
//! 100+ bytes once `String`'s own allocation, `Value`'s enum size, and
//! `HashMap`'s bucket overhead are counted. A declared count safely under
//! the 512MiB *byte* cap can therefore still request tens of gigabytes
//! once multiplied by real per-entry size — confirmed by hand-decoding a
//! captured crash input to a raw count of ~132 million, comfortably under
//! `safe_len`'s threshold as a byte count, but requesting the exact
//! ~21.7GB `HashMap::reserve` allocation this investigation started from.
//!
//! Worse: this is reachable from the **Object Container File's own
//! bootstrapping header**, not just a value inside the user's data
//! schema — the header's metadata is itself Avro-encoded as `map<bytes>`,
//! decoded unconditionally by `Reader::new` before we ever get a hook
//! into anything (no `writer_schema()` to inspect yet, unlike the
//! `Schema::Fixed` case above). There is no way to intercept this from
//! outside `apache_avro` without either vendoring a patched copy of the
//! crate or re-implementing enough of the OCF format ourselves to
//! pre-validate it. [`header_metadata_count_is_plausible`] does exactly
//! the narrow, minimal version of that: it duplicates only the handful of
//! lines needed to decode the header's *leading* zigzag-varint entry
//! count (the same algorithm `apache_avro`'s own `decode.rs` uses,
//! confirmed by direct comparison) and rejects the file outright if that
//! count is wildly implausible for a real header (which never has more
//! than a handful of metadata keys), before `Reader::new` — and the
//! vulnerable `HashMap::reserve` inside it — is ever called. This closes
//! the specific case this crate's fuzz suite actually found. It does
//! *not* close the general vulnerability class: a `Schema::Map` or
//! `Schema::Array` field *inside* a user's data schema hits the exact
//! same unguarded `reserve(len)` during record decoding, past the point
//! this narrow header check can help, and there's no equivalent
//! low-risk pre-check available there (unlike the header, a real schema
//! legitimately can declare an arbitrarily large map/array — there's no
//! "this is implausible" line to draw the way there is for a handful of
//! header metadata keys). That gap is a genuine, currently open issue in
//! the `apache_avro` 0.21.0 dependency itself, not something this crate
//! can fully close from the outside.

use apache_avro::schema::Schema;
use apache_avro::types::Value;
use apache_avro::Reader;
use tidyrs_core::{
    AmbiguityResolver, CleaningReport, ParseOptions, ParseOutcome, RuleBasedResolver, TidyError, TidyParser, TidyResult, TidyTable, TidyValue,
};

pub struct AvroParser {
    resolver: Box<dyn AmbiguityResolver>,
}

impl AvroParser {
    pub fn new() -> Self {
        Self {
            resolver: Box::new(RuleBasedResolver),
        }
    }

    /// See `tidyrs_csv::CsvParser::with_resolver` — same idea, same
    /// extension point.
    pub fn with_resolver(resolver: Box<dyn AmbiguityResolver>) -> Self {
        Self { resolver }
    }
}

impl Default for AvroParser {
    fn default() -> Self {
        Self::new()
    }
}

/// The fixed 4-byte Object Container File magic: "Obj" followed by the
/// format version byte (currently always 1).
const MAGIC: &[u8] = b"Obj\x01";

fn has_avro_extension(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".avro")
}

/// Walks a parsed writer `Schema` looking for a `fixed` type whose
/// declared `size` exceeds `max_size`, returning the first one found.
///
/// `apache_avro` 0.21's own decoder bounds every *data-driven* allocation
/// (a string/bytes/array/map length read from the file's actual bytes)
/// through its internal `safe_len` guard, capped by
/// [`apache_avro::util::DEFAULT_MAX_ALLOCATION_BYTES`] — except one: a
/// `fixed` schema's `size` comes from the (attacker-controlled, since
/// it's embedded in the file's own header) schema JSON, and gets used
/// directly as `vec![0u8; size]` with no such guard the moment a value of
/// that type is decoded. A malformed/adversarial file declaring an
/// enormous `size` (confirmed with a fuzz-found file requesting a
/// ~21.7GB allocation) crashes the process outright — `Vec`'s allocation
/// failure aborts rather than panicking, so the `catch_unwind` already
/// wrapping this crate's decoding cannot catch it (`catch_unwind` only
/// intercepts unwinding panics, not allocator aborts). Reading
/// `Reader::writer_schema()` after construction is safe (the header is
/// small and already funnels through `safe_len`) and happens before any
/// data block — including the first — is decoded, so checking every
/// `Fixed` node here lets us reject the file cleanly before ever calling
/// into the vulnerable path.
///
/// `Schema::Ref` (a self-referencing named type) is treated as a leaf:
/// the named type's own definition appears elsewhere in the tree as a
/// literal `Schema::Fixed` and gets checked there, so following the
/// reference isn't needed to catch an oversized declaration.
fn find_oversized_fixed_type(schema: &Schema, max_size: usize) -> Option<usize> {
    match schema {
        Schema::Fixed(fixed) if fixed.size > max_size => Some(fixed.size),
        Schema::Fixed(_) => None,
        Schema::Array(array) => find_oversized_fixed_type(&array.items, max_size),
        Schema::Map(map) => find_oversized_fixed_type(&map.types, max_size),
        Schema::Decimal(decimal) => find_oversized_fixed_type(&decimal.inner, max_size),
        Schema::Union(union) => union.variants().iter().find_map(|s| find_oversized_fixed_type(s, max_size)),
        Schema::Record(record) => record.fields.iter().find_map(|f| find_oversized_fixed_type(&f.schema, max_size)),
        _ => None,
    }
}

/// Decodes one Avro "long" (zigzag varint) starting at `*pos`, advancing
/// `*pos` past it. Returns `None` on truncated/overflowing input — never
/// panics, since this runs on completely untrusted bytes before any of
/// apache_avro's own validation. The exact same bit-level algorithm as
/// apache_avro's own (private) `zag_i64`/`decode_variable`, duplicated
/// here deliberately narrowly — see the module docs for why.
fn read_zigzag_long(bytes: &[u8], pos: &mut usize) -> Option<i64> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let b = *bytes.get(*pos)?;
        *pos += 1;
        result |= u64::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    Some(if result & 1 == 0 { (result >> 1) as i64 } else { !(result >> 1) as i64 })
}

/// A real Object Container File header never declares more than a
/// handful of metadata entries (`avro.schema`, `avro.codec`, maybe one or
/// two custom keys) — chosen generously above any real usage, nowhere
/// near enough for `HashMap::reserve` to request a dangerous amount of
/// memory even at a pessimistic ~100+ bytes per `(String, Value)` entry.
const MAX_HEADER_METADATA_ENTRIES: i64 = 10_000;

/// Sanity-checks the OCF header's metadata-map entry count *before*
/// calling into `apache_avro`, which (see the module docs' third
/// vulnerability) trusts that count directly for `HashMap::reserve` with
/// no awareness that each entry is many bytes, not one. Returns `true`
/// for anything that isn't confidently an oversized count — including
/// truncated/malformed input, which apache_avro's own error path is
/// better placed to report precisely — so this only ever *narrows*
/// acceptance, never accepts something apache_avro would otherwise
/// reject.
fn header_metadata_count_is_plausible(bytes: &[u8]) -> bool {
    let mut pos = MAGIC.len();
    let Some(raw_count) = read_zigzag_long(bytes, &mut pos) else {
        return true;
    };
    // A negative count means "block byte-size follows, then the real
    // count is the negation" (Avro's own block-encoding for maps/arrays)
    // — we don't need that byte-size value for this check, only the
    // magnitude of the count itself.
    raw_count.unsigned_abs() <= MAX_HEADER_METADATA_ENTRIES as u64
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn date_from_epoch_days(days: i32) -> String {
    match chrono::NaiveDate::from_ymd_opt(1970, 1, 1).and_then(|epoch| epoch.checked_add_signed(chrono::Duration::days(days as i64))) {
        Some(d) => d.format("%Y-%m-%d").to_string(),
        None => format!("<date out of range: {days} days from epoch>"),
    }
}

fn timestamp_from_millis(ms: i64) -> String {
    match chrono::DateTime::from_timestamp_millis(ms) {
        Some(t) => t.to_rfc3339(),
        None => format!("<timestamp out of range: {ms}ms>"),
    }
}

fn timestamp_from_micros(us: i64) -> String {
    match chrono::DateTime::from_timestamp_micros(us) {
        Some(t) => t.to_rfc3339(),
        None => format!("<timestamp out of range: {us}us>"),
    }
}

/// Converts one Avro `Value` into a `TidyValue`. `Union` is unwrapped
/// recursively (see module docs); everything without a clean scalar
/// mapping falls back to `Debug` text.
fn avro_value_to_tidy(value: &Value) -> TidyValue {
    match value {
        Value::Null => TidyValue::Null,
        Value::Boolean(b) => TidyValue::Bool(*b),
        Value::Int(i) => TidyValue::Int(*i as i64),
        Value::Long(i) => TidyValue::Int(*i),
        Value::Float(f) => TidyValue::Float(*f as f64),
        Value::Double(f) => TidyValue::Float(*f),
        Value::String(s) => TidyValue::Text(s.clone()),
        Value::Bytes(b) => TidyValue::Text(hex_encode(b)),
        Value::Fixed(_, b) => TidyValue::Text(hex_encode(b)),
        Value::Enum(_, symbol) => TidyValue::Text(symbol.clone()),
        Value::Union(_, boxed) => avro_value_to_tidy(boxed),
        Value::Date(days) => TidyValue::Text(date_from_epoch_days(*days)),
        Value::TimeMillis(ms) => TidyValue::Text(format!("{ms}ms since midnight")),
        Value::TimeMicros(us) => TidyValue::Text(format!("{us}us since midnight")),
        Value::TimestampMillis(ms) => TidyValue::Text(timestamp_from_millis(*ms)),
        Value::TimestampMicros(us) => TidyValue::Text(timestamp_from_micros(*us)),
        Value::Uuid(u) => TidyValue::Text(u.to_string()),
        // Decimal/Duration/nested Record/Array/Map — no clean short text
        // form; Debug is readable and complete, if not itself
        // machine-parseable. See module docs.
        other => TidyValue::Text(format!("{other:?}")),
    }
}

impl TidyParser for AvroParser {
    fn format_name(&self) -> &'static str {
        "avro"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        let mut score: f32 = 0.0;
        if let Some(name) = filename {
            if has_avro_extension(name) {
                score += 0.3;
            }
        }
        if bytes.starts_with(MAGIC) {
            score += 0.6;
        }
        score.min(1.0)
    }

    fn parse(&self, bytes: &[u8], filename: &str, _options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let mut report = CleaningReport::new(filename, self.format_name());
        report.warning(
            "Avro support is experimental in this version: nested Record/Array/Map values and Decimal/Duration \
             fields are rendered as Rust debug text rather than a fully typed conversion or flattening — \
             see the tidyrs-avro module docs for exactly what this does and doesn't handle."
                .to_string(),
        );

        if !bytes.starts_with(MAGIC) {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "not an Avro Object Container File (missing the 'Obj\\x01' magic header)".into(),
            });
        }

        if !header_metadata_count_is_plausible(bytes) {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "the file's header declares an implausible number of metadata entries — refusing to \
                          decode a file that would attempt an oversized allocation"
                    .into(),
            });
        }

        // apache-avro's decoder isn't proptest-hardened in this workspace
        // the way calamine/pdf-extract/rusqlite have been; isolate it
        // behind catch_unwind on principle, consistent with every other
        // parser here that wraps a third-party binary-format decoder.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let reader = Reader::new(bytes).map_err(|e| e.to_string())?;
            let max_fixed_size = apache_avro::util::DEFAULT_MAX_ALLOCATION_BYTES;
            if let Some(size) = find_oversized_fixed_type(reader.writer_schema(), max_fixed_size) {
                return Err(format!(
                    "the file's schema declares a 'fixed' field of {size} bytes, over the {max_fixed_size}-byte \
                     sanity limit — refusing to decode a file that would attempt an allocation this large"
                ));
            }
            let mut headers: Option<Vec<String>> = None;
            let mut raw_rows: Vec<Vec<TidyValue>> = Vec::new();
            for value in reader {
                let value = value.map_err(|e| e.to_string())?;
                match value {
                    Value::Record(fields) => {
                        if headers.is_none() {
                            headers = Some(fields.iter().map(|(k, _)| k.clone()).collect());
                        }
                        raw_rows.push(fields.iter().map(|(_, v)| avro_value_to_tidy(v)).collect());
                    }
                    // A file whose top-level writer schema isn't a record
                    // (legal in Avro, e.g. a plain array/string/int
                    // schema) — treat the whole value as one unnamed
                    // column, the same "whole document as a single
                    // record" fallback tidyrs-json uses for a bare JSON
                    // scalar.
                    other => {
                        if headers.is_none() {
                            headers = Some(vec!["value".to_string()]);
                        }
                        raw_rows.push(vec![avro_value_to_tidy(&other)]);
                    }
                }
            }
            Ok::<_, String>((headers.unwrap_or_default(), raw_rows))
        }));

        let (headers, raw_rows) = match result {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: format!("could not read Avro file: {e}"),
                })
            }
            Err(_) => {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: "the Avro decoding library panicked on this file (it's likely corrupt or malformed) — \
                              this was caught to avoid crashing the process, but the file could not be parsed"
                        .into(),
                })
            }
        };

        if headers.is_empty() {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "Avro file has no records".into(),
            });
        }

        report.rows_in = raw_rows.len();
        let mut table = TidyTable::new(headers).with_source(filename.to_string());
        for row in raw_rows {
            table.push_row(row);
        }
        table.normalize_row_widths();
        report.rows_out = table.rows.len();

        // Avro already carries its own typed writer schema — no per-cell
        // type ambiguity to resolve the way CSV/fixed-width/PDF have.
        // This field exists purely so with_resolver/new() keep the same
        // shape as every other parser in the workspace.
        let _ = &self.resolver;

        Ok(ParseOutcome { tables: vec![table], report })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_scores_the_avro_magic_header_regardless_of_filename() {
        let mut bytes = b"Obj\x01".to_vec();
        bytes.extend_from_slice(&[0u8; 64]);
        let parser = AvroParser::new();
        assert!(parser.sniff(&bytes, None) > 0.5);
        assert!(parser.sniff(&bytes, Some("data.avro")) > 0.8);
    }

    #[test]
    fn sniff_rejects_content_without_the_magic_header_even_with_a_matching_extension() {
        let parser = AvroParser::new();
        assert!(parser.sniff(b"not an avro file at all, just plain text", Some("data.avro")) < 0.5);
    }

    #[test]
    fn parse_rejects_non_avro_bytes_cleanly() {
        let parser = AvroParser::new();
        let result = parser.parse(b"definitely not an avro file", "fake.avro", &ParseOptions::new());
        assert!(result.is_err());
    }

    #[test]
    fn a_union_wrapped_optional_field_unwraps_to_its_inner_value() {
        let inner = Value::Union(1, Box::new(Value::Long(42)));
        assert_eq!(avro_value_to_tidy(&inner), TidyValue::Int(42));
        let null_case = Value::Union(0, Box::new(Value::Null));
        assert_eq!(avro_value_to_tidy(&null_case), TidyValue::Null);
    }

    #[test]
    fn epoch_day_zero_is_january_first_1970() {
        assert_eq!(date_from_epoch_days(0), "1970-01-01");
    }
}
