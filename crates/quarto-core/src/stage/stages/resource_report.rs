/*
 * stage/stages/resource_report.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `bd-o8pr` Phase 3 — finalize the per-doc resource report.
 */

//! Resource-report finalization stage.
//!
//! Runs after `UserFiltersStage::post`. Reads the post-filter
//! `meta.resources` and pushes any entries that don't appear in
//! `DocumentProfile.resources` (the snapshot taken at frontmatter
//! freeze) into the per-doc `DocumentResourceReport` as
//! `ResourceOrigin::LuaFilter` contributions.
//!
//! This is the *additivity defense* described in the plan: a Lua
//! filter that adds entries by mutating `meta.resources` (rather
//! than calling `quarto.doc.add_resource(path)`) still gets its
//! additions published. A filter that *removes* entries from
//! `meta.resources` is implicitly defended because the static-
//! channel collector reads `profile.resources` (the snapshot),
//! which the filter cannot retroactively edit.
//!
//! The stage emits a debug-level diagnostic when the post-filter
//! set diverges from the snapshot — useful for tracking down
//! filters that misbehave. A future "filter hygiene" pass can
//! upgrade this to a structured warning (see plan §"Future work:
//! misbehaviour detection").

use async_trait::async_trait;

use crate::project_resources::extract_resource_patterns;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Finalize the per-document resource report (`bd-o8pr`).
///
/// Pipeline position: after `UserFiltersStage::post`. Pure additive
/// effects on `ctx.resource_report`; the AST passes through
/// unchanged.
pub struct ResourceReportStage;

impl ResourceReportStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ResourceReportStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for ResourceReportStage {
    fn name(&self) -> &str {
        "resource-report"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Post-filter view of `meta.resources`. May differ from the
        // profile snapshot if a filter mutated metadata.
        let post_filter = extract_resource_patterns(&doc.ast.meta, &["resources"]);

        // Pull the profile snapshot for this document if available.
        // `lookup_by_source` keys on project-relative paths (the
        // form profiles store), so convert from the absolute
        // `doc.path` first. Standalone (non-project) renders have
        // no project_index, so we treat the snapshot as empty and
        // just publish whatever post-filter meta says.
        let project_rel = doc.path.strip_prefix(&ctx.project.dir).unwrap_or(&doc.path);
        // Snapshot is bare strings: filters mutate meta as plain text
        // and the delta is computed on pattern strings, not on
        // pattern + source-info pairs. (bd-c1et2 added source-info to
        // the underlying type; we still compare on pattern.)
        let snapshot: Vec<String> = ctx
            .project_index
            .as_ref()
            .and_then(|idx| idx.lookup_by_source(project_rel))
            .map(|p| p.resources.iter().map(|r| r.pattern.clone()).collect())
            .unwrap_or_default();

        // Compute additions (in post-filter, not in snapshot) and
        // removals (in snapshot, not in post-filter). Removals are
        // already defended structurally because the static-channel
        // collector reads `profile.resources` directly.
        let mut additions: Vec<String> = Vec::new();
        let mut removals: Vec<String> = Vec::new();
        for entry in &post_filter {
            if !snapshot.contains(&entry.pattern) {
                additions.push(entry.pattern.clone());
            }
        }
        for entry in &snapshot {
            if !post_filter.iter().any(|r| &r.pattern == entry) {
                removals.push(entry.clone());
            }
        }

        if !additions.is_empty() {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "resource-report: filter added {} entries to meta.resources",
                additions.len()
            );
            ctx.resource_report
                .add_lua_filter_files(&doc.path, additions.into_iter().map(Into::into));
        }
        if !removals.is_empty() {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "resource-report: filter removed {} entries from meta.resources \
                 (defended by profile snapshot, will still publish)",
                removals.len()
            );
        }

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::format::Format;
    use crate::project::index::ProjectIndex;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::project_resources::ResourceOrigin;
    use crate::stage::DocumentAst;
    use quarto_pandoc_types::ConfigValue;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_meta_with_resources(items: &[&str]) -> ConfigValue {
        let entries: Vec<ConfigValue> = items
            .iter()
            .map(|s| ConfigValue::new_string(*s, SourceInfo::for_test()))
            .collect();
        let resources = ConfigValue::new_array(entries, SourceInfo::for_test());
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "resources".into(),
                key_source: SourceInfo::for_test(),
                value: resources,
            }],
            SourceInfo::for_test(),
        )
    }

    fn make_ctx(snapshot: Vec<String>, doc_path: PathBuf) -> StageContext {
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: Default::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path(&doc_path)],
            output_dir: PathBuf::from("/project/_site"),
        };
        let mut profile = DocumentProfile::default();
        // Profiles store project-relative source paths (matches
        // production behavior — see DocumentProfileStage).
        profile.source_path = doc_path
            .strip_prefix(&project.dir)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| doc_path.clone());
        profile.resources = snapshot
            .into_iter()
            .map(crate::project_resources::RawResourcePattern::without_source)
            .collect();
        let index = ProjectIndex::new(vec![profile]);
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut ctx = StageContext::new(
            runtime,
            Format::html(),
            project,
            DocumentInfo::from_path(&doc_path),
        )
        .unwrap();
        ctx.project_index = Some(Arc::new(index));
        ctx
    }

    #[tokio::test]
    async fn filter_added_meta_resources_show_up_in_report() {
        let doc_path = PathBuf::from("/project/doc.qmd");
        let mut ctx = make_ctx(vec!["author.txt".into()], doc_path.clone());

        let doc = DocumentAst {
            path: doc_path.clone(),
            ast: Pandoc {
                meta: make_meta_with_resources(&["author.txt", "filter-added.txt"]),
                blocks: Vec::new(),
            },
            ast_context: Default::default(),
            source_context: Default::default(),
            warnings: Vec::new(),
            recorded_includes: Vec::new(),
        };

        let stage = ResourceReportStage::new();
        let _ = stage
            .run(PipelineData::DocumentAst(doc), &mut ctx)
            .await
            .unwrap();

        // The filter-added entry is reported as a Lua-filter
        // contribution; the author entry stays where it was (in the
        // profile snapshot, picked up by the static collector).
        assert_eq!(ctx.resource_report.entries.len(), 1);
        assert_eq!(
            ctx.resource_report.entries[0].raw_path,
            PathBuf::from("filter-added.txt")
        );
        assert!(matches!(
            ctx.resource_report.entries[0].origin,
            ResourceOrigin::LuaFilter { .. }
        ));
    }

    #[tokio::test]
    async fn filter_removing_meta_resources_does_not_drop_them() {
        let doc_path = PathBuf::from("/project/doc.qmd");
        let mut ctx = make_ctx(vec!["author.txt".into()], doc_path.clone());

        // Filter removed `author.txt` from meta.
        let doc = DocumentAst {
            path: doc_path.clone(),
            ast: Pandoc {
                meta: make_meta_with_resources(&[]),
                blocks: Vec::new(),
            },
            ast_context: Default::default(),
            source_context: Default::default(),
            warnings: Vec::new(),
            recorded_includes: Vec::new(),
        };

        let stage = ResourceReportStage::new();
        let _ = stage
            .run(PipelineData::DocumentAst(doc), &mut ctx)
            .await
            .unwrap();

        // The report itself is empty — the snapshot defense lives in
        // the static-channel collector at the orchestrator layer,
        // not here. This test pins the stage's contract: it MUST NOT
        // remove anything.
        assert!(ctx.resource_report.entries.is_empty());
    }
}
