mod config;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tidyrs_core::{export, CleaningReport, FormatRegistry, ParseOptions, TidyTable};

#[derive(Parser)]
#[command(name = "tidyloom", version, about = "Universal normalization for chaotic data files")]
struct Cli {
    /// Log output format: "text" (default, human-readable) or "json"
    /// (one structured JSON object per line — for piping into log
    /// aggregation or another tool). Overridable per-run; level is
    /// controlled by the standard `RUST_LOG` env var (default: info).
    #[arg(long, global = true, value_parser = ["text", "json"], default_value = "text")]
    log_format: String,

    #[command(subcommand)]
    command: Commands,
}

/// Sets up two log layers instead of one: INFO (the normal per-file
/// status trail — "detected: csv, rows: N in / M out", "batch complete",
/// ...) goes to stdout, exactly where the old `println!`-based output
/// always went; WARN/ERROR (fallback notices, schema violations, batch
/// failures) go to stderr, exactly where the old `eprintln!` calls always
/// went. Routing is by level, not by call site, so existing scripts that
/// pipe/grep either stream keep working unchanged — this is a genuine
/// upgrade to structured, leveled logging, not a stream-semantics
/// regression wearing a `tracing` costume.
fn init_logging(format: &str) {
    use tracing_subscriber::filter::filter_fn;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    let env_filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let is_stdout_level = |meta: &tracing::Metadata| matches!(*meta.level(), tracing::Level::INFO | tracing::Level::DEBUG | tracing::Level::TRACE);
    let is_stderr_level = |meta: &tracing::Metadata| matches!(*meta.level(), tracing::Level::WARN | tracing::Level::ERROR);

    if format == "json" {
        let stdout_layer = fmt::layer().json().with_writer(std::io::stdout).with_filter(filter_fn(is_stdout_level));
        let stderr_layer = fmt::layer().json().with_writer(std::io::stderr).with_filter(filter_fn(is_stderr_level));
        tracing_subscriber::registry().with(env_filter()).with(stdout_layer).with(stderr_layer).init();
    } else {
        let stdout_layer = fmt::layer()
            .compact()
            .without_time()
            .with_target(false)
            .with_writer(std::io::stdout)
            .with_filter(filter_fn(is_stdout_level));
        let stderr_layer = fmt::layer()
            .compact()
            .without_time()
            .with_target(false)
            .with_writer(std::io::stderr)
            .with_filter(filter_fn(is_stderr_level));
        tracing_subscriber::registry().with(env_filter()).with(stdout_layer).with(stderr_layer).init();
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Clean one file, or every file in a directory with --batch.
    Clean {
        /// Input file to clean (omit when using --batch).
        input: Option<PathBuf>,

        /// Process every file in this directory instead of a single input.
        #[arg(long)]
        batch: Option<PathBuf>,

        /// Output file (single-file mode).
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,

        /// Output directory (batch mode).
        #[arg(long)]
        output_dir: Option<PathBuf>,

        /// Write the full cleaning report as JSON to this file (single-file mode).
        #[arg(long)]
        report_file: Option<PathBuf>,

        /// Write one JSON cleaning report per input file into this directory (batch mode).
        #[arg(long)]
        report_dir: Option<PathBuf>,

        /// Force a specific parser instead of auto-detecting the format.
        #[arg(long, value_parser = ["csv", "xlsx", "json", "xml", "fixed", "pdf"])]
        format: Option<String>,

        /// Output format: csv, json, or parquet. Inferred from --output's
        /// extension when omitted (defaults to csv for --batch).
        #[arg(long, value_parser = ["csv", "json", "parquet"])]
        output_format: Option<String>,

        /// CSV: force a delimiter instead of auto-detecting one.
        #[arg(long)]
        delimiter: Option<String>,

        /// Treat the first row/line as a header (default: on for csv/xlsx, off for fixed-width).
        #[arg(long)]
        has_header: bool,

        /// Treat the first row/line as data, not a header.
        #[arg(long, conflicts_with = "has_header")]
        no_header: bool,

        /// Excel: forward-fill values into cells left empty by merged regions (default: on).
        #[arg(long, conflicts_with = "no_merge_fill")]
        merge_fill: bool,

        #[arg(long, conflicts_with = "merge_fill")]
        no_merge_fill: bool,

        /// Excel: only process this sheet name (default: all sheets).
        #[arg(long)]
        sheet: Option<String>,

        /// Fixed-width: "fixed" (infer column alignment) or "whitespace" (split on any whitespace run).
        #[arg(long, value_parser = ["fixed", "whitespace"])]
        mode: Option<String>,

        /// JSON/XML: separator used when flattening nested keys.
        #[arg(long)]
        separator: Option<String>,

        /// JSON/XML: separator used when joining array values into one column.
        #[arg(long)]
        array_join_sep: Option<String>,

        /// JSON/XML: "join" (default, arrays become one delimited text column)
        /// or "explode" (arrays of objects become extra rows).
        #[arg(long, value_parser = ["join", "explode"])]
        array_mode: Option<String>,

        /// Print the full cleaning report (not just a one-line summary) for each file.
        #[arg(long)]
        verbose_report: bool,

        /// Stream CSV-to-CSV in bounded memory instead of loading the whole
        /// file. Only applies when the (detected or forced) input format is
        /// csv and the output format is csv; falls back to the normal
        /// in-memory path otherwise (with a note explaining why).
        #[arg(long)]
        stream: bool,

        /// Validate the cleaned table against a JSON schema file (see
        /// tidyrs_core::schema for the shape). Not supported in --stream mode.
        #[arg(long)]
        schema: Option<PathBuf>,

        /// What to do when --schema finds violations: "warn" (default, print
        /// them but still write output) or "reject" (print them, exit
        /// non-zero, and don't write output for that file).
        #[arg(long, value_parser = ["warn", "reject"])]
        on_schema_violation: Option<String>,

        /// Show what would be written (a diff against the existing output
        /// file, or a preview if there isn't one yet) without writing
        /// anything. Implies the in-memory path (ignores --stream).
        #[arg(long)]
        dry_run: bool,

        /// Load per-project defaults from this TOML file instead of looking
        /// for ./tidyloom.toml. CLI flags always override config values.
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

fn build_registry() -> FormatRegistry {
    let mut reg = FormatRegistry::new();
    reg.register(Box::new(tidyrs_csv::CsvParser::new()));
    reg.register(Box::new(tidyrs_xlsx::XlsxParser::new()));
    reg.register(Box::new(tidyrs_json::JsonXmlParser::new()));
    reg.register(Box::new(tidyrs_fixed::FixedWidthParser::new()));
    reg.register(Box::new(tidyrs_pdf::PdfParser::new()));
    reg
}

struct RunOptions {
    format: Option<String>,
    output_format: Option<String>,
    delimiter: Option<String>,
    has_header: bool,
    no_header: bool,
    merge_fill: bool,
    no_merge_fill: bool,
    sheet: Option<String>,
    mode: Option<String>,
    separator: Option<String>,
    array_join_sep: Option<String>,
    array_mode: Option<String>,
    verbose_report: bool,
    stream: bool,
    schema: Option<PathBuf>,
    on_schema_violation: Option<String>,
    dry_run: bool,
}

fn parse_options_for(opts: &RunOptions) -> ParseOptions {
    let mut map: HashMap<String, String> = HashMap::new();
    if let Some(d) = &opts.delimiter {
        map.insert("delimiter".into(), d.clone());
    }
    if opts.has_header {
        map.insert("has_header".into(), "true".into());
    } else if opts.no_header {
        map.insert("has_header".into(), "false".into());
    }
    if opts.merge_fill {
        map.insert("merge_fill".into(), "true".into());
    } else if opts.no_merge_fill {
        map.insert("merge_fill".into(), "false".into());
    }
    if let Some(s) = &opts.sheet {
        map.insert("sheet".into(), s.clone());
    }
    if let Some(m) = &opts.mode {
        map.insert("mode".into(), m.clone());
    }
    if let Some(s) = &opts.separator {
        map.insert("separator".into(), s.clone());
    }
    if let Some(s) = &opts.array_join_sep {
        map.insert("array_join_sep".into(), s.clone());
    }
    if let Some(m) = &opts.array_mode {
        map.insert("array_mode".into(), m.clone());
    }
    ParseOptions::from(map)
}

fn output_format_for(opts: &RunOptions, output_path: &Path) -> String {
    if let Some(f) = &opts.output_format {
        return f.clone();
    }
    match output_path.extension().and_then(|e| e.to_str()) {
        Some("json") => "json".to_string(),
        Some("parquet") => "parquet".to_string(),
        _ => "csv".to_string(),
    }
}

fn write_table(table: &TidyTable, output_format: &str, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    match output_format {
        "csv" => {
            let file = std::fs::File::create(path)?;
            export::write_csv(table, file)?;
        }
        "json" => {
            let file = std::fs::File::create(path)?;
            export::write_json(table, file)?;
        }
        "parquet" => {
            export::write_parquet_file(table, path)?;
        }
        other => bail!("unsupported output format: {other}"),
    }
    Ok(())
}

/// The per-file status trail (this summary line, plus its `--verbose-report`
/// sub-notes) is deliberately always logged at INFO — i.e. always on
/// stdout — regardless of whether the underlying cleaning run hit
/// warnings. That's not an oversight: it's the primary "what happened"
/// output every prior version of this CLI printed unconditionally via
/// `println!`, and a file merely containing warnings (e.g. some ragged
/// rows got padded) is still a normal, successful run, not an error a
/// pipeline should have to go digging through stderr for. Only genuinely
/// exceptional conditions — a fallback that changed behavior, a schema
/// violation, a file that failed outright — are logged at WARN/ERROR and
/// routed to stderr elsewhere in this file. The "[warn]"/"[ok]" text
/// prefix still reflects severity; only the log *level* (and therefore
/// the stream) stays constant here.
fn print_report(report: &CleaningReport, verbose: bool) {
    let has_warnings = report.notes.iter().any(|n| matches!(n.severity, tidyrs_core::Severity::Warning));
    let summary = format!(
        "[{}] {} -> detected: {}, rows: {} in / {} out, {} note(s)",
        if has_warnings { "warn" } else { "ok" },
        report.source_file,
        report.detected_format,
        report.rows_in,
        report.rows_out,
        report.notes.len()
    );
    tracing::info!(
        file = %report.source_file,
        format = %report.detected_format,
        rows_in = report.rows_in,
        rows_out = report.rows_out,
        note_count = report.notes.len(),
        has_warnings,
        "{summary}"
    );
    if verbose {
        for note in &report.notes {
            let tag = match note.severity {
                tidyrs_core::Severity::Info => "info",
                tidyrs_core::Severity::Warning => "warn",
            };
            tracing::info!(file = %report.source_file, "    [{tag}] {}", note.message);
        }
    }
}

fn write_report(report: &CleaningReport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, report)?;
    Ok(())
}

const SNIFF_PREFIX_LEN: usize = 65536;

/// Reads up to `SNIFF_PREFIX_LEN` bytes from the start of `path` — enough
/// for format detection without loading the whole file, which is the
/// point of the streaming path in the first place.
fn read_sniff_prefix(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mut buf = vec![0u8; SNIFF_PREFIX_LEN];
    let mut filled = 0;
    loop {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
        if filled == buf.len() {
            break;
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

fn process_file(registry: &FormatRegistry, input: &Path, out_target: &Path, report_target: Option<&Path>, opts: &RunOptions) -> Result<()> {
    let filename = input.file_name().and_then(|n| n.to_str()).unwrap_or("input");
    let output_format = output_format_for(opts, out_target);

    if opts.stream && (opts.schema.is_some() || opts.dry_run) {
        tracing::warn!(
            file = %input.display(),
            "[info] {}: --schema/--dry-run require the in-memory path (there's no table to validate or preview while streaming); ignoring --stream",
            input.display()
        );
    }

    if opts.stream && opts.schema.is_none() && !opts.dry_run {
        let prefix = read_sniff_prefix(input)?;
        let resolved_format = match &opts.format {
            Some(name) => Some(name.clone()),
            None => registry.detect(&prefix, Some(filename)).map(|d| d.parser.format_name().to_string()),
        };

        if resolved_format.as_deref() == Some("csv") && output_format == "csv" {
            let in_file = std::fs::File::open(input).with_context(|| format!("reading {}", input.display()))?;
            let out_file = std::fs::File::create(out_target).with_context(|| format!("writing {}", out_target.display()))?;
            let parse_opts = parse_options_for(opts);
            let report = tidyrs_csv::stream_clean_csv(in_file, out_file, filename, &parse_opts)
                .with_context(|| format!("streaming {}", input.display()))?;
            print_report(&report, opts.verbose_report);
            if let Some(report_path) = report_target {
                write_report(&report, report_path)?;
            }
            return Ok(());
        }

        tracing::warn!(
            file = %input.display(),
            "[info] {}: --stream requested but only applies to csv-to-csv (detected format: {}, output format: {output_format}); \
             falling back to the normal in-memory path",
            input.display(),
            resolved_format.as_deref().unwrap_or("unknown")
        );
    }

    let bytes = std::fs::read(input).with_context(|| format!("reading {}", input.display()))?;

    let parser = if let Some(name) = &opts.format {
        registry.by_name(name).with_context(|| format!("unknown --format '{name}'"))?
    } else {
        registry
            .detect(&bytes, Some(filename))
            .map(|d| d.parser)
            .with_context(|| format!("could not detect the format of {} (pass --format to force one)", input.display()))?
    };

    let parse_opts = parse_options_for(opts);
    let outcome = parser
        .parse(&bytes, filename, &parse_opts)
        .with_context(|| format!("parsing {}", input.display()))?;

    print_report(&outcome.report, opts.verbose_report);

    if let Some(report_path) = report_target {
        write_report(&outcome.report, report_path)?;
    }

    if let Some(schema_path) = &opts.schema {
        let schema_text = std::fs::read_to_string(schema_path).with_context(|| format!("reading schema {}", schema_path.display()))?;
        let schema = tidyrs_core::Schema::from_json(&schema_text).with_context(|| format!("parsing schema {}", schema_path.display()))?;
        let reject = opts.on_schema_violation.as_deref() == Some("reject");

        for table in &outcome.tables {
            let validation = tidyrs_core::validate(table, &schema);
            if !validation.is_valid() {
                let label = table.source.clone().unwrap_or_else(|| filename.to_string());
                tracing::warn!(
                    file = %label,
                    violations = validation.issues.len(),
                    total_rows = validation.total_rows,
                    "[schema] {label}: {} violation(s) out of {} row(s)",
                    validation.issues.len(),
                    validation.total_rows
                );
                for issue in &validation.issues {
                    match issue.row {
                        Some(r) => tracing::warn!(file = %label, row = r, column = %issue.column, "    row {r}, column '{}': {}", issue.column, issue.message),
                        None => tracing::warn!(file = %label, column = %issue.column, "    column '{}': {}", issue.column, issue.message),
                    }
                }
                if reject {
                    bail!("{label}: schema validation failed with {} violation(s) (--on-schema-violation reject)", validation.issues.len());
                }
            }
        }
    }

    let targets: Vec<(&TidyTable, PathBuf)> = if outcome.tables.len() == 1 {
        vec![(&outcome.tables[0], out_target.to_path_buf())]
    } else {
        let stem = out_target.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        let ext = out_target.extension().and_then(|e| e.to_str()).unwrap_or(output_format.as_str());
        let dir = out_target.parent().unwrap_or_else(|| Path::new("."));
        outcome
            .tables
            .iter()
            .map(|table| {
                let suffix = table.source.clone().unwrap_or_default();
                let safe_suffix: String = suffix.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                (table, dir.join(format!("{stem}_{safe_suffix}.{ext}")))
            })
            .collect()
    };

    if opts.dry_run {
        for (table, path) in &targets {
            preview_or_diff(table, &output_format, path)?;
        }
        return Ok(());
    }

    for (table, path) in &targets {
        write_table(table, &output_format, path)?;
        if targets.len() > 1 {
            tracing::info!(path = %path.display(), "    -> wrote {}", path.display());
        }
    }

    Ok(())
}

/// Renders `table` the way it would be written for `output_format`, as
/// bytes — `None` for parquet, which is a binary format with no
/// meaningful line-based diff.
fn render_table_bytes(table: &TidyTable, output_format: &str) -> Result<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    match output_format {
        "csv" => export::write_csv(table, &mut buf)?,
        "json" => export::write_json(table, &mut buf)?,
        "parquet" => return Ok(None),
        other => bail!("unsupported output format: {other}"),
    }
    Ok(Some(buf))
}

/// `--dry-run` output: a unified diff against the existing file at `path`
/// if there is one, or a short preview of what would be created.
fn preview_or_diff(table: &TidyTable, output_format: &str, path: &Path) -> Result<()> {
    let Some(new_bytes) = render_table_bytes(table, output_format)? else {
        println!("[dry-run] {}: would write a parquet file ({} rows, {} columns) — binary, no preview", path.display(), table.rows.len(), table.headers.len());
        return Ok(());
    };
    let new_text = String::from_utf8_lossy(&new_bytes);

    if path.exists() {
        let old_text = std::fs::read_to_string(path).with_context(|| format!("reading existing {}", path.display()))?;
        if old_text == new_text {
            println!("[dry-run] {}: unchanged", path.display());
            return Ok(());
        }
        println!("[dry-run] {}: would change —", path.display());
        let diff = similar::TextDiff::from_lines(old_text.as_str(), new_text.as_ref());
        let mut shown = 0;
        for change in diff.iter_all_changes() {
            let tag = match change.tag() {
                similar::ChangeTag::Delete => "-",
                similar::ChangeTag::Insert => "+",
                similar::ChangeTag::Equal => continue,
            };
            print!("    {tag} {change}");
            shown += 1;
            if shown >= 40 {
                println!("    ... (diff truncated)");
                break;
            }
        }
    } else {
        let preview_lines: Vec<&str> = new_text.lines().take(6).collect();
        println!(
            "[dry-run] {}: would create new file ({} rows, {} columns). Preview:",
            path.display(),
            table.rows.len(),
            table.headers.len()
        );
        for line in preview_lines {
            println!("    {line}");
        }
        if new_text.lines().count() > 6 {
            println!("    ...");
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(&cli.log_format);
    let registry = build_registry();

    match cli.command {
        Commands::Clean {
            input,
            batch,
            output,
            output_dir,
            report_file,
            report_dir,
            format,
            output_format,
            delimiter,
            has_header,
            no_header,
            merge_fill,
            no_merge_fill,
            sheet,
            mode,
            separator,
            array_join_sep,
            array_mode,
            verbose_report,
            stream,
            schema,
            on_schema_violation,
            dry_run,
            config: config_path,
        } => {
            let cfg = config::load(config_path.as_deref())?;
            let d = &cfg.defaults;

            let has_header = if no_header { false } else { config::merge_bool(has_header, d.has_header) };
            let merge_fill = if no_merge_fill { false } else { config::merge_bool(merge_fill, d.merge_fill) };

            let opts = RunOptions {
                format: config::merge_opt(format, d.format.clone()),
                output_format: config::merge_opt(output_format, d.output_format.clone()),
                delimiter: config::merge_opt(delimiter, d.delimiter.clone()),
                has_header,
                no_header,
                merge_fill,
                no_merge_fill,
                sheet: config::merge_opt(sheet, d.sheet.clone()),
                mode: config::merge_opt(mode, d.mode.clone()),
                separator: config::merge_opt(separator, d.separator.clone()),
                array_join_sep: config::merge_opt(array_join_sep, d.array_join_sep.clone()),
                array_mode: config::merge_opt(array_mode, d.array_mode.clone()),
                verbose_report: config::merge_bool(verbose_report, d.verbose_report),
                stream: config::merge_bool(stream, d.stream),
                schema: schema.or_else(|| d.schema.clone()),
                on_schema_violation: config::merge_opt(on_schema_violation, d.on_schema_violation.clone()),
                dry_run: config::merge_bool(dry_run, d.dry_run),
            };

            match (input, batch) {
                (Some(_), Some(_)) => bail!("pass either an input file or --batch, not both"),
                (None, None) => bail!("pass an input file, or --batch <dir>"),
                (Some(input_path), None) => {
                    let out = output.unwrap_or_else(|| input_path.with_extension("clean.csv"));
                    process_file(&registry, &input_path, &out, report_file.as_deref(), &opts)?;
                }
                (None, Some(batch_dir)) => {
                    let out_dir = output_dir.unwrap_or_else(|| PathBuf::from("./clean"));
                    std::fs::create_dir_all(&out_dir)?;
                    if let Some(dir) = &report_dir {
                        std::fs::create_dir_all(dir)?;
                    }
                    let mut count = 0usize;
                    let mut failures = 0usize;
                    for entry in std::fs::read_dir(&batch_dir).with_context(|| format!("reading directory {}", batch_dir.display()))? {
                        let entry = entry?;
                        let path = entry.path();
                        if !path.is_file() {
                            continue;
                        }
                        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
                        let ext = opts.output_format.clone().unwrap_or_else(|| "csv".to_string());
                        let out_path = out_dir.join(format!("{stem}.{ext}"));
                        let report_path = report_dir.as_ref().map(|dir| dir.join(format!("{stem}.report.json")));
                        match process_file(&registry, &path, &out_path, report_path.as_deref(), &opts) {
                            Ok(()) => count += 1,
                            Err(e) => {
                                failures += 1;
                                tracing::error!(file = %path.display(), error = %format!("{e:#}"), "[error] {}: {e:#}", path.display());
                            }
                        }
                    }
                    tracing::info!(cleaned = count, failed = failures, "batch complete: {count} file(s) cleaned, {failures} failure(s)");
                }
            }
        }
    }

    Ok(())
}
