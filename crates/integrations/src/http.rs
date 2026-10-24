//! The HTTP calls the adapters make, behind traits so tests can replay recorded responses.

use std::time::Duration;

use thiserror::Error;

/// Fetches a URL. The real one is [`UreqHttp`]; tests use a fixture server in memory.
pub trait HttpGet: Send + Sync {
    /// `GET url?query`, with `Authorization: Bearer <token>` if a token is given. Returns the body
    /// of a 2xx response.
    fn get(
        &self,
        url: &str,
        query: &[(&str, &str)],
        token: Option<&str>,
    ) -> Result<String, HttpError>;
}

/// Sends JSON. Separate from [`HttpGet`], so adapters that only read can't write.
pub trait HttpPost: Send + Sync {
    /// `POST url` with a JSON `body`, `Authorization: Bearer <token>` and `headers`. Returns the
    /// body of a 2xx response.
    fn post_json(
        &self,
        url: &str,
        body: &str,
        token: &str,
        headers: &[(&str, &str)],
    ) -> Result<String, HttpError>;
}

/// Why a fetch failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HttpError {
    /// No answer: offline, DNS, TLS or a timeout.
    #[error("{0}")]
    Network(String),
    /// The server answered with a non-2xx status.
    #[error("HTTP {0}")]
    Status(u16),
}

/// How long one request may take in total.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The largest response read; a calendar with years of history is a few MB.
const BODY_LIMIT: u64 = 20 * 1024 * 1024;

/// HTTPS with `ureq`, trusting the OS's certificate store (so antivirus or company proxies that
/// add their own root keep working).
#[derive(Debug, Clone)]
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl Default for UreqHttp {
    fn default() -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .user_agent(concat!("Focus/", env!("CARGO_PKG_VERSION")))
            .tls_config(tls)
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl HttpGet for UreqHttp {
    fn get(
        &self,
        url: &str,
        query: &[(&str, &str)],
        token: Option<&str>,
    ) -> Result<String, HttpError> {
        let mut request = self.agent.get(url);
        for (key, value) in query {
            request = request.query(*key, *value);
        }
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = request
            .call()
            .map_err(|error| HttpError::Network(error.to_string()))?;
        read_body(response)
    }
}

impl HttpPost for UreqHttp {
    fn post_json(
        &self,
        url: &str,
        body: &str,
        token: &str,
        headers: &[(&str, &str)],
    ) -> Result<String, HttpError> {
        let mut request = self
            .agent
            .post(url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request
            .send(body)
            .map_err(|error| HttpError::Network(error.to_string()))?;
        read_body(response)
    }
}

/// The body of a 2xx response, or why there isn't one.
fn read_body(mut response: ureq::http::Response<ureq::Body>) -> Result<String, HttpError> {
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(HttpError::Status(status));
    }
    response
        .body_mut()
        .with_config()
        .limit(BODY_LIMIT)
        .read_to_string()
        .map_err(|error| HttpError::Network(error.to_string()))
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::{HttpError, HttpGet, HttpPost};

    /// One POST as it was sent.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct Posted {
        pub(crate) url: String,
        pub(crate) body: serde_json::Value,
        pub(crate) headers: Vec<(String, String)>,
    }

    /// Serves recorded responses by URL and query, and remembers the tokens it was sent.
    #[derive(Debug, Default)]
    pub(crate) struct Recorded {
        responses: HashMap<String, Result<String, HttpError>>,
        tokens: Mutex<Vec<Option<String>>>,
        posted: Mutex<Vec<Posted>>,
    }

    impl Recorded {
        /// Answers `GET url?query` with `response`.
        pub(crate) fn with(
            mut self,
            url: &str,
            query: &[(&str, &str)],
            response: Result<&str, HttpError>,
        ) -> Self {
            self.responses
                .insert(key(url, query), response.map(str::to_owned));
            self
        }

        /// Answers `POST url` with `response`.
        pub(crate) fn with_post(mut self, url: &str, response: Result<&str, HttpError>) -> Self {
            self.responses
                .insert(format!("POST {url}"), response.map(str::to_owned));
            self
        }

        /// The tokens sent so far, one per request.
        pub(crate) fn tokens(&self) -> Vec<Option<String>> {
            self.tokens.lock().unwrap().clone()
        }

        /// The POSTs sent so far.
        pub(crate) fn posted(&self) -> Vec<Posted> {
            self.posted.lock().unwrap().clone()
        }
    }

    impl HttpPost for Recorded {
        fn post_json(
            &self,
            url: &str,
            body: &str,
            token: &str,
            headers: &[(&str, &str)],
        ) -> Result<String, HttpError> {
            self.tokens.lock().unwrap().push(Some(token.to_owned()));
            self.posted.lock().unwrap().push(Posted {
                url: url.to_owned(),
                body: serde_json::from_str(body).unwrap(),
                headers: headers
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                    .collect(),
            });
            self.responses
                .get(&format!("POST {url}"))
                .cloned()
                .unwrap_or_else(|| panic!("no recorded response for POST {url}"))
        }
    }

    impl HttpGet for Recorded {
        fn get(
            &self,
            url: &str,
            query: &[(&str, &str)],
            token: Option<&str>,
        ) -> Result<String, HttpError> {
            self.tokens.lock().unwrap().push(token.map(str::to_owned));
            self.responses
                .get(&key(url, query))
                .cloned()
                .unwrap_or_else(|| panic!("no recorded response for {}", key(url, query)))
        }
    }

    fn key(url: &str, query: &[(&str, &str)]) -> String {
        let query: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{url}?{}", query.join("&"))
    }
}
