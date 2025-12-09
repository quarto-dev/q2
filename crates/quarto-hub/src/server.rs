//! HTTP server setup and routing

use std::str::FromStr;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use samod::DocumentId;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::context::{HubConfig, HubContext, SharedContext};
use crate::error::Result;
use crate::storage::StorageManager;

/// Health check response
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    project_root: String,
    qmd_file_count: usize,
    index_document_id: String,
}

/// List of discovered files (from filesystem)
#[derive(Serialize)]
struct FilesResponse {
    qmd_files: Vec<String>,
}

/// Document entry in the index
#[derive(Serialize)]
struct DocumentEntry {
    path: String,
    document_id: String,
}

/// List of documents (from index)
#[derive(Serialize)]
struct DocumentsResponse {
    documents: Vec<DocumentEntry>,
}

/// Single document response
#[derive(Serialize)]
struct DocumentResponse {
    document_id: String,
    path: Option<String>,
    // For now we just return metadata; actual content would require
    // serializing the automerge document which is a future task
}

/// Error response
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

/// Update document request
#[derive(Deserialize)]
struct UpdateDocumentRequest {
    // For testing: just a simple key-value pair to put in the document
    key: String,
    value: String,
}

/// Health check endpoint
async fn health(State(ctx): State<SharedContext>) -> impl IntoResponse {
    let response = HealthResponse {
        status: "ok",
        project_root: ctx.storage().project_root().display().to_string(),
        qmd_file_count: ctx.project_files().qmd_files.len(),
        index_document_id: ctx.index().document_id(),
    };
    Json(response)
}

/// List discovered files (from filesystem)
async fn list_files(State(ctx): State<SharedContext>) -> impl IntoResponse {
    let response = FilesResponse {
        qmd_files: ctx
            .project_files()
            .qmd_files
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
    };
    Json(response)
}

/// List all documents from the index
async fn list_documents(State(ctx): State<SharedContext>) -> impl IntoResponse {
    let files = ctx.index().get_all_files();

    let documents: Vec<DocumentEntry> = files
        .into_iter()
        .map(|(path, document_id)| DocumentEntry { path, document_id })
        .collect();

    Json(DocumentsResponse { documents })
}

/// Get a single document by ID
async fn get_document(
    State(ctx): State<SharedContext>,
    Path(doc_id_str): Path<String>,
) -> impl IntoResponse {
    // Validate the document ID format
    let doc_id = match DocumentId::from_str(&doc_id_str) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid document ID format".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Try to find the document
    match ctx.repo().find(doc_id).await {
        Ok(Some(_handle)) => {
            // Find the path for this document ID (reverse lookup)
            let path = ctx
                .index()
                .get_all_files()
                .into_iter()
                .find(|(_, id)| id == &doc_id_str)
                .map(|(p, _)| p);

            Json(DocumentResponse {
                document_id: doc_id_str,
                path,
            })
            .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Document not found".to_string(),
            }),
        )
            .into_response(),
        Err(_stopped) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Repository is stopped".to_string(),
            }),
        )
            .into_response(),
    }
}

/// Update a document (for testing)
///
/// This is a simple endpoint that puts a key-value pair into the document.
/// In a real implementation, the document schema would be more structured.
async fn update_document(
    State(ctx): State<SharedContext>,
    Path(doc_id_str): Path<String>,
    Json(request): Json<UpdateDocumentRequest>,
) -> impl IntoResponse {
    use automerge::{transaction::Transactable, ROOT};

    // Validate the document ID format
    let doc_id = match DocumentId::from_str(&doc_id_str) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid document ID format".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Try to find the document
    match ctx.repo().find(doc_id).await {
        Ok(Some(handle)) => {
            // Update the document
            let result = handle.with_document(|doc| {
                doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                    tx.put(ROOT, &request.key, &request.value)?;
                    Ok(())
                })
            });

            match result {
                Ok(_) => Json(serde_json::json!({
                    "status": "updated",
                    "document_id": doc_id_str,
                    "key": request.key,
                    "value": request.value
                }))
                .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Failed to update document: {:?}", e),
                    }),
                )
                    .into_response(),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Document not found".to_string(),
            }),
        )
            .into_response(),
        Err(_stopped) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Repository is stopped".to_string(),
            }),
        )
            .into_response(),
    }
}

/// 404 handler
async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "Not found")
}

/// WebSocket upgrade handler for automerge sync.
///
/// Clients connect here to sync documents in real-time.
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(ctx): State<SharedContext>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket(socket, ctx))
}

/// Handle an upgraded WebSocket connection.
async fn handle_websocket(socket: WebSocket, ctx: SharedContext) {
    // accept_axum returns immediately; the connection runs in the background
    match ctx.repo().accept_axum(socket) {
        Ok(connection) => {
            info!(peer_info = ?connection.info(), "WebSocket client connected");
            // The connection is managed by samod and stays alive until the WebSocket closes.
            // We can optionally wait for it to finish if we want to log disconnection:
            let reason = connection.finished().await;
            info!(peer_info = ?connection.info(), reason = ?reason, "WebSocket client disconnected");
        }
        Err(samod::Stopped) => {
            tracing::warn!("WebSocket rejected: repo is stopped");
        }
    }
}

/// Build the axum router
fn build_router(ctx: SharedContext) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/files", get(list_files))
        .route("/api/documents", get(list_documents))
        .route(
            "/api/documents/{id}",
            get(get_document).put(update_document),
        )
        // WebSocket endpoint for automerge sync
        .route("/ws", get(ws_handler))
        .fallback(not_found)
        .layer(TraceLayer::new_for_http())
        .with_state(ctx)
}

/// Run the hub server.
///
/// This function blocks until the server is shut down.
pub async fn run_server(storage: StorageManager, config: HubConfig) -> Result<()> {
    let addr = format!("{}:{}", config.host, config.port);

    // HubContext::new is now async (initializes samod repo)
    let ctx = Arc::new(HubContext::new(storage, config).await?);

    let router = build_router(ctx);

    let listener = TcpListener::bind(&addr).await?;
    info!(%addr, "Hub server listening");

    axum::serve(listener, router)
        .await
        .map_err(|e| crate::error::Error::Server(e.to_string()))?;

    Ok(())
}
