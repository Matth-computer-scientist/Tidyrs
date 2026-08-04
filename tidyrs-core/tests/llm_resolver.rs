#![cfg(feature = "llm")]

use tidyrs_core::{AmbiguityResolver, ColumnTypeGuess, HttpLlmResolver};

#[test]
fn parses_a_well_formed_classification_response() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"choices":[{"message":{"content":"{\"type\": \"date\", \"confidence\": 0.87}"}}]}"#,
        )
        .create();

    let resolver = HttpLlmResolver::new(server.url(), "test-key", "gpt-4o-mini");
    let samples = vec!["2026-01-03".to_string(), "2026-02-14".to_string()];
    let (guess, confidence) = resolver.resolve_column_type("event_date", &samples);

    mock.assert();
    assert_eq!(guess, ColumnTypeGuess::Date);
    assert!((confidence - 0.87).abs() < 0.001);
}

#[test]
fn falls_back_to_low_confidence_text_on_http_error() {
    let mut server = mockito::Server::new();
    let _mock = server.mock("POST", "/chat/completions").with_status(500).create();

    let resolver = HttpLlmResolver::new(server.url(), "test-key", "gpt-4o-mini");
    let (guess, confidence) = resolver.resolve_column_type("mystery", &["???".to_string()]);

    assert_eq!(guess, ColumnTypeGuess::Text);
    assert_eq!(confidence, 0.0);
}

#[test]
fn falls_back_to_low_confidence_text_on_malformed_json_content() {
    let mut server = mockito::Server::new();
    let _mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"choices":[{"message":{"content":"not json at all"}}]}"#)
        .create();

    let resolver = HttpLlmResolver::new(server.url(), "test-key", "gpt-4o-mini");
    let (guess, confidence) = resolver.resolve_column_type("mystery", &["x".to_string()]);

    assert_eq!(guess, ColumnTypeGuess::Text);
    assert_eq!(confidence, 0.0);
}

#[test]
fn from_env_requires_api_key() {
    std::env::remove_var("TIDYLOOM_LLM_API_KEY");
    assert!(HttpLlmResolver::from_env().is_err());

    std::env::set_var("TIDYLOOM_LLM_API_KEY", "sk-test");
    let resolver = HttpLlmResolver::from_env().unwrap();
    assert_eq!(resolver.model, "gpt-4o-mini");
    assert_eq!(resolver.base_url, "https://api.openai.com/v1");
    std::env::remove_var("TIDYLOOM_LLM_API_KEY");
}
