//! URL parsing and normalisation for the AetherOS browser (P4-T18).
//!
//! The address bar accepts a URL or a free-text query. Everything unclear is
//! sent to the default search engine (DuckDuckGo HTML endpoint, no tracking
//! params). Kept dependency-light so the rules are unit-testable.

use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

const QUERY_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+');

/// What the address bar decided to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Navigation {
    /// A concrete URL to load.
    Url(String),
    /// A search query for the configured engine.
    Search(String),
}

impl Navigation {
    pub fn url(&self) -> Option<&str> {
        match self {
            Navigation::Url(url) => Some(url),
            Navigation::Search(_) => None,
        }
    }

    pub fn search(&self) -> Option<&str> {
        match self {
            Navigation::Url(_) => None,
            Navigation::Search(query) => Some(query),
        }
    }
}

/// Search endpoint used when the input is not a URL.
pub const SEARCH_ENDPOINT: &str = "https://duckduckgo.com/html/?q=";

/// Parse address-bar input.
pub fn parse_input(input: &str) -> Option<Navigation> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Explicit schemes we support. Anything else falls back to search so an
    // unknown scheme can never be handed to the engine.
    if let Some(rest) = trimmed.strip_prefix("about:") {
        return Some(Navigation::Url(format!("about:{}", rest.trim())));
    }
    if let Some(rest) = trimmed.strip_prefix("file://") {
        return Some(Navigation::Url(format!("file://{}", rest)));
    }
    if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
    {
        let (host, path) = split_host(rest);
        if host.is_empty() {
            return None;
        }
        let scheme = if trimmed.starts_with("https://") {
            "https"
        } else {
            "http"
        };
        return Some(Navigation::Url(format!(
            "{scheme}://{host}{}",
            normalise_path(path)
        )));
    }

    // A bare host such as "example.com/path" or "localhost:8080".
    if looks_like_host(trimmed) {
        let (host, path) = split_host(trimmed);
        let scheme = if is_local_host(host) { "http" } else { "https" };
        return Some(Navigation::Url(format!(
            "{scheme}://{host}{}",
            normalise_path(path)
        )));
    }

    Some(Navigation::Search(trimmed.to_string()))
}

/// Full URL for a navigation decision.
pub fn resolve(navigation: &Navigation) -> String {
    match navigation {
        Navigation::Url(url) => url.clone(),
        Navigation::Search(query) => format!(
            "{SEARCH_ENDPOINT}{}",
            utf8_percent_encode(query, QUERY_SET)
        ),
    }
}

fn split_host(input: &str) -> (&str, &str) {
    match input.find('/') {
        Some(index) => (&input[..index], &input[index..]),
        None => (input, ""),
    }
}

fn normalise_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

/// Host detection: a dot, a port, or a known single-label host.
fn looks_like_host(input: &str) -> bool {
    if input.contains(' ') {
        return false;
    }
    let (host, _) = split_host(input);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        return false;
    }
    if matches!(host, "localhost") {
        return true;
    }
    let mut labels = host.split('.');
    let (Some(tld), Some(_)) = (labels.next_back(), labels.next()) else {
        return false;
    };
    tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
}

fn is_local_host(host: &str) -> bool {
    let name = host.split(':').next().unwrap_or(host);
    name == "localhost" || name.starts_with("127.") || name == "::1"
}

/// Display form for the address bar: hide the scheme/path noise of the home
/// page and never show search URLs.
pub fn display_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_urls_round_trip_with_default_path() {
        assert_eq!(
            parse_input("https://example.com"),
            Some(Navigation::Url("https://example.com/".into()))
        );
        assert_eq!(
            parse_input("https://aether.example/docs?q=1"),
            Some(Navigation::Url("https://aether.example/docs?q=1".into()))
        );
    }

    #[test]
    fn bare_hosts_get_a_scheme() {
        assert_eq!(
            parse_input("example.com/page"),
            Some(Navigation::Url("https://example.com/page".into()))
        );
        assert_eq!(
            parse_input("localhost:8080"),
            Some(Navigation::Url("http://localhost:8080/".into()))
        );
    }

    #[test]
    fn plain_text_becomes_a_search() {
        assert_eq!(
            parse_input("rust slint layout"),
            Some(Navigation::Search("rust slint layout".into()))
        );
        assert_eq!(
            resolve(&Navigation::Search("a b&c".into())),
            "https://duckduckgo.com/html/?q=a%20b%26c"
        );
    }

    #[test]
    fn empty_and_unknown_schemes_are_rejected_or_searched() {
        assert_eq!(parse_input("   "), None);
        assert_eq!(
            parse_input("javascript:alert(1)"),
            Some(Navigation::Search("javascript:alert(1)".into()))
        );
    }

    #[test]
    fn about_and_file_urls_are_kept() {
        assert_eq!(
            parse_input("about:blank"),
            Some(Navigation::Url("about:blank".into()))
        );
        assert_eq!(
            parse_input("file:///home/anik/notes.md"),
            Some(Navigation::Url("file:///home/anik/notes.md".into()))
        );
    }

    #[test]
    fn display_form_hides_the_scheme() {
        assert_eq!(display_url("https://aether.example/x"), "aether.example/x");
        assert_eq!(display_url("http://localhost:8080/"), "localhost:8080/");
    }

    #[test]
    fn host_detection_is_conservative() {
        assert!(!looks_like_host("two words"));
        assert!(!looks_like_host("ratio 3:1"));
        assert!(looks_like_host("a.io"));
        assert!(!looks_like_host("file."));
    }
}
