//! Runtime configuration for the L4 proxy.
//!
//! All configuration is sourced from environment variables. The proxy fails
//! fast on startup if any required value (currently only the upstream URL) is
//! missing or malformed — there are no defaults that would silently point the
//! proxy at an unintended endpoint.

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use thiserror::Error;
use url::Url;

/// Environment variable controlling the bind address of the proxy server.
pub const ENV_LISTEN: &str = "REPLAYABLE_LISTEN";

/// Environment variable holding the required upstream LLM API base URL.
pub const ENV_UPSTREAM_URL: &str = "REPLAYABLE_UPSTREAM_URL";

/// Environment variable holding the JSONL trace output path.
pub const ENV_LOG_PATH: &str = "REPLAYABLE_LOG_PATH";

/// Environment variable controlling the bounded trace channel capacity.
pub const ENV_LOG_CHANNEL_CAPACITY: &str = "REPLAYABLE_LOG_CHANNEL_CAPACITY";

/// Environment variable toggling capture of request/response **content**.
///
/// When unset or `false` (the secure default), trace records carry metadata
/// only (provider, model, status, tokens, latency, scrubbed headers). When
/// `true`, raw request and response bodies are written verbatim and a
/// startup warning is logged. See security review C1.
pub const ENV_CAPTURE_CONTENT: &str = "REPLAYABLE_CAPTURE_CONTENT";

/// Environment variable for the maximum accepted request body size in bytes.
///
/// Requests whose body exceeds this cap are rejected with HTTP 413 before
/// the upstream is dialled and before a trace is emitted. See security
/// review H1.
pub const ENV_MAX_REQUEST_BYTES: &str = "REPLAYABLE_MAX_REQUEST_BYTES";

/// Environment variable for the upstream TCP connect timeout, in seconds.
pub const ENV_CONNECT_TIMEOUT_SECS: &str = "REPLAYABLE_CONNECT_TIMEOUT_SECS";

/// Environment variable for the upstream socket read timeout, in seconds.
///
/// Each read resets this timer, so streaming responses with regular chunks
/// are unaffected; the timer only fires on prolonged silence from the
/// upstream.
pub const ENV_READ_TIMEOUT_SECS: &str = "REPLAYABLE_READ_TIMEOUT_SECS";

/// Environment variable allowing plaintext `http://` upstream URLs that are
/// not loopback. Defaults to `false` to push operators toward TLS.
pub const ENV_UPSTREAM_ALLOW_PLAINTEXT: &str = "REPLAYABLE_UPSTREAM_ALLOW_PLAINTEXT";

/// Default bind address when [`ENV_LISTEN`] is unset.
///
/// Bound to loopback by default per security review H4: the proxy is a
/// local sidecar and must not be reachable from other hosts without an
/// explicit opt-in via [`ENV_LISTEN`].
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8080";

/// Default JSONL log path when [`ENV_LOG_PATH`] is unset.
pub const DEFAULT_LOG_PATH: &str = "./replayable-traces.jsonl";

/// Default bounded channel capacity for the trace writer.
pub const DEFAULT_LOG_CHANNEL_CAPACITY: usize = 1024;

/// Default value for [`Config::capture_content`].
pub const DEFAULT_CAPTURE_CONTENT: bool = false;

/// Default cap on accepted request body bytes (10 MiB).
pub const DEFAULT_MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;

/// Default upstream TCP connect timeout (10 s).
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Default upstream read timeout (10 min — accommodates long-running LLM
/// streams while still capping dead sockets).
pub const DEFAULT_READ_TIMEOUT_SECS: u64 = 600;

/// Hosts that must never appear as an upstream URL (cloud metadata
/// services). See security review H3.
const BANNED_UPSTREAM_HOSTS: &[&str] = &[
    "169.254.169.254",
    "metadata.google.internal",
    "metadata.azure.com",
    "metadata.azure.internal",
];

/// Errors produced while parsing configuration from the environment.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// A required environment variable was unset.
    #[error("required environment variable {0} is not set")]
    Missing(&'static str),

    /// A value could not be parsed into the expected type.
    #[error("invalid value for {name}: {reason}")]
    Invalid {
        /// The offending environment variable name.
        name: &'static str,
        /// Human-readable reason for the parse failure.
        reason: String,
    },
}

/// Fully validated proxy configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Socket address the HTTP server binds to.
    pub listen: SocketAddr,
    /// Upstream LLM API base URL. Requests are forwarded relative to this.
    pub upstream_url: String,
    /// Filesystem path where JSONL trace records are written.
    pub log_path: PathBuf,
    /// Bounded channel capacity for the async trace writer.
    pub log_channel_capacity: usize,
    /// When `true`, request/response **bodies** are written verbatim to the
    /// trace log. When `false` (the secure default), only metadata is
    /// captured. See security review C1.
    pub capture_content: bool,
    /// Upper bound on accepted request body size in bytes. Requests that
    /// exceed this cap receive HTTP 413 with no upstream call and no
    /// trace. See security review H1.
    pub max_request_bytes: usize,
    /// TCP connect timeout used by the reqwest client. See security review H2.
    pub connect_timeout: Duration,
    /// Per-read socket timeout used by the reqwest client. See security review H2.
    pub read_timeout: Duration,
}

impl Default for Config {
    /// Returns the proxy defaults. Note that [`Self::upstream_url`] is the
    /// empty string here — the real entry point [`Self::from_env`] requires
    /// the operator to supply one and fails fast otherwise.
    fn default() -> Self {
        Self {
            listen: SocketAddr::from_str(DEFAULT_LISTEN).unwrap_or_else(|_| {
                // DEFAULT_LISTEN is a compile-time literal; this branch is
                // structurally unreachable but we avoid panicking anyway.
                SocketAddr::from(([127, 0, 0, 1], 8080))
            }),
            upstream_url: String::new(),
            log_path: PathBuf::from(DEFAULT_LOG_PATH),
            log_channel_capacity: DEFAULT_LOG_CHANNEL_CAPACITY,
            capture_content: DEFAULT_CAPTURE_CONTENT,
            max_request_bytes: DEFAULT_MAX_REQUEST_BYTES,
            connect_timeout: Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS),
            read_timeout: Duration::from_secs(DEFAULT_READ_TIMEOUT_SECS),
        }
    }
}

impl Config {
    /// Loads configuration from the process environment.
    ///
    /// # Errors
    /// Returns [`ConfigError::Missing`] when a required variable is absent and
    /// [`ConfigError::Invalid`] when a value fails to parse.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| env::var(name).ok())
    }

    /// Builds a [`Config`] from an arbitrary lookup function. Tests use this
    /// form to avoid mutating the process environment.
    ///
    /// # Errors
    /// Same as [`Config::from_env`].
    pub fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let listen_str = lookup(ENV_LISTEN).unwrap_or_else(|| DEFAULT_LISTEN.to_string());
        let listen = SocketAddr::from_str(&listen_str).map_err(|e| ConfigError::Invalid {
            name: ENV_LISTEN,
            reason: e.to_string(),
        })?;

        let upstream_url_raw = lookup(ENV_UPSTREAM_URL)
            .ok_or(ConfigError::Missing(ENV_UPSTREAM_URL))?
            .trim()
            .to_string();
        if upstream_url_raw.is_empty() {
            return Err(ConfigError::Invalid {
                name: ENV_UPSTREAM_URL,
                reason: "value is empty".to_string(),
            });
        }
        let allow_plaintext = parse_bool_env(&lookup, ENV_UPSTREAM_ALLOW_PLAINTEXT, false)?;
        validate_upstream_url(&upstream_url_raw, allow_plaintext)?;

        let log_path =
            lookup(ENV_LOG_PATH).map_or_else(|| PathBuf::from(DEFAULT_LOG_PATH), PathBuf::from);

        let log_channel_capacity = match lookup(ENV_LOG_CHANNEL_CAPACITY) {
            Some(raw) => raw.parse::<usize>().map_err(|e| ConfigError::Invalid {
                name: ENV_LOG_CHANNEL_CAPACITY,
                reason: e.to_string(),
            })?,
            None => DEFAULT_LOG_CHANNEL_CAPACITY,
        };
        if log_channel_capacity == 0 {
            return Err(ConfigError::Invalid {
                name: ENV_LOG_CHANNEL_CAPACITY,
                reason: "must be greater than zero".to_string(),
            });
        }

        let capture_content =
            parse_bool_env(&lookup, ENV_CAPTURE_CONTENT, DEFAULT_CAPTURE_CONTENT)?;

        let max_request_bytes = match lookup(ENV_MAX_REQUEST_BYTES) {
            Some(raw) => raw.parse::<usize>().map_err(|e| ConfigError::Invalid {
                name: ENV_MAX_REQUEST_BYTES,
                reason: e.to_string(),
            })?,
            None => DEFAULT_MAX_REQUEST_BYTES,
        };
        if max_request_bytes == 0 {
            return Err(ConfigError::Invalid {
                name: ENV_MAX_REQUEST_BYTES,
                reason: "must be greater than zero".to_string(),
            });
        }

        let connect_timeout = parse_duration_secs(
            &lookup,
            ENV_CONNECT_TIMEOUT_SECS,
            DEFAULT_CONNECT_TIMEOUT_SECS,
        )?;
        let read_timeout =
            parse_duration_secs(&lookup, ENV_READ_TIMEOUT_SECS, DEFAULT_READ_TIMEOUT_SECS)?;

        Ok(Self {
            listen,
            upstream_url: trim_trailing_slash(&upstream_url_raw),
            log_path,
            log_channel_capacity,
            capture_content,
            max_request_bytes,
            connect_timeout,
            read_timeout,
        })
    }
}

/// Validate the upstream URL against the SSRF deny-list and the
/// plaintext-only-loopback policy (security review H3).
///
/// # Errors
/// Returns [`ConfigError::Invalid`] when the URL is malformed, uses a
/// disallowed scheme, points at a cloud-metadata host, or uses plaintext
/// `http://` against a non-loopback target without
/// [`ENV_UPSTREAM_ALLOW_PLAINTEXT`] set.
pub fn validate_upstream_url(url: &str, allow_plaintext: bool) -> Result<(), ConfigError> {
    let parsed = Url::parse(url).map_err(|e| ConfigError::Invalid {
        name: ENV_UPSTREAM_URL,
        reason: format!("could not parse as URL: {e}"),
    })?;

    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(ConfigError::Invalid {
            name: ENV_UPSTREAM_URL,
            reason: format!(
                "scheme `{scheme}` is not allowed; use https:// or http:// (loopback only)"
            ),
        });
    }

    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    if host.is_empty() {
        return Err(ConfigError::Invalid {
            name: ENV_UPSTREAM_URL,
            reason: "host is empty".to_string(),
        });
    }

    if BANNED_UPSTREAM_HOSTS.iter().any(|b| host == *b) {
        return Err(ConfigError::Invalid {
            name: ENV_UPSTREAM_URL,
            reason: format!("host `{host}` is on the cloud-metadata deny-list"),
        });
    }

    if scheme == "http" {
        let is_loopback = is_loopback_host(&host);
        if !is_loopback && !allow_plaintext {
            return Err(ConfigError::Invalid {
                name: ENV_UPSTREAM_URL,
                reason: format!(
                    "plaintext http:// upstream is only allowed for loopback hosts; set {ENV_UPSTREAM_ALLOW_PLAINTEXT}=true to override (got host `{host}`)",
                ),
            });
        }
    }

    Ok(())
}

/// Returns `true` when `host` is one of the loopback hostnames or IPs.
fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1") || host.starts_with("127.")
}

fn trim_trailing_slash(url: &str) -> String {
    url.strip_suffix('/').unwrap_or(url).to_string()
}

fn parse_bool_env<F>(lookup: &F, name: &'static str, default: bool) -> Result<bool, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    match lookup(name) {
        Some(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" | "" => Ok(false),
            other => Err(ConfigError::Invalid {
                name,
                reason: format!("expected a boolean (true/false/1/0/yes/no), got `{other}`"),
            }),
        },
        None => Ok(default),
    }
}

fn parse_duration_secs<F>(
    lookup: &F,
    name: &'static str,
    default_secs: u64,
) -> Result<Duration, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    let secs = match lookup(name) {
        Some(raw) => raw.parse::<u64>().map_err(|e| ConfigError::Invalid {
            name,
            reason: e.to_string(),
        })?,
        None => default_secs,
    };
    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup<'a>(map: &'a HashMap<&'a str, &'a str>) -> impl Fn(&str) -> Option<String> + 'a {
        move |name: &str| map.get(name).map(|v| (*v).to_string())
    }

    #[test]
    fn fails_when_upstream_missing() {
        let env = HashMap::new();
        let err = Config::from_lookup(lookup(&env)).expect_err("missing upstream must error");
        assert!(matches!(err, ConfigError::Missing(ENV_UPSTREAM_URL)));
    }

    #[test]
    fn parses_defaults() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "https://api.openai.com/");
        let cfg = Config::from_lookup(lookup(&env)).expect("config should parse");
        assert_eq!(cfg.listen.to_string(), DEFAULT_LISTEN);
        assert_eq!(cfg.upstream_url, "https://api.openai.com");
        assert_eq!(cfg.log_path, PathBuf::from(DEFAULT_LOG_PATH));
        assert_eq!(cfg.log_channel_capacity, DEFAULT_LOG_CHANNEL_CAPACITY);
        assert!(!cfg.capture_content);
        assert_eq!(cfg.max_request_bytes, DEFAULT_MAX_REQUEST_BYTES);
        assert_eq!(
            cfg.connect_timeout,
            Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS)
        );
        assert_eq!(
            cfg.read_timeout,
            Duration::from_secs(DEFAULT_READ_TIMEOUT_SECS)
        );
    }

    #[test]
    fn rejects_non_http_upstream() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "file:///etc/passwd");
        let err = Config::from_lookup(lookup(&env)).expect_err("non-http upstream must error");
        assert!(matches!(err, ConfigError::Invalid { name, .. } if name == ENV_UPSTREAM_URL));
    }

    #[test]
    fn rejects_zero_capacity() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "http://localhost:9999");
        env.insert(ENV_LOG_CHANNEL_CAPACITY, "0");
        let err = Config::from_lookup(lookup(&env)).expect_err("zero capacity must error");
        assert!(
            matches!(err, ConfigError::Invalid { name, .. } if name == ENV_LOG_CHANNEL_CAPACITY)
        );
    }

    #[test]
    fn parses_custom_listen() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "http://localhost:9999");
        env.insert(ENV_LISTEN, "127.0.0.1:12345");
        let cfg = Config::from_lookup(lookup(&env)).expect("config should parse");
        assert_eq!(cfg.listen.to_string(), "127.0.0.1:12345");
    }

    #[test]
    fn default_listen_is_loopback() {
        let cfg = Config::default();
        assert!(
            cfg.listen.ip().is_loopback(),
            "Config::default().listen must be loopback; got {}",
            cfg.listen,
        );
    }

    #[test]
    fn rejects_imds_upstream() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "http://169.254.169.254/");
        let err = Config::from_lookup(lookup(&env)).expect_err("imds must be rejected");
        assert!(matches!(err, ConfigError::Invalid { name, .. } if name == ENV_UPSTREAM_URL));
    }

    #[test]
    fn rejects_gcp_metadata_upstream() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "http://metadata.google.internal/");
        let err = Config::from_lookup(lookup(&env)).expect_err("gcp metadata must be rejected");
        assert!(matches!(err, ConfigError::Invalid { name, .. } if name == ENV_UPSTREAM_URL));
    }

    #[test]
    fn rejects_plaintext_non_loopback_upstream() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "http://api.openai.com/");
        let err = Config::from_lookup(lookup(&env)).expect_err("plaintext non-loopback must fail");
        assert!(matches!(err, ConfigError::Invalid { name, .. } if name == ENV_UPSTREAM_URL));
    }

    #[test]
    fn allows_plaintext_loopback_upstream() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "http://127.0.0.1:11434");
        let cfg = Config::from_lookup(lookup(&env)).expect("loopback plaintext must pass");
        assert_eq!(cfg.upstream_url, "http://127.0.0.1:11434");
    }

    #[test]
    fn allows_plaintext_with_override() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "http://api.openai.com/");
        env.insert(ENV_UPSTREAM_ALLOW_PLAINTEXT, "true");
        let cfg =
            Config::from_lookup(lookup(&env)).expect("plaintext with explicit override must pass");
        assert_eq!(cfg.upstream_url, "http://api.openai.com");
    }

    #[test]
    fn accepts_https_upstream() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "https://api.openai.com/");
        let cfg = Config::from_lookup(lookup(&env)).expect("https must always pass");
        assert_eq!(cfg.upstream_url, "https://api.openai.com");
    }

    #[test]
    fn capture_content_opt_in() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "https://api.openai.com");
        env.insert(ENV_CAPTURE_CONTENT, "true");
        let cfg = Config::from_lookup(lookup(&env)).expect("config should parse");
        assert!(cfg.capture_content);
    }

    #[test]
    fn rejects_invalid_capture_content() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "https://api.openai.com");
        env.insert(ENV_CAPTURE_CONTENT, "maybe");
        let err = Config::from_lookup(lookup(&env)).expect_err("invalid bool must error");
        assert!(matches!(err, ConfigError::Invalid { name, .. } if name == ENV_CAPTURE_CONTENT));
    }

    #[test]
    fn parses_max_request_bytes() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "https://api.openai.com");
        env.insert(ENV_MAX_REQUEST_BYTES, "2048");
        let cfg = Config::from_lookup(lookup(&env)).expect("config should parse");
        assert_eq!(cfg.max_request_bytes, 2048);
    }

    #[test]
    fn rejects_zero_max_request_bytes() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "https://api.openai.com");
        env.insert(ENV_MAX_REQUEST_BYTES, "0");
        let err = Config::from_lookup(lookup(&env)).expect_err("zero cap must error");
        assert!(matches!(err, ConfigError::Invalid { name, .. } if name == ENV_MAX_REQUEST_BYTES));
    }

    #[test]
    fn parses_timeouts() {
        let mut env = HashMap::new();
        env.insert(ENV_UPSTREAM_URL, "https://api.openai.com");
        env.insert(ENV_CONNECT_TIMEOUT_SECS, "3");
        env.insert(ENV_READ_TIMEOUT_SECS, "75");
        let cfg = Config::from_lookup(lookup(&env)).expect("config should parse");
        assert_eq!(cfg.connect_timeout, Duration::from_secs(3));
        assert_eq!(cfg.read_timeout, Duration::from_secs(75));
    }
}
