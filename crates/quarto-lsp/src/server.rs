//! LSP server implementation using tower-lsp.

use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use quarto_lsp_core::document::DocumentStore;

use crate::capabilities::server_capabilities;
use crate::convert;

/// The Quarto language server.
pub struct QuartoLanguageServer {
    /// The LSP client for sending notifications.
    client: Client,
    /// Document store for managing open documents.
    documents: Arc<RwLock<DocumentStore>>,
}

impl QuartoLanguageServer {
    /// Create a new language server instance.
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(DocumentStore::new())),
        }
    }

    /// Publish diagnostics for a document.
    async fn publish_diagnostics(&self, uri: Url) {
        let documents = self.documents.read().await;
        let uri_str = uri.as_str();

        if let Some(doc) = documents.get(uri_str) {
            // Get diagnostics from quarto-lsp-core
            let result = quarto_lsp_core::get_diagnostics(doc);

            // Convert to LSP diagnostics
            let diagnostics: Vec<Diagnostic> = result
                .diagnostics
                .iter()
                .map(convert::diagnostic_to_lsp)
                .collect();

            // Publish diagnostics
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for QuartoLanguageServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(ServerInfo {
                name: "quarto-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Quarto LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let text = params.text_document.text;
        let version = params.text_document.version;

        {
            let mut documents = self.documents.write().await;
            documents.open(uri.as_str(), text, version);
        }

        // Publish diagnostics for the opened document
        self.publish_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let version = params.text_document.version;

        // We're using full document sync, so take the last change
        if let Some(change) = params.content_changes.into_iter().last() {
            {
                let mut documents = self.documents.write().await;
                documents.change(uri.as_str(), change.text, version);
            }

            // Publish diagnostics for the changed document
            self.publish_diagnostics(uri).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;

        {
            let mut documents = self.documents.write().await;
            documents.close(uri.as_str());
        }

        // Clear diagnostics for closed document
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let documents = self.documents.read().await;

        if let Some(doc) = documents.get(uri.as_str()) {
            let symbols = quarto_lsp_core::get_symbols(doc);
            let lsp_symbols: Vec<DocumentSymbol> =
                symbols.iter().map(convert::symbol_to_lsp).collect();
            Ok(Some(DocumentSymbolResponse::Nested(lsp_symbols)))
        } else {
            Ok(None)
        }
    }
}

/// Run the LSP server over stdio.
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(QuartoLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
