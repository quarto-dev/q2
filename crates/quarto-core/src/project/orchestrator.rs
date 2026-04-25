/*
 * project/orchestrator.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `ProjectType` trait + two-pass driver.
 */

//! Project orchestration.
//!
//! # Two passes
//!
//! The driver ([`ProjectPipeline`]) runs every file in a project
//! through **two passes**:
//!
//! - **Pass 1** advances each file as far as
//!   [`DocumentProfileStage`](crate::stage::DocumentProfileStage) —
//!   parse + metadata merge only. Pass 1 extracts each file's
//!   [`DocumentProfile`](crate::document_profile::DocumentProfile),
//!   collecting the full `Vec` into a
//!   [`ProjectIndex`](super::index::ProjectIndex).
//! - **Pass 2** runs the full per-file render, with the Pass-1
//!   `ProjectIndex` available on
//!   [`StageContext::project_index`](crate::stage::StageContext).
//!   Phase-1 stages do not consume it; Phase-2+ (sidebar generate,
//!   cross-doc link rewriting) will.
//!
//! Between the two passes, the project's [`ProjectType`]
//! implementation runs its `pre_render` hook. After Pass 2 it runs
//! `post_render`.
//!
//! # Pass-2 resumption (v1)
//!
//! Phase 1 v1 re-runs the head pipeline inside Pass 2. This wastes a
//! parse + metadata-merge per file versus resuming from the cloned
//! `PipelineData::AtProfile`. The re-work is accepted for v1 because
//! it keeps the CLI rewiring scoped to *orchestration* — threading a
//! pre-built `AtProfile` through `render_document_to_file` is a
//! separate refactor.  A follow-up beads issue tracks the
//! optimization.

use async_trait::async_trait;

use quarto_error_reporting::DiagnosticMessage;

use crate::Result;

use super::index::ProjectIndex;
use super::{ProjectContext, ProjectKind};

// Native-only bits: the Pass-2 path calls `render_document_to_file`,
// which isn't compiled for WASM. Hub-client orchestration (Phase 9)
// will wire its own VFS-aware entry points.
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use quarto_system_runtime::SystemRuntime;

#[cfg(not(target_arch = "wasm32"))]
use crate::error::QuartoError;

#[cfg(not(target_arch = "wasm32"))]
use crate::format::Format;

#[cfg(not(target_arch = "wasm32"))]
use crate::render_to_file::{RenderToFileOptions, RenderToFileResult, render_document_to_file};

#[cfg(not(target_arch = "wasm32"))]
use super::DocumentInfo;

// WASM-visible placeholder for `RenderToFileResult` so the trait can
// still compile under `target_arch = "wasm32"`. Phase 9 replaces this
// with a VFS-aware output type.
#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
pub struct RenderToFileResult;

/// Orchestration hooks implemented by each project kind.
///
/// Phase-1 ships [`DefaultProjectType`] (no-op hooks used for
/// single-file and loose-directory renders) and
/// [`WebsiteProjectType`] (identical placeholder; Phase-2+ will fill
/// in the website-specific hooks).
///
/// Trait methods are `async` because future website hooks (sitemap
/// writing, favicon copying, remote resource fetches) want async I/O.
/// The no-op default implementations mean current callers pay zero
/// cost. The `?Send` bound matches the pipeline's own stage trait so
/// an eventual `rayon + pollster-per-worker` parallelism path does
/// not require migrating the rest of the stage graph.
#[async_trait(?Send)]
pub trait ProjectType {
    /// The tag this implementation serves.
    fn kind(&self) -> ProjectKind;

    /// Name of the project's shared "lib" directory (e.g.
    /// `"site_libs"` for websites). The empty string indicates
    /// the project type has **no** shared lib directory: in that
    /// case [`ArtifactScope::Project`] artifacts resolve under
    /// the same per-page resource directory as
    /// [`ArtifactScope::Page`] artifacts (preserving pre-Phase-5
    /// single-doc behavior).
    ///
    /// Returns an owned `String` rather than `&'static str` so
    /// implementations can later read the value from
    /// [`ProjectContext::config`] when the user-config override
    /// (`project.lib-dir:`) lands without churning this trait
    /// signature.
    ///
    /// See `claude-notes/plans/2026-04-24-websites-phase-5.md`
    /// Decision 4 for the design rationale.
    ///
    /// [`ArtifactScope::Project`]: crate::artifact::ArtifactScope::Project
    /// [`ArtifactScope::Page`]: crate::artifact::ArtifactScope::Page
    fn lib_dir(&self) -> String {
        String::new()
    }

    /// Called once per project, after Pass 1 and before Pass 2.
    /// Default: no-op.
    async fn pre_render(&self, _project: &mut ProjectContext, _index: &ProjectIndex) -> Result<()> {
        Ok(())
    }

    /// Called once per project, after Pass 2. Default: no-op.
    ///
    /// **Phase 5:** receives the orchestrator's project-wide
    /// artifact accumulator (filled by per-doc Pass-2 renders
    /// merging their drained Project-scoped artifacts). Website
    /// projects flush this to `{output_dir}/{lib_dir}/...`.
    /// Default projects ignore it (single-doc renders flush
    /// per-doc inside [`render_document_to_file`] when no
    /// orchestrator is involved).
    async fn post_render(
        &self,
        _project: &ProjectContext,
        _index: &ProjectIndex,
        _outputs: &[RenderToFileResult],
        _project_artifacts: &crate::artifact::ArtifactStore,
        _runtime: &dyn quarto_system_runtime::SystemRuntime,
    ) -> Result<()> {
        Ok(())
    }
}

/// No-op orchestration used for the [`ProjectKind::Default`] tag.
///
/// Every single-file and loose-directory render in Phase 1 runs
/// through this type. The orchestrator invariant from Phase 0
/// ("no `is_project?` branch") is satisfied because even a bare file
/// is just a `DefaultProjectType` project with one entry in
/// `project.files`.
pub struct DefaultProjectType;

#[async_trait(?Send)]
impl ProjectType for DefaultProjectType {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Default
    }
}

/// Placeholder website implementation for Phase 1.
///
/// Phase 1 only needs the tag to dispatch correctly. Phase 2 adds
/// sidebar / navbar generate transforms, Phase 5 adds `site_libs/`,
/// Phase 7 adds the post-render hooks (sitemap, favicon).
pub struct WebsiteProjectType;

#[async_trait(?Send)]
impl ProjectType for WebsiteProjectType {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Website
    }

    fn lib_dir(&self) -> String {
        "site_libs".to_string()
    }

    /// Phase 5: flush every Project-scoped artifact accumulated
    /// across Pass-2 renders to disk under `_site/site_libs/`.
    ///
    /// Each artifact's relative `path` is joined onto
    /// `{output_dir}/{lib_dir}/`. Iteration is in sorted-key
    /// order so the on-disk write order is deterministic across
    /// runs.
    #[cfg(not(target_arch = "wasm32"))]
    async fn post_render(
        &self,
        project: &ProjectContext,
        _index: &ProjectIndex,
        _outputs: &[RenderToFileResult],
        project_artifacts: &crate::artifact::ArtifactStore,
        runtime: &dyn SystemRuntime,
    ) -> Result<()> {
        if project_artifacts.is_empty() {
            return Ok(());
        }

        let lib_root = project.output_dir.join(self.lib_dir());

        let mut entries: Vec<(&str, &crate::artifact::Artifact)> =
            project_artifacts.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));

        for (_, artifact) in entries {
            let Some(path) = &artifact.path else { continue };
            let on_disk = lib_root.join(path);
            if let Some(parent) = on_disk.parent() {
                runtime.dir_create(parent, true).map_err(|e| {
                    crate::error::QuartoError::other(format!(
                        "Failed to create site_libs subdirectory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
            runtime
                .file_write(&on_disk, &artifact.content)
                .map_err(|e| {
                    crate::error::QuartoError::other(format!(
                        "Failed to write site_libs artifact {}: {}",
                        on_disk.display(),
                        e
                    ))
                })?;
        }
        Ok(())
    }
}

/// Factory: pick a [`ProjectType`] based on the project's tag.
///
/// Unknown / not-yet-implemented tags fall back to
/// [`DefaultProjectType`] so Phase 1 doesn't crash on `_quarto.yml`
/// files declaring `project.type: book` or `project.type: manuscript`
/// — those kinds are tracked by Phase-1 dispatch but have no behavior
/// yet.
pub fn project_type_for(project: &ProjectContext) -> Box<dyn ProjectType> {
    match project.project_kind() {
        ProjectKind::Default => Box::new(DefaultProjectType),
        ProjectKind::Website => Box::new(WebsiteProjectType),
        ProjectKind::Book | ProjectKind::Manuscript => Box::new(DefaultProjectType),
    }
}

/// Error reported for a single file whose Pass-2 render failed.
#[derive(Debug)]
pub struct FileFailure {
    pub input: std::path::PathBuf,
    pub error: String,
    pub diagnostics: Vec<DiagnosticMessage>,
}

/// Result of a full project render.
#[derive(Debug, Default)]
pub struct ProjectRenderSummary {
    /// Successful per-file outputs (in `project.files` order).
    pub outputs: Vec<RenderToFileResult>,
    /// Pass-1 files that could not be profiled. These are dropped
    /// from the index but do not abort the run.
    pub pass1_failures: Vec<FileFailure>,
    /// Pass-2 files that failed to render. The CLI decides whether
    /// this is a non-zero exit.
    pub pass2_failures: Vec<FileFailure>,
}

impl ProjectRenderSummary {
    /// True if any file (Pass 1 or Pass 2) failed.
    pub fn has_failures(&self) -> bool {
        !self.pass1_failures.is_empty() || !self.pass2_failures.is_empty()
    }
}

/// Two-pass project render driver (native only for Phase 1).
///
/// Wraps a [`ProjectContext`] and a [`ProjectType`] implementation,
/// runs Pass 1 over every file in `project.files`, builds a
/// [`ProjectIndex`], invokes `pre_render`, runs Pass 2, then
/// `post_render`.
///
/// Phase-1 restriction: **sequential**. A follow-up beads issue
/// tracks `rayon + pollster-per-worker` parallelism.
///
/// WASM note: the driver exists only on native targets — hub-client
/// orchestration is Phase 9 of the epic and will wire its own
/// VFS-aware entry points (`build_project_nav`, `render_page_in_project`).
#[cfg(not(target_arch = "wasm32"))]
pub struct ProjectPipeline<'a> {
    project: &'a mut ProjectContext,
    project_type: Box<dyn ProjectType>,
    format: Format,
    format_str: String,
    options: &'a RenderToFileOptions,
    runtime: Arc<dyn SystemRuntime>,
    /// Project-wide artifact accumulator (Phase 5).
    ///
    /// Project-scoped artifacts produced by per-doc Pass-2
    /// renders are drained from the per-doc `StageContext` and
    /// merged into this store by the orchestrator. After Pass 2
    /// completes, [`ProjectType::post_render`] flushes the
    /// accumulated artifacts to disk.
    ///
    /// The orchestrator is the **only** owner that mutates this
    /// store; per-doc workers never touch it. This is what makes
    /// the design ready for future rayon-per-worker parallelism
    /// without redesign — see
    /// `claude-notes/plans/2026-04-24-websites-phase-5.md`
    /// Decision 2.
    project_artifacts: crate::artifact::ArtifactStore,
}

#[cfg(not(target_arch = "wasm32"))]
impl<'a> ProjectPipeline<'a> {
    pub fn new(
        project: &'a mut ProjectContext,
        project_type: Box<dyn ProjectType>,
        format: Format,
        format_str: impl Into<String>,
        options: &'a RenderToFileOptions,
        runtime: Arc<dyn SystemRuntime>,
    ) -> Self {
        Self {
            project,
            project_type,
            format,
            format_str: format_str.into(),
            options,
            runtime,
            project_artifacts: crate::artifact::ArtifactStore::new(),
        }
    }

    /// Read-only view of the project-wide artifact accumulator
    /// (Phase 5). Useful for tests and for `post_render`
    /// implementations that take `&self` on the trait.
    pub fn project_artifacts(&self) -> &crate::artifact::ArtifactStore {
        &self.project_artifacts
    }

    /// Run Pass 1 → `pre_render` → Pass 2 → `post_render`.
    pub async fn run(&mut self) -> Result<ProjectRenderSummary> {
        let (profiles, pass1_failures) = self.pass_one().await;
        let index = Arc::new(ProjectIndex::new(profiles));

        // Map hook errors through so the caller sees exactly which
        // hook failed. The plan specifies hook failures abort the
        // project render entirely (unlike per-file failures).
        self.project_type
            .pre_render(self.project, &index)
            .await
            .map_err(|e| QuartoError::other(format!("pre_render failed: {e}")))?;

        // Skip Pass-2 on files that failed Pass 1 — Pass 2 does
        // strictly more work, so it can only produce duplicate errors.
        let skip: std::collections::HashSet<std::path::PathBuf> =
            pass1_failures.iter().map(|f| f.input.clone()).collect();
        let (outputs, pass2_failures) = self.pass_two(index.clone(), &skip).await;

        self.project_type
            .post_render(
                self.project,
                &index,
                &outputs,
                &self.project_artifacts,
                self.runtime.as_ref(),
            )
            .await
            .map_err(|e| QuartoError::other(format!("post_render failed: {e}")))?;

        Ok(ProjectRenderSummary {
            outputs,
            pass1_failures,
            pass2_failures,
        })
    }

    /// Advance every file to the profile checkpoint, collecting
    /// profiles and any per-file failures.
    async fn pass_one(
        &self,
    ) -> (
        Vec<crate::document_profile::DocumentProfile>,
        Vec<FileFailure>,
    ) {
        let mut profiles = Vec::with_capacity(self.project.files.len());
        let mut failures = Vec::new();
        for doc_info in &self.project.files {
            match self.profile_single_file(doc_info).await {
                Ok(profile) => profiles.push(profile),
                Err(e) => failures.push(FileFailure {
                    input: doc_info.input.clone(),
                    error: e.to_string(),
                    diagnostics: Vec::new(),
                }),
            }
        }
        (profiles, failures)
    }

    /// Profile one file by running the head pipeline up through
    /// [`DocumentProfileStage`](crate::stage::DocumentProfileStage).
    async fn profile_single_file(
        &self,
        doc_info: &DocumentInfo,
    ) -> Result<crate::document_profile::DocumentProfile> {
        use crate::pipeline::run_pipeline;
        use crate::render::{BinaryDependencies, RenderContext};
        use crate::stage::{
            DocumentProfileStage, MetadataMergeStage, ParseDocumentStage, PipelineStage,
        };

        let content = self.runtime.file_read(&doc_info.input).map_err(|e| {
            QuartoError::other(format!(
                "Failed to read {} during pass 1: {}",
                doc_info.input.display(),
                e
            ))
        })?;
        let source_name = doc_info.input.to_string_lossy().to_string();

        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(self.project, doc_info, &self.format, &binaries);

        let stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(ParseDocumentStage::new()),
            Box::new(MetadataMergeStage::new()),
            Box::new(DocumentProfileStage::new()),
        ];

        let (output, _diagnostics) = run_pipeline(
            &content,
            &source_name,
            &mut ctx,
            self.runtime.clone(),
            stages,
        )
        .await?;

        // Extract the `DocumentProfile` from the `AtProfile` variant.
        let profile = output.into_at_profile().ok_or_else(|| {
            QuartoError::other(
                "Pass 1 did not produce an AtProfile variant — pipeline shape unexpected",
            )
        })?;
        Ok(profile.profile)
    }

    /// Re-render every file under the built `ProjectIndex`,
    /// skipping files that failed Pass 1.
    ///
    /// **Phase 5:** each per-doc render drains its
    /// Project-scoped artifacts into the orchestrator's
    /// `project_artifacts` accumulator. The merge is sequential
    /// (no shared mutable state during render) so the function
    /// composes with future rayon-per-worker parallelism — see
    /// `claude-notes/plans/2026-04-24-websites-phase-5.md`
    /// Decision 2.
    async fn pass_two(
        &mut self,
        index: Arc<ProjectIndex>,
        skip: &std::collections::HashSet<std::path::PathBuf>,
    ) -> (Vec<RenderToFileResult>, Vec<FileFailure>) {
        let mut outputs = Vec::with_capacity(self.project.files.len());
        let mut failures = Vec::new();
        // Snapshot the file list to avoid borrowing `self.project`
        // while we also mutate `self.project_artifacts`.
        let files: Vec<crate::project::DocumentInfo> = self.project.files.clone();
        for doc_info in &files {
            if skip.contains(&doc_info.input) {
                continue;
            }
            match render_document_to_file(
                &doc_info.input,
                &self.format_str,
                self.options,
                Some(self.project),
                self.runtime.clone(),
                Some(index.clone()),
                Some(&mut self.project_artifacts),
            ) {
                Ok(result) => outputs.push(result),
                Err(e) => failures.push(FileFailure {
                    input: doc_info.input.clone(),
                    error: e.to_string(),
                    diagnostics: Vec::new(),
                }),
            }
        }
        (outputs, failures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;
    use std::path::PathBuf;

    #[test]
    fn default_project_type_reports_kind() {
        let t = DefaultProjectType;
        assert_eq!(t.kind(), ProjectKind::Default);
    }

    #[test]
    fn website_project_type_reports_kind() {
        let t = WebsiteProjectType;
        assert_eq!(t.kind(), ProjectKind::Website);
    }

    #[tokio::test]
    async fn default_project_type_hooks_are_no_ops() {
        // Build a minimal `ProjectContext` so the trait methods type-check.
        let runtime = NativeRuntime::new();
        let mut project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: Vec::new(),
            output_dir: PathBuf::from("/project"),
        };
        let t = DefaultProjectType;
        let index = ProjectIndex::default();
        let project_artifacts = crate::artifact::ArtifactStore::new();

        assert!(t.pre_render(&mut project, &index).await.is_ok());
        assert!(
            t.post_render(&project, &index, &[], &project_artifacts, &runtime)
                .await
                .is_ok()
        );
    }

    #[test]
    fn project_kind_string_roundtrip_still_holds() {
        // Rename-regression guard: `ProjectKind::try_from` must still
        // accept all canonical strings and round-trip through
        // `as_str`.
        for expected in [
            ProjectKind::Default,
            ProjectKind::Website,
            ProjectKind::Book,
            ProjectKind::Manuscript,
        ] {
            let s = expected.as_str();
            let back = ProjectKind::try_from(s).unwrap();
            assert_eq!(back, expected);
        }
    }

    // === Phase 5 Decision 4: ProjectType::lib_dir ===

    /// Plan test 15: WebsiteProjectType reports `"site_libs"`.
    #[test]
    fn website_project_type_lib_dir_is_site_libs() {
        let t = WebsiteProjectType;
        assert_eq!(t.lib_dir(), "site_libs");
    }

    /// Plan test 16: DefaultProjectType reports the empty
    /// string — its [`ArtifactScope::Project`] artifacts fall
    /// back to the per-page resource directory.
    #[test]
    fn default_project_type_lib_dir_is_empty() {
        let t = DefaultProjectType;
        assert_eq!(t.lib_dir(), "");
    }

    #[test]
    fn factory_dispatches_by_kind() {
        let make = |kind: ProjectKind| ProjectContext {
            dir: PathBuf::from("/p"),
            config: crate::project::ProjectConfig {
                project_kind: kind,
                ..Default::default()
            },
            is_single_file: false,
            files: Vec::new(),
            output_dir: PathBuf::from("/p"),
        };
        assert_eq!(
            project_type_for(&make(ProjectKind::Default)).kind(),
            ProjectKind::Default
        );
        assert_eq!(
            project_type_for(&make(ProjectKind::Website)).kind(),
            ProjectKind::Website
        );
        // Book / Manuscript fall back to Default for Phase 1.
        assert_eq!(
            project_type_for(&make(ProjectKind::Book)).kind(),
            ProjectKind::Default
        );
        assert_eq!(
            project_type_for(&make(ProjectKind::Manuscript)).kind(),
            ProjectKind::Default
        );
    }
}
