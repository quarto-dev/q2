/*
 * lmd/dialect.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! lmd as a pampa source dialect (bd-76ogq4ew): the hook that lets the
//! scoped `require` load `.lmd` modules, in lmd cells, Lua filters and
//! shortcode handlers alike.
//!
//! The desugarer stays in this crate; pampa only sees the
//! [`SourceDialect`] object. Modules are compiled in the line-preserving
//! emit mode under no anchor, so a module's Lua line numbers are the
//! file's own and Lua's `path:line:` positions need no mapping. A
//! desugar failure becomes the `require` call's error; the loader names
//! the module and its path, the dialect's message gives the position inside
//! the file and what went wrong.

use std::path::Path;
use std::sync::Arc;

use pampa::dialect::{DialectError, SourceDialect};
use pampa::lua_options::{ExtensionRoot, HostContext, HostProject, LuaOptions, ModuleRoots};

use super::desugar::{DesugarOptions, EmitMode, LmdError, desugar_lua_with};

/// The lmd dialect: `.lmd` files are Lua with markdown fragments.
#[derive(Debug, Default, Clone, Copy)]
pub struct LmdDialect;

impl SourceDialect for LmdDialect {
    fn name(&self) -> &str {
        super::LMD_LANG
    }

    fn extension(&self) -> &str {
        super::LMD_LANG
    }

    fn compile(&self, source: &str, path: &Path) -> Result<String, DialectError> {
        let opts = DesugarOptions {
            anchor: None,
            mode: EmitMode::LinePreserving,
        };
        desugar_lua_with(source, &opts).map_err(|err| {
            let diagnostics = match &err {
                LmdError::MarkdownParse { diagnostics, .. } => diagnostics.clone(),
                _ => Vec::new(),
            };
            // The loader (and `quarto.lmd.compile`) prefix the module path
            // and name; the message carries the position inside the file.
            let _ = path;
            DialectError {
                message: err.to_string(),
                diagnostics,
            }
        })
    }
}

/// The dialects q2 registers on every Lua state it builds: lmd.
pub fn dialects() -> Vec<Arc<dyn SourceDialect>> {
    vec![Arc::new(LmdDialect)]
}

/// The [`LuaOptions`] a document's Lua environments get: the lmd dialect,
/// and the module roots the project provides (bd-76ogq4ew, phase 3). The
/// project root serves `require("/components/cards")`; every discovered
/// extension is addressable by name, `require("<name>/module")`, built-ins
/// first so a user extension of the same name wins (the discovery order).
/// For a single-file render the "project root" is the document's
/// directory, which `ProjectContext::dir` already is. `quarto.paths.typst()`
/// gets the same binary the render uses (bd-gebcr1sa).
pub fn lua_options(
    project: &crate::project::ProjectContext,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> LuaOptions {
    LuaOptions {
        dialects: dialects(),
        module_roots: ModuleRoots {
            project: Some(project.dir.clone()),
            extensions: project
                .extensions
                .iter()
                .map(|ext| ExtensionRoot {
                    name: ext.id.name.clone(),
                    dir: ext.path.clone(),
                })
                .collect(),
        },
        typst: crate::render::find_typst(runtime),
        host: HostContext::default(),
    }
}

/// [`lua_options`] plus what the scripts of one document may ask about
/// where they run: `quarto.project`, `quarto.doc.input_file` /
/// `output_file`, `PANDOC_STATE`, and the environment `os.getenv`
/// answers from (GH #939, #940). `env_snapshot` is
/// [`crate::stage::StageContext::env_snapshot`]; `None` leaves `os.getenv`
/// to the stdlib.
pub fn lua_options_for_document(
    project: &crate::project::ProjectContext,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
    document: &crate::project::DocumentInfo,
    output_file: std::path::PathBuf,
    env_snapshot: Option<Arc<crate::project::environment::EnvSnapshot>>,
) -> LuaOptions {
    let mut options = lua_options(project, runtime);
    options.host = HostContext {
        input_file: Some(document.input.clone()),
        output_file: Some(output_file),
        project: host_project(project, &document.input),
        profiles: project
            .config
            .active_config_profiles
            .iter()
            .map(|p| p.name.clone())
            .collect(),
        env: env_snapshot,
    };
    options
}

/// `quarto.project`'s paths for `input`, or `None` for a single-file
/// render (Q1: `quarto.project.directory` is nil outside a project).
/// `offset` is Q1's `projectOffset`: the relative path from the input's
/// directory back to the project root, forward slashes, `"."` when
/// they coincide.
fn host_project(project: &crate::project::ProjectContext, input: &Path) -> Option<HostProject> {
    if project.is_single_file {
        return None;
    }
    let input_dir = input.parent().unwrap_or(Path::new("."));
    let offset = pathdiff::diff_paths(&project.dir, input_dir)
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| ".".to_string(), |p| quarto_util::to_forward_slashes(&p));
    Some(HostProject {
        directory: project.dir.clone(),
        output_directory: project.output_dir.clone(),
        offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_project_offset_is_q1_project_offset() {
        let dir = std::path::PathBuf::from("/proj");
        let project = crate::project::ProjectContext {
            dir: dir.clone(),
            output_dir: dir.join("_site"),
            is_single_file: false,
            ..Default::default()
        };
        let root = host_project(&project, &dir.join("index.qmd")).unwrap();
        assert_eq!(root.offset, ".");
        assert_eq!(root.directory, dir);
        assert_eq!(root.output_directory, dir.join("_site"));
        let nested = host_project(&project, &dir.join("a").join("b").join("p.qmd")).unwrap();
        assert_eq!(nested.offset, "../..", "forward slashes on every platform");

        let single = crate::project::ProjectContext {
            dir: dir.clone(),
            output_dir: dir.clone(),
            is_single_file: true,
            ..Default::default()
        };
        assert!(host_project(&single, &dir.join("doc.qmd")).is_none());
    }

    #[test]
    fn compiles_a_module_keeping_its_lines() {
        let src =
            "local M = {}\nfunction M.card(t)\n  return {=: **`{lmd} t`** :=}\nend\nreturn M\n";
        let out = LmdDialect.compile(src, Path::new("cards.lmd")).unwrap();
        assert_eq!(out.lines().count(), src.lines().count(), "{out}");
        assert!(out.contains("pandoc.Blocks("), "{out}");
    }

    #[test]
    fn a_lua_syntax_error_names_the_module_and_position() {
        let err = LmdDialect
            .compile("local = 1\n", Path::new("bad.lmd"))
            .unwrap_err();
        assert!(err.message.contains("Lua syntax error"), "{}", err.message);
        assert!(err.message.contains("line 1"), "{}", err.message);
    }

    #[test]
    fn a_markdown_error_carries_the_readers_diagnostics() {
        let err = LmdDialect
            .compile("return {=: pages ' :=}\n", Path::new("bad.lmd"))
            .unwrap_err();
        assert!(
            err.message.contains("markdown fragment failed to parse"),
            "{}",
            err.message
        );
        assert!(!err.diagnostics.is_empty());
    }

    const CARDS_LMD: &str = "local M = {}\nfunction M.card(title, body)\n  return {=:\n::: {.card}\n### `{lmd} title`\n\n`{lmd} body`\n:::\n:=}\nend\nreturn M\n";

    fn write(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn one_paragraph_doc() -> pampa::pandoc::Pandoc {
        use pampa::pandoc::{Block, Inline, Pandoc, Paragraph, Str};
        Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
        }
    }

    /// A Lua *filter* (not a cell) can `require` an `.lmd` module when the
    /// filter pipeline is given q2's dialects, which is how
    /// `UserFiltersStage` calls it.
    #[test]
    fn a_lua_filter_can_require_an_lmd_module() {
        use pampa::pandoc::Block;
        use pampa::unified_filter::{FilterSpec, apply_filters_with};
        let tmp = tempfile::TempDir::new().unwrap();
        write(tmp.path(), "components/cards.lmd", CARDS_LMD);
        write(
            tmp.path(),
            "filter.lua",
            "local cards = require(\"components/cards\")\nfunction Para(p)\n  return cards.card(\"From a filter\", p.content)\nend\n",
        );
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let output = pollster::block_on(apply_filters_with(
            one_paragraph_doc(),
            pampa::pandoc::ASTContext::new(),
            &[FilterSpec::Lua(tmp.path().join("filter.lua"))],
            "html",
            runtime,
            None,
            tmp.path(),
            &LuaOptions::with_dialects(dialects()),
        ))
        .expect("filter runs");
        assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
        let Block::Div(div) = &output.pandoc.blocks[0] else {
            panic!("expected the card div, got {:?}", output.pandoc.blocks[0]);
        };
        assert_eq!(div.attr.1, vec!["card".to_string()]);
    }

    /// A shortcode handler script can `require` an `.lmd` module when the
    /// engine is built with q2's dialects, as `ShortcodeResolveTransform`
    /// builds it.
    #[test]
    fn a_shortcode_handler_can_require_an_lmd_module() {
        use pampa::lua::{
            LuaShortcodeEngine, LuaShortcodeResult, ShortcodeArgs, ShortcodeCallContext,
        };
        let tmp = tempfile::TempDir::new().unwrap();
        write(tmp.path(), "components/cards.lmd", CARDS_LMD);
        write(
            tmp.path(),
            "cards-shortcode.lua",
            "local cards = require(\"components/cards\")\nreturn {\n  card = function(args, kwargs, meta, raw_args, context)\n    return cards.card(args[1], args[2])\n  end\n}\n",
        );
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut engine =
            LuaShortcodeEngine::new_with("html", runtime, &LuaOptions::with_dialects(dialects()))
                .unwrap();
        pollster::block_on(engine.load_script(&tmp.path().join("cards-shortcode.lua"))).unwrap();
        let args = ShortcodeArgs {
            positional: vec!["Title".to_string(), "body".to_string()],
            keyword: vec![],
            metadata: quarto_pandoc_types::ConfigValue::default(),
        };
        let result = pollster::block_on(engine.call("card", &args, ShortcodeCallContext::Block))
            .expect("handler registered");
        match result {
            LuaShortcodeResult::Blocks(blocks) => {
                assert!(
                    matches!(blocks[0], pampa::pandoc::Block::Div(_)),
                    "{blocks:?}"
                );
            }
            other => panic!("expected blocks, got {other:?}"),
        }
    }

    #[test]
    fn dialects_is_lmd() {
        let ds = dialects();
        assert_eq!(ds.len(), 1);
        assert_eq!((ds[0].name(), ds[0].extension()), ("lmd", "lmd"));
    }
}
