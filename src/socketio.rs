use futures::FutureExt;
use socketio_rs::{Payload, ServerBuilder, ServerClient};
use tracing::info;

pub async fn serve_socketio() {
    let callback = |payload: Payload, _client: ServerClient, _| {
        async move {
            info!("socketio recv {:?}", payload);
        }
        .boxed()
    };

    let server = ServerBuilder::new(9090)
        .on("/", "register", callback)
        .build();
    server.serve().await;
}
