//! Publish command — CLI surface.
//!
//! This module is intentionally thin: parse args, validate flag
//! combinations, build the `PublishInput` / `PublishUx` /
//! `PublishRenderer` / `PublishHost` quartet, and hand off to
//! `quarto_publish::execute`. The actual publish flow (provider
//! lookup, prepare → commit → verify) lives in the `quarto-publish`
//! crate.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::project::render_scripts;
use quarto_core::{Format, ProjectContext, RenderToFileOptions};
use quarto_publish::cli::{PublishCli, validate_and_resolve};
use quarto_publish::renderer::{PublishRenderFlags, PublishRenderer};
use quarto_publish::types::{PublishError, PublishFiles, PublishInput, PublishKind};
use quarto_publish::{ExecuteArgs, NativeHost, ProviderRegistry, execute as run_publish};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Arguments to the `quarto publish` subcommand.
pub struct PublishArgs {
    pub provider: Option<String>,
    pub path: Option<String>,
    pub no_render: bool,
    pub no_prompt: bool,
    pub no_browser: bool,
    pub no_wait: bool,
    pub dry_run: bool,
    pub json: bool,
}

/// Execute the `quarto publish` command.
pub fn execute(args: PublishArgs) -> Result<()> {
    // For Phase 1 the provider is required (no interactive picker
    // yet). Filed as a follow-up.
    let provider_name = args.provider.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "Specify a provider, e.g. `quarto publish gh-pages`. \
             Available providers: {}",
            ProviderRegistry::with_builtins().known_names().join(", ")
        )
    })?;

    // Resolve flag combinations.
    let cli = PublishCli {
        render: if args.no_render { Some(false) } else { None },
        prompt: if args.no_prompt { Some(false) } else { None },
        browser: if args.no_browser { Some(false) } else { None },
        wait: if args.no_wait { Some(false) } else { None },
        dry_run: args.dry_run,
        json: args.json,
    };
    let validated = validate_and_resolve(cli).map_err(|e| anyhow::anyhow!("{}", e))?;
    let ux = validated.ux;

    // Resolve the project path.
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let cwd = runtime
        .cwd()
        .map_err(|e| anyhow::anyhow!("failed to get current directory: {e}"))?;
    let path = match args.path {
        Some(p) => {
            let p = PathBuf::from(p);
            if p.is_absolute() { p } else { cwd.join(p) }
        }
        None => cwd.clone(),
    };

    let project = ProjectContext::discover(&path, runtime.as_ref())
        .context("failed to discover project context for publish")?;

    let project_dir = project.dir.clone();
    let title = derive_title(&project, &project_dir);
    let slug = simple_slug(&title);
    let site_url = quarto_core::project::website_config::website_site_url(
        project
            .config
            .metadata
            .as_ref()
            .unwrap_or(&Default::default()),
    );

    let input = PublishInput {
        project_dir: project_dir.clone(),
        kind: PublishKind::Site,
        title,
        slug,
        site_url,
    };

    // Construct host, renderer, registry.
    let host = NativeHost::new(ux.json);
    let renderer = ProjectPublishRenderer {
        project_dir: project_dir.clone(),
        runtime: runtime.clone(),
    };
    let registry = ProviderRegistry::with_builtins();

    // Surface validation notes (e.g. dry-run silently turning
    // browser off).
    if !ux.json {
        for note in &validated.notes {
            eprintln!("{note}");
        }
    }

    // Drive the publish.
    let outcome = pollster::block_on(run_publish(ExecuteArgs {
        provider_name,
        input,
        ux: ux.clone(),
        registry: &registry,
        renderer: &renderer,
        host: &host,
    }));

    match outcome {
        Ok(outcome) => {
            // Outcome goes to stdout (machine consumers can read it
            // without parsing through stderr noise).
            println!("{}", host.render_outcome(&outcome));
            Ok(())
        }
        Err(e) => {
            if ux.json {
                let payload = serde_json::json!({
                    "error": {
                        "code": e.code(),
                        "provider": e.provider(),
                        "message": e.to_string(),
                    }
                });
                println!("{payload}");
                std::process::exit(1);
            } else {
                Err(anyhow::anyhow!("{}", e))
            }
        }
    }
}

/// Derive a site title from project config or directory name.
fn derive_title(project: &ProjectContext, project_dir: &std::path::Path) -> String {
    if let Some(meta) = project.config.metadata.as_ref()
        && let Some(t) = quarto_core::project::website_config::website_title(meta)
    {
        return t;
    }
    project_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string()
}

/// Lossy-but-fine slug derivation. Phase 1 doesn't need
/// gfm-identifier exactness — providers that care will sanitize
/// further.
fn simple_slug(title: &str) -> String {
    let mut s = String::new();
    let mut last_dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !s.is_empty() {
            s.push('-');
            last_dash = true;
        }
    }
    if s.ends_with('-') {
        s.pop();
    }
    if s.is_empty() {
        "untitled".to_string()
    } else {
        s
    }
}

/// `PublishRenderer` impl that drives the project pipeline.
///
/// Derives `PublishFiles` from the `ProjectRenderSummary` (the
/// orchestrator's concrete output paths) — *not* from a filesystem
/// walk. This is the load-bearing design choice that makes the
/// trait WASM-portable in the future (a browser-side renderer has
/// no filesystem to walk).
struct ProjectPublishRenderer {
    project_dir: PathBuf,
    runtime: Arc<dyn SystemRuntime>,
}

#[async_trait]
impl PublishRenderer for ProjectPublishRenderer {
    async fn render(&self, _flags: &PublishRenderFlags) -> Result<PublishFiles, PublishError> {
        // We block in-place here because ProjectPipeline isn't
        // Send-friendly yet (uses internal !Send state in some of
        // the Pass-1 plumbing). pollster::block_on inside the async
        // fn is fine — we're already inside pollster's event loop
        // at the top level, but ProjectPipeline.run() returns its
        // own future that is sync-friendly.
        //
        // FIXME(bd-t3ny Phase 2): once ProjectPipeline is fully
        // Send + Sync, replace this with a normal `.await`.
        let project_dir = self.project_dir.clone();
        let runtime = self.runtime.clone();

        let result: Result<PublishFiles, PublishError> = pollster::block_on(async move {
            let mut project = ProjectContext::discover(&project_dir, runtime.as_ref())
                .map_err(|e| PublishError::Other(anyhow::anyhow!("{e}")))?;

            // bd-w348iu63: run `project.pre-render` scripts before
            // the pipeline, then re-discover so script-created
            // inputs are rendered and config edits are honored
            // (`project.type` / `project.output-dir` changes are
            // forbidden). Same bracket as `q2 render`'s
            // `execute_project`; a publish renders the full project.
            if !project.config.pre_render_scripts.is_empty() {
                let input_files =
                    publish_relative_paths(project.files.iter().map(|f| &f.input), &project.dir);
                let ctx = render_scripts::RenderScriptsContext {
                    project_dir: &project.dir,
                    output_dir: &project.output_dir,
                    config_path: project.config.config_path.as_deref(),
                    extension_manifest_paths: &project.config.extension_manifest_paths,
                    render_all: true,
                    quiet: false,
                    file_count: input_files.len(),
                };
                render_scripts::run_render_scripts(
                    render_scripts::ScriptPhase::PreRender,
                    &project.config.pre_render_scripts,
                    &ctx,
                    &input_files,
                )
                .map_err(|e| PublishError::Other(anyhow::anyhow!("{e}")))?;

                let re_project = ProjectContext::discover(&project_dir, runtime.as_ref())
                    .map_err(|e| PublishError::Other(anyhow::anyhow!("{e}")))?;
                render_scripts::check_forbidden_mutations(&project.config, &re_project.config)
                    .map_err(|e| PublishError::Other(anyhow::anyhow!("{e}")))?;
                project = re_project;
            }

            let project_type = project_type_for(&project);
            let format = Format::from_format_string("html")
                .map_err(|e| PublishError::Other(anyhow::anyhow!("{e}")))?;
            let format_str = format.identifier.to_string();
            let options = RenderToFileOptions::default();
            let mut pipeline = ProjectPipeline::new(
                &mut project,
                project_type,
                format,
                &format_str,
                &options,
                runtime.clone(),
            );
            let summary = pipeline
                .run()
                .await
                .map_err(|e| PublishError::Other(anyhow::anyhow!("{e}")))?;

            // bd-w348iu63: `project.post-render` scripts run after
            // the render, before the publish upload — files they add
            // to the output dir are picked up by the sidecar walk
            // below. `QUARTO_PROJECT_OUTPUT_FILES` lists the actual
            // pipeline outputs, project-relative.
            if !project.config.post_render_scripts.is_empty() {
                let output_files = publish_relative_paths(
                    summary.outputs.iter().map(|o| &o.output_path),
                    &project.dir,
                );
                let ctx = render_scripts::RenderScriptsContext {
                    project_dir: &project.dir,
                    output_dir: &project.output_dir,
                    config_path: project.config.config_path.as_deref(),
                    extension_manifest_paths: &project.config.extension_manifest_paths,
                    render_all: true,
                    quiet: false,
                    file_count: summary.outputs.len(),
                };
                render_scripts::run_render_scripts(
                    render_scripts::ScriptPhase::PostRender,
                    &project.config.post_render_scripts,
                    &ctx,
                    &output_files,
                )
                .map_err(|e| PublishError::Other(anyhow::anyhow!("{e}")))?;
            }

            // Translate the summary into PublishFiles by collecting
            // each output path relative to the project's output dir.
            let output_dir = project.output_dir.clone();
            let mut files: Vec<String> = Vec::with_capacity(summary.outputs.len());
            for o in &summary.outputs {
                let rel = o
                    .output_path
                    .strip_prefix(&output_dir)
                    .unwrap_or(&o.output_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push(rel);
            }

            // bd-o8pr Phase 4: prefer the render manifest when
            // present. The orchestrator emits
            // `.quarto/render-manifest.json` listing every published
            // resource (with origin metadata) right after the copy
            // step. Reading it here is cheap, gives us the resource
            // entries directly (no dir-walk filtering needed), and
            // means the contract between render and publish is
            // explicit. The manifest's `output` paths are project-
            // relative inside `output_dir`, so they slot into
            // `files` without further math.
            //
            // Falls back to dir-walk when the manifest is absent
            // (renders from a Quarto version older than this one).
            let manifest_path = project
                .dir
                .join(quarto_core::project_resources::RenderManifest::FILENAME);
            let used_manifest = if manifest_path.exists() {
                match std::fs::read_to_string(&manifest_path).ok().and_then(|s| {
                    quarto_core::project_resources::RenderManifest::from_json(&s).ok()
                }) {
                    Some(manifest) => {
                        for r in &manifest.resources {
                            files.push(r.output.clone());
                        }
                        true
                    }
                    None => false,
                }
            } else {
                false
            };

            // Sidecar files (themes, JS deps under site_libs/, etc.)
            // are *not* in the manifest's `resources` array — they
            // live in the artifact store and are flushed separately.
            // Walk the output dir to pick them up. (The walk is
            // bounded to the orchestrator's own output dir, not the
            // user's working tree.)
            collect_sidecar_files(&output_dir, &mut files)
                .map_err(|e| PublishError::Other(anyhow::anyhow!("{e}")))?;

            tracing::debug!(
                "publish: {} files via {} (output_dir={})",
                files.len(),
                if used_manifest {
                    "manifest + dir-walk for sidecars"
                } else {
                    "dir-walk only"
                },
                output_dir.display()
            );

            // Dedup (stable order).
            let mut seen = std::collections::HashSet::new();
            files.retain(|f| seen.insert(f.clone()));

            Ok(PublishFiles {
                base_dir: output_dir,
                root_file: "index.html".to_string(),
                files,
            })
        });

        result
    }
}

/// Make each path relative to `base` for the render-script file-list
/// contract (paths outside `base` pass through unchanged).
fn publish_relative_paths<'a>(
    paths: impl Iterator<Item = &'a PathBuf>,
    base: &std::path::Path,
) -> Vec<PathBuf> {
    paths
        .map(|p| {
            p.strip_prefix(base)
                .map_or_else(|_| p.clone(), |r| r.to_path_buf())
        })
        .collect()
}

/// Walk `output_dir` and append every regular file (relative,
/// forward-slash) to `files`.
fn collect_sidecar_files(output_dir: &std::path::Path, files: &mut Vec<String>) -> Result<()> {
    if !output_dir.exists() {
        return Ok(());
    }
    let mut stack = vec![output_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).context("reading output dir")? {
            let entry = entry?;
            let p = entry.path();
            let ft = entry.file_type()?;
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() {
                let rel = p
                    .strip_prefix(output_dir)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push(rel);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_slug_lowercases_and_dashes() {
        assert_eq!(simple_slug("Hello World"), "hello-world");
        assert_eq!(simple_slug("Title!! With  Spaces"), "title-with-spaces");
        assert_eq!(simple_slug("123 ABC"), "123-abc");
    }

    #[test]
    fn simple_slug_handles_empty_and_punctuation() {
        assert_eq!(simple_slug(""), "untitled");
        assert_eq!(simple_slug("!!!"), "untitled");
        assert_eq!(simple_slug("---"), "untitled");
    }

    /// bd-o8pr Phase 4: declared resources (project YAML +
    /// document YAML + Lua filter) flow through the orchestrator
    /// into `PublishFiles.files` via the render manifest.
    ///
    /// Exercises the production `ProjectPublishRenderer::render`
    /// path against a tiny on-disk project.
    // Multi-threaded runtime needed because `UserFiltersStage`
    // calls `tokio::task::block_in_place` for the (non-Send) Lua
    // engine, which is only valid on multi-thread.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn declared_resources_appear_in_publish_files() {
        let temp = tempfile::TempDir::new().unwrap();
        let project_dir = temp
            .path()
            .canonicalize()
            .unwrap_or_else(|_| temp.path().to_path_buf());

        // Project-level + document-level + Lua-filter-declared.
        std::fs::write(
            project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  resources:\n    - extras/notes.txt\n",
        )
        .unwrap();
        std::fs::create_dir_all(project_dir.join("extras")).unwrap();
        std::fs::write(project_dir.join("extras/notes.txt"), "ok\n").unwrap();
        std::fs::create_dir_all(project_dir.join("blob")).unwrap();
        std::fs::write(project_dir.join("blob/info.txt"), "post-blob\n").unwrap();
        std::fs::write(project_dir.join("from-filter.txt"), "filter contents\n").unwrap();
        std::fs::write(
            project_dir.join("addres.lua"),
            "local r=false\nfunction Para(p)\n  if not r then\n    quarto.doc.add_resource('from-filter.txt')\n    r=true\n  end\n  return p\nend\n",
        )
        .unwrap();
        std::fs::write(
            project_dir.join("doc.qmd"),
            "---\ntitle: Doc\nresources:\n  - blob/info.txt\nfilters:\n  - addres.lua\n---\n\nBody.\n",
        )
        .unwrap();

        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let renderer = ProjectPublishRenderer {
            project_dir: project_dir.clone(),
            runtime,
        };

        let files = renderer
            .render(&PublishRenderFlags::default())
            .await
            .expect("render");

        for expected in &["extras/notes.txt", "blob/info.txt", "from-filter.txt"] {
            assert!(
                files.files.iter().any(|f| f == expected),
                "publish file list should include '{}', got: {:?}",
                expected,
                files.files
            );
        }
        // The rendered HTML output is in there too.
        assert!(
            files.files.iter().any(|f| f == "doc.html"),
            "rendered doc.html should be in publish files, got: {:?}",
            files.files
        );
    }
}
