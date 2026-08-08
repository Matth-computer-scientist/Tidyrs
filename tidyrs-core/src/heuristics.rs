//! Extension point for resolving ambiguous cases that plain rules can't
//! settle with confidence — e.g. a column whose sampled values look like
//! neither a clean number nor a clean date. This version ships only the
//! rule-based resolver. Wiring a real LLM in behind [`AmbiguityResolver`]
//! is a documented TODO, not implemented here (see `LlmAmbiguityResolver`).

/// A guess about what a column actually contains, with a confidence score
/// so callers can decide whether to trust it or fall back to raw text.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnTypeGuess {
    Integer,
    Float,
    Boolean,
    Date,
    Text,
}

pub trait AmbiguityResolver {
    /// Given a column name and a sample of its raw string values, guess the
    /// most likely type and a confidence in [0.0, 1.0].
    fn resolve_column_type(&self, column_name: &str, samples: &[String]) -> (ColumnTypeGuess, f32);
}

/// Default, fully offline resolver: a handful of regex/parse-based rules.
/// This is what tidyloom uses out of the box.
pub struct RuleBasedResolver;

impl AmbiguityResolver for RuleBasedResolver {
    fn resolve_column_type(&self, _column_name: &str, samples: &[String]) -> (ColumnTypeGuess, f32) {
        let non_empty: Vec<&String> = samples.iter().filter(|s| !s.trim().is_empty()).collect();
        if non_empty.is_empty() {
            return (ColumnTypeGuess::Text, 0.0);
        }
        let total = non_empty.len() as f32;

        // A leading-zero value ("00501", "007") is excluded from both
        // counts below, not just the Integer one: `str::parse::<f64>`
        // accepts "00501" just as readily as `i64` does (-> 501.0),
        // losing the same meaningful leading zero either way. A column
        // that's mostly postal codes with a couple of genuinely-numeric
        // values mixed in should fall through to Text (or the ambiguous-
        // column per-cell fallback) rather than have the whole column
        // confidently committed to a numeric type that mangles most of it.
        let ints = non_empty
            .iter()
            .filter(|s| {
                let t = s.trim();
                !crate::has_meaningful_leading_zero(t) && t.parse::<i64>().is_ok()
            })
            .count() as f32
            / total;
        if ints > 0.9 {
            return (ColumnTypeGuess::Integer, ints);
        }

        let floats = non_empty
            .iter()
            .filter(|s| {
                let t = s.trim();
                !crate::has_meaningful_leading_zero(t) && t.parse::<f64>().is_ok()
            })
            .count() as f32
            / total;
        if floats > 0.9 {
            return (ColumnTypeGuess::Float, floats);
        }

        let bools = non_empty
            .iter()
            .filter(|s| matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "false" | "yes" | "no"))
            .count() as f32
            / total;
        if bools > 0.9 {
            return (ColumnTypeGuess::Boolean, bools);
        }

        let dates = non_empty.iter().filter(|s| looks_like_date(s.trim())).count() as f32 / total;
        if dates > 0.9 {
            return (ColumnTypeGuess::Date, dates);
        }

        // Nothing hit the 90% bar. This still might be a genuinely,
        // unambiguously text column (city names: none of the ratios above
        // are anywhere close to matching) — or it might be a mostly-
        // numeric column with a handful of typos/placeholders ("N/A",
        // "seventeen") mixed in, where forcing the *whole* column to Text
        // would silently destroy perfectly good numeric values in every
        // other row. We can't tell those apart from a type name alone, so
        // the confidence reported here reflects it: confidence is high
        // (this really does look like text) exactly when every other
        // candidate ratio was low, and low (this is genuinely ambiguous,
        // let the caller fall back to per-cell inference instead of
        // committing) when some other type came close but not close
        // enough.
        let best_other = ints.max(floats).max(bools).max(dates);
        (ColumnTypeGuess::Text, 1.0 - best_other)
    }
}

fn looks_like_date(s: &str) -> bool {
    let digits_and_seps = s.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '/' || c == '.');
    let has_two_seps = s.chars().filter(|&c| c == '-' || c == '/' || c == '.').count() >= 2;
    digits_and_seps && has_two_seps && s.len() >= 6 && s.len() <= 10
}

/// Placeholder kept for embedders who linked against earlier tidyloom
/// versions without the `llm` feature: real LLM support now lives in
/// [`HttpLlmResolver`] behind `--features llm`. This type intentionally
/// still does nothing — it isn't a config-free "just works" resolver
/// (it would need an API key, a model, a network call), so pretending it
/// does something without configuration would be worse than being
/// explicit about the TODO.
pub struct LlmAmbiguityResolver;

impl AmbiguityResolver for LlmAmbiguityResolver {
    fn resolve_column_type(&self, _column_name: &str, _samples: &[String]) -> (ColumnTypeGuess, f32) {
        unimplemented!(
            "LlmAmbiguityResolver is a placeholder; build with `--features llm` and use \
             HttpLlmResolver instead, or provide your own AmbiguityResolver implementation"
        )
    }
}

#[cfg(feature = "llm")]
mod llm {
    use super::{AmbiguityResolver, ColumnTypeGuess};
    use std::time::Duration;

    /// An [`AmbiguityResolver`] backed by any OpenAI-compatible chat
    /// completions API (OpenAI itself, Azure OpenAI, a local vLLM/Ollama
    /// server that speaks the same wire format, ...). This is the actual
    /// wiring the core `AmbiguityResolver` trait boundary was built for —
    /// gated behind the `llm` feature so the base crate never pulls in an
    /// HTTP client or makes network calls unless explicitly asked to.
    ///
    /// On any failure (network error, bad response, malformed JSON) this
    /// returns `(ColumnTypeGuess::Text, 0.0)` rather than panicking or
    /// erroring — a confidence of 0.0 tells the caller to fall back to
    /// `RuleBasedResolver`'s guess or to raw text, the same way a
    /// low-confidence rule-based guess would be handled.
    pub struct HttpLlmResolver {
        pub base_url: String,
        pub api_key: String,
        pub model: String,
        pub timeout: Duration,
    }

    impl HttpLlmResolver {
        /// Reads configuration from environment variables:
        /// `TIDYLOOM_LLM_BASE_URL` (default: OpenAI's API), `TIDYLOOM_LLM_API_KEY`
        /// (required), `TIDYLOOM_LLM_MODEL` (default: "gpt-4o-mini").
        pub fn from_env() -> Result<Self, String> {
            let api_key = std::env::var("TIDYLOOM_LLM_API_KEY").map_err(|_| "TIDYLOOM_LLM_API_KEY is not set".to_string())?;
            Ok(Self {
                base_url: std::env::var("TIDYLOOM_LLM_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
                api_key,
                model: std::env::var("TIDYLOOM_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
                timeout: Duration::from_secs(10),
            })
        }

        pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
            Self {
                base_url: base_url.into(),
                api_key: api_key.into(),
                model: model.into(),
                timeout: Duration::from_secs(10),
            }
        }

        fn prompt(column_name: &str, samples: &[String]) -> String {
            let sample_list = samples.iter().take(20).map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(", ");
            format!(
                "Column name: {column_name}\nSample values: [{sample_list}]\n\n\
                 Classify this column's most likely data type. Respond with ONLY a JSON object, \
                 no other text: {{\"type\": one of \"integer\"|\"float\"|\"boolean\"|\"date\"|\"text\", \
                 \"confidence\": a number between 0.0 and 1.0}}."
            )
        }

        fn parse_type(s: &str) -> ColumnTypeGuess {
            match s {
                "integer" => ColumnTypeGuess::Integer,
                "float" => ColumnTypeGuess::Float,
                "boolean" => ColumnTypeGuess::Boolean,
                "date" => ColumnTypeGuess::Date,
                _ => ColumnTypeGuess::Text,
            }
        }
    }

    impl AmbiguityResolver for HttpLlmResolver {
        fn resolve_column_type(&self, column_name: &str, samples: &[String]) -> (ColumnTypeGuess, f32) {
            let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": "You are a precise data-type classifier. Reply with strict JSON only."},
                    {"role": "user", "content": Self::prompt(column_name, samples)}
                ],
                "temperature": 0.0,
            });

            let response = ureq::post(&url)
                .set("Authorization", &format!("Bearer {}", self.api_key))
                .timeout(self.timeout)
                .send_json(body);

            let response = match response {
                Ok(r) => r,
                Err(_) => return (ColumnTypeGuess::Text, 0.0),
            };

            let parsed: serde_json::Value = match response.into_json() {
                Ok(v) => v,
                Err(_) => return (ColumnTypeGuess::Text, 0.0),
            };

            let content = parsed["choices"][0]["message"]["content"].as_str().unwrap_or("");
            let classification: serde_json::Value = match serde_json::from_str(content) {
                Ok(v) => v,
                Err(_) => return (ColumnTypeGuess::Text, 0.0),
            };

            let guess = classification["type"].as_str().map(Self::parse_type).unwrap_or(ColumnTypeGuess::Text);
            let confidence = classification["confidence"].as_f64().unwrap_or(0.0) as f32;
            (guess, confidence.clamp(0.0, 1.0))
        }
    }
}

#[cfg(feature = "llm")]
pub use llm::HttpLlmResolver;
