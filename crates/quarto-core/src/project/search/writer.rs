/*
 * project/search/writer.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Post-render search index writes (bd-eip0naf4).
 */

//! The emit half of website search: turn the capture records of a
//! render into the Pagefind bundle, its manifest, and `search.json`.
//!
//! Runs from `WebsiteProjectType::post_resources`, after every other
//! producer of output-dir files, and writes:
//!
//! - `<output>/<lib_dir>/quarto-search/pagefind/…` — the bundle from
//!   [`build_bundle`](super::index_writer::build_bundle). The directory
//!   is replaced wholesale on every run: chunk and fragment names are
//!   content hashes, so stale files from a previous run would
//!   otherwise accumulate.
//! - `<output>/<lib_dir>/quarto-search/manifest.json` — what a
//!   federated parent reads before merging this site's index
//!   ([`SearchManifest`]).
//! - `<output>/search.json` — Quarto 1's record array
//!   ([`super::records`]).
//!
//! ## Incremental renders
//!
//! A `RenderMode::Subset` render re-renders some pages; the index must
//! still cover the rest. The writer keeps one cache file per indexed
//! page under `<project>/.quarto/search/pages/`, holding the page's
//! [`CaptureRecord`]:
//!
//! - a page captured this run refreshes its cache file (or deletes it,
//!   for a [`CaptureRecord::Excluded`] page — the page rendered and
//!   opted out, so a cache entry from before the opt-out is stale);
//! - a page without a record this run (not rendered) is read from its
//!   cache file;
//! - cache files for pages no longer in the project are deleted.
//!
//! Pages that were neither rendered nor cached are summarized in one
//! informational note (they join the index on the next full render).
//!
//! ## Turning search off
//!
//! `website.search: false` removes the outputs a previous run wrote
//! (`<lib_dir>/quarto-search/`, and `search.json` when
//! `.quarto/search/state.json` records that this writer produced it)
//! so a site cannot keep serving an index it no longer maintains.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_system_runtime::{PathKind, SystemRuntime};
use sha2::{Digest, Sha256};

use crate::Result;
use crate::artifact::ArtifactStore;
use crate::error::QuartoError;
use crate::project::ProjectContext;
use crate::project::index::ProjectIndex;

use super::capture::{
    CaptureRecord, PageCapture, artifact_key, profile_is_indexable, synthesize_document,
};
use super::index_writer::{Bundle, IndexPage, build_bundle};
use super::records::extract_records;
use super::{PAGEFIND_BUNDLE_SUBDIR, SEARCH_LIB_SUBDIR, resolve_website_search};

/// `manifest.json` next to the bundle. Read by a federated parent
/// before it merges this site's index (plan: "Federated search across
/// sites", constraint 1).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchManifest {
    /// Manifest format version.
    pub version: u32,
    /// The Quarto version that wrote the bundle.
    pub quarto: String,
    /// The Pagefind version (index format and runtime) of the bundle.
    /// A parent must not merge a bundle whose version differs from its
    /// own runtime's.
    pub pagefind: String,
    /// Site title, the default result context for merged hits.
    pub title: Option<String>,
    /// Languages present in the bundle (`pagefind-entry.json`).
    pub languages: Vec<String>,
    /// Bundle directory relative to this manifest.
    pub bundle: String,
    /// Indexed page count.
    pub pages: usize,
}

pub const MANIFEST_VERSION: u32 = 1;

/// `.quarto/search/state.json`: what this writer produced outside its
/// own lib subdirectory, so it can clean up when search is turned off.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct WriterState {
    version: u32,
    /// `search.json` at the output root was written by us.
    search_json: bool,
}

const STATE_VERSION: u32 = 1;

fn search_state_dir(project: &ProjectContext) -> PathBuf {
    project.dir.join(".quarto").join("search")
}

fn cache_dir(project: &ProjectContext) -> PathBuf {
    search_state_dir(project).join("pages")
}

fn state_path(project: &ProjectContext) -> PathBuf {
    search_state_dir(project).join("state.json")
}

fn cache_file_for(project: &ProjectContext, href: &str) -> PathBuf {
    let digest = Sha256::digest(href.as_bytes());
    cache_dir(project).join(format!("{}.json", &hex::encode(digest)[..24]))
}

fn search_lib_dir(project: &ProjectContext, lib_dir: &str) -> PathBuf {
    project.output_dir.join(lib_dir).join(SEARCH_LIB_SUBDIR)
}

fn io_err(what: &str, path: &Path, err: impl std::fmt::Display) -> QuartoError {
    QuartoError::Io(std::io::Error::other(format!(
        "search index: could not {what} `{}`: {err}",
        path.display()
    )))
}

fn read_state(project: &ProjectContext, runtime: &dyn SystemRuntime) -> WriterState {
    runtime
        .file_read(&state_path(project))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn remove_if_present(path: &Path, runtime: &dyn SystemRuntime) -> Result<()> {
    match runtime.path_exists(path, None) {
        Ok(true) => {
            let is_dir = runtime
                .path_exists(path, Some(PathKind::Directory))
                .unwrap_or(false);
            if is_dir {
                runtime
                    .dir_remove(path, true)
                    .map_err(|e| io_err("remove", path, e))
            } else {
                runtime
                    .file_remove(path)
                    .map_err(|e| io_err("remove", path, e))
            }
        }
        _ => Ok(()),
    }
}

/// Remove everything a previous run wrote. Used when search is off.
fn remove_outputs(
    project: &ProjectContext,
    lib_dir: &str,
    runtime: &dyn SystemRuntime,
) -> Result<()> {
    let state = read_state(project, runtime);
    remove_if_present(&search_lib_dir(project, lib_dir), runtime)?;
    if state.search_json {
        remove_if_present(&project.output_dir.join("search.json"), runtime)?;
    }
    remove_if_present(&search_state_dir(project), runtime)?;
    Ok(())
}

/// Write the search index for a website render. See the module docs.
pub(crate) fn write_search_index(
    project: &ProjectContext,
    index: &ProjectIndex,
    project_artifacts: &ArtifactStore,
    lib_dir: &str,
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    let Some(meta) = project.config.metadata.as_ref() else {
        return Ok(());
    };
    let Some(_opts) = resolve_website_search(meta, diagnostics) else {
        return remove_outputs(project, lib_dir, runtime);
    };

    // ── Collect this run's pages: fresh captures, else the cache. ──
    let cache = cache_dir(project);
    let mut pages: Vec<PageCapture> = Vec::new();
    let mut live_cache_files: Vec<PathBuf> = Vec::new();
    // Pages neither rendered this run nor cached: expected on a Subset
    // render of a site that has never been fully rendered, so this is
    // one informational note, not a warning per page (a warning would
    // fail `--strict` hundreds of times over on `q2 render page.qmd`).
    let mut missing: Vec<String> = Vec::new();
    for profile in index.profiles() {
        let href = &profile.output_href;
        let cache_file = cache_file_for(project, href);
        if !profile_is_indexable(index, profile) {
            remove_if_present(&cache_file, runtime)?;
            continue;
        }
        match project_artifacts.get(&artifact_key(href)) {
            Some(artifact) => match CaptureRecord::from_json(&artifact.as_string()) {
                Some(CaptureRecord::Page(page)) => {
                    runtime
                        .file_write(&cache_file, artifact.as_string().as_bytes())
                        .map_err(|e| io_err("write", &cache_file, e))?;
                    live_cache_files.push(cache_file);
                    pages.push(page);
                }
                // Rendered this run and opted out, or unreadable:
                // either way the page is not in the index now.
                Some(CaptureRecord::Excluded) | None => {
                    remove_if_present(&cache_file, runtime)?;
                }
            },
            None => {
                // Not rendered this run: reuse the previous capture.
                match runtime
                    .file_read(&cache_file)
                    .ok()
                    .and_then(|bytes| CaptureRecord::from_json(&String::from_utf8_lossy(&bytes)))
                {
                    Some(CaptureRecord::Page(page)) => {
                        live_cache_files.push(cache_file);
                        pages.push(page);
                    }
                    _ => missing.push(href.clone()),
                }
            }
        }
    }
    if !missing.is_empty() {
        let shown: Vec<&str> = missing.iter().take(3).map(String::as_str).collect();
        let more = missing.len().saturating_sub(shown.len());
        diagnostics.push(
            DiagnosticMessageBuilder::info(format!(
                "Search index is missing {} page{} not rendered in this run",
                missing.len(),
                if missing.len() == 1 { "" } else { "s" }
            ))
            .problem(format!(
                "{}{} were neither rendered this run nor captured by a previous one; \
                 they join the index on the next full render.",
                shown.join(", "),
                if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                }
            ))
            .build(),
        );
    }
    // Prune cache files for pages that left the project.
    if let Ok(entries) = runtime.dir_list(&cache) {
        for entry in entries {
            if entry.extension().is_some_and(|e| e == "json")
                && !live_cache_files.iter().any(|keep| keep == &entry)
            {
                remove_if_present(&entry, runtime)?;
            }
        }
    }
    pages.sort_by(|a, b| a.href.cmp(&b.href));

    // ── Build. ────────────────────────────────────────────────────
    let search_lib = search_lib_dir(project, lib_dir);
    let bundle_dir = search_lib.join(PAGEFIND_BUNDLE_SUBDIR);
    let index_pages: Vec<IndexPage> = pages
        .iter()
        .map(|p| IndexPage {
            url: p.href.clone(),
            html: synthesize_document(p),
        })
        .collect();
    // Pagefind's indexing is single-threaded for most of its run (each
    // `add_html_file` holds the index lock while it parses the page), so
    // extract the `search.json` records on a second thread meanwhile;
    // that took ~1 s on a 1600-page site, all on the critical path.
    // `QUARTO_JOBS=1` keeps everything on this thread. Extraction is
    // pure (no I/O), so doing it before the bundle result is known only
    // wastes work on the error paths below.
    let (bundle_result, early_records) = if crate::project::orchestrator::worker_count() > 1 {
        std::thread::scope(|scope| {
            let extraction =
                scope.spawn(|| pages.iter().flat_map(extract_records).collect::<Vec<_>>());
            let bundle_result = build_bundle(index_pages);
            let records = extraction
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            (bundle_result, Some(records))
        })
    } else {
        (build_bundle(index_pages), None)
    };
    let bundle: Bundle = match bundle_result {
        Ok(bundle) => bundle,
        Err(err) => {
            diagnostics.push(
                DiagnosticMessageBuilder::warning("Could not build the website search index")
                    .problem(format!(
                        "{err}. The site rendered, but search will not work."
                    ))
                    .build(),
            );
            return Ok(());
        }
    };

    // ── Write: bundle (replaced wholesale), manifest, search.json. ─
    remove_if_present(&bundle_dir, runtime)?;
    if pages.is_empty() {
        // Nothing to search: leave no bundle behind, like Quarto 1
        // (which never wrote an empty search.json).
        remove_if_present(&search_lib, runtime)?;
        let state = read_state(project, runtime);
        if state.search_json {
            remove_if_present(&project.output_dir.join("search.json"), runtime)?;
        }
        return Ok(());
    }
    for file in &bundle.files {
        let path = bundle_dir.join(&file.path);
        runtime
            .file_write(&path, &file.contents)
            .map_err(|e| io_err("write", &path, e))?;
    }

    let languages = bundle
        .files
        .iter()
        .find(|f| f.path.to_string_lossy() == "pagefind-entry.json")
        .and_then(|f| serde_json::from_slice::<serde_json::Value>(&f.contents).ok())
        .and_then(|entry| {
            entry["languages"]
                .as_object()
                .map(|langs| langs.keys().cloned().collect::<Vec<_>>())
        })
        .unwrap_or_default();
    let manifest = SearchManifest {
        version: MANIFEST_VERSION,
        quarto: crate::version().to_string(),
        pagefind: bundle.pagefind_version.clone(),
        title: crate::project::website_config::website_title(meta),
        languages,
        bundle: format!("{PAGEFIND_BUNDLE_SUBDIR}/"),
        pages: pages.len(),
    };
    let manifest_path = search_lib.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest).expect("manifest serializes");
    runtime
        .file_write(&manifest_path, manifest_json.as_bytes())
        .map_err(|e| io_err("write", &manifest_path, e))?;

    let records: Vec<_> =
        early_records.unwrap_or_else(|| pages.iter().flat_map(extract_records).collect());
    let search_json_path = project.output_dir.join("search.json");
    let records_json = serde_json::to_string_pretty(&records).expect("records serialize");
    runtime
        .file_write(&search_json_path, records_json.as_bytes())
        .map_err(|e| io_err("write", &search_json_path, e))?;

    let state = WriterState {
        version: STATE_VERSION,
        search_json: true,
    };
    let state_file = state_path(project);
    runtime
        .file_write(
            &state_file,
            serde_json::to_string(&state)
                .expect("state serializes")
                .as_bytes(),
        )
        .map_err(|e| io_err("write", &state_file, e))?;
    Ok(())
}
