//! LSP capability negotiation.

use lsp_types::{
    OneOf, SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensServerCapabilities,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
};

/// Get the server capabilities to report to the client.
pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        // Text document synchronization
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                // We want to know when documents are opened/closed
                open_close: Some(true),
                // Full document sync for now (simpler implementation)
                // Can be changed to Incremental later for better performance
                change: Some(TextDocumentSyncKind::FULL),
                // We don't need will_save notifications
                will_save: None,
                will_save_wait_until: None,
                // We don't need save notifications for now
                save: None,
            },
        )),

        // Document symbols (outline)
        document_symbol_provider: Some(OneOf::Left(true)),

        // Quarto 2's own highlighting (bd-xnpyb2vv P1). Full documents
        // only; range and delta wait for measurements on large documents.
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: crate::semantic_tokens::legend(),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                range: None,
                ..Default::default()
            },
        )),

        // Protocol extensions (epic bd-3f6ijnw9, C4): each `quarto/*`
        // method with the version of its wire schema. Clients check this
        // before sending them.
        experimental: Some(serde_json::json!({
            "quarto": {
                "documentProfile": quarto_lsp_project::wire::WIRE_SCHEMA_VERSION,
                "documentMetadata": quarto_lsp_project::wire::WIRE_SCHEMA_VERSION,
            }
        })),

        // Features to be added in future phases:
        // hover_provider: Some(HoverProviderCapability::Simple(true)),
        // completion_provider: Some(CompletionOptions { ... }),
        // definition_provider: Some(OneOf::Left(true)),
        // references_provider: Some(OneOf::Left(true)),
        // document_formatting_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_include_document_sync() {
        let caps = server_capabilities();
        assert!(caps.text_document_sync.is_some());
    }

    #[test]
    fn capabilities_include_semantic_tokens() {
        let caps = server_capabilities();
        let Some(SemanticTokensServerCapabilities::SemanticTokensOptions(opts)) =
            caps.semantic_tokens_provider
        else {
            panic!("no semantic tokens provider");
        };
        assert!(!opts.legend.token_types.is_empty());
        assert_eq!(opts.full, Some(SemanticTokensFullOptions::Bool(true)));
    }

    #[test]
    fn capabilities_include_document_symbols() {
        let caps = server_capabilities();
        assert!(caps.document_symbol_provider.is_some());
    }
}
