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

use thiserror::Error;

/// Environment variable controlling the bind address of the proxy server.
pub const ENV_LISTEN: &str = "REPLAYABLE_LISTEN";

/// Environment variable holding the required upstream LLM API base URL.
pub const ENV_UPSTREAM_URL: &str = "REPLAYABLE_UPSTREAM_URL";

/// Environment variable holding the JSONL trace output path.
pub const ENV_LOG_PATH: &str = "REPLAYABLE_LOG_PATH";

/// Environment variable controlling the bounded trace channel capacity.
pub const ENV_LOG_CHANNEL_CAPACITY: &str = "REPLAYABLE_LOG_CHANNEL_CAPACITY";

/// Default bind address when [`ENV_LISTEN`] is unset.
pub const DEFAULT_LISTEN: &str = "0.0.0.0:8080";

/// Default JSONL log path when [`ENV_LOG_PATH`] is unset.
pub const DEFAULT_LOG_PATH: &str = "./replayable-traces.jsonl";

/// Default bounded channel capacity for the trace writer.
pub const DEFAULT_LOG_CHANNEL_CAPACITY: usize = 1024;

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

        let upstream_url = lookup(ENV_UPSTREAM_URL)
            .ok_or(ConfigError::Missing(ENV_UPSTREAM_URL))?
            .trim()
            .to_string();
        if upstream_url.is_empty() {
            return Err(ConfigError::Invalid {
                name: ENV_UPSTREAM_URL,
                reason: "value is empty".to_string(),
            });
        }
        if !upstream_url.starts_with("http://") && !upstream_url.starts_with("https://") {
            return Err(ConfigError::Invalid {
                name: ENV_UPSTREAM_URL,
                reason: "must start with http:// or https://".to_string(),
            });
        }

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

        Ok(Self {
            listen,
            upstream_url: trim_trailing_slash(&upstream_url),
            log_path,
            log_channel_capacity,
        })
    }
}

fn trim_trailing_slash(url: &str) -> String {
    url.strip_suffix('/').unwrap_or(url).to_string()
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
}
