//! End-to-end tests for `format_search_results` per-result rendering,
//! `SearchResult` serde round-trip, and `FetchResult` shape.
//!
//! Sprint 81 of the verification effort. Sprint 41
//! (`web_content_extraction_e2e`) covered `format_fetch_output`
//! and `safe_truncate`; sprint 9 (`web_ssrf_e2e`) covered SSRF
//! refusals; this file fills gaps in `format_search_results`
//! per-result rendering.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::web::{format_search_results, FetchResult, SearchResult};
#[cfg(feature = "browser")]
use openclaudia::web::{parse_bing_results_from_html, parse_duckduckgo_results_from_html};

// ───────────────────────────────────────────────────────────────────────────
// Section A — format_search_results
// ───────────────────────────────────────────────────────────────────────────

fn result(title: &str, url: &str, snippet: &str) -> SearchResult {
    SearchResult {
        title: title.to_string(),
        url: url.to_string(),
        snippet: snippet.to_string(),
    }
}

#[test]
fn format_empty_returns_no_results_message() {
    let formatted = format_search_results(&[]);
    assert!(formatted.contains("No results"));
}

#[test]
fn format_single_result_includes_title_url_snippet() {
    let results = vec![result("My Title", "https://example.com", "A snippet")];
    let formatted = format_search_results(&results);
    assert!(formatted.contains("My Title"));
    assert!(formatted.contains("https://example.com"));
    assert!(formatted.contains("A snippet"));
}

#[test]
fn format_single_result_includes_count_header() {
    let results = vec![result("X", "https://x", "x")];
    let formatted = format_search_results(&results);
    assert!(
        formatted.contains("Found 1 result"),
        "MUST include count header; got {formatted:?}"
    );
}

#[test]
fn format_multiple_results_numbered_starting_at_1() {
    let results = vec![
        result("First", "https://a.example.com", "a-snip"),
        result("Second", "https://b.example.com", "b-snip"),
        result("Third", "https://c.example.com", "c-snip"),
    ];
    let formatted = format_search_results(&results);
    assert!(formatted.contains("1."));
    assert!(formatted.contains("2."));
    assert!(formatted.contains("3."));
    assert!(formatted.contains("Found 3 result"));
    // Each title appears.
    assert!(formatted.contains("First"));
    assert!(formatted.contains("Second"));
    assert!(formatted.contains("Third"));
}

#[test]
fn format_renders_titles_in_bold_markdown() {
    let results = vec![result("Bold Title", "https://x", "snip")];
    let formatted = format_search_results(&results);
    // Documented format uses **title** markdown.
    assert!(
        formatted.contains("**Bold Title**"),
        "MUST render title in bold; got {formatted:?}"
    );
}

#[test]
fn format_includes_url_with_url_prefix() {
    let results = vec![result("X", "https://example.com/path", "snip")];
    let formatted = format_search_results(&results);
    assert!(formatted.contains("URL: https://example.com/path"));
}

#[test]
fn format_preserves_result_order_in_output() {
    let results = vec![
        result("AAA", "https://aaa", "a"),
        result("BBB", "https://bbb", "b"),
        result("CCC", "https://ccc", "c"),
    ];
    let formatted = format_search_results(&results);
    let aaa_pos = formatted.find("AAA").expect("AAA");
    let bbb_pos = formatted.find("BBB").expect("BBB");
    let ccc_pos = formatted.find("CCC").expect("CCC");
    assert!(aaa_pos < bbb_pos);
    assert!(bbb_pos < ccc_pos);
}

#[test]
fn format_handles_empty_snippet_without_panic() {
    let results = vec![result("Title-X", "https://x", "")];
    let formatted = format_search_results(&results);
    assert!(formatted.contains("Title-X"));
    assert!(formatted.contains("https://x"));
}

#[test]
fn format_handles_results_with_special_characters_in_fields() {
    let results = vec![result(
        "Title with <html> & \"quotes\"",
        "https://x/path?q=1&r=2",
        "Snippet with **markdown** and `code`",
    )];
    let formatted = format_search_results(&results);
    // Verbatim — format doesn't escape markdown.
    assert!(formatted.contains("Title with <html> & \"quotes\""));
    assert!(formatted.contains("https://x/path?q=1&r=2"));
    assert!(formatted.contains("Snippet with **markdown** and `code`"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — SearchResult serde
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn search_result_serde_round_trips() {
    let original = SearchResult {
        title: "T".to_string(),
        url: "https://x".to_string(),
        snippet: "S".to_string(),
    };
    let json = serde_json::to_string(&original).expect("ser");
    let back: SearchResult = serde_json::from_str(&json).expect("de");
    assert_eq!(back.title, original.title);
    assert_eq!(back.url, original.url);
    assert_eq!(back.snippet, original.snippet);
}

#[test]
fn search_result_serde_field_names_are_snake_case() {
    let r = SearchResult {
        title: "T".to_string(),
        url: "https://x".to_string(),
        snippet: "S".to_string(),
    };
    let json = serde_json::to_string(&r).expect("ser");
    assert!(json.contains("\"title\""));
    assert!(json.contains("\"url\""));
    assert!(json.contains("\"snippet\""));
}

#[test]
fn search_result_deserializes_from_external_json_shape() {
    let json = r#"{
        "title": "External Title",
        "url": "https://ext.example",
        "snippet": "External snippet"
    }"#;
    let r: SearchResult = serde_json::from_str(json).expect("de");
    assert_eq!(r.title, "External Title");
    assert_eq!(r.url, "https://ext.example");
    assert_eq!(r.snippet, "External snippet");
}

#[cfg(feature = "browser")]
#[test]
fn duckduckgo_parser_applies_limit_after_dropping_unsafe_results() {
    let html = r#"
        <html><body>
          <div class="result">
            <a class="result__a" href="http://127.0.0.1/private">Unsafe</a>
            <a class="result__snippet">blocked loopback</a>
          </div>
          <div class="result">
            <a class="result__a" href="https://1.1.1.1/one">First Safe</a>
            <a class="result__snippet">first safe result</a>
          </div>
          <div class="result">
            <a class="result__a" href="https://8.8.8.8/two">Second Safe</a>
            <a class="result__snippet">second safe result</a>
          </div>
        </body></html>
    "#;

    let results = parse_duckduckgo_results_from_html(html, 2)
        .expect("unsafe leading result should not starve valid DDG results");

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].title, "First Safe");
    assert_eq!(results[1].title, "Second Safe");
}

#[cfg(feature = "browser")]
#[test]
fn bing_parser_applies_limit_after_dropping_unsafe_results() {
    let html = r#"
        <html><body>
          <ol>
            <li class="b_algo">
              <h2><a href="http://127.0.0.1/private">Unsafe</a></h2>
              <p>blocked loopback</p>
            </li>
            <li class="b_algo">
              <h2><a href="https://1.1.1.1/one">First Safe</a></h2>
              <p>first safe result</p>
            </li>
            <li class="b_algo">
              <h2><a href="https://8.8.8.8/two">Second Safe</a></h2>
              <p>second safe result</p>
            </li>
          </ol>
        </body></html>
    "#;

    let results = parse_bing_results_from_html(html, 2)
        .expect("unsafe leading result should not starve valid Bing results");

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].title, "First Safe");
    assert_eq!(results[1].title, "Second Safe");
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — FetchResult shape
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn fetch_result_carries_all_three_fields() {
    let r = FetchResult {
        content: "body".to_string(),
        title: Some("Title".to_string()),
        url: "https://x".to_string(),
    };
    assert_eq!(r.content, "body");
    assert_eq!(r.title.as_deref(), Some("Title"));
    assert_eq!(r.url, "https://x");
}

#[test]
fn fetch_result_title_can_be_none() {
    let r = FetchResult {
        content: "body".to_string(),
        title: None,
        url: "https://x".to_string(),
    };
    assert!(r.title.is_none());
}

#[test]
fn fetch_result_clone_preserves_all_fields() {
    let original = FetchResult {
        content: "c".to_string(),
        title: Some("t".to_string()),
        url: "https://u".to_string(),
    };
    let cloned = original.clone();
    assert_eq!(cloned.content, original.content);
    assert_eq!(cloned.title, original.title);
    assert_eq!(cloned.url, original.url);
}
