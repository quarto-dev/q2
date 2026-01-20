//! LSP capability negotiation.

use tower_lsp::lsp_types::{
    OneOf, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions,
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
    fn capabilities_include_document_symbols() {
        let caps = server_capabilities();
        assert!(caps.document_symbol_provider.is_some());
    }
}
