//! Index document management
//!
//! The index document is a special automerge document that stores the mapping
//! from file paths (relative to project root) to automerge document IDs.
//!
//! Structure:
//! ```
//! ROOT
//! └── files: Map<String, String>  // path -> document_id (bs58-encoded)
//! ```

use std::collections::HashMap;
use std::str::FromStr;

use automerge::{transaction::Transactable, Automerge, ObjType, ReadDoc, ROOT};
use samod::{DocHandle, DocumentId, Repo};
use tracing::{debug, info, warn};

use crate::error::{Error, Result};

/// Key in the ROOT map where file mappings are stored.
const FILES_KEY: &str = "files";

/// Handle to the project index document.
///
/// This wraps a samod DocHandle and provides a typed interface for
/// managing the path -> document_id mapping.
pub struct IndexDocument {
    handle: DocHandle,
}

impl IndexDocument {
    /// Load an existing index document by ID.
    pub async fn load(repo: &Repo, doc_id_str: &str) -> Result<Option<Self>> {
        let doc_id = DocumentId::from_str(doc_id_str)
            .map_err(|e| Error::IndexDocument(format!("invalid document ID: {}", e)))?;

        match repo.find(doc_id).await {
            Ok(Some(handle)) => {
                info!(doc_id = %doc_id_str, "Loaded existing index document");
                Ok(Some(Self { handle }))
            }
            Ok(None) => {
                warn!(doc_id = %doc_id_str, "Index document not found in storage");
                Ok(None)
            }
            Err(_stopped) => Err(Error::IndexDocument("repo is stopped".to_string())),
        }
    }

    /// Create a new index document.
    ///
    /// Returns the new IndexDocument and its document ID as a string.
    pub async fn create(repo: &Repo) -> Result<(Self, String)> {
        // Create the initial document structure
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            // Create the files map at ROOT
            tx.put_object(ROOT, FILES_KEY, ObjType::Map)?;
            Ok(())
        })
        .map_err(|e| Error::IndexDocument(format!("failed to initialize index: {:?}", e)))?;

        // Create the document in the repo
        let handle = repo
            .create(doc)
            .await
            .map_err(|_stopped| Error::IndexDocument("repo is stopped".to_string()))?;

        let doc_id_str = handle.document_id().to_string();
        info!(doc_id = %doc_id_str, "Created new index document");

        Ok((Self { handle }, doc_id_str))
    }

    /// Get the document ID as a string (bs58-encoded).
    pub fn document_id(&self) -> String {
        self.handle.document_id().to_string()
    }

    /// Get all file mappings from the index.
    ///
    /// Returns a map from relative file paths to document IDs (as strings).
    pub fn get_all_files(&self) -> HashMap<String, String> {
        self.handle.with_document(|doc| {
            let mut files = HashMap::new();

            // Get the files map object
            if let Some((_, files_obj)) = doc.get(ROOT, FILES_KEY).ok().flatten() {
                // Collect keys first to avoid borrow issues
                let keys: Vec<String> = doc.keys(&files_obj).map(|k| k.to_string()).collect();

                // Iterate over all keys in the map
                for key in keys {
                    if let Some((value, _)) = doc.get(&files_obj, &key).ok().flatten() {
                        if let Some(doc_id) = value.to_str() {
                            files.insert(key, doc_id.to_string());
                        }
                    }
                }
            }

            files
        })
    }

    /// Add a file mapping to the index.
    ///
    /// - `path`: Relative path to the file (e.g., "chapters/intro.qmd")
    /// - `doc_id`: The document ID for this file (bs58-encoded string)
    pub fn add_file(&self, path: &str, doc_id: &str) -> Result<()> {
        self.handle.with_document(|doc| {
            doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                // Get or create the files map
                let files_obj = match tx.get(ROOT, FILES_KEY)? {
                    Some((_, obj)) => obj,
                    None => tx.put_object(ROOT, FILES_KEY, ObjType::Map)?,
                };

                // Add the mapping
                tx.put(files_obj, path, doc_id)?;
                Ok(())
            })
            .map(|_success| ()) // Extract () from Success<()>
            .map_err(|e| Error::IndexDocument(format!("failed to add file: {:?}", e)))
        })
    }

    /// Remove a file mapping from the index.
    pub fn remove_file(&self, path: &str) -> Result<()> {
        self.handle.with_document(|doc| {
            doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                if let Some((_, files_obj)) = tx.get(ROOT, FILES_KEY)? {
                    tx.delete(files_obj, path)?;
                }
                Ok(())
            })
            .map(|_success| ()) // Extract () from Success<()>
            .map_err(|e| Error::IndexDocument(format!("failed to remove file: {:?}", e)))
        })
    }

    /// Check if a file path exists in the index.
    pub fn has_file(&self, path: &str) -> bool {
        self.handle.with_document(|doc| {
            if let Some((_, files_obj)) = doc.get(ROOT, FILES_KEY).ok().flatten() {
                doc.get(files_obj, path).ok().flatten().is_some()
            } else {
                false
            }
        })
    }

    /// Get the document ID for a specific file path.
    pub fn get_file(&self, path: &str) -> Option<String> {
        self.handle.with_document(|doc| {
            let (_, files_obj) = doc.get(ROOT, FILES_KEY).ok().flatten()?;
            let (value, _) = doc.get(files_obj, path).ok().flatten()?;
            value.to_str().map(|s| s.to_string())
        })
    }

    /// Get the underlying DocHandle for advanced operations.
    pub fn handle(&self) -> &DocHandle {
        &self.handle
    }
}

/// Load or create the index document.
///
/// This is the main entry point for index document lifecycle management.
/// - If `doc_id_str` is Some, attempts to load the existing document
/// - If loading fails or `doc_id_str` is None, creates a new document
///
/// Returns the IndexDocument and optionally the new document ID if one was created.
pub async fn load_or_create_index(
    repo: &Repo,
    doc_id_str: Option<&str>,
) -> Result<(IndexDocument, Option<String>)> {
    // Try to load existing index if we have an ID
    if let Some(id) = doc_id_str {
        if let Some(index) = IndexDocument::load(repo, id).await? {
            debug!(doc_id = %id, "Using existing index document");
            return Ok((index, None));
        }
        // If load returned None, the document wasn't found - create a new one
        warn!(doc_id = %id, "Index document ID was set but document not found, creating new");
    }

    // Create a new index document
    let (index, new_doc_id) = IndexDocument::create(repo).await?;
    Ok((index, Some(new_doc_id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use samod::storage::InMemoryStorage;

    async fn create_test_repo() -> Repo {
        Repo::build_tokio()
            .with_storage(InMemoryStorage::new())
            .load()
            .await
    }

    #[tokio::test]
    async fn test_create_index_document() {
        let repo = create_test_repo().await;

        let (index, doc_id) = IndexDocument::create(&repo).await.unwrap();

        assert!(!doc_id.is_empty());
        assert_eq!(index.document_id(), doc_id);

        // Should have empty files map
        let files = index.get_all_files();
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn test_add_and_get_files() {
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        // Add some files
        index.add_file("index.qmd", "doc-id-1").unwrap();
        index.add_file("chapters/intro.qmd", "doc-id-2").unwrap();

        // Check they exist
        assert!(index.has_file("index.qmd"));
        assert!(index.has_file("chapters/intro.qmd"));
        assert!(!index.has_file("nonexistent.qmd"));

        // Get specific file
        assert_eq!(index.get_file("index.qmd"), Some("doc-id-1".to_string()));

        // Get all files
        let files = index.get_all_files();
        assert_eq!(files.len(), 2);
        assert_eq!(files.get("index.qmd"), Some(&"doc-id-1".to_string()));
        assert_eq!(
            files.get("chapters/intro.qmd"),
            Some(&"doc-id-2".to_string())
        );
    }

    #[tokio::test]
    async fn test_remove_file() {
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        index.add_file("index.qmd", "doc-id-1").unwrap();
        assert!(index.has_file("index.qmd"));

        index.remove_file("index.qmd").unwrap();
        assert!(!index.has_file("index.qmd"));
    }

    #[tokio::test]
    async fn test_load_existing_index() {
        let repo = create_test_repo().await;

        // Create an index and add some data
        let (index1, doc_id) = IndexDocument::create(&repo).await.unwrap();
        index1.add_file("test.qmd", "doc-123").unwrap();

        // Load it again by ID
        let index2 = IndexDocument::load(&repo, &doc_id).await.unwrap().unwrap();

        // Should have the same data
        assert_eq!(index2.get_file("test.qmd"), Some("doc-123".to_string()));
    }

    #[tokio::test]
    async fn test_load_or_create_new() {
        let repo = create_test_repo().await;

        let (index, new_id) = load_or_create_index(&repo, None).await.unwrap();

        // Should have created a new document
        assert!(new_id.is_some());
        assert_eq!(index.document_id(), new_id.unwrap());
    }

    #[tokio::test]
    async fn test_load_or_create_existing() {
        let repo = create_test_repo().await;

        // Create an index first
        let (index1, doc_id) = IndexDocument::create(&repo).await.unwrap();
        index1.add_file("existing.qmd", "existing-doc").unwrap();

        // Load it via load_or_create
        let (index2, new_id) = load_or_create_index(&repo, Some(&doc_id)).await.unwrap();

        // Should have loaded existing (no new ID)
        assert!(new_id.is_none());
        assert_eq!(
            index2.get_file("existing.qmd"),
            Some("existing-doc".to_string())
        );
    }
}
