use std::pin::Pin;
use std::sync::Arc;

use futures::{Future, FutureExt};
use serde_json::Value;
use socketio_rs::{Payload, Server, ServerBuilder, ServerSocket};
use tracing::{info, warn};

type SocketCallback = Pin<Box<dyn Future<Output = ()> + Send>>;

#[derive(Clone)]
pub struct RealtimeSocket {
    server: Arc<Server>,
    port: u16,
}

impl RealtimeSocket {
    pub fn serve() -> Self {
        let port = 9090;
        info!("serve_socektio on port {}", port);
        let callback =
            |payload: Option<Payload>, socket: ServerSocket, _| Self::register(payload, socket);

        let server = ServerBuilder::new(port)
            .on("/", "register", callback)
            .build();

        let server_clone = server.clone();

        tokio::spawn(async move {
            server_clone.serve().await;
        });

        Self { server, port }
    }

    pub async fn notify_sdk(&self, sdk_key: String, event: &str, data: serde_json::Value) {
        info!("notify_sdk {} {} {:?}", sdk_key, event, data);
        self.server.emit_to("/", vec![&sdk_key], event, data).await
    }

    fn register(payload: Option<Payload>, socket: ServerSocket) -> SocketCallback {
        async move {
            info!("socketio recv {:?}", payload);
            if let Some(Payload::Json(value)) = payload {
                match value.get("sdk_key") {
                    Some(Value::String(sdk_key)) => socket.join(vec![sdk_key]).await,
                    _ => {
                        warn!("unkown register payload")
                    }
                }
            }

            let _ = socket.emit("update", serde_json::json!("")).await;
        }
        .boxed()
    }
}

impl std::fmt::Debug for RealtimeSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RealtimeSocket").field(&self.port).finish()
    }
}
