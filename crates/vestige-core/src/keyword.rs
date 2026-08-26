//! Keyword Search (BM25/FTS5)
//!
//! Provides keyword-based search using SQLite FTS5.
//! Includes query sanitization for security.
//!
//! This module is deliberately **not** gated behind the `vector-search` feature:
//! FTS5 queries are issued by [`crate::storage`] on every build, so the sanitizer
//! must be available even when HNSW vector search is compiled out. It is
//! re-exported from [`crate::search`] when that feature is enabled so the
//! `vestige_core::search::sanitize_fts5_query` path keeps working.

// ============================================================================
// FTS5 QUERY SANITIZATION
// ============================================================================

/// Dangerous FTS5 operators that could be used for injection or DoS
const FTS5_OPERATORS: &[&str] = &["OR", "AND", "NOT", "NEAR"];

/// Sanitize input for FTS5 MATCH queries
///
/// Prevents:
/// - Boolean operator injection (OR, AND, NOT, NEAR)
/// - Column targeting attacks (content:secret)
/// - Prefix/suffix wildcards for data extraction
/// - DoS via complex query patterns
pub fn sanitize_fts5_query(query: &str) -> String {
    // Limit query length to prevent DoS (char-aware to avoid UTF-8 boundary issues)
    let limited: String = query.chars().take(1000).collect();

    // Remove FTS5 special characters and operators
    let mut sanitized = limited.to_string();

    // Remove special characters: * : ^ - " ( )
    sanitized = sanitized
        .chars()
        .map(|c| match c {
            '*' | ':' | '^' | '-' | '"' | '(' | ')' | '{' | '}' | '[' | ']' => ' ',
            _ => c,
        })
        .collect();

    // Remove FTS5 boolean operators (case-insensitive)
    for op in FTS5_OPERATORS {
        // Use word boundary replacement to avoid partial matches
        let pattern = format!(" {} ", op);
        sanitized = sanitized.replace(&pattern, " ");
        sanitized = sanitized.replace(&pattern.to_lowercase(), " ");

        // Handle operators at start/end (using char-aware operations)
        let upper = sanitized.to_uppercase();
        let start_pattern = format!("{} ", op);
        if upper.starts_with(&start_pattern) {
            sanitized = sanitized.chars().skip(op.len()).collect();
        }
        let end_pattern = format!(" {}", op);
        if upper.ends_with(&end_pattern) {
            let char_count = sanitized.chars().count();
            sanitized = sanitized.chars().take(char_count.saturating_sub(op.len())).collect();
        }
    }

    // Collapse multiple spaces and trim
    let sanitized = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");

    // If empty after sanitization, return a safe default
    if sanitized.is_empty() {
        return "\"\"".to_string(); // Empty phrase - matches nothing safely
    }

    // Wrap each remaining token as its own phrase so multi-word queries become an
    // implicit AND of single-term matches rather than one consecutive phrase. FTS5
    // then scores each term independently via BM25; the old single-phrase form
    // required every word to appear adjacently, so ordinary multi-word searches
    // returned nothing (openclaw-vestige-6ct).
    let tokens: Vec<String> = sanitized
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t))
        .collect();

    if tokens.is_empty() {
        return "\"\"".to_string();
    }

    tokens.join(" ")
}

// ============================================================================
// KEYWORD SEARCHER
// ============================================================================

/// Keyword search configuration
#[derive(Debug, Clone)]
pub struct KeywordSearchConfig {
    /// Maximum query length
    pub max_query_length: usize,
    /// Enable stemming
    pub enable_stemming: bool,
    /// Boost factor for title matches
    pub title_boost: f32,
    /// Boost factor for tag matches
    pub tag_boost: f32,
}

impl Default for KeywordSearchConfig {
    fn default() -> Self {
        Self {
            max_query_length: 1000,
            enable_stemming: true,
            title_boost: 2.0,
            tag_boost: 1.5,
        }
    }
}

/// Keyword searcher for FTS5 queries
pub struct KeywordSearcher {
    #[allow(dead_code)] // Config will be used when FTS5 stemming/boosting is implemented
    config: KeywordSearchConfig,
}

impl Default for KeywordSearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl KeywordSearcher {
    /// Create a new keyword searcher
    pub fn new() -> Self {
        Self {
            config: KeywordSearchConfig::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(config: KeywordSearchConfig) -> Self {
        Self { config }
    }

    /// Prepare a query for FTS5
    pub fn prepare_query(&self, query: &str) -> String {
        sanitize_fts5_query(query)
    }

    /// Tokenize a query into terms
    pub fn tokenize(&self, query: &str) -> Vec<String> {
        query
            .split_whitespace()
            .map(|s| s.to_lowercase())
            .filter(|s| s.len() >= 2) // Skip very short terms
            .collect()
    }

    /// Build a proximity query (terms must appear near each other)
    pub fn proximity_query(&self, terms: &[&str], distance: usize) -> String {
        let cleaned: Vec<String> = terms
            .iter()
            .map(|t| t.replace(|c: char| !c.is_alphanumeric(), ""))
            .filter(|t| !t.is_empty())
            .collect();

        if cleaned.is_empty() {
            return "\"\"".to_string();
        }

        if cleaned.len() == 1 {
            return format!("\"{}\"", cleaned[0]);
        }

        // FTS5 NEAR query: NEAR(term1 term2, distance)
        format!("NEAR({}, {})", cleaned.join(" "), distance)
    }

    /// Build a prefix query (for autocomplete)
    pub fn prefix_query(&self, prefix: &str) -> String {
        let cleaned = prefix.replace(|c: char| !c.is_alphanumeric(), "");
        if cleaned.is_empty() {
            return "\"\"".to_string();
        }
        format!("\"{}\"*", cleaned)
    }

    /// Highlight matched terms in text
    pub fn highlight(&self, text: &str, terms: &[String]) -> String {
        let mut result = text.to_string();

        for term in terms {
            // Case-insensitive replacement with highlighting
            let lower_text = result.to_lowercase();
            let lower_term = term.to_lowercase();

            if let Some(byte_pos) = lower_text.find(&lower_term) {
                // Convert byte position to char position for safe slicing
                let char_pos = lower_text[..byte_pos].chars().count();
                let term_char_len = lower_term.chars().count();

                // Extract matched portion using char indices
                let prefix: String = result.chars().take(char_pos).collect();
                let matched: String = result.chars().skip(char_pos).take(term_char_len).collect();
                let suffix: String = result.chars().skip(char_pos + term_char_len).collect();

                let highlighted = format!("**{}**", matched);
                result = format!("{}{}{}", prefix, highlighted, suffix);
            }
        }

        result
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_fts5_query_basic() {
        // Each token is quoted individually, giving FTS5 an implicit AND of terms
        // rather than one consecutive phrase.
        assert_eq!(sanitize_fts5_query("hello world"), "\"hello\" \"world\"");
        assert_eq!(sanitize_fts5_query("hello"), "\"hello\"");
    }

    #[test]
    fn test_sanitize_fts5_query_operators() {
        // Boolean operators are stripped; the remaining tokens stay individually quoted.
        assert_eq!(sanitize_fts5_query("hello OR world"), "\"hello\" \"world\"");
        assert_eq!(sanitize_fts5_query("hello AND world"), "\"hello\" \"world\"");
        assert_eq!(sanitize_fts5_query("NOT hello"), "\"hello\"");
    }

    #[test]
    fn test_sanitize_fts5_query_special_chars() {
        // Special characters are stripped; the remaining tokens stay individually quoted.
        assert_eq!(sanitize_fts5_query("hello* world"), "\"hello\" \"world\"");
        assert_eq!(
            sanitize_fts5_query("content:secret"),
            "\"content\" \"secret\""
        );
        assert_eq!(sanitize_fts5_query("^boost"), "\"boost\"");
    }

    #[test]
    fn test_sanitize_fts5_query_multi_word_is_implicit_and() {
        // The bug this guards (openclaw-vestige-6ct): a multi-word query must NOT
        // collapse into a single quoted phrase, which FTS5 only matches when the
        // words appear consecutively — so an ordinary search returned nothing.
        let result = sanitize_fts5_query("tell me about Reccy");
        assert_eq!(result, "\"tell\" \"me\" \"about\" \"Reccy\"");
        assert_ne!(result, "\"tell me about Reccy\"");
    }

    #[test]
    fn test_sanitize_fts5_query_empty() {
        assert_eq!(sanitize_fts5_query(""), "\"\"");
        assert_eq!(sanitize_fts5_query("   "), "\"\"");
        assert_eq!(sanitize_fts5_query("* : ^"), "\"\"");
    }

    #[test]
    fn test_sanitize_fts5_query_length_limit() {
        let long_query = "a".repeat(2000);
        let sanitized = sanitize_fts5_query(&long_query);
        assert!(sanitized.len() <= 1004);
    }

    #[test]
    fn test_tokenize() {
        let searcher = KeywordSearcher::new();
        let terms = searcher.tokenize("Hello World Test");

        assert_eq!(terms, vec!["hello", "world", "test"]);
    }

    #[test]
    fn test_tokenize_filters_short() {
        let searcher = KeywordSearcher::new();
        let terms = searcher.tokenize("a is the test");

        assert_eq!(terms, vec!["is", "the", "test"]);
    }

    #[test]
    fn test_prefix_query() {
        let searcher = KeywordSearcher::new();

        assert_eq!(searcher.prefix_query("hel"), "\"hel\"*");
        assert_eq!(searcher.prefix_query(""), "\"\"");
    }

    #[test]
    fn test_highlight() {
        let searcher = KeywordSearcher::new();
        let terms = vec!["hello".to_string()];

        let highlighted = searcher.highlight("Hello world", &terms);
        assert!(highlighted.contains("**Hello**"));
    }
}
