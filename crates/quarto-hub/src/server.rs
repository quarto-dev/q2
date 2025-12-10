//! HTTP server setup and routing

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{
        Path, State,
        ws::{WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use samod::DocumentId;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower_http::trace::TraceLayer;
use tracing::{debug, info};

use crate::context::{HubConfig, HubContext, SharedContext};
use crate::error::Result;
use crate::storage::StorageManager;
use crate::watch::{FileWatcher, WatchConfig, WatchEvent};

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
    use automerge::{ROOT, transaction::Transactable};

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
async fn ws_handler(ws: WebSocketUpgrade, State(ctx): State<SharedContext>) -> impl IntoResponse {
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
/// On shutdown (SIGTERM, SIGINT, or Ctrl-C), it performs a final filesystem sync
/// to ensure all automerge changes are written to disk.
///
/// If `sync_interval_secs` is configured, a background task will periodically
/// sync all documents to the filesystem for crash resilience.
pub async fn run_server(storage: StorageManager, config: HubConfig) -> Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    let sync_interval = config.sync_interval_secs;
    let watch_enabled = config.watch_enabled;
    let watch_debounce_ms = config.watch_debounce_ms;
    let project_root = storage.project_root().to_path_buf();

    // HubContext::new is now async (initializes samod repo and performs initial sync)
    let ctx = Arc::new(HubContext::new(storage, config).await?);
    let ctx_for_sync = ctx.clone();
    let ctx_for_watch = ctx.clone();
    let ctx_for_shutdown = ctx.clone();

    let router = build_router(ctx);

    let listener = TcpListener::bind(&addr).await?;
    info!(%addr, "Hub server listening");

    // Create shutdown signal channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Spawn task to listen for OS signals and trigger shutdown
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    // Spawn periodic sync task if interval is configured
    let periodic_sync_handle = if let Some(interval_secs) = sync_interval {
        let shutdown_rx = shutdown_rx.clone();
        info!(interval_secs = interval_secs, "Starting periodic sync task");
        Some(tokio::spawn(async move {
            run_periodic_sync(ctx_for_sync, interval_secs, shutdown_rx).await;
        }))
    } else {
        debug!("Periodic sync disabled");
        None
    };

    // Spawn file watcher task if enabled
    let watcher_handle = if watch_enabled {
        let shutdown_rx = shutdown_rx.clone();
        let watch_config = WatchConfig {
            debounce_ms: watch_debounce_ms,
        };
        match FileWatcher::new(&project_root, watch_config) {
            Ok(watcher) => {
                info!("Starting filesystem watcher");
                Some(tokio::spawn(async move {
                    run_file_watcher(ctx_for_watch, watcher, shutdown_rx).await;
                }))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to start filesystem watcher, continuing without it");
                None
            }
        }
    } else {
        debug!("Filesystem watcher disabled");
        None
    };

    // Run server with graceful shutdown
    let mut shutdown_rx_server = shutdown_rx.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            // Wait until shutdown is signaled
            let _ = shutdown_rx_server.wait_for(|&v| v).await;
            info!("Server shutting down...");
        })
        .await
        .map_err(|e| crate::error::Error::Server(e.to_string()))?;

    // Wait for background tasks to finish
    if let Some(handle) = periodic_sync_handle {
        debug!("Waiting for periodic sync task to finish...");
        let _ = handle.await;
    }
    if let Some(handle) = watcher_handle {
        debug!("Waiting for file watcher task to finish...");
        let _ = handle.await;
    }

    // Perform final sync on shutdown
    info!("Performing final filesystem sync before shutdown...");
    let sync_result = ctx_for_shutdown.sync_all().await;
    info!(
        synced = sync_result.total_synced(),
        errors = sync_result.errors.len(),
        "Final filesystem sync complete"
    );

    Ok(())
}

/// Run periodic filesystem sync in a background task.
///
/// This task runs until the shutdown signal is received, syncing all documents
/// to the filesystem at the specified interval.
async fn run_periodic_sync(
    ctx: Arc<HubContext>,
    interval_secs: u64,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

    // First tick completes immediately; we don't want to sync right after startup
    // since we just did an initial sync, so skip it
    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                debug!("Running periodic filesystem sync...");
                let result = ctx.sync_all().await;
                if result.total_synced() > 0 || result.has_errors() {
                    info!(
                        synced = result.total_synced(),
                        no_changes = result.no_changes,
                        automerge_changed = result.automerge_changed,
                        filesystem_changed = result.filesystem_changed,
                        both_changed = result.both_changed,
                        errors = result.errors.len(),
                        "Periodic sync complete"
                    );
                } else {
                    debug!("Periodic sync: no changes");
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    debug!("Periodic sync task shutting down");
                    break;
                }
            }
        }
    }
}

/// Run the filesystem watcher in a background task.
///
/// This task receives events from the file watcher and syncs changed files
/// until the shutdown signal is received.
async fn run_file_watcher(
    ctx: Arc<HubContext>,
    mut watcher: FileWatcher,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            event = watcher.recv() => {
                match event {
                    Some(WatchEvent::Modified(path)) => {
                        debug!(path = %path.display(), "File change detected, syncing...");
                        match ctx.sync_file(&path).await {
                            Ok(Some(result)) => {
                                debug!(
                                    path = %path.display(),
                                    result = ?result,
                                    "File synced successfully"
                                );
                            }
                            Ok(None) => {
                                debug!(path = %path.display(), "File not in index, skipping");
                            }
                            Err(e) => {
                                tracing::warn!(
                                    path = %path.display(),
                                    error = %e,
                                    "Failed to sync file"
                                );
                            }
                        }
                    }
                    None => {
                        // Watcher stopped
                        debug!("File watcher stopped");
                        break;
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    debug!("File watcher task shutting down");
                    break;
                }
            }
        }
    }
}

/// Wait for shutdown signals (Ctrl-C, SIGTERM, SIGINT).
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl-C, initiating graceful shutdown...");
        }
        _ = terminate => {
            info!("Received SIGTERM, initiating graceful shutdown...");
        }
    }
}
