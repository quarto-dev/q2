use thiserror::Error;

#[derive(Debug, Error)]
pub enum HighlightError {
    #[error("tree-sitter query error: {0}")]
    Query(#[from] tree_sitter_highlight::Error),

    #[error("invalid highlight query file: {0}")]
    QueryParse(#[from] tree_sitter::QueryError),

    #[error("failed to serialize highlight spans to JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// A user-supplied grammar provider (native WasmStore, browser JS
    /// callback, etc.) failed to produce output for a class it claimed
    /// to handle. Carries a free-form message describing what went
    /// wrong — the provider implementation is responsible for making
    /// this diagnostic useful.
    #[error("user-grammar provider error: {0}")]
    Provider(String),
}
