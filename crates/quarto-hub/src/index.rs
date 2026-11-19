//! Index document management
//!
//! The index document is a special automerge document that stores the mapping
//! from file paths (relative to project root) to automerge document IDs, plus
//! (V2+) a sidecar map of recorded engine captures keyed by the same paths.
//!
//! Structure:
//! ```text
//! ROOT
//! ├── files: Map<String, String>     // path -> document_id (bs58-encoded)
//! └── captures: Map<String, Map>     // path -> CaptureRef (V2+)
//!     where each CaptureRef map has:
//!       captureDocId: String       (required)
//!       staleness:    bool         (optional)
//!       state:        String       (optional — "idle" | "running" | "error")
//!       lastError:    String       (optional)
//! ```

use std::collections::HashMap;
use std::str::FromStr;

use automerge::{Automerge, ObjType, ROOT, ReadDoc, transaction::Transactable};
use samod::{DocHandle, DocumentId, Repo};
use tracing::{debug, info, warn};

use crate::error::{Error, Result};

/// Key in the ROOT map where file mappings are stored.
const FILES_KEY: &str = "files";

/// Key in the ROOT map where the capture sidecar lives (V2+).
const CAPTURES_KEY: &str = "captures";

/// Key in the ROOT map for the schema version.
const VERSION_KEY: &str = "version";

/// Key in the ROOT map for the identities map (V1+).
const IDENTITIES_KEY: &str = "identities";

/// Key in the ROOT map for the explicit-folder set (V3+).
const FOLDERS_KEY: &str = "folders";

/// Key in the ROOT map holding the git-epochs sidecar docId (V4+).
const EPOCHS_KEY: &str = "epochs";

/// Key in the ROOT map holding the originating folder's `ownerId` (V4+).
const OWNER_KEY: &str = "owner";

/// Key in the ROOT map holding the git repository identity (V4+).
const REPO_KEY: &str = "repo";

/// Current index schema version. Mirrors `CURRENT_SCHEMA_VERSION` in
/// `@quarto/quarto-automerge-schema`; the contract is
/// `claude-notes/designs/git-epoch-registry-contract.md`.
pub const CURRENT_SCHEMA_VERSION: i64 = 4;

/// The `repo` entry of a V4 index: the repository's root commit hashes and
/// the project's path inside the repository (empty for the repo root).
/// Mirrors `RepoIdentity` in `@quarto/quarto-automerge-schema`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoIdentity {
    pub roots: Vec<String>,
    pub prefix: String,
}

/// Server-side state of an engine capture, mirrored in the sidecar.
///
/// Stored as a short string in automerge so the SPA can read it without
/// a discriminant; serialized as JSON `"idle"`/`"running"`/`"error"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    Idle,
    Running,
    Error,
}

impl CaptureState {
    fn as_str(&self) -> &'static str {
        match self {
            CaptureState::Idle => "idle",
            CaptureState::Running => "running",
            CaptureState::Error => "error",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(CaptureState::Idle),
            "running" => Some(CaptureState::Running),
            "error" => Some(CaptureState::Error),
            _ => None,
        }
    }
}

/// Rust mirror of the TS `CaptureRef` shape in `@quarto/quarto-automerge-schema`.
///
/// Optional fields are omitted from the automerge map when None so V2 docs
/// without a recorded state remain forward-compatible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRef {
    pub capture_doc_id: String,
    pub staleness: Option<bool>,
    pub state: Option<CaptureState>,
    pub last_error: Option<String>,
}

/// Field names within a per-path capture sidecar entry. Kept in sync with
/// the TS `CaptureRef` interface in `@quarto/quarto-automerge-schema`.
mod capture_field {
    pub const CAPTURE_DOC_ID: &str = "captureDocId";
    pub const STALENESS: &str = "staleness";
    pub const STATE: &str = "state";
    pub const LAST_ERROR: &str = "lastError";
}

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
            // Version 4 content, the same four keys as the TypeScript
            // `createIndexDocumentContent()`: files, version, identities, folders.
            tx.put_object(ROOT, FILES_KEY, ObjType::Map)?;
            tx.put(ROOT, VERSION_KEY, CURRENT_SCHEMA_VERSION)?;
            tx.put_object(ROOT, IDENTITIES_KEY, ObjType::Map)?;
            tx.put_object(ROOT, FOLDERS_KEY, ObjType::Map)?;
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
                let keys: Vec<String> = doc.keys(&files_obj).collect();

                // Iterate over all keys in the map
                for key in keys {
                    if let Some(doc_id) = read_str_or_text(doc, &files_obj, &key) {
                        files.insert(key, doc_id);
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
            read_str_or_text(doc, &files_obj, path)
        })
    }

    /// The git-epochs sidecar docId (V4+), if a git-aware peer has set one.
    /// Reads both the scalar-string and the Text form.
    pub fn epochs_doc_id(&self) -> Option<String> {
        self.handle
            .with_document(|doc| read_str_or_text(doc, &ROOT, EPOCHS_KEY))
    }

    /// Point the index at the sidecar doc (written in the scalar form).
    pub fn set_epochs_doc_id(&self, doc_id: &str) -> Result<()> {
        self.put_root_str(EPOCHS_KEY, doc_id, "epochs")
    }

    /// The originating folder's `ownerId` (V4+); `None` for hub-owned projects.
    pub fn owner(&self) -> Option<String> {
        self.handle
            .with_document(|doc| read_str_or_text(doc, &ROOT, OWNER_KEY))
    }

    /// Set the owner id (written in the scalar form).
    pub fn set_owner(&self, owner_id: &str) -> Result<()> {
        self.put_root_str(OWNER_KEY, owner_id, "owner")
    }

    /// The git repository identity (V4+), if one was written. Returns `None`
    /// when the key is absent or malformed (no `prefix` string).
    pub fn repo_identity(&self) -> Option<RepoIdentity> {
        self.handle.with_document(|doc| {
            let (_, repo_obj) = doc.get(ROOT, REPO_KEY).ok().flatten()?;
            let prefix = read_str_or_text(doc, &repo_obj, "prefix")?;
            let (_, roots_obj) = doc.get(&repo_obj, "roots").ok().flatten()?;
            let mut roots = Vec::new();
            for i in 0..doc.length(&roots_obj) {
                let (value, id) = doc.get(&roots_obj, i).ok().flatten()?;
                if let Some(s) = value.as_str() {
                    roots.push(s.to_string());
                } else if matches!(value, automerge::Value::Object(ObjType::Text)) {
                    roots.push(doc.text(&id).ok()?);
                }
            }
            Some(RepoIdentity { roots, prefix })
        })
    }

    /// Write the `repo` entry (scalar strings). Replaces the entry's own
    /// `roots` and `prefix`; touches no other key.
    pub fn set_repo_identity(&self, identity: &RepoIdentity) -> Result<()> {
        self.handle.with_document(|doc| {
            doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                let repo_obj = tx.put_object(ROOT, REPO_KEY, ObjType::Map)?;
                let roots = tx.put_object(&repo_obj, "roots", ObjType::List)?;
                for (i, r) in identity.roots.iter().enumerate() {
                    tx.insert(&roots, i, r.as_str())?;
                }
                tx.put(&repo_obj, "prefix", identity.prefix.as_str())?;
                Ok(())
            })
            .map(|_| ())
            .map_err(|e| Error::IndexDocument(format!("failed to set repo: {:?}", e)))
        })
    }

    fn put_root_str(&self, key: &str, value: &str, what: &str) -> Result<()> {
        self.handle.with_document(|doc| {
            doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                tx.put(ROOT, key, value)?;
                Ok(())
            })
            .map(|_| ())
            .map_err(|e| Error::IndexDocument(format!("failed to set {}: {:?}", what, e)))
        })
    }

    /// Get the underlying DocHandle for advanced operations.
    pub fn handle(&self) -> &DocHandle {
        &self.handle
    }

    /// Read all capture sidecar entries from the index. Returns an empty
    /// map when the `captures` key is absent (V0/V1 docs that haven't
    /// been written-to since the V2 schema landed).
    pub fn get_all_captures(&self) -> HashMap<String, CaptureRef> {
        self.handle.with_document(|doc| {
            let mut out = HashMap::new();
            let Some((_, captures_obj)) = doc.get(ROOT, CAPTURES_KEY).ok().flatten() else {
                return out;
            };
            let paths: Vec<String> = doc.keys(&captures_obj).collect();
            for path in paths {
                if let Some((_, entry_obj)) = doc.get(&captures_obj, &path).ok().flatten()
                    && let Some(cap) = read_capture_entry(doc, &entry_obj)
                {
                    out.insert(path, cap);
                }
            }
            out
        })
    }

    /// Read the capture sidecar entry for a single path.
    pub fn get_capture(&self, path: &str) -> Option<CaptureRef> {
        self.handle.with_document(|doc| {
            let (_, captures_obj) = doc.get(ROOT, CAPTURES_KEY).ok().flatten()?;
            let (_, entry_obj) = doc.get(&captures_obj, path).ok().flatten()?;
            read_capture_entry(doc, &entry_obj)
        })
    }

    /// Test whether the sidecar has an entry for the given path.
    pub fn has_capture(&self, path: &str) -> bool {
        self.handle.with_document(|doc| {
            let Some((_, captures_obj)) = doc.get(ROOT, CAPTURES_KEY).ok().flatten() else {
                return false;
            };
            doc.get(&captures_obj, path).ok().flatten().is_some()
        })
    }

    /// Insert or overwrite a capture sidecar entry. Creates the `captures`
    /// map on the ROOT object on first use (lazy V2 migration of legacy
    /// docs — opening for read remains a no-op).
    pub fn set_capture(&self, path: &str, capture: &CaptureRef) -> Result<()> {
        self.handle.with_document(|doc| {
            doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                let captures_obj = match tx.get(ROOT, CAPTURES_KEY)? {
                    Some((_, obj)) => obj,
                    None => tx.put_object(ROOT, CAPTURES_KEY, ObjType::Map)?,
                };
                // Overwrite the entry: delete-then-create to avoid leaving
                // stale optional fields from a previous entry.
                if tx.get(&captures_obj, path)?.is_some() {
                    tx.delete(&captures_obj, path)?;
                }
                let entry_obj = tx.put_object(&captures_obj, path, ObjType::Map)?;
                tx.put(
                    &entry_obj,
                    capture_field::CAPTURE_DOC_ID,
                    capture.capture_doc_id.as_str(),
                )?;
                if let Some(stale) = capture.staleness {
                    tx.put(&entry_obj, capture_field::STALENESS, stale)?;
                }
                if let Some(state) = capture.state {
                    tx.put(&entry_obj, capture_field::STATE, state.as_str())?;
                }
                if let Some(err) = &capture.last_error {
                    tx.put(&entry_obj, capture_field::LAST_ERROR, err.as_str())?;
                }
                Ok(())
            })
            .map(|_| ())
            .map_err(|e| Error::IndexDocument(format!("failed to set capture: {:?}", e)))
        })
    }

    /// Delete a capture sidecar entry, if present.
    pub fn remove_capture(&self, path: &str) -> Result<()> {
        self.handle.with_document(|doc| {
            doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                if let Some((_, captures_obj)) = tx.get(ROOT, CAPTURES_KEY)?
                    && tx.get(&captures_obj, path)?.is_some()
                {
                    tx.delete(&captures_obj, path)?;
                }
                Ok(())
            })
            .map(|_| ())
            .map_err(|e| Error::IndexDocument(format!("failed to remove capture: {:?}", e)))
        })
    }
}

/// Read a string-valued map entry that may be stored as EITHER a scalar `Str`
/// OR an automerge `Text` object.
///
/// hub-client (`@automerge/automerge` 3.x, via `automerge-repo`) stores plain
/// string map-values — including the file-document ids in the `files` map — as
/// `Text` objects, not scalar strings (a `doc.files[path] = id` assignment
/// becomes a collaborative `Text`). A Rust reader using `Value::as_str()` alone
/// silently drops the `Text` form, so a project *created in hub-client* reads as
/// having **zero files** — which broke `q2 provide-hub` materialization
/// (bd-bm0vaetl). Rust-authored docs (project-mode hub) keep using the scalar
/// form; this accepts both. Same class of bug as the `metadata-as-str` lint.
fn read_str_or_text<D: ReadDoc>(doc: &D, obj: &automerge::ObjId, key: &str) -> Option<String> {
    let (value, id) = doc.get(obj, key).ok().flatten()?;
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if matches!(value, automerge::Value::Object(ObjType::Text)) {
        return doc.text(&id).ok();
    }
    None
}

/// Read a single CaptureRef out of an automerge entry map.
/// Returns None when the required `captureDocId` field is absent
/// (treated as a corrupt entry — caller may ignore or log).
fn read_capture_entry<D: ReadDoc>(doc: &D, entry_obj: &automerge::ObjId) -> Option<CaptureRef> {
    let (capture_doc_id_val, _) = doc
        .get(entry_obj, capture_field::CAPTURE_DOC_ID)
        .ok()
        .flatten()?;
    let capture_doc_id = capture_doc_id_val.as_str()?.to_string();

    let staleness = doc
        .get(entry_obj, capture_field::STALENESS)
        .ok()
        .flatten()
        .and_then(|(v, _)| v.as_bool());

    let state = doc
        .get(entry_obj, capture_field::STATE)
        .ok()
        .flatten()
        .and_then(|(v, _)| v.as_str().and_then(CaptureState::from_str));

    let last_error = doc
        .get(entry_obj, capture_field::LAST_ERROR)
        .ok()
        .flatten()
        .and_then(|(v, _)| v.as_str().map(|s| s.to_string()));

    Some(CaptureRef {
        capture_doc_id,
        staleness,
        state,
        last_error,
    })
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
    async fn get_all_files_reads_text_valued_ids() {
        // hub-client (@automerge/automerge 3.x) stores files[path] = docId as a
        // Text object, not a scalar string. get_all_files/get_file must read
        // both forms, or a project created in hub-client reads as zero files
        // and `q2 provide-hub` materializes nothing (bd-bm0vaetl).
        let repo = create_test_repo().await;

        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let files = tx.put_object(ROOT, FILES_KEY, ObjType::Map)?;
            // JS style: the id is a Text object.
            let t = tx.put_object(&files, "index.qmd", ObjType::Text)?;
            tx.update_text(&t, "doc-id-from-js")?;
            // Rust style: the id is a scalar string.
            tx.put(&files, "about.qmd", "doc-id-from-rust")?;
            Ok(())
        })
        .unwrap();
        let handle = repo.create(doc).await.unwrap();
        let doc_id = handle.document_id().to_string();
        let index = IndexDocument::load(&repo, &doc_id).await.unwrap().unwrap();

        let files = index.get_all_files();
        assert_eq!(files.len(), 2, "both the Text and scalar ids must be read");
        assert_eq!(files.get("index.qmd"), Some(&"doc-id-from-js".to_string()));
        assert_eq!(
            files.get("about.qmd"),
            Some(&"doc-id-from-rust".to_string())
        );

        assert_eq!(
            index.get_file("index.qmd"),
            Some("doc-id-from-js".to_string())
        );
        assert!(index.has_file("index.qmd"));
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
    async fn test_captures_initially_empty() {
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        let captures = index.get_all_captures();
        assert!(captures.is_empty());
        assert!(!index.has_capture("index.qmd"));
        assert_eq!(index.get_capture("index.qmd"), None);
    }

    #[tokio::test]
    async fn test_set_and_get_capture() {
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        let cap = CaptureRef {
            capture_doc_id: "cap-doc-1".to_string(),
            staleness: None,
            state: Some(CaptureState::Idle),
            last_error: None,
        };
        index.set_capture("index.qmd", &cap).unwrap();

        assert!(index.has_capture("index.qmd"));
        let got = index.get_capture("index.qmd").unwrap();
        assert_eq!(got.capture_doc_id, "cap-doc-1");
        assert_eq!(got.state, Some(CaptureState::Idle));
        assert_eq!(got.staleness, None);
        assert_eq!(got.last_error, None);
    }

    #[tokio::test]
    async fn test_set_capture_with_all_fields_roundtrips() {
        let repo = create_test_repo().await;
        let (index, doc_id) = IndexDocument::create(&repo).await.unwrap();

        let cap = CaptureRef {
            capture_doc_id: "cap-doc-2".to_string(),
            staleness: Some(true),
            state: Some(CaptureState::Error),
            last_error: Some("engine timed out".to_string()),
        };
        index.set_capture("posts/post1.qmd", &cap).unwrap();

        // Load via a fresh handle to verify it survives an automerge roundtrip
        let reloaded = IndexDocument::load(&repo, &doc_id).await.unwrap().unwrap();
        let got = reloaded.get_capture("posts/post1.qmd").unwrap();
        assert_eq!(got.capture_doc_id, "cap-doc-2");
        assert_eq!(got.staleness, Some(true));
        assert_eq!(got.state, Some(CaptureState::Error));
        assert_eq!(got.last_error, Some("engine timed out".to_string()));
    }

    #[tokio::test]
    async fn test_overwrite_capture() {
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        let cap1 = CaptureRef {
            capture_doc_id: "cap-doc-1".to_string(),
            staleness: None,
            state: Some(CaptureState::Running),
            last_error: None,
        };
        index.set_capture("index.qmd", &cap1).unwrap();

        let cap2 = CaptureRef {
            capture_doc_id: "cap-doc-2".to_string(),
            staleness: Some(false),
            state: Some(CaptureState::Idle),
            last_error: None,
        };
        index.set_capture("index.qmd", &cap2).unwrap();

        let got = index.get_capture("index.qmd").unwrap();
        assert_eq!(got.capture_doc_id, "cap-doc-2");
        assert_eq!(got.state, Some(CaptureState::Idle));
    }

    #[tokio::test]
    async fn test_remove_capture() {
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        let cap = CaptureRef {
            capture_doc_id: "cap-doc-1".to_string(),
            staleness: None,
            state: None,
            last_error: None,
        };
        index.set_capture("index.qmd", &cap).unwrap();
        assert!(index.has_capture("index.qmd"));

        index.remove_capture("index.qmd").unwrap();
        assert!(!index.has_capture("index.qmd"));
        assert_eq!(index.get_capture("index.qmd"), None);
    }

    #[tokio::test]
    async fn test_get_all_captures() {
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        let cap_a = CaptureRef {
            capture_doc_id: "cap-a".to_string(),
            staleness: None,
            state: None,
            last_error: None,
        };
        let cap_b = CaptureRef {
            capture_doc_id: "cap-b".to_string(),
            staleness: Some(true),
            state: Some(CaptureState::Running),
            last_error: None,
        };
        index.set_capture("a.qmd", &cap_a).unwrap();
        index.set_capture("b.qmd", &cap_b).unwrap();

        let all = index.get_all_captures();
        assert_eq!(all.len(), 2);
        assert_eq!(all["a.qmd"].capture_doc_id, "cap-a");
        assert_eq!(all["b.qmd"].capture_doc_id, "cap-b");
        assert_eq!(all["b.qmd"].state, Some(CaptureState::Running));
    }

    #[tokio::test]
    async fn test_files_and_captures_are_independent_maps() {
        // The sidecar (captures) is keyed by the same path as files
        // but the maps are independent — removing a file does not
        // automatically remove its capture entry, and vice versa.
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        index.add_file("index.qmd", "doc-id-1").unwrap();
        let cap = CaptureRef {
            capture_doc_id: "cap-1".to_string(),
            staleness: None,
            state: None,
            last_error: None,
        };
        index.set_capture("index.qmd", &cap).unwrap();

        index.remove_file("index.qmd").unwrap();
        assert!(!index.has_file("index.qmd"));
        // Capture sidecar entry is still present — caller is responsible
        // for cleaning it up alongside the file (tracked as a risk in
        // the Phase C plan §Risks #3).
        assert!(index.has_capture("index.qmd"));
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

    /// D8 audit: hub-server writes stay authorless.
    ///
    /// Server-authored changes (files map, capture sidecar) do not
    /// originate from a signed-in or locally identified user, and the
    /// author-ID transition deliberately mints no synthetic "hub" author
    /// (plan 2026-09-30-automerge-author-id-transition, D8). Every change
    /// the server writes must therefore decode with `author: null` —
    /// attribution readers fall back to the actor. Pinning that here
    /// makes a future `LoadOptions::author` / `set_author` on a server
    /// write path a deliberate, test-visible act.
    #[tokio::test]
    async fn server_writes_carry_no_author() {
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();

        // Drive every server write path on the index document: files
        // map (add/remove) and capture sidecar (set/remove).
        index.add_file("index.qmd", "doc-id-1").unwrap();
        index.add_file("chapters/intro.qmd", "doc-id-2").unwrap();
        let cap = CaptureRef {
            capture_doc_id: "cap-doc-1".to_string(),
            staleness: None,
            state: Some(CaptureState::Idle),
            last_error: None,
        };
        index.set_capture("index.qmd", &cap).unwrap();
        index.remove_file("chapters/intro.qmd").unwrap();
        index.remove_capture("index.qmd").unwrap();

        index.handle().with_document(|doc| {
            let changes = doc.get_changes(&[]);
            assert!(
                changes.len() >= 5,
                "expected the writes above to have produced changes, got {}",
                changes.len()
            );
            for change in &changes {
                assert!(
                    change.author().is_none(),
                    "server-written change must carry no author (seq {})",
                    change.seq(),
                );
            }
        });
    }

    // ---- Version 4 schema (git epochs, GE-1) -------------------------------

    /// Render an automerge value as JSON: maps and lists recursively, text
    /// objects and scalar strings both as strings, ints as numbers.
    fn value_json<D: ReadDoc>(doc: &D, obj: &automerge::ObjId) -> serde_json::Value {
        use serde_json::{Map, Value as J};
        match doc.object_type(obj).unwrap() {
            ObjType::Map | ObjType::Table => {
                let mut m = Map::new();
                let mut keys: Vec<String> = doc.keys(obj).collect();
                keys.sort();
                for k in keys {
                    let (v, id) = doc.get(obj, &k).unwrap().unwrap();
                    m.insert(k, one_json(doc, v, id));
                }
                J::Object(m)
            }
            ObjType::List => {
                let n = doc.length(obj);
                J::Array(
                    (0..n)
                        .map(|i| {
                            let (v, id) = doc.get(obj, i).unwrap().unwrap();
                            one_json(doc, v, id)
                        })
                        .collect(),
                )
            }
            ObjType::Text => J::String(doc.text(obj).unwrap()),
        }
    }

    fn one_json<D: ReadDoc>(
        doc: &D,
        v: automerge::Value<'_>,
        id: automerge::ObjId,
    ) -> serde_json::Value {
        use serde_json::Value as J;
        match v {
            automerge::Value::Object(_) => value_json(doc, &id),
            automerge::Value::Scalar(sc) => match sc.as_ref() {
                automerge::ScalarValue::Str(s) => J::String(s.to_string()),
                automerge::ScalarValue::Int(i) => J::from(*i),
                automerge::ScalarValue::Uint(u) => J::from(*u),
                other => panic!("unexpected scalar {other:?}"),
            },
        }
    }

    fn root_json(index: &IndexDocument) -> serde_json::Value {
        index.handle().with_document(|doc| value_json(doc, &ROOT))
    }

    fn golden_index_v4() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../ts-packages/quarto-git-epochs/fixtures/golden/index-v4.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn create_writes_the_version_4_content_of_the_golden_file() {
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();
        let json = root_json(&index);
        assert_eq!(
            json,
            golden_index_v4(),
            "Rust creation must equal createIndexDocumentContent()"
        );
        // The version is an Automerge int, not a float or a string.
        index.handle().with_document(|doc| {
            let (v, _) = doc.get(ROOT, "version").unwrap().unwrap();
            assert!(matches!(
                v,
                automerge::Value::Scalar(ref s) if matches!(s.as_ref(), automerge::ScalarValue::Int(4))
            ));
        });
    }

    /// A doc with the new keys (in the Rust scalar form) and an invented
    /// future key that this code has never heard of.
    async fn index_with_unknown_keys(repo: &Repo) -> IndexDocument {
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            tx.put_object(ROOT, FILES_KEY, ObjType::Map)?;
            tx.put(ROOT, "version", 4_i64)?;
            tx.put(ROOT, "epochs", "sidecar-doc")?;
            tx.put(ROOT, "owner", "owner-1")?;
            let repo_obj = tx.put_object(ROOT, "repo", ObjType::Map)?;
            let roots = tx.put_object(&repo_obj, "roots", ObjType::List)?;
            tx.insert(&roots, 0, "abc")?;
            tx.put(&repo_obj, "prefix", "docs")?;
            let fut = tx.put_object(ROOT, "future_key", ObjType::Map)?;
            tx.put(&fut, "nested", "value")?;
            tx.put(&fut, "n", 7_i64)?;
            Ok(())
        })
        .unwrap();
        let handle = repo.create(doc).await.unwrap();
        IndexDocument::load(repo, &handle.document_id().to_string())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn every_public_mutator_and_accessor_preserves_unknown_keys() {
        let repo = create_test_repo().await;
        let index = index_with_unknown_keys(&repo).await;
        let unknown = |j: &serde_json::Value| {
            let o = j.as_object().unwrap();
            ["version", "epochs", "owner", "repo", "future_key"]
                .iter()
                .map(|k| (k.to_string(), o.get(*k).cloned()))
                .collect::<Vec<_>>()
        };
        let before = unknown(&root_json(&index));

        // Accessors.
        let _ = index.document_id();
        let _ = index.get_all_files();
        let _ = index.has_file("a.qmd");
        let _ = index.get_file("a.qmd");
        let _ = index.get_all_captures();
        let _ = index.get_capture("a.qmd");
        let _ = index.has_capture("a.qmd");
        let _ = index.epochs_doc_id();
        let _ = index.owner();
        let _ = index.repo_identity();
        // Mutators.
        index.add_file("a.qmd", "doc-a").unwrap();
        index.add_file("b.qmd", "doc-b").unwrap();
        index.remove_file("b.qmd").unwrap();
        let cap = CaptureRef {
            capture_doc_id: "cap".to_string(),
            staleness: Some(false),
            state: Some(CaptureState::Idle),
            last_error: None,
        };
        index.set_capture("a.qmd", &cap).unwrap();
        index.remove_capture("a.qmd").unwrap();
        index.set_epochs_doc_id("sidecar-doc").unwrap();
        index.set_owner("owner-1").unwrap();
        index
            .set_repo_identity(&RepoIdentity {
                roots: vec!["abc".to_string()],
                prefix: "docs".to_string(),
            })
            .unwrap();

        assert_eq!(unknown(&root_json(&index)), before);
    }

    #[tokio::test]
    async fn epochs_owner_and_repo_read_in_both_string_forms() {
        let repo = create_test_repo().await;

        // JS form: every string is a Text object.
        let mut js = Automerge::new();
        js.transact::<_, _, automerge::AutomergeError>(|tx| {
            tx.put_object(ROOT, FILES_KEY, ObjType::Map)?;
            for (k, v) in [("epochs", "sidecar-doc"), ("owner", "owner-1")] {
                let t = tx.put_object(ROOT, k, ObjType::Text)?;
                tx.update_text(&t, v)?;
            }
            let r = tx.put_object(ROOT, "repo", ObjType::Map)?;
            let roots = tx.put_object(&r, "roots", ObjType::List)?;
            let t0 = tx.insert_object(&roots, 0, ObjType::Text)?;
            tx.update_text(&t0, "abc")?;
            let t1 = tx.insert_object(&roots, 1, ObjType::Text)?;
            tx.update_text(&t1, "def")?;
            let p = tx.put_object(&r, "prefix", ObjType::Text)?;
            tx.update_text(&p, "docs/site")?;
            Ok(())
        })
        .unwrap();
        let h = repo.create(js).await.unwrap();
        let js_index = IndexDocument::load(&repo, &h.document_id().to_string())
            .await
            .unwrap()
            .unwrap();

        // Rust form: scalar strings.
        let (rs_index, _) = IndexDocument::create(&repo).await.unwrap();
        rs_index.set_epochs_doc_id("sidecar-doc").unwrap();
        rs_index.set_owner("owner-1").unwrap();
        rs_index
            .set_repo_identity(&RepoIdentity {
                roots: vec!["abc".to_string(), "def".to_string()],
                prefix: "docs/site".to_string(),
            })
            .unwrap();

        for index in [&js_index, &rs_index] {
            assert_eq!(index.epochs_doc_id().as_deref(), Some("sidecar-doc"));
            assert_eq!(index.owner().as_deref(), Some("owner-1"));
            assert_eq!(
                index.repo_identity(),
                Some(RepoIdentity {
                    roots: vec!["abc".to_string(), "def".to_string()],
                    prefix: "docs/site".to_string(),
                })
            );
        }
    }

    #[tokio::test]
    async fn absent_new_keys_read_as_none() {
        let repo = create_test_repo().await;
        let (index, _) = IndexDocument::create(&repo).await.unwrap();
        assert_eq!(index.epochs_doc_id(), None);
        assert_eq!(index.owner(), None);
        assert_eq!(index.repo_identity(), None);
    }
}
