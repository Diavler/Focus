//! URL patterns such as `reddit.com` or `reddit.com/r/rust/comments/*`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::url::{ParsedUrl, normalize_host, normalize_path, strip_scheme};

/// A host (which also covers its subdomains) plus an optional path glob, where `*` matches anything.
///
/// `reddit.com` matches every page on `reddit.com` and `old.reddit.com`; `reddit.com/` matches only
/// the front page; `reddit.com/r/rust/comments/*` matches every post in r/rust. Queries and fragments
/// are ignored, and hosts and paths compare case-insensitively.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UrlPattern {
    source: String,
    host: String,
    path: Option<String>,
}

/// Why a pattern string was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PatternError {
    /// The pattern was empty or only whitespace.
    #[error("pattern is empty")]
    Empty,
    /// The host part had characters other than letters, digits, `-` and `.`, or an empty label.
    #[error(
        "invalid host {host:?}: use letters, digits, '-' and '.' only, with no wildcards or port"
    )]
    InvalidHost {
        /// The host part as written.
        host: String,
    },
    /// The pattern had a `?` or `#`; patterns only match host and path.
    #[error("pattern {pattern:?} has a query or fragment, but patterns only match the path")]
    QueryNotSupported {
        /// The pattern as written.
        pattern: String,
    },
}

impl UrlPattern {
    /// Parses `host` or `host/path-glob`; a leading `scheme://` is allowed and ignored.
    pub fn parse(input: &str) -> Result<Self, PatternError> {
        let source = input.trim();
        if source.is_empty() {
            return Err(PatternError::Empty);
        }
        if source.contains(['?', '#']) {
            return Err(PatternError::QueryNotSupported {
                pattern: source.to_owned(),
            });
        }
        let rest = strip_scheme(source);
        let (raw_host, raw_path) = match rest.find('/') {
            Some(slash) => (&rest[..slash], Some(&rest[slash..])),
            None => (rest, None),
        };
        let host = normalize_host(raw_host)
            .filter(|host| is_valid_host(host))
            .ok_or_else(|| PatternError::InvalidHost {
                host: raw_host.to_owned(),
            })?;
        Ok(Self {
            source: source.to_owned(),
            host,
            path: raw_path.map(|path| normalize_path(&collapse_stars(path))),
        })
    }

    /// The pattern as it was written.
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Whether `url` falls under this pattern.
    pub(crate) fn matches(&self, url: &ParsedUrl) -> bool {
        self.matches_host(&url.host)
            && self
                .path
                .as_deref()
                .is_none_or(|glob| glob_match(glob, &url.path))
    }

    /// How specific the pattern is: more host labels first, then more literal path characters.
    pub(crate) fn specificity(&self) -> (usize, usize) {
        let labels = self.host.split('.').count();
        let literal = self
            .path
            .as_deref()
            .map_or(0, |path| path.bytes().filter(|&b| b != b'*').count());
        (labels, literal)
    }

    /// Exact host or any subdomain of it, never a different host that merely ends the same way.
    fn matches_host(&self, host: &str) -> bool {
        host.strip_suffix(self.host.as_str())
            .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('.'))
    }
}

impl fmt::Display for UrlPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl FromStr for UrlPattern {
    type Err = PatternError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl TryFrom<String> for UrlPattern {
    type Error = PatternError;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        Self::parse(&input)
    }
}

impl From<UrlPattern> for String {
    fn from(pattern: UrlPattern) -> Self {
        pattern.source
    }
}

/// Non-empty dot-separated labels of letters, digits and `-`.
fn is_valid_host(host: &str) -> bool {
    host.split('.')
        .all(|label| !label.is_empty() && label.chars().all(|c| c.is_alphanumeric() || c == '-'))
}

/// Turns runs of `*` into a single `*`; they mean the same and this keeps matching linear.
fn collapse_stars(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if !(c == '*' && out.ends_with('*')) {
            out.push(c);
        }
    }
    out
}

/// Glob match where `*` matches any run of characters (including `/`) and everything else is literal.
fn glob_match(glob: &str, text: &str) -> bool {
    let (glob, text) = (glob.as_bytes(), text.as_bytes());
    let (mut g, mut t) = (0, 0);
    // Position of the last `*` seen and the text position it is currently stretched to.
    let mut backtrack: Option<(usize, usize)> = None;
    while t < text.len() {
        if glob.get(g) == Some(&b'*') {
            backtrack = Some((g, t));
            g += 1;
        } else if glob.get(g) == Some(&text[t]) {
            g += 1;
            t += 1;
        } else if let Some((star, stretched)) = backtrack {
            backtrack = Some((star, stretched + 1));
            g = star + 1;
            t = stretched + 1;
        } else {
            return false;
        }
    }
    glob[g..].iter().all(|&b| b == b'*')
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn matches(pattern: &str, url: &str) -> bool {
        let url = ParsedUrl::parse(url).expect("test URL has a host");
        UrlPattern::parse(pattern)
            .expect("test pattern is valid")
            .matches(&url)
    }

    #[test]
    fn bare_host_matches_site_and_subdomains() {
        assert!(matches("reddit.com", "https://reddit.com/r/rust"));
        assert!(matches("reddit.com", "https://old.reddit.com/"));
        assert!(!matches("reddit.com", "https://notreddit.com/"));
        assert!(!matches("old.reddit.com", "https://reddit.com/"));
    }

    #[test]
    fn root_path_matches_only_the_front_page() {
        assert!(matches("reddit.com/", "https://www.reddit.com/?feed=home"));
        assert!(!matches("reddit.com/", "https://www.reddit.com/r/rust"));
    }

    #[test]
    fn star_matches_across_segments() {
        let pattern = "reddit.com/r/rust/comments/*";
        assert!(matches(pattern, "reddit.com/r/rust/comments/abc/title/"));
        assert!(!matches(pattern, "reddit.com/r/rust"));
        assert!(!matches(pattern, "reddit.com/r/rustjerk/comments/abc"));
        assert!(matches(
            "youtube.com/*/videos",
            "youtube.com/@someone/videos"
        ));
    }

    #[test]
    fn scheme_and_case_are_ignored() {
        assert!(matches(
            "HTTPS://YouTube.com/Shorts/*",
            "youtube.com/shorts/x"
        ));
        assert!(matches(
            "youtube.com/shorts/*",
            "https://m.youtube.com/SHORTS/x"
        ));
    }

    #[test]
    fn rejects_bad_patterns() {
        assert_eq!(UrlPattern::parse("  "), Err(PatternError::Empty));
        assert!(matches!(
            UrlPattern::parse("*.reddit.com"),
            Err(PatternError::InvalidHost { .. })
        ));
        assert!(matches!(
            UrlPattern::parse("localhost:3000"),
            Err(PatternError::InvalidHost { .. })
        ));
        assert!(matches!(
            UrlPattern::parse("a..b/x"),
            Err(PatternError::InvalidHost { .. })
        ));
        assert!(matches!(
            UrlPattern::parse("google.com/search?q=x"),
            Err(PatternError::QueryNotSupported { .. })
        ));
    }

    #[test]
    fn serde_uses_the_written_form() {
        let pattern = UrlPattern::parse("Reddit.com/r/*").unwrap();
        let json = serde_json::to_string(&pattern).unwrap();
        assert_eq!(json, r#""Reddit.com/r/*""#);
        assert_eq!(serde_json::from_str::<UrlPattern>(&json).unwrap(), pattern);
        assert!(serde_json::from_str::<UrlPattern>(r#""""#).is_err());
    }

    #[test]
    fn specificity_prefers_longer_hosts_then_longer_paths() {
        let spec = |p: &str| UrlPattern::parse(p).unwrap().specificity();
        assert!(spec("old.reddit.com") > spec("reddit.com/r/rust/*"));
        assert!(spec("reddit.com/r/rust/*") > spec("reddit.com/"));
        assert!(spec("reddit.com/") > spec("reddit.com"));
    }

    /// A host label: lowercase letters, digits and `-`.
    fn label() -> impl Strategy<Value = String> {
        "[a-z0-9-]{1,12}"
    }

    /// A path of 0..5 segments of URL-ish characters, without `*`, `?` or `#`.
    fn path() -> impl Strategy<Value = String> {
        prop::collection::vec("[a-zA-Z0-9._~%-]{1,10}", 0..5)
            .prop_map(|segments| format!("/{}", segments.join("/")))
    }

    proptest! {
        #[test]
        fn parsing_never_panics(input in "\\PC{0,40}") {
            let _ = UrlPattern::parse(&input);
            let _ = ParsedUrl::parse(&input);
        }

        #[test]
        fn bare_host_matches_every_path_and_subdomain(
            host in label(), tld in label(), sub in prop::option::of(label()), path in path(),
        ) {
            let site = format!("{host}.{tld}");
            let url = match &sub {
                Some(sub) => format!("https://{sub}.{site}{path}"),
                None => format!("https://{site}{path}"),
            };
            prop_assert!(matches(&site, &url));
        }

        #[test]
        fn never_matches_a_lookalike_host(host in label(), tld in label(), prefix in "[a-z0-9]{1,8}", path in path()) {
            let site = format!("{host}.{tld}");
            let lookalike = format!("https://{prefix}{site}{path}");
            prop_assert!(!matches(&site, &lookalike));
        }

        #[test]
        fn literal_path_matches_itself_and_star_matches_any_suffix(
            path in path(), suffix in "[a-zA-Z0-9/._-]{0,20}",
        ) {
            let literal = format!("example.com{path}");
            prop_assert!(matches(&literal, &literal));
            let prefixed = format!("example.com{path}/*");
            let longer = format!("example.com{path}/{suffix}x");
            prop_assert!(matches(&prefixed, &longer));
        }
    }
}
