use futures::FutureExt;
use socketio_rs::{Payload, ServerBuilder, ServerSocket};
use tracing::info;

pub async fn serve_socketio() {
    let callback = |payload: Option<Payload>, _socket: ServerSocket, _| {
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
