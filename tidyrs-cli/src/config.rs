//! Declarative per-project defaults, loaded from a `tidyloom.toml` file
//! (or a path given via `--config`) so a project doesn't have to repeat
//! the same flags on every `tidyloom clean` invocation. CLI flags always
//! win when both are given — this file only fills in what wasn't
//! explicitly passed.
//!
//! Example `tidyloom.toml`:
//!
//! ```toml
//! [defaults]
//! output_format = "parquet"
//! has_header = true
//! schema = "schemas/orders.json"
//! on_schema_violation = "reject"
//! verbose_report = true
//! ```

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub defaults: Defaults,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Defaults {
    pub format: Option<String>,
    pub output_format: Option<String>,
    pub delimiter: Option<String>,
    pub has_header: Option<bool>,
    pub merge_fill: Option<bool>,
    pub sheet: Option<String>,
    pub mode: Option<String>,
    pub separator: Option<String>,
    pub array_join_sep: Option<String>,
    pub array_mode: Option<String>,
    pub verbose_report: Option<bool>,
    pub stream: Option<bool>,
    pub schema: Option<PathBuf>,
    pub on_schema_violation: Option<String>,
    pub dry_run: Option<bool>,
}

/// Loads config from `explicit_path` if given, otherwise looks for
/// `tidyloom.toml` in the current directory. Returns the default (empty)
/// config, not an error, when no file is found — a config file is
/// optional. An explicitly-passed `--config` path that doesn't exist or
/// doesn't parse *is* an error, since that's presumably a typo the user
/// wants to know about.
pub fn load(explicit_path: Option<&Path>) -> anyhow::Result<Config> {
    let path = match explicit_path {
        Some(p) => p.to_path_buf(),
        None => {
            let default = PathBuf::from("tidyloom.toml");
            if !default.exists() {
                return Ok(Config::default());
            }
            default
        }
    };

    let text = std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("reading config {}: {e}", path.display()))?;
    let config: Config = toml::from_str(&text).map_err(|e| anyhow::anyhow!("parsing config {}: {e}", path.display()))?;
    Ok(config)
}

/// `cli` wins when set; otherwise falls back to `config`.
pub fn merge_opt(cli: Option<String>, config: Option<String>) -> Option<String> {
    cli.or(config)
}

/// A plain CLI bool flag has no way to distinguish "not passed" from
/// "passed as false" (clap gives `false` either way), so an explicit
/// `true` on the CLI always wins; otherwise the config value applies
/// (defaulting to `false` if neither is set). This means a config file
/// can't be overridden back to `false` from the CLI for these flags — a
/// deliberate, documented simplification rather than adding `--no-*`
/// counterparts for every boolean.
pub fn merge_bool(cli: bool, config: Option<bool>) -> bool {
    cli || config.unwrap_or(false)
}
