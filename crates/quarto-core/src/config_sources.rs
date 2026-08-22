/*
 * config_sources.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Bind the correct config-source file to a diagnostic's FileId
 * (bd-m6wmztln).
 */

//! Bind the *correct* config-source file to a diagnostic's FileId
//! (bd-m6wmztln).
//!
//! Project-level configuration is assembled from more than one YAML
//! file: the user's `_quarto.yml` plus every discovered extension's
//! `_extension.yml` (`contributes.metadata.project` /
//! `contributes.project`, merged in
//! [`crate::project`]'s `apply_metadata_project_contributions` and
//! `resolve_project_type`). A merged value's
//! [`SourceInfo`] keeps pointing into the file it was *written* in,
//! via quarto-yaml's filename-hash FileId scheme
//! ([`quarto_yaml::file_id_for_filename`]).
//!
//! A diagnostic site that resolves such a SourceInfo must therefore
//! not assume the offsets belong to `_quarto.yml`: registering the
//! wrong file's content under the FileId renders the right offsets
//! against the wrong text — a confidently wrong ariadne span when the
//! offsets fit inside the file, a silently dropped snippet when they
//! don't. See
//! `claude-notes/plans/2026-08-09-q58-extension-script-diagnostic-span.md`.
//!
//! [`bind_config_source`] is the shared correct pattern (precedent:
//! [`crate::theme_diagnostic::sass_error_to_parse_error`]): re-derive
//! each candidate file's FileId from its path and register only the
//! matching one. bd-nv4p0eb1 tracks auditing the remaining ad-hoc
//! sites tree-wide and hardening the API so the wrong pairing is
//! unrepresentable.

use std::path::Path;

use quarto_source_map::{FileId, SourceContext, SourceInfo};

/// Register in `source_context` the candidate file that `info`'s
/// resolved FileId actually refers to, and return it.
///
/// Each candidate's FileId is re-derived from its path with
/// [`quarto_yaml::file_id_for_filename`] — the same hash
/// `quarto_yaml::parse_file` used when the file was parsed — so a
/// match means the SourceInfo's byte offsets are offsets *into that
/// file*. The matched file is registered only when its content can be
/// read (a `SourceFile` without content makes the renderer re-read
/// from disk and fail on absence); an unreadable match is still
/// returned so callers can attribute the diagnostic in prose.
///
/// Returns `None` — registering nothing, so the diagnostic degrades
/// to a span-less render — when the SourceInfo has no resolvable byte
/// range (generated/synthetic provenance) or no candidate matches
/// (e.g. the value came from a file the caller doesn't know about).
/// Never binds a non-matching file: a wrong span is strictly worse
/// than no span.
pub fn bind_config_source<'a>(
    source_context: &mut SourceContext,
    info: &SourceInfo,
    candidates: impl IntoIterator<Item = &'a Path>,
) -> Option<&'a Path> {
    bind_source_candidates(
        source_context,
        info,
        candidates.into_iter().map(|p| {
            let fid = quarto_yaml::file_id_for_filename(&p.to_string_lossy());
            (fid, p)
        }),
    )
}

/// Generalization of [`bind_config_source`] for candidate lists that
/// span **both** FileId schemes: quarto-yaml filename-hash ids for
/// standalone config files, and dense parse-context ids for documents
/// (a document's front-matter spans root at the `FileId` its own
/// parse context assigned — `FileId(0)` for the primary slot — not at
/// a filename hash). The caller supplies each candidate as an
/// explicit `(FileId, &Path)` pair; the first pair whose id equals
/// `info`'s resolved id is registered (content permitting) and
/// returned. Same never-bind-a-non-match contract as
/// [`bind_config_source`]. Precedent:
/// `theme_diagnostic::sass_error_to_parse_error`'s candidate list.
pub fn bind_source_candidates<'a>(
    source_context: &mut SourceContext,
    info: &SourceInfo,
    candidates: impl IntoIterator<Item = (FileId, &'a Path)>,
) -> Option<&'a Path> {
    // `root_file_id()`, not `resolve_byte_range()`: this function only
    // ever wants the id (the discarded `_, _` above used to hide that),
    // and `root_file_id()` resolves it for `Concat`/`Substring{parent:
    // Concat}` shapes too — which `resolve_byte_range()` refuses,
    // returning `None` and skipping registration entirely (bd-related
    // regression: a multi-line block scalar's re-parsed diagnostics
    // carry exactly this shape after content provenance was threaded
    // into the re-parse bases). Using the same accessor the renderer
    // uses (`root_file_id()`, `diagnostic.rs:819`/`:1022`) also makes
    // the binder agree with the renderer about how to obtain the id.
    let fid = info.root_file_id()?;
    for (candidate_id, candidate) in candidates {
        if candidate_id != fid {
            continue;
        }
        if source_context.get_file(fid).is_none()
            && let Ok(content) = std::fs::read_to_string(candidate)
        {
            source_context.add_file_with_id(
                fid,
                candidate.to_string_lossy().into_owned(),
                Some(content),
            );
        }
        return Some(candidate);
    }
    None
}

/// [`bind_source_candidates`] for diagnostics that must span **several
/// documents at once**, re-keying each span onto its own file.
///
/// The problem this solves: a document's front matter roots its spans
/// at the parse context's dense `FileId(0)`, and *every* document uses
/// that same id. [`bind_source_candidates`] registers the first file to
/// claim an id and skips the rest, which is right when one `ParseError`
/// concerns one document — but a diagnostic about two pages colliding
/// concerns two, and the second document's offsets would then be
/// rendered against the first document's text. That is the exact
/// mis-pairing this module exists to prevent, arrived at from the other
/// direction.
///
/// The fix is to stop using the dense id in the merged context. The
/// matched file is registered under the FileId derived from its *own
/// path* ([`register_config_source`]), and the returned `SourceInfo` is
/// the input's byte range re-keyed to that id. Offsets are unchanged
/// and remain offsets into that same file — `FileId(0)`'s content *is*
/// the document's full text — so the span still renders exactly where
/// the author wrote it, while two documents can now coexist in one
/// `SourceContext`.
///
/// Candidate selection is identical to [`bind_source_candidates`]:
/// match by re-derived id, never bind a non-match. Returns `None` — so
/// the diagnostic degrades to a span-less render — when the span has no
/// resolvable byte range or no candidate matches.
///
/// A file whose path-derived id already equals the span's id (any
/// standalone YAML config) round-trips unchanged; only the dense
/// document ids are actually rewritten.
pub fn rebase_source_candidates<'a>(
    source_context: &mut SourceContext,
    info: &SourceInfo,
    candidates: impl IntoIterator<Item = (FileId, &'a Path)>,
) -> Option<(&'a Path, SourceInfo)> {
    let (fid_usize, start_offset, end_offset) = info.resolve_byte_range()?;
    let fid = FileId(fid_usize);
    for (candidate_id, candidate) in candidates {
        if candidate_id != fid {
            continue;
        }
        if !register_config_source(source_context, candidate) {
            // Unreadable: attribute in prose, but never claim a span
            // we cannot render.
            return None;
        }
        let rebased = SourceInfo::Original {
            file_id: quarto_yaml::file_id_for_filename(&candidate.to_string_lossy()),
            start_offset,
            end_offset,
        };
        return Some((candidate, rebased));
    }
    None
}

/// Register `path` in `source_context` under its own derived FileId
/// (`quarto_yaml::file_id_for_filename` of the path's spelling),
/// content permitting. The triple cannot mis-pair — id, path, and
/// content all come from the one `path` — so this is the safe way to
/// pre-register a *known* config source for later span rendering
/// (as opposed to [`bind_config_source`], which selects among
/// candidates by a diagnostic's resolved id). Returns `true` when the
/// file was registered (or already present).
pub fn register_config_source(source_context: &mut SourceContext, path: &Path) -> bool {
    let name = path.to_string_lossy();
    let fid = quarto_yaml::file_id_for_filename(&name);
    if source_context.get_file(fid).is_some() {
        return true;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    source_context.add_file_with_id(fid, name.into_owned(), Some(content));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// A SourceInfo anchored in `path` the way `quarto_yaml::parse_file`
    /// would anchor a scalar: filename-hash FileId + byte offsets.
    fn source_info_in(path: &Path, start: usize, end: usize) -> SourceInfo {
        SourceInfo::Original {
            file_id: quarto_yaml::file_id_for_filename(&path.to_string_lossy()),
            start_offset: start,
            end_offset: end,
        }
    }

    #[test]
    fn binds_the_matching_candidate_not_the_first() {
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("_quarto.yml");
        let manifest = temp.path().join("_extension.yml");
        std::fs::write(&config, "project:\n  type: default\n").unwrap();
        std::fs::write(&manifest, "contributes:\n  metadata: {}\n").unwrap();

        let info = source_info_in(&manifest, 0, 11);
        let mut sc = SourceContext::new();
        let matched = bind_config_source(&mut sc, &info, [config.as_path(), manifest.as_path()]);
        assert_eq!(matched, Some(manifest.as_path()));
        let fid = quarto_yaml::file_id_for_filename(&manifest.to_string_lossy());
        let registered = sc.get_file(fid).expect("manifest must be registered");
        assert_eq!(
            registered.content.as_deref(),
            Some("contributes:\n  metadata: {}\n"),
            "registered content must be the manifest's, not the config's"
        );
    }

    #[test]
    fn binds_the_config_file_when_it_matches() {
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("_quarto.yml");
        let manifest = temp.path().join("_extension.yml");
        std::fs::write(&config, "project:\n  type: default\n").unwrap();
        std::fs::write(&manifest, "contributes:\n  metadata: {}\n").unwrap();

        let info = source_info_in(&config, 0, 7);
        let mut sc = SourceContext::new();
        let matched = bind_config_source(&mut sc, &info, [config.as_path(), manifest.as_path()]);
        assert_eq!(matched, Some(config.as_path()));
    }

    #[test]
    fn unknown_file_id_registers_nothing() {
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("_quarto.yml");
        std::fs::write(&config, "project:\n  type: default\n").unwrap();

        let elsewhere = PathBuf::from("/nonexistent/other.yml");
        let info = source_info_in(&elsewhere, 0, 5);
        let mut sc = SourceContext::new();
        let matched = bind_config_source(&mut sc, &info, [config.as_path()]);
        assert_eq!(matched, None);
        let config_fid = quarto_yaml::file_id_for_filename(&config.to_string_lossy());
        assert!(
            sc.get_file(config_fid).is_none(),
            "a non-matching candidate must never be registered"
        );
    }

    #[test]
    fn unresolvable_source_info_registers_nothing() {
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("_quarto.yml");
        std::fs::write(&config, "project:\n  type: default\n").unwrap();

        let info = SourceInfo::generated(quarto_source_map::By::programmatic_config());
        let mut sc = SourceContext::new();
        assert_eq!(bind_config_source(&mut sc, &info, [config.as_path()]), None);
    }

    /// A `Concat`-backed `SourceInfo`, wrapped in a `Substring` — the
    /// exact shape a diagnostic re-parsed out of a multi-line block
    /// scalar carries once content provenance is threaded into the
    /// re-parse bases (commit `1b6d30c08`): decoding strips each
    /// line's leading indent, so the decoded content is discontiguous
    /// in the source and its `SourceInfo` is a `Concat` of per-line
    /// pieces, all rooted in the same file.
    ///
    /// `resolve_byte_range()` refuses any `Concat`-backed location
    /// (`SourceInfo::Concat { .. } => None`, and a `Substring` whose
    /// parent is a `Concat` inherits that `None`) — that used to be
    /// `bind_source_candidates`'s first and only way to obtain the
    /// file id, so this exact shape used to return `None` and
    /// register nothing, degrading the diagnostic to a span-less
    /// render (task C5's regression). `root_file_id()` resolves it
    /// fine: it recurses through the `Substring` to the `Concat`, then
    /// `find_map`s over the pieces to the first one with a root.
    fn concat_backed_source_info_in(path: &Path) -> SourceInfo {
        let fid = quarto_yaml::file_id_for_filename(&path.to_string_lossy());
        let piece_a = SourceInfo::substring(SourceInfo::original(fid, 0, 20), 0, 8);
        let piece_b = SourceInfo::substring(SourceInfo::original(fid, 0, 20), 9, 17);
        let concat = SourceInfo::concat(vec![(piece_a, 8), (piece_b, 8)]);
        SourceInfo::substring(concat, 2, 10)
    }

    #[test]
    fn binds_a_concat_backed_source_info() {
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("_quarto.yml");
        std::fs::write(&config, "project:\n  type: default\n").unwrap();

        let info = concat_backed_source_info_in(&config);
        let mut sc = SourceContext::new();
        let matched = bind_config_source(&mut sc, &info, [config.as_path()]);
        assert_eq!(
            matched,
            Some(config.as_path()),
            "a Concat-backed location must still resolve to its root file"
        );
        let fid = quarto_yaml::file_id_for_filename(&config.to_string_lossy());
        let registered = sc
            .get_file(fid)
            .expect("Concat-backed match must still register the file's content");
        assert_eq!(
            registered.content.as_deref(),
            Some("project:\n  type: default\n"),
        );
    }

    #[test]
    fn unreadable_match_is_returned_but_not_registered() {
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join("_extension.yml");
        // Never written to disk: the FileId matches, the read fails.
        let info = source_info_in(&missing, 0, 5);
        let mut sc = SourceContext::new();
        let matched = bind_config_source(&mut sc, &info, [missing.as_path()]);
        assert_eq!(
            matched,
            Some(missing.as_path()),
            "the match is still reported for prose attribution"
        );
        let fid = quarto_yaml::file_id_for_filename(&missing.to_string_lossy());
        assert!(
            sc.get_file(fid).is_none(),
            "no content ⇒ no registration (span-less degradation)"
        );
    }
}
