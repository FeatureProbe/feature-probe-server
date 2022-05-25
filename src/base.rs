use std::time::Duration;

use feature_probe_server_sdk::Url;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error, Deserialize)]
pub enum FPServerError {
    #[error("not found {0}")]
    NotFound(String),
    #[error("user base64 decode error")]
    UserDecodeError,
    #[error("config error: {0}")]
    ConfigError(String),
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub toggles_url: Url,
    pub events_url: Url,
    pub keys_url: Option<Url>,
    pub refresh_interval: Duration,
    pub server_sdk_key: Option<String>,
    pub client_sdk_key: Option<String>,
    pub server_port: u16,
}

impl ServerConfig {
    pub fn try_parse(config: config::Config) -> Result<ServerConfig, FPServerError> {
        let toggles_url = match config.get_string("toggles_url") {
            Err(_) => {
                return Err(FPServerError::ConfigError(
                    "NOT SET FP_TOGGLES_URL".to_owned(),
                ))
            }
            Ok(url) => match Url::parse(&url) {
                Err(e) => {
                    return Err(FPServerError::ConfigError(format!(
                        "INVALID FP_TOGGLES_URL: {}",
                        e,
                    )))
                }
                Ok(u) => u,
            },
        };

        let events_url = match config.get_string("events_url") {
            Err(_) => {
                return Err(FPServerError::ConfigError(
                    "NOT SET FP_EVENTS_URL".to_owned(),
                ))
            }
            Ok(url) => match Url::parse(&url) {
                Err(e) => {
                    return Err(FPServerError::ConfigError(format!(
                        "INVALID FP_EVENTS_URL: {}",
                        e,
                    )))
                }
                Ok(u) => u,
            },
        };
        let client_sdk_key = config.get_string("client_sdk_key").ok();
        let server_sdk_key = config.get_string("server_sdk_key").ok();

        let keys_url = match config.get_string("keys_url") {
            Ok(url) => match Url::parse(&url) {
                Err(e) => {
                    return Err(FPServerError::ConfigError(format!(
                        "INVALID FP_KEYS_URL: {}",
                        e,
                    )))
                }
                Ok(u) => Some(u),
            },
            Err(_) => {
                if client_sdk_key.is_none() {
                    return Err(FPServerError::ConfigError(
                        "NOT SET FP_CLIENT_SDK_KEY".to_owned(),
                    ));
                }
                if server_sdk_key.is_none() {
                    return Err(FPServerError::ConfigError(
                        "NOT SET FP_SERVER_SDK_KEY".to_owned(),
                    ));
                }
                None
            }
        };

        let refresh_interval = match config.get_int("refresh_seconds") {
            Err(_) => {
                return Err(FPServerError::ConfigError(
                    "NOT SET FP_REFRESH_SECONDS".to_owned(),
                ))
            }
            Ok(interval) => Duration::from_secs(interval as u64),
        };
        let server_port = match config.get_int("server_port") {
            Err(_) => 9000, // default port
            Ok(server_port) => server_port as u16,
        };

        Ok(ServerConfig {
            toggles_url,
            events_url,
            keys_url,
            refresh_interval,
            client_sdk_key,
            server_sdk_key,
            server_port,
        })
    }
}
