//! The VFS miss routes (preview VFS contract D2, D7, D8).
//!
//! The browser render asks the filesystem three questions: read this
//! file, does this path exist, what is in this directory. When its VFS
//! cannot answer, it asks these routes. Each applies the exposure policy
//! (contract D4) before touching the disk:
//!
//! - `GET /api/preview/file?path=` returns the bytes and promotes the file
//!   into the Automerge project, so it is synced, watched and shared from
//!   then on.
//! - `GET /api/preview/stat?path=` answers `{exists, isFile, isDir}`.
//! - `GET /api/preview/list?path=` answers `{entries: [{name, kind}]}`,
//!   holding only names the policy admits.
//!
//! A refused path is `403` with the reason as text (for diagnostics; to the
//! render it is just "no"); an absent one is `404` (`stat` answers
//! `exists: false` instead). `path` is project-relative, with or without
//! the VFS's `/project/` prefix. Every directory a route answered for is
//! watched, so a file created there later is enrolled (contract D8).

use std::path::Path;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use quarto_hub::context::SharedContext;
use quarto_hub::exposure::Refusal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatResponse {
    exists: bool,
    is_file: bool,
    is_dir: bool,
}

#[derive(Debug, Serialize)]
struct ListEntry {
    name: String,
    kind: &'static str,
}

#[derive(Debug, Serialize)]
struct ListResponse {
    entries: Vec<ListEntry>,
}

fn refused(reason: &Refusal) -> Response {
    (StatusCode::FORBIDDEN, reason.to_string()).into_response()
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "no such file").into_response()
}

/// `GET /api/preview/file?path=`.
pub async fn file_handler(
    State(ctx): State<SharedContext>,
    Query(query): Query<PathQuery>,
) -> Response {
    let Some(policy) = ctx.exposure() else {
        return not_found();
    };
    let admitted = match policy.admit_pull(Path::new(&query.path)) {
        Ok(admitted) => admitted,
        Err(Refusal::NotFound) => return not_found(),
        Err(reason) => return refused(&reason),
    };
    let bytes = match tokio::fs::read(&admitted.abs).await {
        Ok(bytes) => bytes,
        Err(_) => return not_found(),
    };
    if let Err(e) = ctx.import_path(&admitted.rel).await {
        tracing::warn!(path = %admitted.rel.display(), error = %e, "Failed to promote a pulled file");
    }
    ctx.watch_file(&admitted.abs);
    ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response()
}

/// `GET /api/preview/stat?path=`.
pub async fn stat_handler(
    State(ctx): State<SharedContext>,
    Query(query): Query<PathQuery>,
) -> Response {
    let Some(policy) = ctx.exposure() else {
        return not_found();
    };
    let rel = match policy.normalize(Path::new(&query.path)) {
        Ok(rel) => rel,
        Err(reason) => return refused(&reason),
    };
    let answer = |exists, is_file, is_dir| {
        Json(StatResponse {
            exists,
            is_file,
            is_dir,
        })
        .into_response()
    };
    if policy.root().join(&rel).is_dir() {
        return match policy.admit_dir(&rel) {
            Ok((_, abs)) => {
                ctx.watch_dir(&abs);
                answer(true, false, true)
            }
            Err(reason) => refused(&reason),
        };
    }
    match policy.admit(&rel) {
        Ok(admitted) => {
            if let Some(dir) = admitted.abs.parent() {
                ctx.watch_dir(dir);
            }
            answer(true, true, false)
        }
        Err(Refusal::NotFound) => {
            // A miss is still an answer about the directory: watch it, so
            // the file is enrolled when it appears.
            if let Some(parent) = rel.parent()
                && let Ok((_, dir)) = policy.admit_dir(parent)
            {
                ctx.watch_dir(&dir);
            }
            answer(false, false, false)
        }
        Err(reason) => refused(&reason),
    }
}

/// `GET /api/preview/list?path=`.
pub async fn list_handler(
    State(ctx): State<SharedContext>,
    Query(query): Query<PathQuery>,
) -> Response {
    let Some(policy) = ctx.exposure() else {
        return not_found();
    };
    let (rel, abs) = match policy.admit_dir(Path::new(&query.path)) {
        Ok(found) => found,
        Err(Refusal::NotFound | Refusal::WrongType) => return not_found(),
        Err(reason) => return refused(&reason),
    };
    let Ok(read_dir) = std::fs::read_dir(&abs) else {
        return not_found();
    };
    let mut entries: Vec<ListEntry> = read_dir
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let child = rel.join(&name);
            if policy.admit_dir(&child).is_ok() {
                Some(ListEntry { name, kind: "dir" })
            } else if policy.admit(&child).is_ok() {
                Some(ListEntry { name, kind: "file" })
            } else {
                None
            }
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    ctx.watch_dir(&abs);
    Json(ListResponse { entries }).into_response()
}
