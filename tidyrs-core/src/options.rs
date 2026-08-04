use std::collections::HashMap;

/// Generic string-keyed option bag passed from the CLI (or any embedder)
/// down into a specific parser. Keeping this untyped at the core level is
/// what lets each format crate own its own configuration surface without
/// the core crate having to know about every flag that will ever exist.
#[derive(Debug, Clone, Default)]
pub struct ParseOptions(HashMap<String, String>);

impl ParseOptions {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn set(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.insert(key.into(), value.into());
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        match self.0.get(key).map(|s| s.as_str()) {
            Some("true") | Some("1") | Some("yes") => true,
            Some("false") | Some("0") | Some("no") => false,
            _ => default,
        }
    }

    pub fn get_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.get(key).unwrap_or(default)
    }
}

impl From<HashMap<String, String>> for ParseOptions {
    fn from(map: HashMap<String, String>) -> Self {
        Self(map)
    }
}
