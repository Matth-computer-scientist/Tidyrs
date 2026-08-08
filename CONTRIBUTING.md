# Contributing to tidyloom

This is an early-stage project — issues and PRs are welcome, especially
around:

- Real-world (anonymized) fixture files that expose parsing edge cases,
  for any format.
- Widening the "stable" set (JSON/XML flattening, PDF extraction) with
  more test coverage behind each change.
- New `TidyParser` implementations for other formats — the trait
  boundary in `tidyrs-core` exists specifically so adding a format
  doesn't require touching the core crate. See [Adding a new
  format](#adding-a-new-format) below.

If you're looking for a low-risk first contribution, check the repo's
issues for anything labeled `good first issue`.

## Before you start

- `cargo test --workspace` should pass.
- `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets
  -- -D warnings` must both be clean — CI enforces this on every push/PR
  (see `.github/workflows/ci.yml`).
- New behavior should come with a fixture + test the same way existing
  formats do — see any `tests/fixtures.rs` for the pattern.
- Robustness matters here more than in most projects: if you touch a
  parser, run its `tests/robustness.rs` suite (`cargo test -p <crate>
  --test robustness`) before and after your change. These are
  `proptest`-driven fuzz tests that feed arbitrary and mutated bytes
  through the parser — the whole point of this project is turning messy,
  untrusted files into clean output without crashing or silently
  corrupting data, and that guarantee is only as good as the fuzz
  coverage behind it.

## Regenerating binary fixtures

Some formats (PDF, Excel, Avro, Parquet, SQLite) commit binary fixture
files under `fixtures/<format>/` rather than constructing them inline in
test code, since building a realistic file (with real headers, real
compression, real embedded schemas) inline would be more fragile than
generating it once and checking it in. Each of these crates has a
`gen_fixtures*` example that (re)builds its fixtures deterministically:

```bash
cargo run -p tidyrs-pdf --example gen_fixtures_pdf
cargo run -p tidyrs-xlsx --example gen_fixtures
cargo run -p tidyrs-avro --example gen_fixtures_avro
cargo run -p tidyrs-parquet --example gen_fixtures_parquet
cargo run -p tidyrs-sqlite --example gen_fixtures
```

**A known gotcha:** regenerating a format's fixtures with its
`gen_fixtures*` example often rewrites *every* fixture for that format,
not just the one you actually changed — the generator is one file that
builds them all. Some formats embed a timestamp or similar
non-deterministic metadata, so files you didn't intend to touch can come
back byte-different (same size, different bytes) even though nothing
about their content changed. Before committing, check `git diff --stat`
on the fixture directory: if a file changed size, that's real; if it
shows `Bin N -> N bytes` (same size, 0 insertions/deletions reported),
that's the timestamp-noise case — `git checkout --` it to revert the
unintended regeneration and keep only the fixture you actually meant to
add or change.

## Commit style

Commit messages here are short, imperative titles ("Fix X", "Add Y",
"Investigate Z: confirm bounded, no fix needed") — no enforced prefix
convention (no required `feat:`/`fix:` tags). The body explains the *why*
behind a change, not a restatement of the diff: what was actually broken
or missing, how it was found (a fuzz test, an external report, reading a
dependency's source), and why the chosen fix is the right one — especially
if an alternative was tried and rejected. `git log` is the best reference
for the expected tone and depth.

## Adding a new format

Every input format implements `tidyrs_core::TidyParser` — `sniff()` for
content/filename-based format detection, and `parse()` to produce a
`TidyTable` plus a `CleaningReport` describing what happened (rows
in/out, notes, warnings). A new format crate:

1. Lives in its own `tidyrs-<format>` crate, depending only on
   `tidyrs-core` and whatever parsing library the format needs.
2. Implements `TidyParser`, converting the format's own value types into
   `TidyValue` (`Null`/`Bool`/`Int`/`Float`/`Text` — see existing crates
   for how each handles richer native types that don't map cleanly, e.g.
   dates, nested structures, or decimals).
3. Ships fixtures + `tests/fixtures.rs` covering realistic cases, and a
   `tests/robustness.rs` proptest suite (see any existing crate's for the
   pattern — arbitrary bytes, bytes with the format's magic header
   forced on the front, and mutated real fixture bytes are the three
   baseline cases every format's suite covers).
4. Gets registered in `tidyrs-cli`'s parser list and added to the
   workspace `Cargo.toml`.

Look at `tidyrs-ini` or `tidyrs-fixed` for a relatively small, complete
example crate to model a new one on.

## Releasing (maintainers)

Pushing a tag matching `v*.*.*` (e.g. `v0.1.0`) triggers
`.github/workflows/release.yml`, which builds `tidyloom` for
Linux/macOS (x86_64 + aarch64)/Windows and attaches the binaries to a
GitHub Release. Not published to crates.io yet.
