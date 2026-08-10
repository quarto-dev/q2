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
    let (fid_usize, _, _) = info.resolve_byte_range()?;
    let fid = FileId(fid_usize);
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
