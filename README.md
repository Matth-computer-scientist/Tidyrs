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

- **One CLI, five input formats** — CSV, Excel, JSON/XML, fixed-width/log
  text, and (experimentally) PDF tables, auto-detected from file
  *content*, not just extension.
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
| Fixed-width / logs | **Stable** | Whitespace-alignment column inference, or plain whitespace-token splitting for log lines |
| JSON / XML | **Experimental** | Parsing is solid; flattening uses a simple, documented dot-notation + array-join strategy (with an opt-in `explode` mode for arrays of objects), not a fully general one |
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
| `--format <csv\|xlsx\|json\|xml\|fixed\|pdf>` | — | Force a specific parser instead of auto-detecting |
| `--report-file <FILE>` | — | Write the full `CleaningReport` as JSON (single-file mode) |
| `--report-dir <DIR>` | — | Write one JSON report per input file (batch mode) |
| `--verbose-report` | — | Print every note, not just the one-line summary |
| `--delimiter <CHAR>` | CSV | Force a delimiter instead of auto-detecting |
| `--has-header` / `--no-header` | CSV, Excel, fixed-width | Whether the first row/line is a header |
| `--merge-fill` / `--no-merge-fill` | Excel | Fill cells left empty by merged regions (default: on) |
| `--sheet <NAME>` | Excel | Only process this sheet (default: all sheets) |
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
`tidyrs-xlsx`, `tidyrs-json`, `tidyrs-fixed`, `tidyrs-pdf`) plus
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
tidyrs-json   — JSON/XML parser (experimental)
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

### Real-world scenario tests

Every other test in this repo isolates one specific behavior in a small,
purpose-built fixture (a few rows, one issue). `tidyrs-cli/tests/real_world_scenarios.rs`
is different: it runs the CLI end-to-end against larger, deliberately
messy fixtures under `fixtures/real_world/` (a 130-row sales export with
mixed date formats/currency symbols/ragged rows, a 3-sheet financial
report workbook, 40 nested JSON orders with inconsistent optional fields,
an 80-line server log) that mix several kinds of mess in the same file
the way an actual export would — covering workflows like a CI schema
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
`.xlsx`/`.pdf`); regenerate them if you change the generator:

```sh
cargo run -p tidyrs-xlsx --example gen_fixtures_xlsx
cargo run -p tidyrs-pdf --example gen_fixtures_pdf
```

## Contributing

This is an early-stage project — issues and PRs are welcome, especially
around:

- Real-world (anonymized) fixture files that expose parsing edge cases,
  for any format.
- Widening the "stable" set (JSON/XML flattening, PDF extraction) with
  more test coverage behind each change.
- New `TidyParser` implementations for other formats (Avro, ORC, INI,
  Parquet-as-input, ...) — the trait boundary exists specifically so this
  doesn't require touching `tidyrs-core`.

Before opening a PR: `cargo test --workspace` should pass, and new
behavior should come with a fixture + test the same way existing formats
do (see any `tests/fixtures.rs` for the pattern). Robustness matters here
more than in most projects — if you touch a parser, run its
`tests/robustness.rs` suite (`cargo test -p <crate> --test robustness`)
before and after. `cargo fmt --all -- --check` and `cargo clippy
--workspace --all-targets -- -D warnings` must both be clean — CI enforces
this on every push/PR (see the badge above and `.github/workflows/ci.yml`).

### Releasing (maintainers)

Pushing a tag matching `v*.*.*` (e.g. `v0.1.0`) triggers
`.github/workflows/release.yml`, which builds `tidyloom` for
Linux/macOS (x86_64 + aarch64)/Windows and attaches the binaries to a
GitHub Release. Not published to crates.io yet — see
[Installation](#installation).

## License

MIT OR Apache-2.0
