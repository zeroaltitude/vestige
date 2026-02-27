//! HTTP Streamable Transport for MCP
//!
//! Native HTTP transport implementing the MCP Streamable HTTP protocol.
//! Eliminates the need for supergateway by serving MCP directly over HTTP.
//!
//! Endpoints:
//! - POST /mcp — JSON-RPC request → SSE or JSON response
//! - GET /mcp — standalone SSE stream for server notifications
//! - DELETE /mcp — terminate session

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{delete, get, post},
};
use futures::stream;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;
use vestige_core::Storage;

use super::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::cognitive::CognitiveEngine;
use crate::server::McpServer;

/// Per-session state
struct Session {
    server: Mutex<McpServer>,
}

/// Shared application state
struct AppState {
    /// Session store: session_id → Session
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// Shared storage backend (cloned into each McpServer)
    storage: Arc<Storage>,
    /// Shared cognitive engine
    cognitive: Arc<Mutex<CognitiveEngine>>,
}

/// Configuration for the HTTP transport
pub struct HttpTransportConfig {
    pub host: String,
    pub port: u16,
}

impl Default for HttpTransportConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3100,
        }
    }
}

/// HTTP Transport for MCP server
pub struct HttpTransport {
    config: HttpTransportConfig,
}

impl HttpTransport {
    pub fn new(config: HttpTransportConfig) -> Self {
        Self { config }
    }

    /// Run the HTTP MCP server
    pub async fn run(self, storage: Arc<Storage>, cognitive: Arc<Mutex<CognitiveEngine>>) -> Result<(), std::io::Error> {
        let state = Arc::new(AppState {
            sessions: Mutex::new(HashMap::new()),
            storage,
            cognitive,
        });

        let app = Router::new()
            .route("/mcp", post(handle_post))
            .route("/mcp", get(handle_get))
            .route("/mcp", delete(handle_delete))
            .with_state(state);

        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

        info!("MCP HTTP server listening on http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;

        info!("MCP HTTP server shutting down");
        Ok(())
    }
}

/// Get or create a session, returning (session, session_id, is_new)
async fn get_or_create_session(
    state: &AppState,
    headers: &HeaderMap,
) -> (Arc<Session>, String, bool) {
    let existing_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let mut sessions = state.sessions.lock().await;

    if let Some(id) = &existing_id {
        if let Some(session) = sessions.get(id) {
            return (session.clone(), id.clone(), false);
        }
    }

    // Create new session
    let session_id = Uuid::new_v4().to_string();
    let server = McpServer::new(state.storage.clone(), state.cognitive.clone());
    let session = Arc::new(Session {
        server: Mutex::new(server),
    });
    sessions.insert(session_id.clone(), session.clone());
    info!("Created new MCP session: {}", session_id);
    (session, session_id, true)
}

/// Look up an existing session by header
async fn get_existing_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<(Arc<Session>, String)> {
    let id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())?;

    let sessions = state.sessions.lock().await;
    sessions.get(id).map(|s| (s.clone(), id.to_string()))
}

/// POST /mcp — Handle JSON-RPC request
async fn handle_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    // Parse the JSON-RPC request
    let request: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to parse JSON-RPC request: {}", e);
            let error_resp = JsonRpcResponse::error(None, JsonRpcError::parse_error());
            return (
                StatusCode::BAD_REQUEST,
                [(header::CONTENT_TYPE, "application/json")],
                serde_json::to_string(&error_resp).unwrap_or_default(),
            )
                .into_response();
        }
    };

    // Get or create session
    let (session, session_id, _is_new) = get_or_create_session(&state, &headers).await;

    // Handle the request
    let mut server = session.server.lock().await;
    let response = server.handle_request(request).await;
    drop(server);

    match response {
        Some(resp) => {
            let json = serde_json::to_string(&resp).unwrap_or_else(|e| {
                error!("Failed to serialize response: {}", e);
                format!(
                    r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32603,"message":"Internal error"}}}}"#
                )
            });

            // Check Accept header to decide response format
            let accept = headers
                .get(header::ACCEPT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json");

            if accept.contains("text/event-stream") {
                // Respond with SSE
                let event = Event::default()
                    .data(&json);

                let sse_stream = stream::once(async move { Ok::<_, std::convert::Infallible>(event) });

                (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE.as_str(), "text/event-stream"),
                        ("mcp-session-id", &session_id),
                        (header::CACHE_CONTROL.as_str(), "no-cache"),
                    ],
                    Sse::new(sse_stream),
                )
                    .into_response()
            } else {
                // Plain JSON response
                (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE.as_str(), "application/json"),
                        ("mcp-session-id", &session_id),
                    ],
                    json,
                )
                    .into_response()
            }
        }
        None => {
            // Notification — no response body (202 Accepted)
            (
                StatusCode::ACCEPTED,
                [("mcp-session-id", session_id.as_str())],
            )
                .into_response()
        }
    }
}

/// GET /mcp — Open standalone SSE stream
async fn handle_get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let session = get_existing_session(&state, &headers).await;

    match session {
        Some((_session, session_id)) => {
            // Send a keep-alive ping event, keep stream open
            // For now, just send a comment and close — server-initiated messages
            // can be added later when needed
            let events = vec![
                Ok::<_, std::convert::Infallible>(Event::default().comment("connected")),
            ];
            let sse_stream = stream::iter(events);

            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE.as_str(), "text/event-stream"),
                    ("mcp-session-id", session_id.as_str()),
                    (header::CACHE_CONTROL.as_str(), "no-cache"),
                ],
                Sse::new(sse_stream),
            )
                .into_response()
        }
        None => {
            // No session — need to POST first
            (StatusCode::NOT_FOUND, "No active session. Send POST /mcp first.").into_response()
        }
    }
}

/// DELETE /mcp — Terminate session
async fn handle_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok());

    match session_id {
        Some(id) => {
            let mut sessions = state.sessions.lock().await;
            if sessions.remove(id).is_some() {
                info!("Terminated MCP session: {}", id);
                StatusCode::OK.into_response()
            } else {
                (StatusCode::NOT_FOUND, "Session not found").into_response()
            }
        }
        None => (StatusCode::BAD_REQUEST, "Missing mcp-session-id header").into_response(),
    }
}

/// Graceful shutdown signal
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl+c");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("Received Ctrl+C, shutting down"),
        _ = terminate => info!("Received SIGTERM, shutting down"),
    }
}
