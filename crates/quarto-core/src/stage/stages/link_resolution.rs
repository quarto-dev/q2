/*
 * stage/stages/link_resolution.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pass-1 stage that walks the AST and records every cross-document
 * body-link target into `DocumentProfile.body_link_targets`.
 */

//! Pass-1 link resolution stage.
//!
//! Runs immediately after [`DocumentProfileStage`], on the
//! `AtProfile` bundle. Walks every `Inline::Link` in the AST,
//! calls
//! [`resolve_doc_relative_target`](crate::transforms::navigation_href::resolve_doc_relative_target),
//! deduplicates the resulting target paths, and writes them into
//! `bundle.profile.body_link_targets`.
//!
//! This is the **read-only**, side-effect-free counterpart to
//! Phase 6's [`LinkRewriteTransform`](crate::transforms::link_rewrite::LinkRewriteTransform),
//! which mutates the AST in Pass-2. The two share the same
//! resolution helper so the dependency-graph view of body-link
//! edges is consistent with what the rendered output actually
//! links to. See
//! `claude-notes/designs/body-link-resolution-contract.md` for the
//! prose contract.
//!
//! ## Inputs / outputs
//!
//! - Input kind:  [`PipelineDataKind::AtProfile`] —
//!   the bundle produced by `DocumentProfileStage`.
//! - Output kind: [`PipelineDataKind::AtProfile`] — same shape,
//!   `bundle.profile.body_link_targets` populated.
//!
//! ## Behavior in absence of a project index
//!
//! Without a `ProjectIndex` (standalone single-doc render), there
//! is nothing to resolve against — every internal `.qmd` reference
//! becomes "no profile to link to," and the resulting target set is
//! empty. The stage runs anyway (no need to branch in the pipeline
//! builder) and writes an empty list. This matches the Phase-0
//! "no project root branch" invariant.

use std::path::PathBuf;

use async_trait::async_trait;
use quarto_pandoc_types::Slot;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{Inline, Inlines};

use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};
use crate::transforms::navigation_href::resolve_doc_relative_target;

pub struct LinkResolutionStage;

impl LinkResolutionStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinkResolutionStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for LinkResolutionStage {
    fn name(&self) -> &str {
        "link-resolution"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::AtProfile
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::AtProfile
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::AtProfile(mut bundle) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Source path the link helper expects: project-relative,
        // forward-slash. The profile has it in canonical form
        // already (Phase-0 invariant).
        let source_relative = bundle.profile.source_path.to_string_lossy().to_string();

        // The dependency graph needs sibling profiles to look up
        // against. With no `ProjectIndex` the target set is empty
        // (we leave `body_link_targets` as the empty Vec the
        // profile already carries from `extract`).
        if let Some(index) = ctx.project_index.as_deref() {
            let mut collector = TargetCollector {
                source: &source_relative,
                index,
                seen: Vec::new(),
            };
            for block in &bundle.ast.ast.blocks {
                collector.visit_block(block);
            }
            bundle.profile.body_link_targets = collector.seen;
        }

        Ok(PipelineData::AtProfile(bundle))
    }
}

/// Walks the AST collecting unique project-relative `.qmd` link
/// targets. Order-preserving (first-occurrence-wins) so the
/// resulting list is deterministic across runs.
struct TargetCollector<'a> {
    source: &'a str,
    index: &'a crate::project::index::ProjectIndex,
    seen: Vec<PathBuf>,
}

impl<'a> TargetCollector<'a> {
    fn record(&mut self, raw: &str) {
        if let Some(target) = resolve_doc_relative_target(raw, self.source, self.index) {
            if !self.seen.contains(&target) {
                self.seen.push(target);
            }
        }
    }

    fn visit_block(&mut self, block: &Block) {
        match block {
            Block::Plain(p) => self.visit_inlines(&p.content),
            Block::Paragraph(p) => self.visit_inlines(&p.content),
            Block::LineBlock(lb) => {
                for line in lb.content.iter() {
                    self.visit_inlines(line);
                }
            }
            Block::BlockQuote(bq) => {
                for b in bq.content.iter() {
                    self.visit_block(b);
                }
            }
            Block::OrderedList(ol) => {
                for item in ol.content.iter() {
                    for b in item.iter() {
                        self.visit_block(b);
                    }
                }
            }
            Block::BulletList(bl) => {
                for item in bl.content.iter() {
                    for b in item.iter() {
                        self.visit_block(b);
                    }
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in dl.content.iter() {
                    self.visit_inlines(term);
                    for def in defs.iter() {
                        for b in def.iter() {
                            self.visit_block(b);
                        }
                    }
                }
            }
            Block::Header(h) => self.visit_inlines(&h.content),
            Block::Div(d) => {
                for b in d.content.iter() {
                    self.visit_block(b);
                }
            }
            Block::Figure(f) => {
                for b in f.content.iter() {
                    self.visit_block(b);
                }
            }
            Block::Table(t) => {
                if let Some(short) = t.caption.short.as_ref() {
                    self.visit_inlines(short);
                }
                if let Some(long) = t.caption.long.as_ref() {
                    for b in long.iter() {
                        self.visit_block(b);
                    }
                }
                for row in t.head.rows.iter().chain(t.foot.rows.iter()) {
                    for cell in row.cells.iter() {
                        for b in cell.content.iter() {
                            self.visit_block(b);
                        }
                    }
                }
                for body in t.bodies.iter() {
                    for row in body.body.iter() {
                        for cell in row.cells.iter() {
                            for b in cell.content.iter() {
                                self.visit_block(b);
                            }
                        }
                    }
                }
            }
            Block::CaptionBlock(cb) => self.visit_inlines(&cb.content),
            Block::Custom(c) => {
                for (_name, slot) in c.slots.iter() {
                    self.visit_slot(slot);
                }
            }
            Block::CodeBlock(_)
            | Block::RawBlock(_)
            | Block::HorizontalRule(_)
            | Block::BlockMetadata(_)
            | Block::NoteDefinitionPara(_)
            | Block::NoteDefinitionFencedBlock(_) => {}
        }
    }

    fn visit_inlines(&mut self, inlines: &Inlines) {
        for inline in inlines.iter() {
            self.visit_inline(inline);
        }
    }

    fn visit_inline(&mut self, inline: &Inline) {
        match inline {
            Inline::Link(link) => {
                self.record(&link.target.0);
                // Link content can itself contain inlines (rich
                // anchor text). Walk so nested Inline::Link nodes
                // are reached too. (Pandoc does allow link-in-link
                // through some custom-node paths.)
                self.visit_inlines(&link.content);
            }
            Inline::Image(img) => {
                // Walk image alt-text inlines; leave img.target.0
                // alone — images point at static resources, not
                // project documents (Phase 6 parity).
                self.visit_inlines(&img.content);
            }
            Inline::Emph(e) => self.visit_inlines(&e.content),
            Inline::Underline(u) => self.visit_inlines(&u.content),
            Inline::Strong(s) => self.visit_inlines(&s.content),
            Inline::Strikeout(s) => self.visit_inlines(&s.content),
            Inline::Superscript(s) => self.visit_inlines(&s.content),
            Inline::Subscript(s) => self.visit_inlines(&s.content),
            Inline::SmallCaps(s) => self.visit_inlines(&s.content),
            Inline::Quoted(q) => self.visit_inlines(&q.content),
            Inline::Span(s) => self.visit_inlines(&s.content),
            Inline::Insert(i) => self.visit_inlines(&i.content),
            Inline::Delete(d) => self.visit_inlines(&d.content),
            Inline::Highlight(h) => self.visit_inlines(&h.content),
            Inline::Note(n) => {
                for b in n.content.iter() {
                    self.visit_block(b);
                }
            }
            Inline::Custom(c) => {
                for (_name, slot) in c.slots.iter() {
                    self.visit_slot(slot);
                }
            }
            // No-op variants: leaves with no rewritable nested content.
            Inline::Str(_)
            | Inline::Cite(_)
            | Inline::Code(_)
            | Inline::Space(_)
            | Inline::SoftBreak(_)
            | Inline::LineBreak(_)
            | Inline::Math(_)
            | Inline::RawInline(_)
            | Inline::Shortcode(_)
            | Inline::NoteReference(_)
            | Inline::Attr(_)
            | Inline::EditComment(_) => {}
        }
    }

    fn visit_slot(&mut self, slot: &Slot) {
        match slot {
            Slot::Block(b) => self.visit_block(b),
            Slot::Blocks(bs) => {
                for b in bs.iter() {
                    self.visit_block(b);
                }
            }
            Slot::Inline(i) => self.visit_inline(i),
            Slot::Inlines(is) => self.visit_inlines(is),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::format::Format;
    use crate::project::index::ProjectIndex;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::stage::{DocumentAst, DocumentAtProfile};
    use pampa::readers::qmd;
    use quarto_source_map::SourceContext;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Mock runtime sufficient to construct a StageContext.
    struct MockRuntime;

    #[async_trait::async_trait]
    impl quarto_system_runtime::SystemRuntime for MockRuntime {
        fn file_read(&self, _: &std::path::Path) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn file_write(
            &self,
            _: &std::path::Path,
            _: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_exists(
            &self,
            _: &std::path::Path,
            _: Option<quarto_system_runtime::PathKind>,
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
            _: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            unimplemented!()
        }
        fn file_copy(
            &self,
            _: &std::path::Path,
            _: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_rename(
            &self,
            _: &std::path::Path,
            _: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn file_remove(&self, _: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_create(
            &self,
            _: &std::path::Path,
            _: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_remove(
            &self,
            _: &std::path::Path,
            _: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_list(
            &self,
            _: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            Ok(vec![])
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/"))
        }
        fn temp_dir(
            &self,
            _: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            Ok(quarto_system_runtime::TempDir::new(PathBuf::from(
                "/tmp/test",
            )))
        }
        fn exec_pipe(
            &self,
            _: &str,
            _: &[&str],
            _: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn exec_command(
            &self,
            _: &str,
            _: &[&str],
            _: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            Ok(quarto_system_runtime::CommandOutput {
                code: 0,
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn env_get(&self, _: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
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
            _: &str,
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
            _: quarto_system_runtime::XdgDirKind,
            _: Option<&std::path::Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/xdg"))
        }
        fn stdout_write(&self, _: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
    }

    fn make_ctx(index: Option<Arc<ProjectIndex>>) -> StageContext {
        let runtime = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project/_site"),
        };
        let doc = DocumentInfo::from_path("/project/page.qmd");
        let format = Format::html();
        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();
        ctx.project_index = index;
        ctx
    }

    /// Parse a qmd fragment and wrap it in an `AtProfile` bundle
    /// with the supplied source path. The body link targets start
    /// empty — `LinkResolutionStage` is expected to populate them.
    fn parse_to_at_profile(qmd_src: &str, source_relative: &str) -> DocumentAtProfile {
        let mut stderr = Vec::new();
        let (pandoc, ast_context, _w) = qmd::read(
            qmd_src.as_bytes(),
            false,
            source_relative,
            &mut stderr,
            true,
            None,
        )
        .expect("parse fixture");

        let mut source_context = SourceContext::new();
        source_context.add_file(source_relative.to_string(), Some(qmd_src.to_string()));

        let ast = DocumentAst {
            path: PathBuf::from(source_relative),
            ast: pandoc,
            ast_context,
            source_context,
            warnings: vec![],
            recorded_includes: Vec::new(),
        };
        let profile = DocumentProfile {
            source_path: PathBuf::from(source_relative),
            output_href: source_relative.replace(".qmd", ".html"),
            format_id: "html".to_string(),
            ..DocumentProfile::default()
        };
        DocumentAtProfile { profile, ast }
    }

    fn make_index() -> Arc<ProjectIndex> {
        Arc::new(ProjectIndex::new(vec![
            DocumentProfile {
                source_path: PathBuf::from("about.qmd"),
                output_href: "about.html".to_string(),
                format_id: "html".to_string(),
                title: Some("About".to_string()),
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("docs/api.qmd"),
                output_href: "docs/api.html".to_string(),
                format_id: "html".to_string(),
                title: Some("API".to_string()),
                ..DocumentProfile::default()
            },
        ]))
    }

    /// Run the stage and return the (possibly mutated) bundle.
    async fn run_stage(
        bundle: DocumentAtProfile,
        index: Option<Arc<ProjectIndex>>,
    ) -> DocumentAtProfile {
        let mut ctx = make_ctx(index);
        let stage = LinkResolutionStage::new();
        let out = stage
            .run(PipelineData::AtProfile(bundle), &mut ctx)
            .await
            .expect("stage runs");
        out.into_at_profile().expect("AtProfile out")
    }

    #[tokio::test]
    async fn empty_doc_records_no_targets() {
        let bundle = parse_to_at_profile("Just text.\n", "page.qmd");
        let out = run_stage(bundle, Some(make_index())).await;
        assert!(out.profile.body_link_targets.is_empty());
    }

    #[tokio::test]
    async fn body_link_records_target() {
        let bundle = parse_to_at_profile("See [the about page](about.qmd) for more.\n", "page.qmd");
        let out = run_stage(bundle, Some(make_index())).await;
        assert_eq!(
            out.profile.body_link_targets,
            vec![PathBuf::from("about.qmd")]
        );
    }

    #[tokio::test]
    async fn doc_relative_link_resolves() {
        let bundle = parse_to_at_profile("See [the about page](../about.qmd).\n", "docs/api.qmd");
        let out = run_stage(bundle, Some(make_index())).await;
        assert_eq!(
            out.profile.body_link_targets,
            vec![PathBuf::from("about.qmd")]
        );
    }

    #[tokio::test]
    async fn dedupes_repeated_links() {
        let bundle = parse_to_at_profile(
            "See [first](about.qmd) and also [second](about.qmd).\n",
            "page.qmd",
        );
        let out = run_stage(bundle, Some(make_index())).await;
        assert_eq!(
            out.profile.body_link_targets,
            vec![PathBuf::from("about.qmd")]
        );
    }

    #[tokio::test]
    async fn external_links_excluded() {
        let bundle = parse_to_at_profile(
            "See [external](https://example.com) and [internal](about.qmd).\n",
            "page.qmd",
        );
        let out = run_stage(bundle, Some(make_index())).await;
        assert_eq!(
            out.profile.body_link_targets,
            vec![PathBuf::from("about.qmd")]
        );
    }

    #[tokio::test]
    async fn fragment_only_links_excluded() {
        let bundle =
            parse_to_at_profile("See [section](#anchor) and [doc](about.qmd).\n", "page.qmd");
        let out = run_stage(bundle, Some(make_index())).await;
        assert_eq!(
            out.profile.body_link_targets,
            vec![PathBuf::from("about.qmd")]
        );
    }

    #[tokio::test]
    async fn unresolvable_link_excluded() {
        let bundle = parse_to_at_profile(
            "See [missing](missing.qmd) and [real](about.qmd).\n",
            "page.qmd",
        );
        let out = run_stage(bundle, Some(make_index())).await;
        assert_eq!(
            out.profile.body_link_targets,
            vec![PathBuf::from("about.qmd")]
        );
    }

    #[tokio::test]
    async fn no_index_yields_empty_targets() {
        // Standalone render: no project context. Stage runs but
        // contributes nothing — a pure pass-through with empty
        // body_link_targets.
        let bundle = parse_to_at_profile("See [the about](about.qmd).\n", "page.qmd");
        let out = run_stage(bundle, None).await;
        assert!(out.profile.body_link_targets.is_empty());
    }

    #[tokio::test]
    async fn nested_inlines_walked() {
        // Link inside a header.
        let bundle = parse_to_at_profile("# Section [linked](about.qmd)\n\nText.\n", "page.qmd");
        let out = run_stage(bundle, Some(make_index())).await;
        assert_eq!(
            out.profile.body_link_targets,
            vec![PathBuf::from("about.qmd")]
        );
    }

    #[tokio::test]
    async fn order_is_first_occurrence() {
        // The deterministic-order property: first-appearance in
        // document order wins, multiple distinct targets keep that
        // order.
        let bundle = parse_to_at_profile(
            "See [api](docs/api.qmd) then [about](about.qmd) then [api again](docs/api.qmd).\n",
            "page.qmd",
        );
        let out = run_stage(bundle, Some(make_index())).await;
        assert_eq!(
            out.profile.body_link_targets,
            vec![PathBuf::from("docs/api.qmd"), PathBuf::from("about.qmd")]
        );
    }

    #[tokio::test]
    async fn rejects_wrong_input_kind() {
        let mut ctx = make_ctx(Some(make_index()));
        let stage = LinkResolutionStage::new();
        let bogus = PipelineData::LoadedSource(crate::stage::LoadedSource::new(
            PathBuf::from("/x.qmd"),
            b"".to_vec(),
        ));
        let err = stage
            .run(bogus, &mut ctx)
            .await
            .expect_err("must reject non-AtProfile");
        let msg = err.to_string();
        assert!(msg.contains("link-resolution"));
    }
}
