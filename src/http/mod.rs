mod handler;

use axum::{
    extract::{Extension, Query},
    handler::Handler,
    headers::HeaderName,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router, TypedHeader,
};
use reqwest::header;

use crate::FPServerError;
use feature_probe_event::collector::{post_events, EventHandler};
use feature_probe_server_sdk::{SdkAuthorization, Segment, Toggle};
pub use handler::{FpHttpHandler, HttpHandler, LocalFileHttpHandler};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;

pub async fn serve_http<T>(port: u16, handler: T)
where
    T: HttpHandler + EventHandler + Clone + Send + Sync + 'static,
{
    let app = Router::new()
        .route("/", get(root_handler))
        .route(
            "/api/client-sdk/toggles",
            get(client_sdk_toggles::<T>).options(client_cors),
        )
        .route("/api/server-sdk/toggles", get(server_sdk_toggles::<T>))
        .route("/api/server/toggles", post(update_toggles::<T>))
        .route("/api/server/segments", post(update_segments::<T>))
        .route("/api/server/check_secrets", post(check_secrets::<T>))
        .route("/internal/all_secrets", get(all_secrets::<T>)) // not for public network
        .route("/api/events", post(post_events::<T>))
        .layer(Extension(handler))
        .fallback(handler_404.into_service());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn client_cors() -> Response {
    (StatusCode::OK, cors_headers()).into_response()
}

async fn client_sdk_toggles<T>(
    params: Query<ClientParams>,
    sdk_key: TypedHeader<SdkAuthorization>,
    Extension(handler): Extension<T>,
) -> Result<Response, FPServerError>
where
    T: HttpHandler + Clone + Send + Sync + 'static,
{
    handler.client_sdk_toggles(params, sdk_key).await
}

async fn server_sdk_toggles<T>(
    sdk_key: TypedHeader<SdkAuthorization>,
    Extension(handler): Extension<T>,
) -> Result<Response, FPServerError>
where
    T: HttpHandler + Clone + Send + Sync + 'static,
{
    handler.server_sdk_toggles(sdk_key).await
}

async fn update_toggles<T>(
    params: Json<ToggleUpdateParams>,
    Extension(handler): Extension<T>,
) -> Result<Response, FPServerError>
where
    T: HttpHandler + Clone + Send + Sync + 'static,
{
    handler.update_toggles(params).await
}

async fn update_segments<T>(
    params: Json<SegmentUpdateParams>,
    Extension(handler): Extension<T>,
) -> Result<Response, FPServerError>
where
    T: HttpHandler + Clone + Send + Sync + 'static,
{
    handler.update_segments(params).await
}

async fn check_secrets<T>(
    params: Json<SecretsParams>,
    Extension(handler): Extension<T>,
) -> Result<Json<HashMap<String, String>>, FPServerError>
where
    T: HttpHandler + Clone + Send + Sync + 'static,
{
    handler.check_secrets(params).await
}

async fn all_secrets<T>(
    Extension(handler): Extension<T>,
) -> Result<Json<HashMap<String, String>>, FPServerError>
where
    T: HttpHandler + Clone + Send + Sync + 'static,
{
    handler.all_secrets().await
}

async fn root_handler() -> Html<&'static str> {
    Html("<h1>Feature Probe Server</h1>")
}

async fn handler_404() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "")
}

#[derive(Debug, Deserialize)]
pub struct ClientParams {
    user: String,
}

#[derive(Debug, Deserialize)]
pub struct ToggleUpdateParams {
    sdk_key: String,
    toggles: HashMap<String, Toggle>,
}

#[derive(Debug, Deserialize)]
pub struct SegmentUpdateParams {
    segments: HashMap<String, Segment>,
}

#[derive(Debug, Deserialize)]
pub struct SecretsParams {
    _secrets: HashMap<String, String>,
}

impl IntoResponse for FPServerError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            FPServerError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            FPServerError::UserDecodeError => (StatusCode::BAD_REQUEST, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": error_message,
        }));

        (status, cors_headers(), body).into_response()
    }
}

pub fn cors_headers() -> [(HeaderName, &'static str); 4] {
    [
        (header::CONTENT_TYPE, "application/json"),
        (header::ACCESS_CONTROL_ALLOW_HEADERS, "*"),
        (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        (
            header::ACCESS_CONTROL_ALLOW_METHODS,
            "GET, POST, PUT, DELETE, OPTIONS",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::{handler::LocalFileHttpHandler, *};
    use crate::{base::ServerConfig, repo::SdkRepository};
    use axum::http::StatusCode;
    use feature_probe_server_sdk::{FPDetail, FPUser, Repository, SdkAuthorization};
    use reqwest::{header::AUTHORIZATION, Client, Error, Method, Url};
    use serde_json::Value;
    use std::{fs, path::PathBuf, sync::Arc, time::Duration};

    #[tokio::test]
    async fn test_fp_server_connect_fp_api() {
        let server_sdk_key = "server-sdk-key1".to_owned();
        let client_sdk_key = "client-sdk-key1".to_owned();
        let mock_api_port = 9002;
        let fp_server_port = 9003;
        setup_mock_api(mock_api_port);
        tokio::time::sleep(Duration::from_millis(100)).await; // wait mock api port listen
        let repo = setup_fp_server(
            mock_api_port,
            fp_server_port,
            &client_sdk_key,
            &server_sdk_key,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await; // wait fp server port listen
        let repo_string = repo.server_sdk_repo_string(&server_sdk_key);
        assert!(repo_string.is_some());

        let resp = http_get(
            format!("http://127.0.0.1:{}/api/server-sdk/toggles", fp_server_port),
            server_sdk_key.clone(),
        )
        .await;
        assert!(resp.is_ok());
        let body = resp.unwrap().text().await;
        assert!(body.is_ok());
        let body = body.unwrap();
        let r = serde_json::from_str::<Repository>(&body).unwrap();
        assert_eq!(r, repo_from_test_file());

        let resp = http_get(
            format!("http://127.0.0.1:{}/api/server-sdk/toggles", fp_server_port),
            "no_exist_server_key".to_owned(),
        )
        .await;
        assert!(resp.is_ok());
        let resp = resp.unwrap();
        assert!(resp.status() == StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_server_sdk_toggles() {
        let port = 9004;
        let handler = LocalFileHttpHandler {};
        tokio::spawn(crate::http::serve_http::<LocalFileHttpHandler>(
            port, handler,
        ));
        tokio::time::sleep(Duration::from_millis(100)).await; // wait port listen

        let resp = http_get(
            format!("http://127.0.0.1:{}/api/server-sdk/toggles", port),
            "sdk-key".to_owned(),
        )
        .await;
        assert!(resp.is_ok(), "response invalid");
        let body = resp.unwrap().text().await;
        assert!(body.is_ok(), "response body error");
        let body = body.unwrap();
        let r = serde_json::from_str::<Repository>(&body).unwrap();
        assert_eq!(r, repo_from_test_file());
    }

    #[tokio::test]
    async fn test_client_sdk_toggles() {
        let server_sdk_key = "server-sdk-key1".to_owned();
        let client_sdk_key = "client-sdk-key1".to_owned();
        let mock_api_port = 9005;
        let fp_server_port = 9006;
        setup_mock_api(mock_api_port);
        tokio::time::sleep(Duration::from_millis(100)).await; // wait mock api port listen
        let _ = setup_fp_server(
            mock_api_port,
            fp_server_port,
            &client_sdk_key,
            &server_sdk_key,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await; // wait fp server port listen

        let user = FPUser::new("some-key").with("city", "1");
        let user_json = serde_json::to_string(&user).unwrap();
        let user_base64 = base64::encode(&user_json);
        let resp = http_get(
            format!(
                "http://127.0.0.1:{}/api/client-sdk/toggles?user={}",
                fp_server_port, user_base64
            ),
            client_sdk_key.clone(),
        )
        .await;
        assert!(resp.is_ok(), "response invalid");
        let text = resp.unwrap().text().await;
        assert!(text.is_ok(), "response text error");
        let text = text.unwrap();
        let toggles = serde_json::from_str::<HashMap<String, FPDetail<Value>>>(&text);
        assert!(toggles.is_ok(), "response can not deserialize");
        let toggles = toggles.unwrap();
        let t = toggles.get("bool_toggle");
        assert!(t.is_some(), "toggle not found");
        let t = t.unwrap();
        assert_eq!(t.rule_index, Some(0));
        assert_eq!(t.value.as_bool(), Some(true));
    }

    async fn http_get(url: String, sdk_key: String) -> Result<reqwest::Response, Error> {
        let toggles_url = Url::parse(&url).unwrap();
        let timeout = Duration::from_secs(1);
        let auth = SdkAuthorization(sdk_key).encode();
        let request = Client::new()
            .request(Method::GET, toggles_url)
            .header(AUTHORIZATION, auth)
            .timeout(timeout);
        request.send().await
    }

    fn repo_from_test_file() -> Repository {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("resources/fixtures/repo.json");
        let json_str = fs::read_to_string(path).unwrap();
        serde_json::from_str::<Repository>(&json_str).unwrap()
    }

    fn setup_mock_api(port: u16) {
        let mock_feature_probe_api = LocalFileHttpHandler {};
        tokio::spawn(crate::http::serve_http::<LocalFileHttpHandler>(
            port,
            mock_feature_probe_api,
        ));
    }

    async fn setup_fp_server(
        target_port: u16,
        listen_port: u16,
        client_sdk_key: &str,
        server_sdk_key: &str,
    ) -> Arc<SdkRepository> {
        let toggles_url = Url::parse(&format!(
            "http://127.0.0.1:{}/api/server-sdk/toggles",
            target_port
        ))
        .unwrap();
        let events_url =
            Url::parse(&format!("http://127.0.0.1:{}/api/events", target_port)).unwrap();
        let repo = SdkRepository::new(ServerConfig {
            toggles_url,
            events_url: events_url.clone(),
            refresh_interval: Duration::from_secs(1),
            keys_url: None,
            client_sdk_key: Some(client_sdk_key.to_owned()),
            server_sdk_key: Some(server_sdk_key.to_owned()),
            server_port: listen_port,
        });
        repo.sync(client_sdk_key.to_owned(), server_sdk_key.to_owned());
        let repo = Arc::new(repo);
        let feature_probe_server = FpHttpHandler {
            repo: repo.clone(),
            events_url,
            events_timeout: Duration::from_secs(10),
            http_client: Default::default(),
        };
        tokio::spawn(crate::http::serve_http::<FpHttpHandler>(
            listen_port,
            feature_probe_server,
        ));
        repo
    }
}
