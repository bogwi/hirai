// src/web.rs
use crate::config::AppConfig;
use crate::event::Event;
use anyhow::Result;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{
    broadcast::{error::RecvError, Sender as BroadcastSender},
    mpsc::Receiver as MpscReceiver,
    watch::Receiver as WatchReceiver,
};
use tracing::{debug, error, info, warn};

/// Shared application state for the web server.
///
/// Holds a broadcast channel sender for distributing file events to all connected WebSocket clients.
#[derive(Clone)]
struct AppState {
    /// Broadcast sender for file events.
    event_tx: BroadcastSender<Event>,
}

/// Serves the main HTML page for the web UI.
///
/// Returns the contents of `static/index.html` as an HTML response.
async fn serve_home() -> impl IntoResponse {
    Html(include_str!("../static/index.html"))
}

/// Handles incoming WebSocket upgrade requests.
///
/// Upgrades the HTTP connection to a WebSocket and delegates to the socket handler.
async fn websocket_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    info!("New WebSocket connection request.");
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handles a single WebSocket client connection.
///
/// Spawns two tasks:
/// - One for sending file events to the client as JSON.
/// - One for receiving (and logging) messages from the client.
///
/// The connection is closed when either task finishes.
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    info!("WebSocket client connected.");
    let (mut sender, mut receiver): (SplitSink<WebSocket, Message>, SplitStream<WebSocket>) =
        socket.split();
    let mut rx = state.event_tx.subscribe();

    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    match serde_json::to_string(&event) {
                        Ok(json_payload) => {
                            if sender.send(Message::Text(json_payload)).await.is_err() {
                                warn!("Failed to send message to WebSocket client, client disconnected?");
                                break;
                            }
                            debug!("Sent event to WebSocket client: {:?}", event.op);
                        }
                        Err(e) => {
                            error!("Failed to serialize event for WebSocket: {}", e);
                        }
                    }
                }
                Err(RecvError::Lagged(missed_count)) => {
                    warn!(
                        "WebSocket client lagged behind, missed {} messages.",
                        missed_count
                    );
                }
                Err(RecvError::Closed) => {
                    info!("Broadcast channel closed, WebSocket send task for client finishing.");
                    break;
                }
            }
        }
        info!("WebSocket send task for a client finished.");
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(t) => {
                    debug!("Received text from WebSocket client: {}", t);
                }
                Message::Binary(_) => {
                    debug!("Received binary from WebSocket client.");
                }
                Message::Ping(_) => {
                    debug!("Received Ping from WebSocket client, Axum handles Pong automatically.");
                }
                Message::Pong(_) => {
                    debug!("Received Pong from WebSocket client.");
                }
                Message::Close(_) => {
                    debug!("WebSocket client sent Close frame.");
                    break;
                }
            }
        }
        info!("WebSocket receive task for a client finished.");
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
    info!("WebSocket client connection handler finished.");
}

/// Starts the web server and WebSocket event hub.
///
/// - Serves the web UI at the root path (`/`).
/// - Handles WebSocket connections at `/ws`.
/// - Forwards file events from the provided event source channel to all connected WebSocket clients.
/// - Shuts down gracefully when the shutdown signal is triggered.
///
/// # Arguments
/// - `app_config`: Shared application configuration.
/// - `event_source_rx`: Channel receiving file events to broadcast to WebSocket clients.
/// - `event_broadcast_tx`: Broadcast channel for distributing events to WebSocket clients.
/// - `shutdown_signal`: Watch channel for graceful shutdown notification.
///
/// # Returns
/// Returns `Ok(())` when the server shuts down cleanly, or an error if startup fails.
pub async fn start_server(
    app_config: Arc<AppConfig>,
    mut event_source_rx: MpscReceiver<Event>,
    event_broadcast_tx: BroadcastSender<Event>,
    shutdown_signal: WatchReceiver<bool>,
) -> Result<()> {
    let web_addr_str = app_config.web_addr.clone();
    let socket_addr: SocketAddr = web_addr_str.parse()?;

    let app_state = Arc::new(AppState {
        event_tx: event_broadcast_tx.clone(),
    });

    // Forward events from the event source channel to the broadcast channel for WebSocket clients.
    let broadcast_tx_clone = event_broadcast_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = event_source_rx.recv().await {
            if broadcast_tx_clone.receiver_count() > 0 {
                if let Err(e) = broadcast_tx_clone.send(event.clone()) {
                    debug!(
                        "Failed to broadcast event to WebSocket hub (no subscribers?): {}",
                        e
                    );
                }
            } else {
                debug!(
                    "No WebSocket subscribers, event not broadcasted: {:?}",
                    event.op
                );
            }
        }
        info!("Event source for Web UI finished.");
    });

    let app = Router::new()
        .route("/", get(serve_home))
        .route("/ws", get(websocket_handler))
        .with_state(app_state);

    info!("Web server starting on http://{}", socket_addr);

    let mut shutdown = shutdown_signal.clone(); // Clone for the async move block
    axum::serve(
        tokio::net::TcpListener::bind(socket_addr).await?,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        // Added move here
        shutdown.changed().await.ok();
        info!("Web server shutting down gracefully.");
    })
    .await?;

    info!("Web server stopped.");
    Ok(())
}
