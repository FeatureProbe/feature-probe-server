use crate::base::ServerConfig;
use crate::repo::SdkRepository;
use anyhow::{bail, Result};
use base::FPServerError;
use base::LogFormatter;
use config::builder::DefaultState;
use config::{Config, ConfigBuilder};
use http::FpHttpHandler;
use std::sync::Arc;
use time::macros::format_description;
use time::UtcOffset;
use tracing::error;
use tracing_subscriber::fmt::layer;
use tracing_subscriber::fmt::time::{OffsetTime, SystemTime};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

mod base;
mod http;
mod push;
mod repo;

#[tokio::main]
async fn main() -> Result<()> {
    let server_config = match init_server_config(None) {
        Ok(c) => c,
        Err(e) => {
            bail!("server config error: {}", e);
        }
    };
    start(server_config).await?;
    tokio::signal::ctrl_c().await.expect("shut down");
    Ok(())
}

async fn start(server_config: ServerConfig) -> Result<()> {
    init_log();
    error!("FeatureProbe Server Commit: {}", env!("VERGEN_GIT_SHA"));
    error!(
        "FeatureProbe Server BuildTs: {}",
        env!("VERGEN_BUILD_TIMESTAMP")
    );
    error!(
        "FeatureProbe Server CommitTs: {}",
        env!("VERGEN_GIT_COMMIT_TIMESTAMP")
    );
    error!(
        "FeatureProbe Server Cargo Profile: {}",
        env!("VERGEN_CARGO_PROFILE")
    );
    error!("FeatureProbe Server Config: {}", server_config);
    let server_port = server_config.server_port;
    let handler = match init_handler(server_config) {
        Ok(h) => h,
        Err(e) => {
            bail!("server config error: {}", e);
        }
    };
    tokio::spawn(crate::http::serve_http::<FpHttpHandler>(
        server_port,
        handler,
    ));

    tokio::spawn(crate::push::serve_socketio());
    Ok(())
}

fn init_server_config(
    config: Option<ConfigBuilder<DefaultState>>,
) -> Result<ServerConfig, FPServerError> {
    let config = match config {
        Some(c) => c,
        None => Config::builder(),
    };
    let config = config
        .add_source(config::Environment::with_prefix("FP"))
        .build()
        .map_err(|e| FPServerError::ConfigError(e.to_string()))?;

    ServerConfig::try_parse(config)
}

fn init_handler(server_config: ServerConfig) -> Result<FpHttpHandler, FPServerError> {
    let repo = SdkRepository::new(server_config.clone());
    if let Some(keys_url) = server_config.keys_url {
        repo.sync_with(keys_url)
    } else if let (Some(ref client_sdk_key), Some(ref server_sdk_key)) =
        (server_config.client_sdk_key, server_config.server_sdk_key)
    {
        repo.sync(client_sdk_key.clone(), server_sdk_key.clone(), 1);
    } else {
        return Err(FPServerError::ConfigError(
            "not set FP_SERVER_SDK and FP_CLIENT_SDK".to_owned(),
        ));
    }

    Ok(FpHttpHandler {
        repo: Arc::new(repo),
        http_client: Default::default(),
        events_url: server_config.events_url,
        events_timeout: server_config.refresh_interval,
    })
}

pub fn init_log() {
    let _ = tracing_subscriber::fmt();
    let subscriber = tracing_subscriber::registry().with(EnvFilter::from_default_env());

    if let Ok(offset) = UtcOffset::current_local_offset() {
        let format = format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3][offset_hour sign:mandatory][offset_minute]"
        );
        let timer = OffsetTime::new(offset, format);
        subscriber
            .with(layer().event_format(LogFormatter::with_timer(timer)))
            .init();
    } else {
        subscriber
            .with(layer().event_format(LogFormatter::with_timer(SystemTime)))
            .init();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::LocalFileHttpHandler;

    #[tokio::test]
    async fn test_main() {
        let mock_api_port = 9009;
        let toggles_url = format!("http://127.0.0.1:{}/api/server-sdk/toggles", mock_api_port);
        let events_url = format!("http://127.0.0.1:{}/api/events", mock_api_port);
        let server_sdk_key = "server-sdk-key1".to_owned();
        let client_sdk_key = "client-sdk-key1".to_owned();
        let config = Config::builder()
            .set_default("toggles_url", toggles_url)
            .unwrap()
            .set_default("events_url", events_url)
            .unwrap()
            .set_default("client_sdk_key", client_sdk_key)
            .unwrap()
            .set_default("server_sdk_key", server_sdk_key)
            .unwrap()
            .set_default("refresh_seconds", "1")
            .unwrap();

        setup_mock_api(mock_api_port);

        let server_config = init_server_config(Some(config));
        assert!(server_config.is_ok());

        let server_config = server_config.unwrap();
        let r = start(server_config).await;
        log::info!("test_main"); // trigger log format
        assert!(r.is_ok());
    }

    fn setup_mock_api(port: u16) {
        let mock_feature_probe_api = LocalFileHttpHandler {};
        tokio::spawn(crate::http::serve_http::<LocalFileHttpHandler>(
            port,
            mock_feature_probe_api,
        ));
    }
}
