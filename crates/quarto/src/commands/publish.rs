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
    if let Some(meta) = project.config.metadata.as_ref() {
        if let Some(t) = quarto_core::project::website_config::website_title(meta) {
            return t;
        }
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

            // Plus any sidecar files (themes, JS deps) that landed
            // in the output dir. The orchestrator already wrote
            // them; we walk the output dir to pick them up. This
            // *is* a filesystem walk on the native path — but it's
            // bounded to the orchestrator's own output, not the
            // user's working tree. A future ProjectRenderSummary
            // extension that returns a manifest of sidecar files
            // can replace the walk.
            collect_sidecar_files(&output_dir, &mut files)
                .map_err(|e| PublishError::Other(anyhow::anyhow!("{e}")))?;

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
}
