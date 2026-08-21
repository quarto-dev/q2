/*
 * config_json.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! JSON projection of merged document configuration for `q2 get-config`
//! (bd-xoaic, GH #256).
//!
//! [`MetadataMergeStage`] leaves the fully-merged document metadata in
//! `ast.meta` as a [`ConfigValue`]. `ConfigValue`'s own `Serialize` impl emits
//! an internal, tagged shape (`{"PandocInlines": …}`, merge-op wrappers, source
//! ids) that is unsuitable for an external tool contract. This module projects
//! a `ConfigValue` into a clean, idiomatic [`serde_json::Value`]:
//!
//! - scalars → JSON scalars
//! - maps / arrays → JSON objects / arrays
//! - prose values (`PandocInlines` / `PandocBlocks`) → either a faithful
//!   markdown string ([`ProseMode::Value`], D1) or a self-contained, source-free
//!   Pandoc AST fragment ([`ProseMode::Pandoc`], D7)
//! - `!path` / `!glob` / `!expr` → the underlying string, tag ignored (D6)
//!
//! It also provides [`navigate`], a dot-separated path lookup that supports
//! numeric array indices (D4), e.g. `authors.0.name`.
//!
//! [`MetadataMergeStage`]: ../../quarto_core/stage/struct.MetadataMergeStage.html

use crate::pandoc::ASTContext;
use crate::writers::{json as json_writer, qmd as qmd_writer};
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use serde_json::Value;
use yaml_rust2::Yaml;

/// How prose-valued metadata (`PandocInlines` / `PandocBlocks`) is rendered in
/// the JSON output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProseMode {
    /// Render prose back to a faithful markdown string (D1).
    ///
    /// `title: Hello _world_!` ⇒ `"Hello *world*!"`. The qmd writer normalizes
    /// emphasis delimiters (it defaults to `*`), so the string is semantically
    /// faithful but not necessarily byte-identical to the source. Recovering the
    /// exact source substring would require source-map slicing — a documented
    /// future refinement.
    Value,
    /// Render prose as a self-contained, source-free Pandoc AST fragment (D7).
    ///
    /// `title: Hello _world_!` ⇒ the `[{"t":"Str","c":"Hello"}, {"t":"Space"},
    /// {"t":"Emph","c":[{"t":"Str","c":"world"}]}, {"t":"Str","c":"!"}]` shape.
    Pandoc,
}

/// Project a merged [`ConfigValue`] into clean JSON.
///
/// `context` is only consulted in [`ProseMode::Pandoc`] (for the Pandoc JSON
/// writer, with source resolution off); [`ProseMode::Value`] ignores it.
pub fn config_value_to_json(value: &ConfigValue, mode: ProseMode, context: &ASTContext) -> Value {
    match &value.value {
        ConfigValueKind::Scalar { yaml, .. } => yaml_to_json(yaml),

        // D6: drop the tag, emit the deferred expression / pattern / path as a
        // plain string. These are unresolved at the profile checkpoint.
        ConfigValueKind::Path(s) | ConfigValueKind::Glob(s) | ConfigValueKind::Expr(s) => {
            Value::String(s.clone())
        }

        ConfigValueKind::PandocInlines(inlines) => match mode {
            ProseMode::Value => Value::String(inlines_to_markdown(inlines)),
            ProseMode::Pandoc => json_writer::inlines_to_source_free_json(inlines, context),
        },
        ConfigValueKind::PandocBlocks(blocks) => match mode {
            ProseMode::Value => Value::String(blocks_to_markdown(blocks)),
            ProseMode::Pandoc => json_writer::blocks_to_source_free_json(blocks, context),
        },

        ConfigValueKind::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| config_value_to_json(item, mode, context))
                .collect(),
        ),
        ConfigValueKind::Map(entries) => {
            let mut map = serde_json::Map::with_capacity(entries.len());
            for entry in entries {
                map.insert(
                    entry.key.clone(),
                    config_value_to_json(&entry.value, mode, context),
                );
            }
            Value::Object(map)
        }
    }
}

/// Navigate a dot-separated `path` into `root`.
///
/// An empty path returns `root` (the whole merged metadata). For each segment:
/// if the current node is an [`Array`](ConfigValueKind::Array) the segment is
/// parsed as a numeric index (D4); otherwise it is treated as a map key.
/// Returns `None` if any segment cannot be resolved.
pub fn navigate<'a>(root: &'a ConfigValue, path: &str) -> Option<&'a ConfigValue> {
    if path.is_empty() {
        return Some(root);
    }
    let mut current = root;
    for segment in path.split('.') {
        current = match &current.value {
            ConfigValueKind::Array(items) => {
                let index: usize = segment.parse().ok()?;
                items.get(index)?
            }
            _ => current.get(segment)?,
        };
    }
    Some(current)
}

/// Map a raw `yaml_rust2::Yaml` scalar (the `Scalar` variant payload) to JSON.
fn yaml_to_json(yaml: &Yaml) -> Value {
    match yaml {
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Integer(i) => Value::Number((*i).into()),
        Yaml::Real(s) => s
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| Value::String(s.clone()), Value::Number),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Null => Value::Null,
        Yaml::Array(arr) => Value::Array(arr.iter().map(yaml_to_json).collect()),
        Yaml::Hash(hash) => {
            let mut map = serde_json::Map::new();
            for (k, v) in hash {
                if let Yaml::String(key) = k {
                    map.insert(key.clone(), yaml_to_json(v));
                }
            }
            Value::Object(map)
        }
        Yaml::Alias(_) | Yaml::BadValue => Value::Null,
    }
}

/// Render a run of inlines to a markdown string via the qmd writer.
///
/// Writer IO into an in-memory `Vec` does not fail in practice; on the
/// unexpected error path we degrade to whatever was written so far rather than
/// panicking inside a read-only query command.
fn inlines_to_markdown(inlines: &[quarto_pandoc_types::inline::Inline]) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let _ = qmd_writer::write_inlines(inlines, &mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Render a sequence of blocks to a markdown string via the qmd writer,
/// trimming the trailing newline each block contributes.
fn blocks_to_markdown(blocks: &[quarto_pandoc_types::block::Block]) -> String {
    let mut buf: Vec<u8> = Vec::new();
    for block in blocks {
        let _ = qmd_writer::write_single_block(block, &mut buf);
    }
    String::from_utf8_lossy(&buf)
        .trim_end_matches('\n')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::inline::{Emph, Inline, Space, Str};
    use quarto_source_map::SourceInfo;
    use serde_json::json;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn s(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: si(),
        })
    }

    /// Inlines for `Hello _world_!`.
    fn hello_world_inlines() -> Vec<Inline> {
        vec![
            s("Hello"),
            Inline::Space(Space { source_info: si() }),
            Inline::Emph(Emph {
                content: vec![s("world")],
                source_info: si(),
            }),
            s("!"),
        ]
    }

    fn entry(key: &str, value: ConfigValue) -> quarto_pandoc_types::config_value::ConfigMapEntry {
        quarto_pandoc_types::config_value::ConfigMapEntry {
            key: key.to_string(),
            key_source: si(),
            value,
        }
    }

    #[test]
    fn scalar_string_to_json() {
        let v = ConfigValue::new_string("hello", si());
        assert_eq!(
            config_value_to_json(&v, ProseMode::Value, &ASTContext::default()),
            json!("hello")
        );
    }

    #[test]
    fn scalar_int_bool_null_float_to_json() {
        let ctx = ASTContext::default();
        let int = ConfigValue::new_scalar(Yaml::Integer(42), si());
        assert_eq!(
            config_value_to_json(&int, ProseMode::Value, &ctx),
            json!(42)
        );

        let boolean = ConfigValue::new_bool(true, si());
        assert_eq!(
            config_value_to_json(&boolean, ProseMode::Value, &ctx),
            json!(true)
        );

        let null = ConfigValue::new_scalar(Yaml::Null, si());
        assert_eq!(
            config_value_to_json(&null, ProseMode::Value, &ctx),
            Value::Null
        );

        let real = ConfigValue::new_scalar(Yaml::Real("3.5".to_string()), si());
        assert_eq!(
            config_value_to_json(&real, ProseMode::Value, &ctx),
            json!(3.5)
        );
    }

    #[test]
    fn nested_map_to_json() {
        let inner =
            ConfigValue::new_map(vec![entry("toc", ConfigValue::new_bool(true, si()))], si());
        let outer = ConfigValue::new_map(vec![entry("html", inner)], si());
        let root = ConfigValue::new_map(vec![entry("format", outer)], si());
        assert_eq!(
            config_value_to_json(&root, ProseMode::Value, &ASTContext::default()),
            json!({"format": {"html": {"toc": true}}})
        );
    }

    #[test]
    fn array_to_json() {
        let arr = ConfigValue::new_array(
            vec![
                ConfigValue::new_string("a", si()),
                ConfigValue::new_string("b", si()),
            ],
            si(),
        );
        assert_eq!(
            config_value_to_json(&arr, ProseMode::Value, &ASTContext::default()),
            json!(["a", "b"])
        );
    }

    #[test]
    fn prose_value_mode_is_markdown_string() {
        let v = ConfigValue::new_inlines(hello_world_inlines(), si());
        // The qmd writer normalizes emphasis to `*` (D1 documented behavior).
        assert_eq!(
            config_value_to_json(&v, ProseMode::Value, &ASTContext::default()),
            json!("Hello *world*!")
        );
    }

    #[test]
    fn prose_pandoc_mode_is_source_free_ast() {
        let v = ConfigValue::new_inlines(hello_world_inlines(), si());
        let out = config_value_to_json(&v, ProseMode::Pandoc, &ASTContext::default());

        // Expect an array of source-free Pandoc nodes; no `s` keys anywhere.
        let arr = out
            .as_array()
            .expect("pandoc mode yields an array of inlines");
        assert!(arr.iter().any(|n| n["t"] == json!("Emph")));
        assert!(
            no_source_keys(&out),
            "pandoc output must not contain `s` keys: {out}"
        );

        // The Emph node wraps a Str "world".
        let emph = arr.iter().find(|n| n["t"] == json!("Emph")).unwrap();
        assert_eq!(emph["c"][0]["t"], json!("Str"));
        assert_eq!(emph["c"][0]["c"], json!("world"));
    }

    fn no_source_keys(v: &Value) -> bool {
        match v {
            Value::Object(map) => !map.contains_key("s") && map.values().all(no_source_keys),
            Value::Array(items) => items.iter().all(no_source_keys),
            _ => true,
        }
    }

    #[test]
    fn deferred_tags_emit_underlying_string() {
        let ctx = ASTContext::default();
        let path = ConfigValue::new_path("data.csv".to_string(), si());
        assert_eq!(
            config_value_to_json(&path, ProseMode::Value, &ctx),
            json!("data.csv")
        );

        let glob = ConfigValue::new_glob("*.qmd".to_string(), si());
        assert_eq!(
            config_value_to_json(&glob, ProseMode::Value, &ctx),
            json!("*.qmd")
        );

        let expr = ConfigValue::new_expr("1 + 1".to_string(), si());
        assert_eq!(
            config_value_to_json(&expr, ProseMode::Value, &ctx),
            json!("1 + 1")
        );
    }

    #[test]
    fn navigate_empty_path_returns_root() {
        let root = ConfigValue::new_map(
            vec![entry("title", ConfigValue::new_string("T", si()))],
            si(),
        );
        let got = navigate(&root, "").unwrap();
        assert!(matches!(&got.value, ConfigValueKind::Map(_)));
    }

    #[test]
    fn navigate_nested_map_key() {
        let inner = ConfigValue::new_map(
            vec![entry("name", ConfigValue::new_string("Alice", si()))],
            si(),
        );
        let root = ConfigValue::new_map(vec![entry("author", inner)], si());
        let got = navigate(&root, "author.name").unwrap();
        assert_eq!(got.as_plain_text().as_deref(), Some("Alice"));
    }

    #[test]
    fn navigate_array_index() {
        let a0 = ConfigValue::new_map(
            vec![entry("name", ConfigValue::new_string("Alice", si()))],
            si(),
        );
        let a1 = ConfigValue::new_map(
            vec![entry("name", ConfigValue::new_string("Bob", si()))],
            si(),
        );
        let authors = ConfigValue::new_array(vec![a0, a1], si());
        let root = ConfigValue::new_map(vec![entry("authors", authors)], si());

        let got = navigate(&root, "authors.1.name").unwrap();
        assert_eq!(got.as_plain_text().as_deref(), Some("Bob"));
    }

    #[test]
    fn navigate_missing_key_is_none() {
        let root = ConfigValue::new_map(
            vec![entry("title", ConfigValue::new_string("T", si()))],
            si(),
        );
        assert!(navigate(&root, "nope").is_none());
        assert!(navigate(&root, "title.deeper").is_none());
    }

    #[test]
    fn navigate_array_index_out_of_bounds_is_none() {
        let authors = ConfigValue::new_array(vec![ConfigValue::new_string("Alice", si())], si());
        let root = ConfigValue::new_map(vec![entry("authors", authors)], si());
        assert!(navigate(&root, "authors.5").is_none());
        // Non-numeric segment into an array fails cleanly.
        assert!(navigate(&root, "authors.name").is_none());
    }
}
