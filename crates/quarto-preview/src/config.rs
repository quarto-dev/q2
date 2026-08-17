//! Preview-server configuration knobs that aren't covered by
//! [`PreviewConfig`](crate::PreviewConfig) — currently the
//! `preview.engine` policy (Phase C.6, bd-kw93.6).
//!
//! The policy flows through `MetadataMergeStage` like any other key
//! (so per-doc YAML frontmatter and `_quarto.yml` both contribute);
//! the *consumer* of the resolved value is the
//! [`capture_driver`](crate::capture_driver), not the pipeline.
//! Plan §C.6.
//!
//! The CLI reads `_quarto.yml` once at session start via
//! [`read_engine_policy_from_project`] and stashes the result in
//! `PreviewConfig`. Re-reading on `_quarto.yml` changes is a Phase D
//! follow-up; for the MVP the policy is fixed for the lifetime of
//! the `q2 preview` invocation.

use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;

/// What the preview server should do with engine execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnginePolicy {
    /// Eager capture on first sight; server detects staleness on edit
    /// and surfaces it via the SPA overlay; user must click
    /// "Re-execute" (POST /api/preview/re-execute). Phase C.5 default.
    #[default]
    Manual,
    /// Eager capture on first sight; server automatically re-executes
    /// on every settled code-cell change. No user opt-in required.
    Auto,
    /// Server never executes. C.1's eager run is skipped, the
    /// file-watcher staleness hook is a no-op, and code cells render
    /// as inert source in the SPA.
    Off,
}

/// Parse an `EnginePolicy` from a resolved metadata `ConfigValue`.
///
/// Looks up `preview.engine`. Unknown values, missing keys, or
/// unrecognized types all yield [`EnginePolicy::Manual`] — the
/// safe-default policy that matches the pre-C.6 behaviour.
///
/// Note: YAML's `off` and `no` are parsed as bools, not strings, by
/// the YAML loader. We accept both the bool form (`false` → Off) and
/// the string form (`"off"`/`"none"` → Off).
pub fn read_engine_policy_from_metadata(meta: &ConfigValue) -> EnginePolicy {
    let Some(value) = meta.get_path(&["preview", "engine"]) else {
        return EnginePolicy::Manual;
    };
    if let Some(s) = value.as_str() {
        return parse_policy_str(s);
    }
    if let Some(b) = value.as_bool() {
        return if b {
            // `engine: true` doesn't have a natural meaning; treat as
            // Manual (safe default) rather than Auto, since enabling
            // auto-execution should require an explicit opt-in.
            EnginePolicy::Manual
        } else {
            EnginePolicy::Off
        };
    }
    EnginePolicy::Manual
}

fn parse_policy_str(s: &str) -> EnginePolicy {
    match s.trim().to_ascii_lowercase().as_str() {
        "auto" => EnginePolicy::Auto,
        "off" | "false" | "none" | "no" => EnginePolicy::Off,
        // "manual" + everything else falls back to Manual. The plan
        // §Q-C5 doesn't enumerate aliases; this is conservative and
        // matches "don't auto-execute under any ambiguity."
        _ => EnginePolicy::Manual,
    }
}

/// Read the engine policy from the project's `_quarto.yml` (if any).
///
/// Discovers the project context rooted at `project_root` using
/// `ProjectContext::discover`, which already handles single-file
/// projects (no `_quarto.yml` → returns the default policy).
///
/// Returns [`EnginePolicy::Manual`] if discovery fails or no project
/// metadata is present — the same safe default as a value-level
/// fallback.
pub fn read_engine_policy_from_project(
    project_root: &std::path::Path,
    runtime: &dyn SystemRuntime,
) -> EnginePolicy {
    let Ok(project) = quarto_core::project::ProjectContext::discover(project_root, runtime) else {
        return EnginePolicy::Manual;
    };
    let Some(meta) = project.config.metadata.as_ref() else {
        return EnginePolicy::Manual;
    };
    read_engine_policy_from_metadata(meta)
}

/// Resolve the `.html` files made visible by the project's
/// `project.resources:` declarations, as project-root-relative paths
/// (forward-slash separated) suitable for the hub's VFS source layer.
/// (bd-kjrpya2d, part 2)
///
/// Embedded example decks (`.embed-example-iframe`) are declared as
/// project resources so `q2 render` copies them into `_site/`. In
/// `q2 preview` the page renders in-browser via WASM with no disk
/// server, so the deck must instead live in the VFS *source* tree —
/// where the iframe post-processor's source-path fallback
/// (`readArtifactOrSource`) reads it. The bare hub discovery walk can't
/// see `.html` (it falls through every category), so we resolve the
/// resources-scoped set here — `quarto-preview` has `quarto-core`,
/// `quarto-hub` does not — and inject it via
/// `HubConfig::resource_files` → `ProjectFiles::with_resource_files`.
///
/// **Best-effort.** Discovery failure, absence of `_quarto.yml`, an
/// empty/absent `resources:`, or a pattern that fails to expand all
/// yield an empty list — preview must still start. Genuine resource
/// errors surface at render time through the normal pipeline; this is
/// only a sync-availability convenience, not a validation gate.
///
/// **Scope note (bd-teh4hbli).** Restricting the synced `.html` to the
/// `resources:` set is the interim trust boundary: `resources:` is a
/// *publish* control, not an *upload* control. The hardening strand
/// decouples "what may upload to a sync server" from `resources:`.
pub fn resolve_project_resource_html(
    project_root: &std::path::Path,
    runtime: &dyn SystemRuntime,
) -> Vec<std::path::PathBuf> {
    use quarto_core::project_resources::{ResourceOrigin, ResourceScope, expand_patterns};

    let Ok(project) = quarto_core::project::ProjectContext::discover(project_root, runtime) else {
        return Vec::new();
    };
    let patterns = &project.config.resources;
    if patterns.is_empty() {
        return Vec::new();
    }

    // Project-scope patterns are anchored at the (canonical) project
    // root that `ProjectContext::discover` resolved.
    let root = project.dir.as_path();
    let Ok(resolved) = expand_patterns(
        root,
        root,
        patterns,
        runtime,
        || ResourceOrigin::ProjectMetadata,
        ResourceScope::Project,
    ) else {
        return Vec::new();
    };

    let mut html: Vec<std::path::PathBuf> = resolved
        .into_iter()
        .filter(|r| {
            std::path::Path::new(&r.output_relative)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("html"))
        })
        // `output_relative` is the project-relative source path,
        // forward-slash separated — exactly the VFS source key.
        .map(|r| std::path::PathBuf::from(r.output_relative))
        .collect();
    html.sort();
    html.dedup();
    html
}

/// Resolve the project's **full** `resources:` set to `(output-relative URL
/// path, absolute source path on disk)` pairs. (bd-kjrpya2d)
///
/// Unlike [`resolve_project_resource_html`] (which filters to `.html` for the
/// VFS-source text sync), this returns *every* declared resource file — the
/// deck HTML **and** its `slides_files/…` sidecar assets — so the preview hub
/// can SERVE them on disk at the artifact-rooted path the embed iframe requests
/// (`/.quarto/project-artifacts/<output-relative>`). Decks now LINK their
/// assets (reveal.js linked-assets, bd-jij5gge2), so they must be served, not
/// inlined.
///
/// Best-effort + scoped to the declared `resources:` set — the same publish
/// trust boundary as `resolve_project_resource_html` (bd-teh4hbli). The
/// `output_relative` is project-relative + forward-slash separated, matching
/// both the artifact URL suffix and `expand_patterns`' containment guarantee.
/// This serving is CLI/disk-only; diskless hub-client needs the service-worker
/// over the VFS (separate workstream).
pub fn resolve_project_resource_files(
    project_root: &std::path::Path,
    runtime: &dyn SystemRuntime,
) -> Vec<(String, std::path::PathBuf)> {
    use quarto_core::project_resources::{ResourceOrigin, ResourceScope, expand_patterns};

    let Ok(project) = quarto_core::project::ProjectContext::discover(project_root, runtime) else {
        return Vec::new();
    };
    let patterns = &project.config.resources;
    if patterns.is_empty() {
        return Vec::new();
    }

    let root = project.dir.as_path();
    let Ok(resolved) = expand_patterns(
        root,
        root,
        patterns,
        runtime,
        || ResourceOrigin::ProjectMetadata,
        ResourceScope::Project,
    ) else {
        return Vec::new();
    };

    resolved
        .into_iter()
        .map(|r| (r.output_relative, r.source))
        .collect()
}

/// The transitive static dependency closure of a single-file deck — the
/// sibling files `q2 preview deck.qmd` must sync into the preview VFS so the
/// deck renders like `q2 render` / project-mode preview, without walking the
/// deck's directory (the `bd-tnm3k` safety property). Supersedes the earlier
/// direct-image-only resolution (bd-kpuweafo): the closure now also includes
/// `{{< include >}}`d `.qmd` files (transitively) and the images referenced
/// *inside* them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SingleFileDeps {
    /// Included `.qmd` files (project-root-relative). Synced as **text**, kept
    /// out of `qmd_files` so they are invisible VFS-only dependencies.
    pub qmd_files: Vec<std::path::PathBuf>,
    /// Referenced image assets (project-root-relative). Synced as **binary**.
    pub binary_files: Vec<std::path::PathBuf>,
}

/// Resolve a single-file deck's full transitive static dependency closure
/// (bd-9cyza5vy) by running the renderer's **own** `ParseDocumentStage` +
/// `IncludeExpansionStage` natively against the real filesystem, then reading
/// back the files the renderer actually consumed.
///
/// Reusing the real stages (rather than re-deriving include resolution in a
/// parallel walker) keeps the preview's path-resolution semantics in exact
/// lock-step with render — including the un-retargeted image anchor *and* the
/// nested-include anchor (a latent render bug, bd-udrn0q47, which this resolver
/// will track automatically once it is fixed). See
/// `claude-notes/research/2026-06-16-include-shortcode-path-resolution.md`.
///
/// - **Includes** come from `DocumentAst::recorded_includes` — the exact set of
///   files the stage spliced, transitively, with the stage's own cycle
///   detection. Each is canonical-absolute; we re-express it project-root-
///   relative and drop anything that escapes the deck dir.
/// - **Images** come from `collect_referenced_asset_urls` over the *expanded*
///   AST, resolved relative to the **deck dir** (the same anchor
///   `ResourceCollectorTransform` uses — render parity for free).
///
/// `single_file_rel` is the deck path relative to `project_root` (its parent
/// directory in single-file mode). On any parse / IO error this returns an
/// empty closure — a missing dependency just renders broken, exactly as before.
pub fn resolve_single_file_deps(
    project_root: &std::path::Path,
    single_file_rel: &std::path::Path,
    runtime: std::sync::Arc<dyn SystemRuntime>,
) -> SingleFileDeps {
    use std::path::{Path, PathBuf};

    use quarto_core::project::{DocumentInfo, ProjectContext};
    use quarto_core::stage::{
        IncludeExpansionStage, LoadedSource, ParseDocumentStage, PipelineData, PipelineStage,
        StageContext,
    };

    let abs_deck = project_root.join(single_file_rel);
    let Ok(source) = runtime.file_read(&abs_deck) else {
        return SingleFileDeps::default();
    };

    // Build a single-file render context and run the renderer's OWN parse +
    // include-expansion stages natively. Reusing the real stages keeps path
    // resolution identical to `q2 render` (see the module + research notes).
    let Ok(project) = ProjectContext::single_file(&abs_deck, runtime.as_ref()) else {
        return SingleFileDeps::default();
    };
    let document = DocumentInfo::from_path(&abs_deck);
    let Ok(mut ctx) = StageContext::new(
        runtime.clone(),
        quarto_core::Format::html(),
        project,
        document,
    ) else {
        return SingleFileDeps::default();
    };

    // The stage `run` futures are `?Send`; drive them on this thread without a
    // tokio runtime (the same pattern `quarto-preview` uses elsewhere). Include
    // expansion reads included files through `ctx.runtime` (the native FS).
    let parse = ParseDocumentStage;
    let expand = IncludeExpansionStage::new();
    let expanded = pollster::block_on(async {
        let parsed = parse
            .run(
                PipelineData::LoadedSource(LoadedSource::new(abs_deck.clone(), source)),
                &mut ctx,
            )
            .await?;
        expand.run(parsed, &mut ctx).await
    });
    let Ok(PipelineData::DocumentAst(doc)) = expanded else {
        return SingleFileDeps::default();
    };

    let canonical_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    // Re-express a canonical absolute path as project-root-relative, keeping
    // only existing files that stay under the deck dir (no `../` escape).
    let to_in_tree_rel = |canon: PathBuf| -> Option<PathBuf> {
        if !canon.is_file() || !canon.starts_with(&canonical_root) {
            return None;
        }
        canon
            .strip_prefix(&canonical_root)
            .ok()
            .map(Path::to_path_buf)
    };

    // Text deps: the `.qmd` files the stage actually spliced (transitive,
    // cycle-truncated). `IncludeEntry::path` is canonical-absolute.
    let mut qmd_files: Vec<PathBuf> = Vec::new();
    let mut seen_qmd = std::collections::HashSet::new();
    for entry in &doc.recorded_includes {
        let Ok(canon) = entry.path.canonicalize() else {
            continue;
        };
        if let Some(rel) = to_in_tree_rel(canon)
            && seen_qmd.insert(rel.clone())
        {
            qmd_files.push(rel);
        }
    }

    // Binary deps: images referenced by the EXPANDED AST, resolved relative to
    // the deck dir (matches `ResourceCollectorTransform`'s anchor — render
    // parity, including images that came from included files).
    let deck_dir = single_file_rel.parent().unwrap_or_else(|| Path::new(""));
    let mut binary_files: Vec<PathBuf> = Vec::new();
    let mut seen_bin = std::collections::HashSet::new();
    for url in quarto_core::transforms::collect_referenced_asset_urls(&doc.ast.blocks) {
        let ext = Path::new(&url)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if !quarto_hub::resource::is_binary_extension(ext) {
            continue;
        }
        let Ok(canon) = project_root.join(deck_dir).join(&url).canonicalize() else {
            continue;
        };
        if let Some(rel) = to_in_tree_rel(canon)
            && seen_bin.insert(rel.clone())
        {
            binary_files.push(rel);
        }
    }

    // Document-declared `resources:` (bd-k5rxujiy Layer 1). The AST image walk
    // above can only see `Image` nodes; assets referenced through metadata or
    // raw HTML — `logo:`, footer images, shortcode/CSS `url()` refs — are
    // invisible to it. `resources:` is the author's explicit declaration of
    // exactly those files (the same one `q2 render` / publish uses, and the
    // sync upload trust boundary — see `resources_scoped_html_files` above), so
    // we honor it here too: expand the patterns (globs included) against the
    // deck dir, the same anchor as the image walk, and fold the matches into
    // the binary closure so they sync into the preview VFS.
    //
    // Note: this gets the bytes into the VFS (Layer 1). A raw `<img src=…>`
    // still needs its `src` rewritten to the minted blob URL to actually load
    // (Layer 2 — `preview-renderer` asset manifest); see the plan.
    {
        use quarto_core::project_resources::{
            ResourceOrigin, ResourceScope, expand_patterns, extract_resource_patterns,
        };
        let patterns = extract_resource_patterns(&doc.ast.meta, &["resources"]);
        if !patterns.is_empty() {
            // Anchor relative patterns at the deck dir (render parity with the
            // image walk); contain + relativize against the canonical root, so
            // the resulting keys match the VFS source keys produced above.
            let anchor = canonical_root.join(deck_dir);
            if let Ok(resolved) = expand_patterns(
                &canonical_root,
                &anchor,
                &patterns,
                runtime.as_ref(),
                || ResourceOrigin::DocumentMetadata {
                    source: abs_deck.clone(),
                },
                ResourceScope::Page {
                    source: abs_deck.clone(),
                },
            ) {
                for r in resolved {
                    let rel = PathBuf::from(&r.output_relative);
                    if seen_bin.insert(rel.clone()) {
                        binary_files.push(rel);
                    }
                }
            }
        }
    }

    qmd_files.sort();
    binary_files.sort();
    SingleFileDeps {
        qmd_files,
        binary_files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigValue;
    use quarto_system_runtime::NativeRuntime;
    use tempfile::TempDir;

    fn meta_with_preview_engine(value: &str) -> ConfigValue {
        // `from_path(&["preview", "engine"], v)` builds: `preview: { engine: v }`.
        ConfigValue::from_path(&["preview", "engine"], value)
    }

    // ── resolve_single_file_deps (bd-9cyza5vy) ──────────────────────

    use std::sync::Arc;

    fn native_arc() -> Arc<dyn SystemRuntime> {
        Arc::new(NativeRuntime::new())
    }

    /// An image referenced *inside* an included file is part of the closure —
    /// the include `.qmd` is a text dep, the image is a binary dep. (This is
    /// the exact gap the strand reported: in single-file preview, includes and
    /// their images both used to be missing.)
    #[test]
    fn single_file_deps_include_and_image_inside_include() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("inc.png"), b"\x89PNG\r\n").unwrap();
        std::fs::write(
            root.join("part.qmd"),
            "## Included Section\n\n![](inc.png)\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.qmd"),
            "---\ntitle: T\n---\n\n{{< include part.qmd >}}\n",
        )
        .unwrap();

        let deps = resolve_single_file_deps(root, std::path::Path::new("main.qmd"), native_arc());
        assert_eq!(
            deps,
            SingleFileDeps {
                qmd_files: vec![std::path::PathBuf::from("part.qmd")],
                binary_files: vec![std::path::PathBuf::from("inc.png")],
            }
        );
    }

    /// A document `resources:` declaration is honored in single-file preview
    /// (bd-k5rxujiy Layer 1): declared files land in `binary_files` even when
    /// nothing in the AST references them — the `logo:` / raw-HTML / shortcode
    /// case the AST image walker can't see.
    #[test]
    fn single_file_deps_includes_declared_resources() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("logo.svg"), b"<svg/>").unwrap();
        std::fs::write(
            root.join("main.qmd"),
            "---\ntitle: T\nlogo: logo.svg\nresources:\n  - logo.svg\n---\n\n# Hi\n",
        )
        .unwrap();

        let deps = resolve_single_file_deps(root, std::path::Path::new("main.qmd"), native_arc());
        assert!(
            deps.binary_files
                .contains(&std::path::PathBuf::from("logo.svg")),
            "declared resource logo.svg should sync into the VFS; got {:?}",
            deps.binary_files,
        );
    }

    /// `resources:` accepts globs — reuses the publish-path `expand_patterns`,
    /// so a `*.svg` pattern pulls every matching sibling into the closure.
    #[test]
    fn single_file_deps_resources_glob() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("a.svg"), b"<svg/>").unwrap();
        std::fs::write(root.join("b.svg"), b"<svg/>").unwrap();
        std::fs::write(
            root.join("main.qmd"),
            "---\ntitle: T\nresources:\n  - \"*.svg\"\n---\n\n# Hi\n",
        )
        .unwrap();

        let mut deps =
            resolve_single_file_deps(root, std::path::Path::new("main.qmd"), native_arc());
        deps.binary_files.sort();
        assert_eq!(
            deps.binary_files,
            vec![
                std::path::PathBuf::from("a.svg"),
                std::path::PathBuf::from("b.svg"),
            ],
        );
    }

    /// Includes are followed transitively: `main → a → b`, and an image in `b`
    /// is collected. All three deps (a.qmd, b.qmd, the image) appear.
    #[test]
    fn single_file_deps_transitive_includes_and_images() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("c.png"), b"\x89PNG\r\n").unwrap();
        std::fs::write(root.join("b.qmd"), "From B\n\n![](c.png)\n").unwrap();
        std::fs::write(root.join("a.qmd"), "From A\n\n{{< include b.qmd >}}\n").unwrap();
        std::fs::write(root.join("main.qmd"), "{{< include a.qmd >}}\n").unwrap();

        let mut deps =
            resolve_single_file_deps(root, std::path::Path::new("main.qmd"), native_arc());
        deps.qmd_files.sort();
        assert_eq!(
            deps,
            SingleFileDeps {
                qmd_files: vec![
                    std::path::PathBuf::from("a.qmd"),
                    std::path::PathBuf::from("b.qmd"),
                ],
                binary_files: vec![std::path::PathBuf::from("c.png")],
            }
        );
    }

    /// Render parity: an image written inside a *subdirectory* include resolves
    /// relative to the **deck dir**, not the include's dir (the "no
    /// retargeting" design — see the research note). Here `img.png` exists at
    /// BOTH the deck root and `sub/`; the closure must pick the **root** one.
    #[test]
    fn single_file_deps_image_in_subdir_include_anchored_at_deck_dir() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("img.png"), b"ROOT").unwrap();
        std::fs::write(root.join("sub/img.png"), b"SUB").unwrap();
        std::fs::write(root.join("sub/part.qmd"), "Part\n\n![](img.png)\n").unwrap();
        std::fs::write(root.join("main.qmd"), "{{< include sub/part.qmd >}}\n").unwrap();

        let deps = resolve_single_file_deps(root, std::path::Path::new("main.qmd"), native_arc());
        assert_eq!(
            deps.binary_files,
            vec![std::path::PathBuf::from("img.png")],
            "image inside a subdir include must resolve to the DECK dir (img.png), \
             not the include's dir (sub/img.png) — render parity / no retargeting"
        );
        assert_eq!(
            deps.qmd_files,
            vec![std::path::PathBuf::from("sub/part.qmd")]
        );
    }

    /// A self-referential include terminates (the stage's own cycle detection)
    /// and records the included file exactly once.
    #[test]
    fn single_file_deps_cycle_terminates() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        // a.qmd includes itself.
        std::fs::write(root.join("a.qmd"), "From A\n\n{{< include a.qmd >}}\n").unwrap();
        std::fs::write(root.join("main.qmd"), "{{< include a.qmd >}}\n").unwrap();

        let deps = resolve_single_file_deps(root, std::path::Path::new("main.qmd"), native_arc());
        assert_eq!(deps.qmd_files, vec![std::path::PathBuf::from("a.qmd")]);
        assert!(deps.binary_files.is_empty());
    }

    /// The under-deck-dir guard drops `../` escapes (both include and image),
    /// missing files, and external URLs — nothing outside the deck dir is
    /// synced even if it exists.
    #[test]
    fn single_file_deps_guard_drops_escapes_missing_and_external() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        // Real files OUTSIDE the deck dir that must NOT be pulled in.
        std::fs::write(root.join("secret.png"), b"\x89PNG\r\n").unwrap();
        std::fs::write(root.join("escape.qmd"), "secret include\n").unwrap();
        let deck_dir = root.join("deck");
        std::fs::create_dir(&deck_dir).unwrap();
        std::fs::write(
            deck_dir.join("main.qmd"),
            "{{< include ../escape.qmd >}}\n\n\
             ![a](../secret.png)\n\n\
             ![b](./missing.png)\n\n\
             ![c](https://example.com/r.png)\n",
        )
        .unwrap();

        // project_root is the deck's own dir in single-file mode.
        let deps =
            resolve_single_file_deps(&deck_dir, std::path::Path::new("main.qmd"), native_arc());
        assert_eq!(
            deps,
            SingleFileDeps::default(),
            "escapes, missing, and external refs are all dropped; got {deps:?}"
        );
    }

    /// The deck's own direct image (no include involved) is still collected —
    /// `resolve_single_file_deps` is a superset of the old direct-image path.
    #[test]
    fn single_file_deps_collects_deck_own_direct_image() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("logo.png"), b"\x89PNG\r\n").unwrap();
        std::fs::write(root.join("deck.qmd"), "![ok](logo.png)\n").unwrap();

        let deps = resolve_single_file_deps(root, std::path::Path::new("deck.qmd"), native_arc());
        assert_eq!(
            deps,
            SingleFileDeps {
                qmd_files: vec![],
                binary_files: vec![std::path::PathBuf::from("logo.png")],
            }
        );
    }

    #[test]
    fn missing_key_defaults_to_manual() {
        // A metadata blob without `preview.engine` → Manual.
        let meta = ConfigValue::from_path(&["title"], "Whatever");
        assert_eq!(
            read_engine_policy_from_metadata(&meta),
            EnginePolicy::Manual
        );
    }

    #[test]
    fn manual_value_parses() {
        let meta = meta_with_preview_engine("manual");
        assert_eq!(
            read_engine_policy_from_metadata(&meta),
            EnginePolicy::Manual
        );
    }

    #[test]
    fn auto_value_parses() {
        let meta = meta_with_preview_engine("auto");
        assert_eq!(read_engine_policy_from_metadata(&meta), EnginePolicy::Auto);
    }

    #[test]
    fn off_value_parses() {
        let meta = meta_with_preview_engine("off");
        assert_eq!(read_engine_policy_from_metadata(&meta), EnginePolicy::Off);
    }

    #[test]
    fn case_insensitive_match() {
        assert_eq!(
            read_engine_policy_from_metadata(&meta_with_preview_engine("AUTO")),
            EnginePolicy::Auto
        );
        assert_eq!(
            read_engine_policy_from_metadata(&meta_with_preview_engine("Off")),
            EnginePolicy::Off
        );
    }

    #[test]
    fn unknown_value_falls_back_to_manual() {
        let meta = meta_with_preview_engine("nonsense");
        assert_eq!(
            read_engine_policy_from_metadata(&meta),
            EnginePolicy::Manual
        );
    }

    #[test]
    fn read_from_project_no_quarto_yml_is_manual() {
        // No _quarto.yml in the project root: single-file pseudo-project,
        // no metadata, fall back to Manual.
        let temp = TempDir::with_prefix("c6-config-").unwrap();
        let runtime = NativeRuntime::new();
        assert_eq!(
            read_engine_policy_from_project(temp.path(), &runtime),
            EnginePolicy::Manual
        );
    }

    #[test]
    fn read_from_project_with_auto_quarto_yml() {
        let temp = TempDir::with_prefix("c6-config-auto-").unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "preview:\n  engine: auto\n",
        )
        .unwrap();
        let runtime = NativeRuntime::new();
        assert_eq!(
            read_engine_policy_from_project(temp.path(), &runtime),
            EnginePolicy::Auto
        );
    }

    #[test]
    fn read_from_project_with_off_quarto_yml() {
        let temp = TempDir::with_prefix("c6-config-off-").unwrap();
        std::fs::write(temp.path().join("_quarto.yml"), "preview:\n  engine: off\n").unwrap();
        let runtime = NativeRuntime::new();
        assert_eq!(
            read_engine_policy_from_project(temp.path(), &runtime),
            EnginePolicy::Off
        );
    }

    // ── resolve_project_resource_html (bd-kjrpya2d, part 2) ──────────

    #[test]
    fn resource_html_empty_without_quarto_yml() {
        let temp = TempDir::with_prefix("kj-res-none-").unwrap();
        std::fs::write(temp.path().join("slides.html"), "<html></html>").unwrap();
        let runtime = NativeRuntime::new();
        // No `_quarto.yml` → no project `resources:` → nothing synced.
        assert!(resolve_project_resource_html(temp.path(), &runtime).is_empty());
    }

    #[test]
    fn resource_html_empty_when_resources_absent() {
        let temp = TempDir::with_prefix("kj-res-absent-").unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "project:\n  type: website\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("slides.html"), "<html></html>").unwrap();
        let runtime = NativeRuntime::new();
        // `.html` is NOT auto-synced just because it exists — only the
        // resources-scoped set is (bd-teh4hbli trust boundary).
        assert!(resolve_project_resource_html(temp.path(), &runtime).is_empty());
    }

    #[test]
    fn resource_html_resolves_directory_pattern_html_only() {
        let temp = TempDir::with_prefix("kj-res-dir-").unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "project:\n  type: website\n  resources:\n    - examples\n",
        )
        .unwrap();
        std::fs::create_dir(temp.path().join("examples")).unwrap();
        std::fs::write(
            temp.path().join("examples/slides.html"),
            "<html><body>deck</body></html>",
        )
        .unwrap();
        // A non-html resource in the same dir must be excluded — the
        // deck's images flow through the binary asset walker, not here.
        std::fs::write(temp.path().join("examples/logo.png"), [0x89, 0x50]).unwrap();

        let runtime = NativeRuntime::new();
        let html = resolve_project_resource_html(temp.path(), &runtime);
        assert_eq!(html, vec![std::path::PathBuf::from("examples/slides.html")]);
    }

    #[test]
    fn resource_html_resolves_explicit_glob() {
        let temp = TempDir::with_prefix("kj-res-glob-").unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "project:\n  type: website\n  resources:\n    - \"decks/*.html\"\n",
        )
        .unwrap();
        std::fs::create_dir(temp.path().join("decks")).unwrap();
        std::fs::write(temp.path().join("decks/a.html"), "<html>a</html>").unwrap();
        std::fs::write(temp.path().join("decks/b.html"), "<html>b</html>").unwrap();

        let runtime = NativeRuntime::new();
        let html = resolve_project_resource_html(temp.path(), &runtime);
        assert_eq!(
            html,
            vec![
                std::path::PathBuf::from("decks/a.html"),
                std::path::PathBuf::from("decks/b.html"),
            ]
        );
    }

    #[test]
    fn resource_files_resolves_full_set_with_disk_paths() {
        // The disk-serve route needs EVERY declared resource file (the deck
        // HTML *and* its slides_files/ sidecars), mapped to its absolute
        // source path — not just the .html.
        let temp = TempDir::with_prefix("kj-res-files-").unwrap();
        std::fs::write(
            temp.path().join("_quarto.yml"),
            "project:\n  type: website\n  resources:\n    - examples\n",
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("examples/d/slides_files/revealjs")).unwrap();
        std::fs::write(temp.path().join("examples/d/slides.html"), "<html></html>").unwrap();
        std::fs::write(
            temp.path()
                .join("examples/d/slides_files/revealjs/reveal.js"),
            "/*js*/",
        )
        .unwrap();
        std::fs::write(
            temp.path()
                .join("examples/d/slides_files/revealjs/reveal.css"),
            "/*css*/",
        )
        .unwrap();

        let runtime = NativeRuntime::new();
        let mut files = resolve_project_resource_files(temp.path(), &runtime);
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let rels: Vec<&str> = files.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(
            rels,
            vec![
                "examples/d/slides.html",
                "examples/d/slides_files/revealjs/reveal.css",
                "examples/d/slides_files/revealjs/reveal.js",
            ],
            "must include the deck HTML AND its slides_files sidecars"
        );
        // Each maps to a real, readable absolute source path.
        for (rel, disk) in &files {
            assert!(
                disk.is_absolute(),
                "{rel} → non-absolute disk path {disk:?}"
            );
            assert!(disk.is_file(), "{rel} → {disk:?} is not a file");
        }
        // Spot-check one maps to the right on-disk file.
        let js = files
            .iter()
            .find(|(r, _)| r == "examples/d/slides_files/revealjs/reveal.js")
            .unwrap();
        assert_eq!(std::fs::read_to_string(&js.1).unwrap(), "/*js*/");
    }
}
