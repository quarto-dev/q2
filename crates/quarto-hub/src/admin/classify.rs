//! Document classification and reference extraction for `hub admin`
//! (bd-eiku4ymo).
//!
//! Pure functions over loaded [`Automerge`] documents — no storage,
//! no repo — so the safety-critical logic (what a doc *is*, and what
//! it *references*) is unit-testable in isolation.
//!
//! Classification is by ROOT shape, mirroring the schemas in
//! `crate::index` (project index), `ts-packages/quarto-automerge-schema`
//! (project set), and `crate::resource` (text/binary docs):
//!
//! | kind            | ROOT signature                                |
//! |-----------------|-----------------------------------------------|
//! | `ProjectIndex`  | `files` map (V2 adds a `captures` sidecar)    |
//! | `ProjectSet`    | `projects` map                                |
//! | `TextFile`      | `text`                                        |
//! | `EngineCapture` | `content` + `mimeType == CAPTURE_MIME_TYPE`   |
//! | `BinaryFile`    | `content` (any other MIME)                    |
//! | `Unknown`       | anything else                                 |
//!
//! **`Unknown` is load-bearing for safety**: a doc written by a
//! future schema this scanner doesn't know classifies as `Unknown`
//! and is therefore never collectible (the collector's allowlist
//! admits only `EngineCapture`). Never "improve" classification by
//! guessing.

use automerge::{Automerge, ObjType, ROOT, ReadDoc};

use crate::resource::CAPTURE_MIME_TYPE;

/// What a stored automerge document is, by ROOT shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocKind {
    /// A project index (`crate::index::IndexDocument`). Always a
    /// liveness root.
    ProjectIndex,
    /// A collections/projects-home set document. Always a liveness
    /// root (its only inbound pointers live in client IndexedDB).
    ProjectSet,
    /// A text file document.
    TextFile,
    /// An engine-capture binary doc — the only collectible kind.
    EngineCapture,
    /// Any other binary doc (images, PDFs, fonts, …).
    BinaryFile,
    /// Unrecognized shape. Never collectible; reported for audit.
    Unknown,
}

impl DocKind {
    /// Stable string for manifests and human summaries.
    pub fn as_str(&self) -> &'static str {
        match self {
            DocKind::ProjectIndex => "project-index",
            DocKind::ProjectSet => "project-set",
            DocKind::TextFile => "text-file",
            DocKind::EngineCapture => "engine-capture",
            DocKind::BinaryFile => "binary-file",
            DocKind::Unknown => "unknown",
        }
    }

    /// Liveness roots: docs whose inbound pointers live outside the
    /// store (share URLs, client IndexedDB), so they must be treated
    /// as always-reachable.
    pub fn is_root(&self) -> bool {
        matches!(self, DocKind::ProjectIndex | DocKind::ProjectSet)
    }
}

/// Classify a document by its ROOT shape. Total: every doc gets a
/// kind, unrecognized shapes get [`DocKind::Unknown`].
pub fn classify(doc: &Automerge) -> DocKind {
    let has = |key: &str| doc.get(ROOT, key).ok().flatten().is_some();

    if has("files") {
        return DocKind::ProjectIndex;
    }
    if has("projects") {
        return DocKind::ProjectSet;
    }
    if has("text") {
        return DocKind::TextFile;
    }
    if has("content") {
        let mime = doc
            .get(ROOT, "mimeType")
            .ok()
            .flatten()
            .and_then(|(value, _)| value.to_str().map(str::to_string));
        return if mime.as_deref() == Some(CAPTURE_MIME_TYPE) {
            DocKind::EngineCapture
        } else {
            DocKind::BinaryFile
        };
    }
    DocKind::Unknown
}

/// Extract every document id this document references — the edges of
/// the liveness graph.
///
/// - `ProjectIndex`: values of the `files` map (path → docId) and the
///   `captureDocId` of every `captures` sidecar entry.
/// - `ProjectSet`: keys of the `projects` map (indexDocIds), plus each
///   entry's `indexDocId` field defensively (schema says they match).
/// - Everything else references nothing.
///
/// Ids are normalized by stripping an `automerge:` prefix when
/// present (the TS schema stores keys without it, but defensiveness
/// is cheap and a missed reference means an unsafe collection).
pub fn referenced_doc_ids(doc: &Automerge) -> Vec<String> {
    let mut out = Vec::new();
    match classify(doc) {
        DocKind::ProjectIndex => {
            if let Some((_, files_obj)) = doc.get(ROOT, "files").ok().flatten() {
                let keys: Vec<String> = doc.keys(&files_obj).collect();
                for key in keys {
                    if let Some(id) = read_str(doc, &files_obj, &key) {
                        out.push(normalize_id(&id));
                    }
                }
            }
            if let Some((_, caps_obj)) = doc.get(ROOT, "captures").ok().flatten() {
                let keys: Vec<String> = doc.keys(&caps_obj).collect();
                for key in keys {
                    if let Some((value, entry_obj)) = doc.get(&caps_obj, &key).ok().flatten()
                        && matches!(value, automerge::Value::Object(ObjType::Map))
                        && let Some(id) = read_str(doc, &entry_obj, "captureDocId")
                    {
                        out.push(normalize_id(&id));
                    }
                }
            }
        }
        DocKind::ProjectSet => {
            if let Some((_, projects_obj)) = doc.get(ROOT, "projects").ok().flatten() {
                let keys: Vec<String> = doc.keys(&projects_obj).collect();
                for key in keys {
                    out.push(normalize_id(&key));
                    if let Some((value, entry_obj)) = doc.get(&projects_obj, &key).ok().flatten()
                        && matches!(value, automerge::Value::Object(ObjType::Map))
                        && let Some(id) = read_str(doc, &entry_obj, "indexDocId")
                    {
                        out.push(normalize_id(&id));
                    }
                }
            }
        }
        _ => {}
    }
    out.sort();
    out.dedup();
    out
}

fn read_str(doc: &Automerge, obj: &automerge::ObjId, key: &str) -> Option<String> {
    let (value, _) = doc.get(obj, key).ok().flatten()?;
    value.to_str().map(str::to_string)
}

fn normalize_id(id: &str) -> String {
    id.strip_prefix("automerge:").unwrap_or(id).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::transaction::Transactable;

    use crate::resource::{CaptureDocMeta, create_binary_document, create_capture_document};

    fn index_doc(files: &[(&str, &str)], captures: &[(&str, &str)]) -> Automerge {
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let files_obj = tx.put_object(ROOT, "files", ObjType::Map)?;
            for (path, id) in files {
                tx.put(&files_obj, *path, *id)?;
            }
            if !captures.is_empty() {
                let caps_obj = tx.put_object(ROOT, "captures", ObjType::Map)?;
                for (path, id) in captures {
                    let entry = tx.put_object(&caps_obj, *path, ObjType::Map)?;
                    tx.put(&entry, "captureDocId", *id)?;
                    tx.put(&entry, "staleness", false)?;
                }
            }
            Ok(())
        })
        .unwrap();
        doc
    }

    fn project_set_doc(entries: &[(&str, &str)]) -> Automerge {
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let projects_obj = tx.put_object(ROOT, "projects", ObjType::Map)?;
            for (key, index_id) in entries {
                let entry = tx.put_object(&projects_obj, *key, ObjType::Map)?;
                tx.put(&entry, "indexDocId", *index_id)?;
                tx.put(&entry, "name", "A project")?;
            }
            Ok(())
        })
        .unwrap();
        doc
    }

    fn text_doc() -> Automerge {
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let text_obj = tx.put_object(ROOT, "text", ObjType::Text)?;
            tx.update_text(&text_obj, "hello")?;
            Ok(())
        })
        .unwrap();
        doc
    }

    fn capture_doc() -> Automerge {
        create_capture_document(
            b"gz",
            &CaptureDocMeta {
                source_path: "a.qmd".into(),
                engines: vec!["knitr".into()],
            },
        )
        .unwrap()
    }

    #[test]
    fn classifies_every_known_kind() {
        assert_eq!(classify(&index_doc(&[], &[])), DocKind::ProjectIndex);
        assert_eq!(classify(&project_set_doc(&[])), DocKind::ProjectSet);
        assert_eq!(classify(&text_doc()), DocKind::TextFile);
        assert_eq!(classify(&capture_doc()), DocKind::EngineCapture);
        assert_eq!(
            classify(&create_binary_document(b"png", "image/png").unwrap()),
            DocKind::BinaryFile
        );
        assert_eq!(classify(&Automerge::new()), DocKind::Unknown);
    }

    #[test]
    fn legacy_capture_without_meta_still_classifies_by_mime() {
        // Pre-envelope capture docs carry only the MIME discriminator.
        let doc = create_binary_document(b"gz", CAPTURE_MIME_TYPE).unwrap();
        assert_eq!(classify(&doc), DocKind::EngineCapture);
    }

    #[test]
    fn unknown_shape_is_never_a_root_and_never_collectible_kind() {
        // A future schema (here: a doc with only a `widgets` map) must
        // classify as Unknown — the collector's allowlist then protects
        // it automatically.
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            tx.put_object(ROOT, "widgets", ObjType::Map)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(classify(&doc), DocKind::Unknown);
        assert!(!classify(&doc).is_root());
    }

    #[test]
    fn roots_are_index_and_project_set() {
        assert!(DocKind::ProjectIndex.is_root());
        assert!(DocKind::ProjectSet.is_root());
        assert!(!DocKind::TextFile.is_root());
        assert!(!DocKind::EngineCapture.is_root());
        assert!(!DocKind::BinaryFile.is_root());
    }

    #[test]
    fn index_references_files_and_capture_sidecar() {
        let doc = index_doc(
            &[("a.qmd", "docA"), ("img.png", "docB")],
            &[("a.qmd", "capA")],
        );
        assert_eq!(referenced_doc_ids(&doc), vec!["capA", "docA", "docB"]);
    }

    #[test]
    fn project_set_references_index_ids_from_keys_and_entries() {
        // Entry value disagreeing with its key: both are collected
        // (defensive — a missed reference is an unsafe collection).
        let doc = project_set_doc(&[("idx1", "idx1"), ("idx2", "automerge:idx2b")]);
        assert_eq!(referenced_doc_ids(&doc), vec!["idx1", "idx2", "idx2b"]);
    }

    #[test]
    fn leaf_docs_reference_nothing() {
        assert!(referenced_doc_ids(&text_doc()).is_empty());
        assert!(referenced_doc_ids(&capture_doc()).is_empty());
        assert!(referenced_doc_ids(&Automerge::new()).is_empty());
    }
}
