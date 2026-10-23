//! Splits a URL into the host and path that rules match against.

/// The parts of a URL that rules look at: lower-cased host and path, without query or fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedUrl {
    /// Lower-cased host without port, user info or trailing dot.
    pub(crate) host: String,
    /// Lower-cased path starting with `/`, with any trailing `/` removed (except for the root).
    pub(crate) path: String,
}

impl ParsedUrl {
    /// Parses `https://host:port/path?query#fragment`; the scheme is optional. `None` if there is no host.
    pub(crate) fn parse(input: &str) -> Option<Self> {
        let rest = strip_scheme(input.trim());
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(authority_end);
        let host = normalize_host(host_of(authority))?;
        let path_end = tail.find(['?', '#']).unwrap_or(tail.len());
        Some(Self {
            host,
            path: normalize_path(&tail[..path_end]),
        })
    }
}

/// Drops a leading `scheme://`, if there is one before the first path character.
pub(crate) fn strip_scheme(input: &str) -> &str {
    match input.find("://") {
        Some(i) if !input[..i].contains(['/', '?', '#']) => &input[i + 3..],
        _ => input,
    }
}

/// The host part of `user@host:port`, keeping IPv6 brackets intact.
fn host_of(authority: &str) -> &str {
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if host_port.starts_with('[') {
        host_port
            .find(']')
            .map_or(host_port, |end| &host_port[..=end])
    } else {
        host_port
            .split_once(':')
            .map_or(host_port, |(host, _)| host)
    }
}

/// Lower-cases a host and drops a trailing dot; `None` if nothing is left.
pub(crate) fn normalize_host(host: &str) -> Option<String> {
    let host = host.strip_suffix('.').unwrap_or(host).to_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Lower-cases a path; empty paths become `/` and a trailing `/` is dropped from any other path.
///
/// Paths compare case-insensitively so `/Shorts/` can't slip past a rule for `/shorts/*`.
pub(crate) fn normalize_path(path: &str) -> String {
    if path.is_empty() || path == "/" {
        return "/".to_owned();
    }
    path.strip_suffix('/').unwrap_or(path).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(input: &str) -> Option<(String, String)> {
        ParsedUrl::parse(input).map(|url| (url.host, url.path))
    }

    fn some(host: &str, path: &str) -> Option<(String, String)> {
        Some((host.to_owned(), path.to_owned()))
    }

    #[test]
    fn splits_full_url() {
        assert_eq!(
            parts("https://user:pw@WWW.Reddit.com.:443/r/Rust/?sort=new#top"),
            some("www.reddit.com", "/r/rust")
        );
    }

    #[test]
    fn scheme_is_optional_and_root_is_slash() {
        assert_eq!(parts("reddit.com"), some("reddit.com", "/"));
        assert_eq!(parts("reddit.com?feed=home"), some("reddit.com", "/"));
        assert_eq!(parts("http://reddit.com/"), some("reddit.com", "/"));
    }

    #[test]
    fn scheme_inside_query_is_not_a_scheme() {
        assert_eq!(
            parts("reddit.com/out?u=https://x.com"),
            some("reddit.com", "/out")
        );
    }

    #[test]
    fn keeps_ipv6_host() {
        assert_eq!(parts("http://[::1]:8080/a"), some("[::1]", "/a"));
    }

    #[test]
    fn rejects_missing_host() {
        assert_eq!(parts(""), None);
        assert_eq!(parts("https:///path"), None);
        assert_eq!(parts("/just/a/path"), None);
    }
}
