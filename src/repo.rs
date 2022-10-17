use crate::base::ServerConfig;
#[cfg(feature = "unstable")]
use crate::FPServerError;
use feature_probe_server_sdk::{FPConfig, FPUser, FeatureProbe as FPClient, Url};
#[cfg(feature = "unstable")]
use feature_probe_server_sdk::{Segment, Toggle};
use parking_lot::RwLock;
use reqwest::Method;
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct SdkRepository {
    inner: Arc<Inner>,
}

#[derive(Deserialize, Debug)]
struct SecretMapping {
    pub mapping: HashMap<String, String>,
}

#[derive(Debug)]
struct Inner {
    server_config: ServerConfig,
    http_client: reqwest::Client,
    sdk_clients: RwLock<HashMap<String, FPClient>>,
    secret_keys: RwLock<HashMap<String, String>>,
}

impl SdkRepository {
    pub fn new(server_config: ServerConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                server_config,
                http_client: Default::default(),
                sdk_clients: Default::default(),
                secret_keys: Default::default(),
            }),
        }
    }

    #[cfg(feature = "unstable")]
    pub fn update_segments(&self, segments: HashMap<String, Segment>) -> Result<(), FPServerError> {
        // TODO: perf
        let mut sdks = self.inner.sdk_clients.write();
        sdks.iter_mut()
            .for_each(|(_, sdk)| sdk.update_segments(segments.clone()));
        Ok(())
    }

    #[cfg(feature = "unstable")]
    pub fn update_toggles(
        &self,
        server_sdk_key: &str,
        toggles: HashMap<String, Toggle>,
    ) -> Result<(), FPServerError> {
        let mut sdks = self.inner.sdk_clients.write();
        let _ = match sdks.get_mut(server_sdk_key) {
            //: TODO: create sdk if not exist
            None => debug!("update_toggles server_sdk_key not exit {}", server_sdk_key),
            Some(sdk) => sdk.update_toggles(toggles),
        };
        Ok(())
    }

    pub fn secret_keys(&self) -> HashMap<String, String> {
        let secret_keys = self.inner.secret_keys.read();
        (*secret_keys).clone()
    }

    pub fn sync(&self, client_sdk_key: String, server_sdk_key: String) {
        self.inner.sync(&server_sdk_key);
        let mut secret_keys = self.inner.secret_keys.write();
        (*secret_keys).insert(client_sdk_key, server_sdk_key);
    }

    pub fn sync_with(&self, keys_url: Url) {
        self.sync_secret_keys(keys_url);
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(inner.server_config.refresh_interval);
            loop {
                {
                    let secret_keys = inner.secret_keys.read();
                    for server_sdk_key in secret_keys.values() {
                        inner.sync(server_sdk_key)
                    }
                } // drop(secret_keys);
                interval.tick().await;
            }
        });
    }

    fn sync_secret_keys(&self, keys_url: Url) {
        let inner = self.inner.clone();
        let mut interval = tokio::time::interval(inner.server_config.refresh_interval);
        tokio::spawn(async move {
            loop {
                let url = keys_url.clone();
                let request = inner
                    .http_client
                    .request(Method::GET, url)
                    .timeout(inner.server_config.refresh_interval);
                match request.send().await {
                    Err(e) => error!("sync_secret_keys error: {}", e),
                    Ok(resp) => match resp.text().await {
                        Err(e) => error!("sync_secret_keys: {}", e),
                        Ok(body) => match serde_json::from_str::<SecretMapping>(&body) {
                            Err(e) => error!("sync_secret_keys json error: {}", e),
                            Ok(r) => {
                                debug!("sync_secret_keys success {:?}", r.mapping);
                                let mut secret_keys = inner.secret_keys.write();
                                secret_keys.extend(r.mapping);
                            }
                        },
                    },
                }
                interval.tick().await;
            }
        });
    }

    pub fn server_sdk_repo_string(&self, server_sdk_key: &str) -> Option<String> {
        let sdk_clients = self.inner.sdk_clients.read();
        sdk_clients
            .get(server_sdk_key)
            .map(|sdk_client| sdk_client.repo_string())
    }

    pub fn client_sdk_eval_string(&self, client_sdk_key: &str, user: &FPUser) -> Option<String> {
        let sdk_clients = self.inner.sdk_clients.read();
        let secret_keys = self.inner.secret_keys.read();
        let server_sdk_key = secret_keys.get(client_sdk_key)?;
        sdk_clients
            .get(server_sdk_key)
            .map(|sdk_client| sdk_client.all_evaluated_string(user))
    }

    #[cfg(test)]
    #[cfg(feature = "unstable")]
    fn sdk_client(&self, sdk_key: &str) -> Option<FPClient> {
        let sdk_clients = self.inner.sdk_clients.read();
        sdk_clients.get(sdk_key).map(|c| c.clone())
    }
}

impl Inner {
    pub fn sync(&self, server_sdk_key: &str) {
        let mut sdks = self.sdk_clients.write();
        if (*sdks).get(server_sdk_key).is_none() {
            let config = FPConfig {
                server_sdk_key: server_sdk_key.to_owned(),
                remote_url: "".to_owned(),
                toggles_url: Some(self.server_config.toggles_url.to_string()),
                refresh_interval: self.server_config.refresh_interval,
                http_client: Some(self.http_client.clone()),
                wait_first_resp: false,
                ..Default::default()
            };
            match FPClient::new(config) {
                Err(e) => error!("sync error: {}", e),
                Ok(sdk) => {
                    let _ = sdks.insert(server_sdk_key.to_owned(), sdk);
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use axum::{routing::get, Json, Router, TypedHeader};
    #[cfg(feature = "unstable")]
    use feature_probe_server_sdk::FPUser;
    use feature_probe_server_sdk::{Repository, SdkAuthorization};
    #[cfg(feature = "unstable")]
    use serde_json::json;
    use std::{fs, net::SocketAddr, path::PathBuf, time::Duration};

    #[tokio::test]
    async fn test_repo_sync() {
        let port = 9590;
        setup_mock_api(port);
        let client_sdk_key = "client-sdk-key".to_owned();
        let server_sdk_key = "server-sdk-key".to_owned();
        let repository = setup_repository(port, &client_sdk_key, &server_sdk_key).await;

        let repo_string = repository.server_sdk_repo_string(&server_sdk_key);
        assert!(repo_string.is_some());
        let r = serde_json::from_str::<Repository>(&repo_string.unwrap()).unwrap();
        assert!(r == repo_from_test_file());

        let secret_keys = repository.secret_keys();
        assert!(secret_keys.len() == 1);
        assert!(secret_keys.get(&client_sdk_key) == Some(&server_sdk_key));
    }

    #[tokio::test]
    async fn test_repo_sync2() {
        let port = 9591;
        setup_mock_api(port);
        let client_sdk_key = "client-sdk-key".to_owned();
        let server_sdk_key = "server-sdk-key".to_owned();
        let repository = setup_repository2(port).await;

        let repo_string = repository.server_sdk_repo_string(&server_sdk_key);
        assert!(repo_string.is_some());
        let r = serde_json::from_str::<Repository>(&repo_string.unwrap()).unwrap();
        assert!(r == repo_from_test_file());

        let secret_keys = repository.secret_keys();
        assert!(secret_keys.len() == 1);
        assert!(secret_keys.get(&client_sdk_key) == Some(&server_sdk_key));
    }

    #[cfg(feature = "unstable")]
    #[tokio::test]
    async fn test_update_toggles() {
        let port = 9592;
        setup_mock_api(port);

        let server_sdk_key = "sdk-key1".to_owned();
        let client_sdk_key = "client-sdk-key".to_owned();
        let repository = setup_repository(port, &client_sdk_key, &server_sdk_key).await;
        let client = repository.sdk_client(&server_sdk_key);
        assert!(client.is_some());

        let client = client.unwrap();
        let user = FPUser::new().with("city", "4");
        let default: HashMap<String, String> = HashMap::default();
        let v = client.json_value("json_toggle", &user, json!(default));
        assert!(v.get("variation_1").is_some());

        let mut map = update_toggles_from_file();
        let update_toggles = map.remove(&server_sdk_key);
        assert!(update_toggles.is_some());

        let update_toggles = update_toggles.unwrap();
        let result = repository.update_toggles(&server_sdk_key, update_toggles);
        assert!(result.is_ok());
    }

    async fn setup_repository(
        port: u16,
        client_sdk_key: &str,
        server_sdk_key: &str,
    ) -> SdkRepository {
        let toggles_url =
            Url::parse(&format!("http://127.0.0.1:{}/api/server-sdk/toggles", port)).unwrap();
        let events_url = Url::parse(&format!("http://127.0.0.1:{}/api/events", port)).unwrap();
        let repo = SdkRepository::new(ServerConfig {
            toggles_url,
            events_url,
            refresh_interval: Duration::from_secs(1),
            client_sdk_key: Some(client_sdk_key.to_owned()),
            server_sdk_key: Some(server_sdk_key.to_owned()),
            keys_url: None,
            server_port: port,
        });
        repo.sync(client_sdk_key.to_owned(), server_sdk_key.to_owned());
        tokio::time::sleep(Duration::from_millis(100)).await;
        repo
    }

    async fn setup_repository2(port: u16) -> SdkRepository {
        let toggles_url =
            Url::parse(&format!("http://127.0.0.1:{}/api/server-sdk/toggles", port)).unwrap();
        let events_url = Url::parse(&format!("http://127.0.0.1:{}/api/events", port)).unwrap();
        let keys_url = Url::parse(&format!("http://127.0.0.1:{}/api/secret-keys", port)).unwrap();
        let repo = SdkRepository::new(ServerConfig {
            toggles_url,
            events_url,
            refresh_interval: Duration::from_millis(10),
            client_sdk_key: None,
            server_sdk_key: None,
            keys_url: Some(keys_url.clone()),
            server_port: port,
        });
        repo.sync_with(keys_url);
        tokio::time::sleep(Duration::from_millis(100)).await;
        repo
    }

    async fn server_sdk_toggles(
        TypedHeader(SdkAuthorization(_sdk_key)): TypedHeader<SdkAuthorization>,
    ) -> Json<Repository> {
        repo_from_test_file().into()
    }

    async fn secret_keys() -> String {
        r#" { "mapping": { "client-sdk-key": "server-sdk-key" } }"#.to_owned()
    }

    fn setup_mock_api(port: u16) {
        let app = Router::new()
            .route("/api/secret-keys", get(secret_keys))
            .route("/api/server-sdk/toggles", get(server_sdk_toggles));
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        tokio::spawn(async move {
            let _ = axum::Server::bind(&addr)
                .serve(app.into_make_service())
                .await;
        });
    }

    fn repo_from_test_file() -> Repository {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("resources/fixtures/repo.json");
        let json_str = fs::read_to_string(path).unwrap();
        serde_json::from_str::<Repository>(&json_str).unwrap()
    }

    #[cfg(feature = "unstable")]
    fn update_toggles_from_file() -> HashMap<String, HashMap<String, Toggle>> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("resources/fixtures/toggles_update.json");
        let json_str = fs::read_to_string(path).unwrap();
        serde_json::from_str::<HashMap<String, HashMap<String, Toggle>>>(&json_str).unwrap()
    }
}
