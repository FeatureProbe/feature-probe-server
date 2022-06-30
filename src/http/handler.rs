use super::{cors_headers, ClientParams, SdkAuthorization};
#[cfg(feature = "unstable")]
use super::{SecretsParams, SegmentUpdateParams, ToggleUpdateParams};
use crate::{repo::SdkRepository, FPServerError};
use axum::{
    async_trait,
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json, TypedHeader,
};
use feature_probe_event::{
    collector::{EventHandler, FPEventError},
    event::PackedData,
};
use feature_probe_server_sdk::{FPUser, Url};
use reqwest::{
    header::{self, AUTHORIZATION, USER_AGENT},
    Client, Method,
};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tracing::{debug, error};

#[async_trait]
pub trait HttpHandler {
    async fn server_sdk_toggles(
        &self,
        TypedHeader(SdkAuthorization(sdk_key)): TypedHeader<SdkAuthorization>,
    ) -> Result<Response, FPServerError>;

    async fn client_sdk_toggles(
        &self,
        Query(params): Query<ClientParams>,
        TypedHeader(SdkAuthorization(sdk_key)): TypedHeader<SdkAuthorization>,
    ) -> Result<Response, FPServerError>;

    #[cfg(feature = "unstable")]
    async fn update_toggles(
        &self,
        Json(params): Json<ToggleUpdateParams>,
    ) -> Result<Response, FPServerError>;

    #[cfg(feature = "unstable")]
    async fn update_segments(
        &self,
        Json(params): Json<SegmentUpdateParams>,
    ) -> Result<Response, FPServerError>;

    #[cfg(feature = "unstable")]
    async fn check_secrets(
        &self,
        Json(_params): Json<SecretsParams>,
    ) -> Result<Json<HashMap<String, String>>, FPServerError>;

    async fn all_secrets(&self) -> Result<Json<HashMap<String, String>>, FPServerError>;
}

#[derive(Clone)]
pub struct FpHttpHandler {
    pub repo: Arc<SdkRepository>,
    pub http_client: Arc<Client>,
    pub events_url: Url,
    pub events_timeout: Duration,
}

#[async_trait]
impl HttpHandler for FpHttpHandler {
    async fn server_sdk_toggles(
        &self,
        TypedHeader(SdkAuthorization(sdk_key)): TypedHeader<SdkAuthorization>,
    ) -> Result<Response, FPServerError> {
        match self.repo.server_sdk_repo_string(&sdk_key) {
            Some(body) => Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                body,
            )
                .into_response()),
            None => Err(FPServerError::NotFound(format!("toggles of {}", sdk_key))),
        }
    }

    async fn client_sdk_toggles(
        &self,
        Query(params): Query<ClientParams>,
        TypedHeader(SdkAuthorization(sdk_key)): TypedHeader<SdkAuthorization>,
    ) -> Result<Response, FPServerError> {
        let user = decode_user(params.user)?;
        match self.repo.client_sdk_eval_string(&sdk_key, &user) {
            Some(body) => Ok((StatusCode::OK, cors_headers(), body).into_response()),
            None => Err(FPServerError::NotFound(format!(
                "toggles of client-sdk-key {}",
                sdk_key
            ))),
        }
    }

    #[cfg(feature = "unstable")]
    async fn update_toggles(
        &self,
        Json(params): Json<ToggleUpdateParams>,
    ) -> Result<Response, FPServerError> {
        self.repo.update_toggles(&params.sdk_key, params.toggles)?;
        let status = StatusCode::OK;
        Ok(status.into_response())
    }
    #[cfg(feature = "unstable")]

    async fn update_segments(
        &self,
        Json(params): Json<SegmentUpdateParams>,
    ) -> Result<Response, FPServerError> {
        self.repo.update_segments(params.segments)?;
        let status = StatusCode::OK;
        let body = "";
        Ok((status, body).into_response())
    }

    #[cfg(feature = "unstable")]
    async fn check_secrets(
        &self,
        Json(_params): Json<SecretsParams>,
    ) -> Result<Json<HashMap<String, String>>, FPServerError> {
        Ok(HashMap::new().into())
    }

    async fn all_secrets(&self) -> Result<Json<HashMap<String, String>>, FPServerError> {
        let secret_keys = self.repo.secret_keys();
        Ok(secret_keys.into())
    }
}

#[async_trait]
impl EventHandler for FpHttpHandler {
    async fn handle_events(
        &self,
        sdk_key: String,
        user_agent: String,
        data: VecDeque<PackedData>,
    ) -> Result<Response, FPEventError> {
        let http_client = self.http_client.clone();
        let events_url = self.events_url.clone();
        let timeout = self.events_timeout;
        tokio::spawn(async move {
            let auth = SdkAuthorization(sdk_key).encode();
            let request = http_client
                .request(Method::POST, events_url.clone())
                .header(AUTHORIZATION, auth)
                .header(USER_AGENT, user_agent)
                .timeout(timeout)
                .json(&data);
            match request.send().await {
                Err(e) => error!("event post error: {}", e),
                Ok(r) => debug!("{:?}", r),
            };
        });
        Ok((StatusCode::OK, cors_headers(), "").into_response())
    }
}

fn decode_user(user: String) -> Result<FPUser, FPServerError> {
    if let Ok(user) = base64::decode(user) {
        if let Ok(user) = String::from_utf8(user) {
            if let Ok(user) = serde_json::from_str::<FPUser>(&user) {
                return Ok(user);
            }
        }
    }
    Err(FPServerError::UserDecodeError)
}

#[derive(Clone)]
pub struct LocalFileHttpHandler;

#[async_trait]
impl HttpHandler for LocalFileHttpHandler {
    async fn server_sdk_toggles(
        &self,
        TypedHeader(SdkAuthorization(_sdk_key)): TypedHeader<SdkAuthorization>,
    ) -> Result<Response, FPServerError> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("resources/fixtures/repo.json");
        let body = fs::read_to_string(path).unwrap();
        Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response())
    }

    async fn client_sdk_toggles(
        &self,
        Query(_params): Query<ClientParams>,
        TypedHeader(SdkAuthorization(_sdk_key)): TypedHeader<SdkAuthorization>,
    ) -> Result<Response, FPServerError> {
        let body = "{}".to_owned();
        Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response())
    }

    #[cfg(feature = "unstable")]
    async fn update_toggles(
        &self,
        Json(_params): Json<ToggleUpdateParams>,
    ) -> Result<Response, FPServerError> {
        let status = StatusCode::OK;
        let body = "";
        Ok((status, body).into_response())
    }

    #[cfg(feature = "unstable")]
    async fn update_segments(
        &self,
        Json(_params): Json<SegmentUpdateParams>,
    ) -> Result<Response, FPServerError> {
        let status = StatusCode::OK;
        let body = "";
        Ok((status, body).into_response())
    }

    #[cfg(feature = "unstable")]
    async fn check_secrets(
        &self,
        Json(_params): Json<SecretsParams>,
    ) -> Result<Json<HashMap<String, String>>, FPServerError> {
        Ok(HashMap::new().into())
    }

    async fn all_secrets(&self) -> Result<Json<HashMap<String, String>>, FPServerError> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("resources/fixtures/secrets.json");
        let json_str = fs::read_to_string(path).unwrap();
        let secret_keys =
            serde_json::from_str::<HashMap<String, HashMap<String, String>>>(&json_str).unwrap();
        let secret_keys = secret_keys.get("mapping").unwrap().to_owned();
        Ok(secret_keys.into())
    }
}

#[async_trait]
impl EventHandler for LocalFileHttpHandler {
    async fn handle_events(
        &self,
        _sdk_key: String,
        _user_agent: String,
        _data: VecDeque<PackedData>,
    ) -> Result<Response, FPEventError> {
        Ok((StatusCode::OK, cors_headers(), "").into_response())
    }
}
