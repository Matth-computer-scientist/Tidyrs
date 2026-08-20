<p align="center">
  <img src="assets/logo/svg/banner-dark.svg" alt="Tidyrs — data, tidied" width="100%">
</p>

# tidyloom

**Universal normalization for chaotic data files.** A headless Rust
library and CLI you drop into a script, a cron job, or a CI/CD pipeline —
not a desktop app, not a SaaS, not something a human has to click through.

[![CI](https://github.com/Matth-computer-scientist/Tidyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/Matth-computer-scientist/Tidyrs/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
![Rust edition 2021](https://img.shields.io/badge/rust-2021%20edition-orange.svg)

> **Naming note:** this repository is `Tidyrs`; the product/binary built
> from it is called **tidyloom** (the individual Rust crates keep the
> `tidyrs-*` prefix — `tidyrs-core`, `tidyrs-cli`, etc. — that's just
> their internal naming, unrelated to the product name). The `tidyloom`
> name was chosen after `tidyrs` turned out to already be an unrelated
> project on GitHub; the crates were never renamed since doing so had no
> functional benefit. Everywhere below, "tidyloom" is the tool you build
> and run; `tidyrs-*` is what its source is organized into.

---

## Table of contents

- [The problem](#the-problem)
- [Positioning](#positioning)
- [Key features](#key-features)
- [Supported formats](#supported-formats)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Use cases](#use-cases)
- [CLI reference](#cli-reference)
- [Library usage](#library-usage)
- [Architecture](#architecture)
- [The heuristic / LLM extension point](#the-heuristic--llm-extension-point)
- [Schema validation](#schema-validation)
- [Project-wide defaults (`tidyloom.toml`)](#project-wide-defaults-tidyloomtoml)
- [Before / after](#before--after)
- [Feasibility notes](#feasibility-notes)
- [Performance notes](#performance-notes)
- [Reliability notes](#reliability-notes)
- [Building & testing](#building--testing)
- [Contributing](#contributing)
- [License](#license)

---

## The problem

Real-world data files are rarely clean: CSVs with the wrong delimiter or a
foreign encoding, Excel exports with merged cells and a stray title row,
PDFs with tables held together by whitespace, JSON where the same key is
sometimes a string and sometimes an object. Cleaning that by hand, per
file, per project, doesn't scale — and most existing tools assume a human
is sitting in front of a GUI, which is exactly the wrong shape for
something that needs to run unattended, every night, on whatever file
landed in a folder.

tidyloom takes one of those files and produces a normalized table (CSV,
JSON, or Parquet) plus an audit report of exactly what was detected and
fixed along the way — so the transformation is inspectable, not a black
box.

## Positioning

| Tool | Interface | Automatable |
|---|---|---|
| OpenRefine | Desktop GUI | No |
| Querri / CleanMyExcel.io | Web SaaS | No |
| Trifacta / Power Query | Enterprise platform | Partially |
| **tidyloom** | **CLI + Rust library** | **Yes — built for it** |

None of the existing tools are designed to be called headlessly from a
script or a pipeline step. tidyloom is: a single binary or a Cargo
dependency, one unified interface across five input formats, no UI to
drive.

## Key features

- **One CLI, twelve input formats** — CSV, Excel, SQLite, JSON/XML/YAML/NDJSON,
  INI/.env, fixed-width/log text, and (experimentally) ORC, Parquet, Avro,
  and PDF tables, auto-detected from file *content*, not just extension.
- **Auditable, not a black box** — every run produces a `CleaningReport`
  listing exactly what was detected and fixed (delimiter, encoding,
  ragged rows, merged cells, ambiguous columns, ...), exportable as JSON.
- **Column-aware type inference** — a column doesn't get silently
  downgraded to text because of one typo; see
  [the extension point](#the-heuristic--llm-extension-point).
- **Schema validation as a pipeline gate** — declare expected types and
  nullability, `--on-schema-violation reject` to fail the build on drift.
- **`--dry-run`** — see a diff of what would change before committing to it.
- **`--stream`** — bounded-memory CSV-to-CSV cleaning for very large files.
- **Structured logging** — `--log-format json` for one parseable JSON
  object per log line, or human-readable text by default.
- **Project-wide config** (`tidyloom.toml`) so pipelines don't repeat the
  same flags everywhere.
- **Typed Parquet export** — per-column type inference (Int64/Double/
  Boolean/Utf8), not "everything as a string."
- **Extension points, not dead ends** — a documented, working `AmbiguityResolver`
  trait (rule-based by default, swap in an LLM-backed one behind a
  feature flag) and a `TidyParser` trait for adding new formats without
  touching the core crate.
- **Proptest-hardened** — every parser is fuzz-tested to never panic, even
  on arbitrary or corrupted bytes; this caught and fixed two real panics
  deep in third-party dependencies during development (see
  [Reliability notes](#reliability-notes)).

## Supported formats

| Format | Status | Notes |
|---|---|---|
| CSV | **Stable** | Quote-aware delimiter auto-detection, encoding detection (UTF-8/Latin-1/Windows-1252/...), ragged-row tolerance |
| Excel (.xlsx/.xls) | **Stable** | Exact merge-region filling for real `.xlsx`/`.xlsm` (forward-fill heuristic fallback for `.xls`/`.xlsb`/`.ods`), junk header/footer row skipping, independent multi-sheet handling |
| SQLite | **Stable** | One `TidyTable` per user table (`--table` to restrict to one), read with SQLite's own native column types (no re-inference needed, unlike text formats). Detection keys off SQLite's fixed 16-byte file-header magic string — no heuristic scoring involved, unlike every other format here |
| Fixed-width / logs | **Stable** | Whitespace-alignment column inference, or plain whitespace-token splitting for log lines |
| INI / .env | **Stable** | `[section]` blocks become one row each (a `credentials`-style multi-profile file becomes genuinely tabular data), or one row for a flat file with no sections. Handles `.env`'s `export KEY=VALUE` and quoted values. Content-only detection requires the `[section]`/`key=value` grammar on most sampled lines, not just "the file contains an `=`" |
| JSON / XML / YAML / NDJSON | **Experimental** | Parsing is solid (YAML and NDJSON are both parsed straight into the same value tree as JSON, so they share one flattening pass); flattening uses a simple, documented dot-notation + array-join strategy (with an opt-in `explode` mode for arrays of objects), not a fully general one. YAML content-only detection uses a `key:`/`- item` line-shape scan plus a real parse-to-mapping/sequence check, since YAML (unlike JSON/XML) has no unique leading character to key off of. NDJSON detection requires every sampled line to independently parse as its own complete JSON object, scored to clearly outrank CSV (whose delimiter-consistency check would otherwise treat the comma inside each line's JSON as a real column separator) |
| ORC | **Experimental** | Boolean/integer/float columns get native `TidyValue` types; Date/Timestamp/Decimal and nested Struct/List/Map columns render as Arrow's own display text rather than a fully typed conversion or dot-notation flattening. Detection keys off ORC's fixed magic trailer at the *end* of the file (ORC's footer is written last, unlike every other magic-byte format here). Full type/compression coverage (including Snappy) via `orc-rust`, chosen over the only other ORC crate on crates.io for licensing (that one's non-commercial-only license is incompatible with this project's) and real feature gaps (no floats, no dates, no Snappy) |
| Parquet (reading) | **Experimental** | Same native-type-where-unambiguous policy as ORC (they share the same Arrow-based conversion approach), pinned to the same Arrow major version tidyloom's own Parquet *writer* already uses. Writing Parquet (`--output-format parquet`) remains fully typed and stable — this only affects reading an *existing* Parquet file as input |
| Avro | **Experimental** | Reads Object Container Files via the official `apache-avro` crate. Union-wrapped optional fields (Avro's standard `["null", T]` encoding) are transparently unwrapped rather than surfaced as a wrapper; logical date/timestamp types convert to real calendar values. Nested Record/Array/Map and Decimal/Duration fields render as Rust debug text, the same documented simplification ORC/Parquet use for their own rich schemas |
| PDF (text-based tables) | **Experimental proof of concept** | Reconstructs tables from real glyph positions (works with proportional fonts, not just monospace) with a title-line detector. No OCR — scanned/image PDFs are out of scope. Review output before trusting it |

See [Feasibility notes](#feasibility-notes) for why PDF in particular
stays experimental, and exactly what "experimental" does and doesn't mean
here.

## Installation

tidyloom isn't published to crates.io yet — build it from source. You
need a recent stable Rust toolchain ([rustup.rs](https://rustup.rs) if
you don't have one).

```sh
git clone git@github.com:Matth-computer-scientist/Tidyrs.git
cd Tidyrs
cargo build --release -p tidyrs-cli
# binary at ./target/release/tidyloom (tidyloom.exe on Windows)
```

Optionally put it on your `PATH`:

```sh
cargo install --path tidyrs-cli
```

To embed tidyloom as a library instead of using the CLI, add the crates
you need to your own `Cargo.toml` — see [Library usage](#library-usage).

## Quick start

```sh
tidyloom clean messy.csv --output clean.csv --verbose-report
```

```
[warn] messy.csv -> detected: csv, rows: 3 in / 3 out, 2 note(s)
    [info] detected delimiter: ';'
    [warn] 2 row(s) had an inconsistent column count and were padded/truncated to 4 columns
```

That's the whole workflow: point it at a file, get a clean one back plus
a report of what it did. See [Before / after](#before--after) for the
full input/output, and [CLI reference](#cli-reference) for every flag.

## Use cases

- **A CI/CD data-quality gate.** Run `tidyloom clean data.csv --schema
  schema.json --on-schema-violation reject` as a pipeline step before
  data reaches a warehouse — a source file that silently changed shape
  fails the build instead of shipping bad data downstream.
- **ETL preprocessing.** Normalize whatever format a data source hands
  you (a partner's Excel export, a vendor's fixed-width feed, a JSON API
  dump) into consistent CSV/Parquet before it hits your loader, with
  `--batch` for a folder of mixed formats in one pass.
- **Ad-hoc data janitor work.** `tidyloom clean weird_file.xlsx --output
  clean.csv --dry-run` to see what would happen before committing, then
  drop `--dry-run` once you trust it.
- **Large CSV cleaning in constrained environments.** `--stream` for
  bounded-memory processing of files too large to comfortably load whole
  (see [Performance notes](#performance-notes) for numbers).
- **Embedding in a larger Rust tool.** Depend on `tidyrs-core` +
  whichever format crates you need directly, without shelling out to a
  CLI — see [Library usage](#library-usage).
- **Log/report normalization.** Turn whitespace-aligned report exports or
  ad-hoc log lines into structured CSV via the `fixed`/`whitespace` modes.

## CLI reference

```
tidyloom [--log-format text|json] clean [INPUT] [OPTIONS]
```

| Flag | Applies to | Description |
|---|---|---|
| `INPUT` | — | Input file to clean (omit when using `--batch`) |
| `--batch <DIR>` | — | Process every file in this directory instead of a single input |
| `-o, --output <FILE>` | — | Output file (single-file mode) |
| `--output-dir <DIR>` | — | Output directory (batch mode) |
| `--output-format <csv\|json\|parquet>` | — | Inferred from `--output`'s extension when omitted |
| `--format <csv\|xlsx\|json\|xml\|fixed\|pdf\|ini\|sqlite\|orc\|parquet\|avro>` | — | Force a specific parser instead of auto-detecting |
| `--report-file <FILE>` | — | Write the full `CleaningReport` as JSON (single-file mode) |
| `--report-dir <DIR>` | — | Write one JSON report per input file (batch mode) |
| `--verbose-report` | — | Print every note, not just the one-line summary |
| `--delimiter <CHAR>` | CSV | Force a delimiter instead of auto-detecting |
| `--has-header` / `--no-header` | CSV, Excel, fixed-width | Whether the first row/line is a header |
| `--merge-fill` / `--no-merge-fill` | Excel | Fill cells left empty by merged regions (default: on) |
| `--sheet <NAME>` | Excel | Only process this sheet (default: all sheets) |
| `--table <NAME>` | SQLite | Only process this table (default: all user tables) |
| `--mode <fixed\|whitespace>` | fixed-width | Column-alignment inference vs. whitespace-token splitting |
| `--separator <STR>` | JSON/XML | Separator used when flattening nested keys |
| `--array-join-sep <STR>` | JSON/XML | Separator used when joining array values into one column |
| `--array-mode <join\|explode>` | JSON/XML | Join arrays into one column (default) or explode into extra rows |
| `--stream` | CSV → CSV | Bounded-memory single-pass cleaning; falls back to in-memory otherwise |
| `--schema <FILE>` | — | Validate the cleaned table against a JSON schema |
| `--on-schema-violation <warn\|reject>` | — | Warn and still write output (default), or reject and write nothing |
| `--dry-run` | — | Preview/diff what would be written, write nothing |
| `--config <FILE>` | — | Load defaults from this TOML file instead of `./tidyloom.toml` |
| `--log-format <text\|json>` | global | Human-readable (default) or one JSON object per log line |

Run `tidyloom clean --help` for the exact, always-up-to-date list.

## Library usage

```rust
use tidyrs_core::{FormatRegistry, ParseOptions, TidyParser, export};

let mut registry = FormatRegistry::new();
registry.register(Box::new(tidyrs_csv::CsvParser::new()));
registry.register(Box::new(tidyrs_xlsx::XlsxParser::new()));

let bytes = std::fs::read("messy.csv")?;
let detection = registry.detect(&bytes, Some("messy.csv"))
    .ok_or("could not detect format")?;

let outcome = detection.parser.parse(&bytes, "messy.csv", &ParseOptions::new())?;
println!("{} row(s), {} note(s)", outcome.tables[0].rows.len(), outcome.report.notes.len());

let mut out = Vec::new();
export::write_csv(&outcome.tables[0], &mut out)?;
```

Depend only on the format crates you actually need (`tidyrs-csv`,
`tidyrs-xlsx`, `tidyrs-json`, `tidyrs-fixed`, `tidyrs-ini`, `tidyrs-sqlite`,
`tidyrs-orc`, `tidyrs-parquet`, `tidyrs-avro`, `tidyrs-pdf`) plus
`tidyrs-core` — none of them pull the others in.

## Architecture

```
tidyrs-core   — TidyParser trait, TidyValue/TidyTable data model,
                format detection registry, CleaningReport, schema
                validation, CSV/JSON/Parquet export, and the
                AmbiguityResolver extension point.
tidyrs-csv    — CSV parser (stable) + a separate streaming entry point
tidyrs-xlsx   — Excel parser (stable)
tidyrs-fixed  — Fixed-width / whitespace-log parser (stable)
tidyrs-ini    — INI/.env key-value config parser (stable)
tidyrs-sqlite — SQLite database reader, one table per input table (stable)
tidyrs-orc    — Apache ORC reader, via orc-rust/Arrow (experimental)
tidyrs-parquet — Apache Parquet reader, via parquet/Arrow (experimental)
tidyrs-avro   — Apache Avro reader, via apache-avro (experimental)
tidyrs-json   — JSON/XML/YAML parser (experimental)
tidyrs-pdf    — PDF table extraction (experimental)
tidyrs-cli    — `tidyloom` binary tying it all together
```

Every format lives in its own crate and implements one trait:

```rust
pub trait TidyParser {
    fn format_name(&self) -> &'static str;
    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32;
    fn parse(&self, bytes: &[u8], filename: &str, options: &ParseOptions) -> TidyResult<ParseOutcome>;
}
```

Adding a new format means writing a new crate that implements this trait —
`tidyrs-core` and `tidyrs-cli` never need to change. Format detection
(`FormatRegistry::detect`) asks every registered parser how confident it
is about a file's real content, independent of its extension, and picks
the best match.

## The heuristic / LLM extension point

Some decisions (e.g. "what type is this column, really?") can't always be
resolved by rules alone. `tidyrs_core::heuristics::AmbiguityResolver` is
the trait boundary for that: this version ships `RuleBasedResolver` (a
handful of parse-based rules) by default, plus an actual working
`HttpLlmResolver` behind the `llm` feature flag (any OpenAI-compatible
chat completions endpoint — OpenAI, Azure OpenAI, a local Ollama/vLLM
server). It's off by default so the base crate never pulls in an HTTP
client or makes network calls unless you ask for it:

```sh
cargo build -p tidyrs-core --features llm
```

```rust
use tidyrs_core::HttpLlmResolver; // requires --features llm
let resolver = HttpLlmResolver::from_env()?; // TIDYLOOM_LLM_API_KEY, etc.
let (guess, confidence) = resolver.resolve_column_type("ship_date", &samples);
```

On any failure (network error, malformed response) it returns confidence
`0.0` rather than panicking, so callers can fall back to the rule-based
guess. `LlmAmbiguityResolver` (the old placeholder) still exists but is
unimplemented — kept only so code written against earlier tidyloom
versions still compiles.

This is actually wired into parsing, not just available: `tidyrs-csv`,
`tidyrs-fixed`, and `tidyrs-pdf` (the parsers that start from raw
strings — JSON and Excel already carry typed values from `serde_json`/
`calamine`) collect each column's raw values and ask the resolver once
what the column's type is, via `tidyrs_core::type_columns`, instead of
inferring every cell's type independently. That distinction matters: a
mostly-numeric column with one typo (`["30", "41", "N/A", "25"]`) no
longer gets silently downgraded to all-text just because one value didn't
parse — the resolver reports low confidence for that case specifically,
falls back to per-cell inference (so `30`/`41`/`25` still come out as
integers and only `N/A` stays text), and the fallback is recorded as an
`info` note on the `CleaningReport` so it's visible, not silent. Swap in
a stronger resolver (e.g. `HttpLlmResolver`) per-parser via
`CsvParser::with_resolver(...)` / `FixedWidthParser::with_resolver(...)`
/ `PdfParser::with_resolver(...)`.

## Schema validation

```json
{
  "columns": [
    { "name": "id", "type": "integer", "nullable": false },
    { "name": "amount", "type": "float" }
  ],
  "strict": false
}
```

`type` is one of `integer` / `float` / `boolean` / `date` / `text` / `any`.
`nullable` defaults to `true`. `strict: true` also flags any table column
not declared in the schema. See `tidyrs_core::schema` for the full
programmatic API (`Schema`, `validate()`, `ValidationReport`).

```sh
tidyloom clean orders.csv --output orders.clean.csv --schema schema.json --on-schema-violation reject
```

## Project-wide defaults (`tidyloom.toml`)

```toml
[defaults]
output_format = "parquet"
has_header = true
schema = "schemas/orders.json"
on_schema_violation = "reject"
verbose_report = true
```

Picked up automatically from the current directory, or pass `--config
path/to/file.toml`. CLI flags always override a config value; see
`tidyrs-cli/src/config.rs` for exactly which flags this covers and how
boolean flags merge (an explicit CLI `true` always wins; there's no way to
force a config `true` back to `false` from the CLI for a plain flag —
documented there).

## Before / after

`messy.csv` (semicolons, a ragged row, mixed types):

```
name;age;city;notes
Alice;30;Paris;likes tea
Bob;;Lyon
Charlotte;25;Marseille;"loves; markets";extra_field
```

```sh
$ tidyloom clean messy.csv --output clean.csv --verbose-report
[warn] messy.csv -> detected: csv, rows: 3 in / 3 out, 2 note(s)
    [info] detected delimiter: ';'
    [warn] 2 row(s) had an inconsistent column count and were padded/truncated to 4 columns
```

`clean.csv`:

```
name,age,city,notes
Alice,30,Paris,likes tea
Bob,,Lyon,
Charlotte,25,Marseille,loves; markets
```

More usage patterns:

```sh
# Batch mode: mixed formats in one folder
tidyloom clean --batch ./dossier/ --output-dir ./clean/

# Excel: only one sheet, disable merged-cell forward-fill
tidyloom clean report.xlsx --output report.csv --sheet "Q1" --no-merge-fill

# SQLite: every user table becomes its own output file (app_users.csv, app_orders.csv, ...)
tidyloom clean app.db --output app.csv
tidyloom clean app.db --output app.csv --table orders

# ORC: booleans/integers/floats come through typed; dates/decimals/nested columns as text
tidyloom clean events.orc --output events.csv

# Parquet/Avro: same experimental typing policy as ORC
tidyloom clean events.parquet --output events.csv
tidyloom clean events.avro --output events.csv

# Save the full audit trail as JSON for downstream tooling
tidyloom clean input.csv --output clean.csv --report-file clean.report.json
tidyloom clean --batch ./dossier/ --output-dir ./clean/ --report-dir ./reports/

# Typed Parquet output (Int64/Double/Boolean/Utf8 inferred per column)
tidyloom clean input.csv --output clean.parquet

# JSON: explode an array of objects into extra rows instead of joining it
tidyloom clean orders.json --output orders.csv --array-mode explode

# Stream CSV-to-CSV in bounded memory instead of loading the whole file
tidyloom clean huge.csv --output huge.clean.csv --stream

# See what would change without writing anything
tidyloom clean orders.csv --output orders.clean.csv --dry-run

# Structured JSON logs instead of human-readable text
tidyloom --log-format json clean input.csv --output clean.csv
```

## Feasibility notes

This is a v1. Full, uniformly reliable coverage of five very different
chaotic formats — CSV, Excel, PDF, JSON/XML, and fixed-width — in one pass
is not realistic, and PDF in particular deserves an honest callout: PDF
has no notion of a "table", only positioned glyphs, and reconstructing
tabular structure from that is what entire dedicated tools (Camelot,
Tabula, pdfplumber) exist to attack — imperfectly, even after years of
work. `tidyrs-pdf` reads real glyph positions via `pdf-extract`'s
lower-level `OutputDev` API and clusters them into rows/columns by actual
coordinates (see `tidyrs-pdf/src/glyphs.rs`) rather than eyeballing
flattened text, so it now handles proportional fonts, not just monospace —
but it remains a documented proof of concept and will still misfire on
multi-line cells, rotated text, or visually complex layouts. JSON/XML
flattening is similarly a deliberately simple v1 strategy rather than a
fully general solution to arbitrary nested inconsistency — see
`tidyrs-json`'s module docs for exactly what it does and doesn't handle.

**OCR is explicitly out of scope, not just "not implemented yet."** Every
viable Rust OCR path (`leptess`, `tesseract-rs`, ...) binds to the system
Tesseract + Leptonica libraries via pkg-config/vcpkg — there's no
pure-Rust OCR engine of comparable quality. Making that a default
dependency would break the build for anyone without those system
libraries installed, which cuts against the whole "single binary, drop it
in a pipeline" pitch. If this gets added, it should be an opt-in feature
flag with a clearly documented system prerequisite, the same shape as the
`llm` feature.

Fixtures and tests for every format, including the experimental ones, live
under [`fixtures/`](fixtures/) and each crate's `tests/` directory.

## Performance notes

The default `CsvParser`/`TidyParser::parse` path still materializes the
whole parsed table in memory (`TidyTable` is not lazy) — same for
`tidyrs-xlsx`, which additionally depends on `calamine` loading each sheet
into a `Vec<Vec<Data>>` up front. For CSV specifically there's now a real
bounded-memory alternative: `tidyrs_csv::stream_clean_csv` (wired into the
CLI as `--stream`) processes the file in a single pass without holding
more than one row plus a small sniffing prefix in memory — see
`tidyrs-csv/src/stream.rs` for exactly what it does and doesn't cover
(CSV-to-CSV output only; non-UTF-8 input falls back to the in-memory
path). Excel has no streaming equivalent — `calamine`'s API doesn't offer
one — so very large workbooks remain the most likely bottleneck; convert
to CSV upstream if that matters for your use case.

**`--stream` output is not always byte-for-byte identical to the
in-memory path**, confirmed with realistic (not uniformly-formatted)
data in [Real-world scenario tests](#real-world-scenario-tests):
streaming writes each field's original text straight through, while the
in-memory path parses numbers and re-serializes them — so `"3756.90"` in
the source stays `"3756.90"` when streamed but becomes `"3756.9"` from
the in-memory path. Don't rely on the two being interchangeable for a
column whose values don't already share one consistent decimal-place
format.

Rough numbers on a dev machine, release build (`cargo run -p
tidyrs-cli --release --example bench_large_files`; see the file for the
methodology caveat — synthetic uniform data, single machine, not a
real-world corpus):

| Rows | CSV file size | CSV parse (in-memory) | CSV parse (`--stream`) | XLSX file size | XLSX parse time |
|---|---|---|---|---|---|
| 10,000 | 0.3 MB | 113 ms | 49 ms | 0.2 MB | 263 ms |
| 100,000 | 3.8 MB | 1.69 s | 439 ms | 2.2 MB | 2.49 s |
| 500,000 | 20.0 MB | 5.62 s | 2.25 s | *(not benched — generation itself gets slow)* | |

CSV scales close to linearly and comfortably handles files in the
hundreds-of-MB range even without `--stream`. `--stream` isn't just a
memory bound here — it's also consistently ~2.5x faster wall-clock, since
it writes rows straight through as text instead of building a typed
`TidyValue` per cell; that's a genuine reason to reach for it beyond
"the file is huge," not only a memory-pressure escape hatch.

## Reliability notes

Every format crate has a `tests/robustness.rs` proptest suite asserting
the parser never panics — not even on arbitrary random bytes, and not
even on a real fixture file with random byte mutations or truncation
applied. This isn't cosmetic: running it during development caught two
real panics inside third-party dependencies on corrupted input (an
`.unwrap()` deep in `pdf-extract`'s font handling, and an out-of-bounds
index in `calamine`'s XLSX cell reader) that would otherwise have crashed
the whole process on a merely-corrupt file. Both are now isolated behind
`catch_unwind` at the parser boundary and turned into a normal `Err`
instead — see the `catch_unwind` comments in `tidyrs-pdf/src/lib.rs` and
`tidyrs-xlsx/src/lib.rs`.

`tidyrs-cli/tests/idempotence.rs` checks the other reliability property
that matters for a pipeline tool: cleaning an already-clean file must be a
no-op (byte-for-byte identical output on a second pass), for every stable
format.

External QA testing against the release binary (every input format, every
CLI flag, deliberately adversarial edge cases — not just the automated
suite) found and led to fixing six more issues: a `--batch` collision
that silently overwrote one input's output with another's (two files
sharing a stem but not an extension); `--batch` silently skipping
subdirectories with no indication anything was excluded; duplicate
header names in CSV/fixed-width/PDF output passing through unrenamed
(unlike `tidyrs-xlsx`, which already disambiguated its own header row);
an undocumented `--output` fallback and a `--no-merge-fill` flag with no
help text at all; an inconsistent PascalCase `severity` field in the JSON
cleaning report; and, in `tidyrs-pdf`, a `find_header_offset` search that
could — on a title line above ragged data with some legitimately blank
cells — skip past the real header *and* a real data row entirely, because
its whitespace-alignment scoring threshold was a fraction of however many
rows remained in the shrinking slice being scored, making that threshold
easier to clear the further it over-skipped. See the `MAX_TITLE_SKIP`
docs in `tidyrs-pdf/src/lib.rs` for the fix, and the module-level docs in
the same file for a related, precisely diagnosed (not just fixed)
limitation the same QA pass surfaced: a page mixing a real table with a
separate free-text paragraph reads every character correctly (verified by
dumping `tidyrs-pdf`'s glyph extraction directly) but gets column-sliced
as if the paragraph were more table data, since there's no detector for
where the tabular region ends — the PDF equivalent of the footer-trimming
`tidyrs-xlsx` already has, which this crate doesn't.

A follow-up QA round independently confirmed the free-text finding above
(a closer, 2-table-plus-paragraph reproduction of the reported file
showed the raw glyph extraction reproducing every sentence intact — a
wrapped word merely lands split across two CSV *cells*, not missing
characters) and surfaced one more real, precisely diagnosed limitation in
`find_header_offset`: a multi-word title's own internal word gaps can
coincidentally subdivide a region the real table only ever sees as one
wide gap, so a title can score *more* inferred columns when included than
the table scores without it — the opposite of what the title-skip search
assumes. A more direct fix (compute the table's columns from every line
except the first, then ask directly whether that first line's content
falls inside two or more of them) was prototyped and rejected: it can't
tell a title apart from a genuine header, whose whole point is to put a
label in every column, and started misreading real headers as titles
across several previously-passing fixtures — a clear case of a fix
causing more harm than the bug it targeted. Left as a known, bounded
limitation instead (the title survives as extra ambiguous columns, not
lost data — see `title_with_no_ragged_data.pdf` and its regression test).

A later, severity-ranked QA report flagged a third `find_header_offset`
counter-example, worse than the two above because the header is lost
outright rather than merely merged into extra columns:
`right_aligned_numbers.pdf` has a genuine header ("product qty amount")
over data rows whose product names contain an internal space ("Widget
A", "Widget B", "Widget C"), right-aligned within wide numeric columns.
All three data rows happen to share a whitespace gap at the exact same
character position (between the product name and its letter suffix)
that the header text doesn't share, so *excluding* the header scores
more inferred columns than *including* it — the mirror image of the
title-line problem, where *including* a junk line was what scored too
many columns. Same root flaw either way: total column count isn't a
safe proxy for "found the real table" in either direction, and — as
with the other two cases — no further heuristic redesign was attempted
here, since every prior attempt broke more common cases than it fixed.
Left as a known, bounded limitation: the header and the first data row's
column split are lost, but every actual product/quantity/amount value
still survives (see
`a_data_cell_that_looks_like_two_words_can_still_cost_the_header` in
`tidyrs-pdf/tests/fixtures.rs`).

A follow-up investigation into the free-text-paragraph limitation
described earlier (a real table followed by a separate "Comments:" block,
whose word-wrapped lines get column-sliced since there's no "table ends
here" detector) found a narrower, genuinely fixable bug inside that same
case: a prose character can land exactly on a "gap" column position that
every real table row leaves blank — pure word-wrap coincidence — and row
extraction used to map each inferred column span over the line
independently, with no way to preserve a character falling *between*
spans. That character was silently dropped, not just misplaced ("regions"
losing its leading "r" entirely). Unlike the `find_header_offset`
counter-examples above, this had a safe, targeted fix: `extract_row`
(replacing the old per-span `extract_span` mapping) glues a gap-position
character onto its nearest cell instead of discarding it. The paragraph
still doesn't reconstruct correctly — that's still the same out-of-scope
"table end" problem — but no character is silently lost doing it, which
is the actual guarantee this crate promises elsewhere. Safe to fix
outright because it's a no-op on any well-aligned table: a gap position
is blank on nearly every row by definition. See
`a_free_text_paragraph_below_a_table_loses_no_characters` in
`tidyrs-pdf/tests/fixtures.rs`.

The original report actually described a slightly different shape —
`multi_table.pdf`: two real tables with *different* column layouts on one
page, plus the same trailing paragraph — which was investigated
separately to confirm the guarantee still holds there too. With two
mismatched layouts in the same alignment pass, `infer_column_spans`
barely agrees on any gap position across the combined set, so instead of
mis-splitting either table's numbers into the wrong columns, the whole
page collapses to one wide, largely unsplit text span — arguably a safer
failure than partial mis-splitting, since every row's full original text
survives intact as one string. Combined with the `extract_row` fix above,
no value from either table or the paragraph is lost. See
`a_page_with_two_differently_shaped_tables_and_a_paragraph_loses_no_values`
in `tidyrs-pdf/tests/fixtures.rs`.

### Silent numeric/encoding corruption fixed via external QA testing

A further QA round specifically targeted whether *values survive
unchanged*, not just whether structure/detection was right, and found
several real, silent corruption bugs — the most serious class of bug this
project has shipped a fix for, since none of them produced an error or
warning:

- **A leading zero was silently stripped** ("00501" → 501, "007" → 7) —
  in CSV, fixed-width, and PDF's shared `TidyValue::infer_from_str` path,
  *and* independently in the column-wide `AmbiguityResolver` typing path
  (a column of mostly-numeric postal codes could get confidently
  committed to `Integer` and then have every value's leading zero
  stripped during conversion). Real, silent loss of a postal code, phone
  extension, or padded ID's actual value, with nothing anywhere in the
  report suggesting it happened. Fixed with a single shared
  `has_meaningful_leading_zero` check (`tidyrs-core/src/value.rs`) wired
  into both places a raw string gets parsed as a number.
- **Excel silently coerced across cell types.** `cell_to_tidy` used to
  call calamine's `as_i64()`/`as_f64()`, which — unlike the exact
  `get_int()`/`get_float()`/`get_string()` accessors it's built on —
  *coerce* rather than report a cell's real stored type: a column
  explicitly formatted as Text in the source spreadsheet ("007") got
  silently `str::parse`d as a number, and a cell holding `1e300` got
  silently cast to `i64` via Rust's *saturating* `as` operator, becoming
  `9223372036854775807` (`i64::MAX`) — not a rounding error, a completely
  different, wrong number. Fixed by switching to the exact accessors.
  Excel dates also used to read as their raw, meaningless serial number
  (`46027`) because calamine's `dates` cargo feature was never enabled —
  now on, with a `get_datetime()` branch producing a real calendar value.
- **A leading UTF-8 byte-order mark polluted the first field/key** in
  every text-based parser (CSV, fixed-width, JSON/XML/YAML, INI/.env): a
  BOM is valid UTF-8 (decodes to U+FEFF) so it survives
  `from_utf8`/`from_utf8_lossy` unchanged, and isn't whitespace so
  `trim()` doesn't remove it either — it glued itself onto a CSV header's
  first column name, or broke JSON's own leading-`{`/`[` detection
  outright. Fixed with a shared `strip_utf8_bom` (`tidyrs-core/src/sniffing.rs`)
  applied at the start of every parser's decode step.
- **INI/.env never stripped a trailing `; comment`**, only a full-line
  one — `name = "My App"  ; a comment` kept the comment (and the quotes)
  as part of the value. Fixed with a quote-aware trailing-comment scan
  (a marker only ends the value when preceded by whitespace *and* isn't
  inside a quoted string, so `key=http://x#frag` and `key="a; b"` are
  both left alone).
- **NDJSON (`.ndjson`/`.jsonl`) wasn't recognized as a format at all** —
  a comma inside each line's JSON reads as a perfectly consistent CSV
  "delimiter", so it silently lost to `tidyrs-csv`'s detection and
  produced garbage (every line split wherever its first comma landed).
  Rather than just reject it more loudly, `tidyrs-json` now genuinely
  supports it: each line parses as its own independent JSON value, one
  row per line, reusing the exact same flattening pass JSON/YAML already
  share.

A follow-up ("round 4") QA report confirmed the three PDF fixes above and
flagged one item from the numeric-corruption round that the Excel fix
hadn't covered: **an integer literal too big for `i64` was silently
rounded through `f64`** in CSV and JSON — `"9999999999999999999"` (20
digits) became `"10000000000000000000"`, and `i64::MIN` could become
`"-9223372036854776000"` whenever it landed in a column resolved to
`Float` alongside genuinely non-integer values. `f64` only has ~15-17
significant decimal digits of precision, so this wasn't losing a few
trailing digits — it was rounding the *entire* value to the nearest
representable float, silently. Fixed in `tidyrs-core` with a
`looks_like_a_whole_number` check (`tidyrs-core/src/value.rs`) that keeps
a whole number as exact `Text` instead of routing it through `f64`,
wired into both `TidyValue::infer_from_str` and the column-wide
`convert_column` path — the same two places `has_meaningful_leading_zero`
already guards, since it's the same class of bug triggered by magnitude
instead of a leading zero. JSON needed a second, more fundamental fix:
`serde_json` itself converts an oversized integer literal to a lossy
`f64` *at parse time* (confirmed directly — `Number(1e+26)` before any of
this project's own code runs) unless its `arbitrary_precision` feature is
enabled, which the workspace `Cargo.toml` now does.

A later, separate QA report described a PDF with a table spanning two
pages coming out with mis-split columns (a header word like "Prix"
reading as "column_3" + "rix") while every value still survived —
initially assumed consistent with the already-documented, deliberately
unfixed `find_header_offset` limitations above. Investigating it
properly found a different, genuine bug instead: `tidyrs-pdf`'s glyph
row-clustering (`glyphs::group_into_rows`) grouped glyphs purely by Y
position with no concept of a page boundary. Each PDF page's own
coordinate flip is relative to *that page's* media box, so two unrelated
rows on different pages routinely land at nearly the same (x, y) — both
pages' first rows sit the same distance from their own top edge, for
instance — and got merged, their glyphs interleaved character-by-
character. Unlike the title-detection heuristics, this had a real,
non-regressing fix: page boundaries are unambiguous ground truth from
the PDF's own structure, so a page change now forces a new row
unconditionally, the same way the existing Y-tolerance already did for
genuinely different lines on the same page. See
`a_table_spanning_two_pages_does_not_merge_rows_across_the_page_break` in
`tidyrs-pdf/tests/fixtures.rs`.

One report from this round turned out **not** to be a bug: a formula
cell reading back blank instead of evaluated, and a workbook's `0.1+0.2`
column reading as `0`. Both traced to the specific *test fixture*
generator (`rust_xlsxwriter`, used for this project's own synthetic test
files) writing a formula with no cached result — calamine, like every
other production Excel reader, reads a formula's cached value rather than
implementing a calculation engine, and a real Excel-saved file always has
one. Confirmed by writing an equivalent probe file and observing calamine
correctly return an empty cell for the uncached formula, not garbage —
the reader is doing exactly what every other spreadsheet tool does with
this input.

### Real-world scenario tests

Every other test in this repo isolates one specific behavior in a small,
purpose-built fixture (a few rows, one issue). `tidyrs-cli/tests/real_world_scenarios.rs`
is different: it runs the CLI end-to-end against larger, deliberately
messy fixtures under `fixtures/real_world/` (a 130-row sales export with
mixed date formats/currency symbols/ragged rows, a 3-sheet financial
report workbook, 40 nested JSON orders with inconsistent optional fields,
an 80-line server log, a 45-record YAML account export with an
inconsistently-shaped optional field, a 4-environment `.ini` config with
gaps where a key was never set, a `.env` deployment-secrets file, and a
3-table SQLite shop database) that mix several kinds of mess in the same
file the way an actual export would — covering workflows like a CI schema
gate that must reject bad data, a mixed-format batch folder that must
survive one corrupted file without aborting, and a dry-run-then-apply
sequence, not just isolated parsing correctness.

This is exactly the kind of testing that finds bugs unit tests miss purely
by being closer to reality: it caught a real one during development — a
single-column Excel sheet ("Notes" tab) had every data row misread as
footer junk and silently dropped, because a legitimate one-column data
row and a stray trailing note look identical by cell count alone once the
table only has one column. Fixed in `tidyrs-xlsx`, with both the targeted
regression test (`single_column_sheet_keeps_all_its_data_rows`) and the
scenario-level assertion in `financial_report_produces_one_csv_per_sheet_with_correct_row_counts`
that exposed it in the first place. It also surfaced a real, correct-but-
easy-to-assume-otherwise behavior difference between `--stream` and the
in-memory path — see [Performance notes](#performance-notes).

```sh
cargo test -p tidyrs-cli --test real_world_scenarios

# regenerate the fixtures (deterministic — same seed, same output)
cargo run -p tidyrs-cli --example gen_real_world_fixtures
```

### Detection accuracy

`tidyrs-cli/tests/detection_accuracy.rs` runs `FormatRegistry::detect`
against every fixture committed to the repo (content-only, no filename
hint) and asserts each one is classified as the format it actually is —
the concrete bar a change to a `sniff()` scoring formula has to clear,
instead of "feels more principled." Running it against the real-world
fixtures caught two real detection bugs:

- CSV delimiter-consistency scoring required the *exact same* delimiter
  count on every sampled line to score above zero content-only — which
  directly contradicted this parser's own headline feature (tolerating
  ragged rows): a genuinely messy CSV with a couple of short/long rows
  could fail to be detected as CSV at all once there was no filename
  extension to fall back on. Scoring is now graduated (partial credit for
  "present in most lines, roughly consistent") instead of all-or-nothing.
- Both `sniff()` implementations only ever read the first 4096 bytes of
  the file — invisible to a table that starts later (a real export can
  have a preamble/comment block first). Detection now samples the start,
  middle, and end of larger files (`tidyrs_core::sample_for_sniffing`),
  and the delimiter/alignment checks themselves sample lines from both
  ends of that text, not just its head (`tidyrs_core::representative_lines`).
  Fixing this also exposed that `tidyrs-fixed`'s "multiple whitespace-
  separated tokens" signal alone was too weak — it matches ordinary prose
  sentences just as readily as real tabular data — so it now also
  requires the token count to be *consistent* line to line, which prose
  isn't and real tabular/log data is.

YAML support (added after these fixes) had to be designed around the same
lesson from the start: unlike JSON (`{`/`[`) or XML (`<`), YAML has no
unique leading character to key content-only detection off of. Its
detection instead requires two independent signals to agree — most
sampled lines matching a `key:` / `- item` shape, *and* the sample
actually parsing as a YAML mapping or sequence (not a bare scalar string,
which is what `serde_yaml` happily produces for nearly any text). Either
signal alone false-positives: the syntax scan alone matches prose
containing a stray colon (`Note: see below`), and the parse check alone
accepts almost anything as a one-line string. `detection_accuracy.rs`
includes a regression case for timestamp-heavy log lines (`09:15:02 INFO
...`) specifically because a naive colon scan would otherwise treat the
first colon in a timestamp as a YAML key separator.

Adding INI/.env support (`tidyrs-ini`) surfaced a real bug in the
*existing* JSON detection, not just a design question for the new format:
`detect_kind`'s content-only path treated any leading `[` as proof of
JSON, with no validation — but an INI `[section]` header starts with `[`
too. A multi-section `.ini` file with no filename hint used to get
misclaimed by the JSON parser on sight and then fail outright (JSON's own
"invalid JSON" parse error), never reaching the parser that could
actually have handled it, rather than losing gracefully to a lower-scoring
correct guess. Fixed by requiring an actual successful `serde_json` parse
before returning a JSON match, the same validate-before-claiming
discipline already applied to YAML. INI's own content-only detection
follows the same two-signal shape as YAML's: most sampled lines matching
the `[section]` / `key=value` grammar, with a conservative definition of
"key" (identifier-like characters only) that keeps a URL query string or
an HTTP request dump from being read as config.

SQLite is the one format in this list where detection needed *no*
scoring design at all: every SQLite file starts with the same fixed
16-byte magic string (`"SQLite format 3\0"`), so `sniff()` just checks
for it directly. Adding `tidyrs-sqlite` did surface one more real bug,
though — not in detection this time, but in the SQLite reader itself:
`rusqlite`'s `Statement::column_names()` panics outright (rather than
returning an `Err`) on a non-UTF-8 column name, which a single mutated
byte in the fuzz suite's corrupted-database case reliably triggered. This
is the same class of third-party-panics-on-corrupt-input problem already
hit with `calamine` (`tidyrs-xlsx`) and `pdf-extract` (`tidyrs-pdf`), and
is handled the same way: the whole parse is isolated behind
`catch_unwind`.

ORC (`tidyrs-orc`) is the other magic-byte format, but with a twist
SQLite doesn't have: its magic ("ORC") sits at the *end* of the file, not
the start — ORC's footer is only finalized once every column has been
written, so it can't live up front the way SQLite's or a ZIP-based
format's does. `sniff()` accordingly scans the file's tail instead of its
head, tolerating a few bytes of postscript padding after the magic rather
than requiring an exact final-3-bytes match. No content-only false
positives against any other fixture in this repo were found in practice
(`every_committed_fixture_is_detected_correctly_from_content_alone`
covers it), which makes sense once you consider how narrow a
coincidental "ends in the literal bytes O-R-C" is compared to, say, "ends
in whitespace" would have been.

Parquet and Avro both went back to a leading-magic-header check like
SQLite's — no design surprises there, just `"PAR1"` and `"Obj\x01"`
respectively at the front of the file. The interesting decision for both
lives in the *parsing* layer rather than detection: both rich, fully-typed
formats can carry types `TidyValue` has no variant for, and both handle
it the same documented way ORC does — a native `Bool`/`Int`/`Float`
where the mapping is unambiguous, readable text (Arrow's own display
formatting for ORC/Parquet, Rust's `Debug` for Avro's own `Value` enum)
everywhere else, rather than a partial/lossy native conversion or a
JSON-style flattening pass that would have to reimplement type-specific
decimal/date/timestamp logic for each format separately.

```sh
cargo test -p tidyrs-cli --test detection_accuracy
```

## Building & testing

```sh
cargo build --workspace
cargo test --workspace

# with the optional real-LLM ambiguity resolver
cargo build -p tidyrs-core --features llm
cargo test -p tidyrs-core --features llm
```

Some fixtures are generated rather than hand-written (binary formats like
`.xlsx`/`.pdf`/`.db`); regenerate them if you change the generator:

```sh
cargo run -p tidyrs-xlsx --example gen_fixtures_xlsx
cargo run -p tidyrs-pdf --example gen_fixtures_pdf
cargo run -p tidyrs-sqlite --example gen_fixtures
cargo run -p tidyrs-parquet --example gen_fixtures_parquet
cargo run -p tidyrs-avro --example gen_fixtures_avro
```

`fixtures/orc/` is the one exception: no ORC *writer* exists in the Rust
ecosystem (`orc-rust` is read-only), so those two files are reused
directly from `orc-rust`'s own Apache-2.0-licensed test suite
(`tests/basic/data/alltypes.snappy.orc` and `nested_struct.orc` in
[datafusion-contrib/orc-rust](https://github.com/datafusion-contrib/orc-rust))
rather than generated — chosen specifically because upstream's own test
suite already asserts their exact expected values, so this crate's tests
are checked against an independently-verified source of truth, not just
"whatever gets produced."

## Contributing

This is an early-stage project — issues and PRs are welcome, especially
around real-world fixture files that expose parsing edge cases, widening
the "stable" set with more test coverage, and new `TidyParser`
implementations for other formats. See [CONTRIBUTING.md](CONTRIBUTING.md)
for the full guide: what to test before opening a PR, how to regenerate
binary fixtures, commit style, and a walkthrough of adding a new format.
Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).
See [ROADMAP.md](ROADMAP.md) for where the project is headed next.

## License

MIT OR Apache-2.0
