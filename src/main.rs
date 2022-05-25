use crate::base::ServerConfig;
use crate::repo::SdkRepository;
pub use axum::async_trait;
pub use base::FPServerError;
use config::Config;
use http::FpHttpHandler;
use std::sync::Arc;
use tracing::error;

mod base;
mod http;
mod repo;
mod stream;

#[tokio::main]
async fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("feature_probe_server_sdk=trace,feature_probe_server=trace")
        .init();

    let server_config = match init_server_config() {
        Ok(c) => c,
        Err(e) => {
            error!("server config error: {}", e);
            return;
        }
    };
    let server_port = server_config.server_port;
    let handler = match init_handler(server_config) {
        Ok(h) => h,
        Err(e) => {
            error!("server config error: {}", e);
            return;
        }
    };
    tokio::spawn(crate::http::serve_http::<FpHttpHandler>(
        server_port,
        handler,
    ));
    tokio::signal::ctrl_c().await.expect("shut down");
}

fn init_server_config() -> Result<ServerConfig, FPServerError> {
    let config = Config::builder()
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
        repo.sync(client_sdk_key.clone(), server_sdk_key.clone());
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
