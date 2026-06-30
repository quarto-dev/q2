/*
 * stage/stages/document_profile.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Document-profile checkpoint stage.
 */

//! Pipeline checkpoint that extracts a static [`DocumentProfile`].
//!
//! This stage runs immediately after [`MetadataMergeStage`] and before
//! any AST-mutating stage. It produces a [`DocumentAtProfile`] that
//! carries both the extracted profile and the original `DocumentAst`.
//! The immediately following [`UnwrapProfileStage`] hands the `DocumentAst`
//! back to downstream stages so they keep their existing input type.
//!
//! This two-stage split is the Phase-0 vehicle for Option A in the
//! Phase-0 plan (see `claude-notes/plans/2026-04-23-websites-phase-0.md`
//! §Shape): the pipeline *type* records the checkpoint, so Phase 1 can
//! build a two-pass orchestrator that consumes `AtProfile` directly,
//! while Phase 0 touches no downstream stage signatures.
//!
//! [`MetadataMergeStage`]: crate::stage::MetadataMergeStage
//! [`UnwrapProfileStage`]: crate::stage::stages::UnwrapProfileStage

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;

use crate::document_profile::DocumentProfile;
use crate::stage::{
    DocumentAtProfile, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};

/// Pipeline stage that extracts the static document profile.
///
/// Input kind: [`PipelineDataKind::DocumentAst`].
/// Output kind: [`PipelineDataKind::AtProfile`].
pub struct DocumentProfileStage;

impl DocumentProfileStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DocumentProfileStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for DocumentProfileStage {
    fn name(&self) -> &str {
        "document-profile"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::AtProfile
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        let source_path = project_relative_source(&doc.path, &ctx.project.dir);
        let output_href = project_relative_output_href(&ctx.output_path(), &ctx.project.output_dir);
        let format_id = ctx.format.target_format.clone();

        let mut profile =
            DocumentProfile::extract(&doc.ast, &source_path, &output_href, &format_id);

        // Drain the include side-channel populated by
        // `IncludeExpansionStage` into the profile so cache-key
        // construction and `bd-r82e` invalidation can see every
        // (transitive) child file the parent depends on.
        profile.includes = std::mem::take(&mut doc.recorded_includes);

        Ok(PipelineData::AtProfile(DocumentAtProfile {
            profile,
            ast: doc,
        }))
    }
}

/// Compute the project-relative source path used in a profile.
///
/// Per the Phase-0 plan §"Project root invariant": there is no "no
/// project" branch. A single-file render has `project.dir` equal to
/// the file's directory, so stripping yields just the file name.
/// Any absolute-or-outside path falls back to the file name alone so
/// the profile remains portable.
fn project_relative_source(source: &Path, project_dir: &Path) -> PathBuf {
    if let Ok(rel) = source.strip_prefix(project_dir) {
        rel.to_path_buf()
    } else if let Some(name) = source.file_name() {
        PathBuf::from(name)
    } else {
        source.to_path_buf()
    }
}

/// Compute the project-output-relative `output_href` used in a profile.
///
/// Forward-slash separated so the value is directly usable as an href
/// in rendered HTML and as a cache key across platforms.
fn project_relative_output_href(output_path: &Path, project_output_dir: &Path) -> String {
    let rel = output_path.strip_prefix(project_output_dir).map_or_else(
        |_| {
            output_path
                .file_name()
                .map_or_else(|| output_path.to_path_buf(), PathBuf::from)
        },
        |p| p.to_path_buf(),
    );
    to_forward_slash(&rel)
}

fn to_forward_slash(path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => parts.push("..".to_string()),
            Component::Prefix(_) | Component::RootDir => {
                // A project-relative path should never contain a
                // root/prefix component. If it does, fall back to a
                // raw, platform-flavored string so callers see the
                // anomaly instead of silently losing it.
                parts.push(component.as_os_str().to_string_lossy().into_owned());
            }
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::stage::DocumentAst;
    use pampa::pandoc::ASTContext;
    use quarto_error_reporting::DiagnosticMessage;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::SourceContext;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Minimal mock runtime. Mirrors the pattern used elsewhere in
    /// `stage/stages/*` so tests stay self-contained.
    struct MockRuntime;

    #[async_trait::async_trait]
    impl quarto_system_runtime::SystemRuntime for MockRuntime {
        fn file_read(&self, _p: &std::path::Path) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn file_write(
            &self,
            _p: &std::path::Path,
            _c: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_exists(
            &self,
            _p: &std::path::Path,
            _k: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            Ok(true)
        }
        fn canonicalize(
            &self,
            p: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(p.to_path_buf())
        }
        fn path_metadata(
            &self,
            _p: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            unimplemented!()
        }
        fn file_copy(
            &self,
            _s: &std::path::Path,
            _d: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_rename(
            &self,
            _o: &std::path::Path,
            _n: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn file_remove(&self, _p: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_create(
            &self,
            _p: &std::path::Path,
            _r: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_remove(
            &self,
            _p: &std::path::Path,
            _r: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_list(
            &self,
            _p: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            Ok(vec![])
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/"))
        }
        fn temp_dir(
            &self,
            _t: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            Ok(quarto_system_runtime::TempDir::new(PathBuf::from(
                "/tmp/test",
            )))
        }
        fn exec_pipe(
            &self,
            _c: &str,
            _a: &[&str],
            _s: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn exec_command(
            &self,
            _c: &str,
            _a: &[&str],
            _s: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            Ok(quarto_system_runtime::CommandOutput {
                code: 0,
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn env_get(&self, _n: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
            Ok(None)
        }
        fn env_all(
            &self,
        ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
        {
            Ok(std::collections::HashMap::new())
        }
        async fn fetch_url(
            &self,
            _u: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            Err(quarto_system_runtime::RuntimeError::NotSupported(
                "mock".into(),
            ))
        }
        fn os_name(&self) -> &'static str {
            "mock"
        }
        fn arch(&self) -> &'static str {
            "mock"
        }
        fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
            Ok(0)
        }
        fn xdg_dir(
            &self,
            _k: quarto_system_runtime::XdgDirKind,
            _s: Option<&std::path::Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/xdg"))
        }
        fn stdout_write(&self, _d: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _d: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
    }

    fn make_ctx() -> StageContext {
        let runtime = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/sub/page.qmd");
        let format = Format::html();
        StageContext::new(runtime, format, project, doc).unwrap()
    }

    fn make_doc_ast() -> DocumentAst {
        DocumentAst {
            path: PathBuf::from("/project/sub/page.qmd"),
            ast: Pandoc::default(),
            ast_context: ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
        }
    }

    #[tokio::test]
    async fn stage_extracts_profile_from_document_ast() {
        let mut ctx = make_ctx();
        let stage = DocumentProfileStage::new();

        let out = stage
            .run(PipelineData::DocumentAst(make_doc_ast()), &mut ctx)
            .await
            .expect("stage runs");

        assert_eq!(out.kind(), PipelineDataKind::AtProfile);
        let bundle = out.into_at_profile().expect("variant is AtProfile");
        // Source path is project-relative, forward-slash.
        assert_eq!(
            bundle.profile.source_path,
            PathBuf::from("sub/page.qmd"),
            "source_path is project-relative"
        );
        // Output href: the default HTML format writes alongside the
        // source with an .html extension; project output_dir == project
        // dir here, so the relative output mirrors the source.
        assert_eq!(
            bundle.profile.output_href, "sub/page.html",
            "output_href is project-output-relative with .html"
        );
        assert_eq!(bundle.profile.format_id, "html");
        assert_eq!(bundle.profile.profile_version, DocumentProfile::VERSION);
    }

    #[tokio::test]
    async fn stage_rejects_wrong_input_kind() {
        let mut ctx = make_ctx();
        let stage = DocumentProfileStage::new();

        let wrong_input = PipelineData::LoadedSource(crate::stage::LoadedSource::new(
            PathBuf::from("/project/test.qmd"),
            b"# Hi".to_vec(),
        ));

        let err = stage
            .run(wrong_input, &mut ctx)
            .await
            .expect_err("stage must reject non-DocumentAst input with UnexpectedInput");

        // The error carries input-kind context; assert on its Display.
        let msg = err.to_string();
        assert!(
            msg.contains("document-profile"),
            "error mentions stage name: {msg}"
        );
        assert!(
            msg.contains("DocumentAst") && msg.contains("LoadedSource"),
            "error mentions both expected and actual kinds: {msg}"
        );
    }

    #[tokio::test]
    async fn stage_preserves_warnings() {
        let mut ctx = make_ctx();
        let stage = DocumentProfileStage::new();

        let mut doc = make_doc_ast();
        let warn_in = DiagnosticMessage::warning("pretend a prior stage saw this");
        doc.warnings.push(warn_in.clone());

        let out = stage
            .run(PipelineData::DocumentAst(doc), &mut ctx)
            .await
            .unwrap();
        let bundle = out.into_at_profile().unwrap();

        assert_eq!(
            bundle.ast.warnings.len(),
            1,
            "profile stage must preserve upstream warnings on the inner AST"
        );
        assert_eq!(bundle.ast.warnings[0].title, warn_in.title);
    }

    // --- Sanity tests for the path-normalization helpers. ------------
    // These aren't in the Phase-0 plan's numbered list but codify the
    // "no project-root branch" invariant from §"Project root invariant".

    #[test]
    fn project_relative_source_strips_project_dir() {
        let p = project_relative_source(Path::new("/proj/sub/page.qmd"), Path::new("/proj"));
        assert_eq!(p, PathBuf::from("sub/page.qmd"));
    }

    #[test]
    fn project_relative_source_single_file_root_is_file_name() {
        // Bare-file case: project.dir equals the file's parent dir, so
        // stripping yields just the file name. No branch on "is this a
        // project?" — the math works uniformly.
        let p = project_relative_source(Path::new("/some/where/doc.qmd"), Path::new("/some/where"));
        assert_eq!(p, PathBuf::from("doc.qmd"));
    }

    #[test]
    fn project_relative_source_unrelated_path_falls_back_to_file_name() {
        let p = project_relative_source(Path::new("/elsewhere/page.qmd"), Path::new("/proj"));
        assert_eq!(p, PathBuf::from("page.qmd"));
    }

    #[test]
    fn project_relative_output_href_uses_forward_slash() {
        let s = project_relative_output_href(Path::new("/proj/sub/page.html"), Path::new("/proj"));
        assert_eq!(s, "sub/page.html");
    }
}
