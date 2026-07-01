//! Materialize a hub project's automerge VFS into a real on-disk directory
//! (bd-sfet3264, Phase 4a).
//!
//! Native engines (knitr/jupyter) read files from the filesystem, so before we
//! can run a document we copy the whole project out of automerge into a fresh
//! temp dir. Per the Phase 3 decision this is **read-only and per-run**: we
//! never write back, and each execution starts from a clean tree. (The hub's
//! own `sync_all_documents` is bidirectional and sync-state-heavy; this is a
//! lean one-way reader reusing the `resource::*` primitives.)
//!
//! Text file docs store a `text` Automerge `Text` object; binary file docs
//! store a `content` bytes field (`quarto_hub::resource`). We detect the type
//! and write the raw bytes under `<dest>/<path>`.

use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use automerge::{Automerge, ROOT, ReadDoc};
use quarto_hub::index::IndexDocument;
use quarto_hub::resource::{DocumentType, detect_document_type, read_binary_content};
use samod::{DocumentId, Repo};
use tracing::warn;

use crate::ProviderError;

/// Copy every file tracked in `index` out of the automerge repo and into
/// `dest`, preserving relative paths. Returns the number of files written.
///
/// Files whose document can't be found or whose content can't be read are
/// **skipped with a warning** rather than failing the whole run — a single
/// unreadable asset shouldn't block executing the document the user asked for.
/// Paths that would escape `dest` (absolute, or containing `..`) are rejected.
pub async fn materialize_project(
    repo: &Repo,
    index: &IndexDocument,
    dest: &Path,
) -> Result<usize, ProviderError> {
    let mut written = 0usize;
    for (rel_path, doc_id_str) in index.get_all_files() {
        let Some(target) = safe_join(dest, &rel_path) else {
            warn!(path = %rel_path, "skipping file with an unsafe path during materialization");
            continue;
        };

        let doc_id = match DocumentId::from_str(&doc_id_str) {
            Ok(id) => id,
            Err(e) => {
                warn!(path = %rel_path, error = %e, "skipping file with an invalid document id");
                continue;
            }
        };

        let handle = repo
            .find(doc_id)
            .await
            .map_err(|_| ProviderError::Repo("repo is stopped".into()))?;
        let Some(handle) = handle else {
            warn!(path = %rel_path, "skipping file whose document was not found on the hub");
            continue;
        };

        let bytes = handle.with_document(|doc| read_file_bytes(doc));
        let Some(bytes) = bytes else {
            warn!(path = %rel_path, "skipping file with unreadable content");
            continue;
        };

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ProviderError::Protocol(format!("creating {}: {e}", parent.display()))
            })?;
        }
        std::fs::write(&target, &bytes)
            .map_err(|e| ProviderError::Protocol(format!("writing {}: {e}", target.display())))?;
        written += 1;
    }
    Ok(written)
}

/// Read a file document's bytes: the hydrated UTF-8 of a text doc, or the raw
/// `content` bytes of a binary doc. Returns `None` for an unrecognized shape.
fn read_file_bytes(doc: &Automerge) -> Option<Vec<u8>> {
    match detect_document_type(doc) {
        DocumentType::Text => {
            let (_, text_obj) = doc.get(ROOT, "text").ok().flatten()?;
            let text = doc.text(&text_obj).ok()?;
            Some(text.into_bytes())
        }
        DocumentType::Binary => read_binary_content(doc),
        DocumentType::Invalid => None,
    }
}

/// Join `rel` under `base`, rejecting anything that would escape it (absolute
/// paths, `..` components, or Windows drive prefixes). Returns `None` when the
/// path is unsafe. Cross-platform: uses `Path::components` rather than string
/// separators.
fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    let rel_path = Path::new(rel);
    let mut out = base.to_path_buf();
    for component in rel_path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            // Reject anything non-relative or upward-traversing.
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
            // A leading `./` is harmless.
            Component::CurDir => {}
        }
    }
    // Guard against a path that normalized to just `base` (e.g. "" or ".").
    if out == base { None } else { Some(out) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_accepts_nested_relative_paths() {
        let base = Path::new("/tmp/x");
        assert_eq!(
            safe_join(base, "chapters/intro.qmd"),
            Some(PathBuf::from("/tmp/x/chapters/intro.qmd"))
        );
        assert_eq!(
            safe_join(base, "./a.qmd"),
            Some(PathBuf::from("/tmp/x/a.qmd"))
        );
    }

    #[test]
    fn safe_join_rejects_traversal_and_absolute() {
        let base = Path::new("/tmp/x");
        assert_eq!(safe_join(base, "../escape.qmd"), None);
        assert_eq!(safe_join(base, "a/../../escape.qmd"), None);
        assert_eq!(safe_join(base, "/etc/passwd"), None);
        assert_eq!(safe_join(base, ""), None);
        assert_eq!(safe_join(base, "."), None);
    }
}
