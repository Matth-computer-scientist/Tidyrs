# Roadmap

A rough sense of direction, not a commitment or a schedule. If you want
to work on any of this, say so in the relevant issue (or open one) before
starting — helps avoid duplicate work.

## Now

- **Stabilize JSON/XML/YAML.** Currently experimental — the flattening
  behavior (array explosion, key-collision handling, nested type
  fallbacks) needs more real-world fixture coverage before it gets the
  same "stable" label as CSV/Excel/fixed-width/INI/SQLite.
- **Stabilize PDF extraction**, or clearly draw the line on what it will
  never handle well. The whitespace-alignment heuristic has several
  precisely-documented, deliberately-unfixed limitations (see the
  "Reliability notes" section of the README) — multi-word titles, a data
  cell that coincidentally looks like two words, mixed free-text +
  table content. These are architectural limits of a heuristic approach,
  not bugs waiting for a quick fix; a real fix means glyph-position-based
  table-region detection, which is a bigger project.
- **Close the remaining `apache_avro` allocation vulnerability.** A fix
  landed for the specific case this project's fuzz suite found (an
  adversarial OCF header), but the general case — a `Map`/`Array` field
  inside a real data schema hitting the same unguarded
  `HashMap::reserve`/`Vec::reserve` — is still open upstream. Worth
  tracking or reporting against `apache_avro` itself.
- **Investigate the `tidyrs-parquet` allocation-size issue properly.**
  Documented as a real, bounded (~2GB/page) vulnerability in the
  `parquet` crate's page decompression path, found by reading its source
  after a fuzz-suite crash that couldn't be reproduced on demand. A safe
  fix needs partial Thrift compact-protocol decoding to pre-validate a
  page header — real engineering, not attempted yet.

## Next

- **Publish to crates.io.** `cargo install tidyloom` instead of clone +
  build — the single biggest friction reducer for anyone who just wants
  to try it. Needs the JSON/PDF stabilization work above to land first
  (or an explicit "these two are experimental in v0.1.0" release note).
- **Test coverage reporting (codecov or similar).** No coverage badge in
  the README yet because there's no coverage tooling wired into CI —
  needs a codecov (or equivalent) account and a CI step to generate and
  upload a coverage report.
- **OCR for scanned/image PDFs**, currently explicitly out of scope
  (`tidyrs-pdf` only handles text-based PDFs). Would be a separate,
  feature-flagged code path, not a change to the existing text-extraction
  approach.
- **A pluggable `AmbiguityResolver` provider beyond the built-in
  rule-based one** — the LLM-backed resolver behind the `llm` feature
  flag already proves the extension point works; more providers
  (local models, a different hosted API) would validate it further.

## Later / exploratory

- Streaming (bounded-memory) cleaning for formats beyond CSV — currently
  the only format with a dedicated streaming entry point
  (`stream_clean_csv`).
- A schema-inference mode that suggests a `tidyloom.toml` schema gate
  from a sample file, rather than requiring one to be hand-written.
- Parallel processing for `--batch` mode.

## Won't do (for now)

- A GUI or desktop app. This is deliberately a headless
  library/CLI — see the README's opening line.
- A hosted/SaaS version.

---

Contributions toward any of this are welcome — see
[CONTRIBUTING.md](CONTRIBUTING.md). If you want to pick up something not
listed here, open an issue first so the direction can be agreed on before
you sink time into it.
