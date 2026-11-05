/*
 * engine/knitr/types.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Request/response types for knitr R communication.
 */

//! Request and response types for knitr R subprocess communication.
//!
//! These types define the JSON protocol between Rust and the R scripts.
//! The request is sent via stdin, and the response is read from a temp file.
//!
//! # Request Format
//!
//! ```json
//! {
//!   "action": "execute",
//!   "params": {
//!     "input": "/path/to/doc.qmd",
//!     "markdown": "# Hello\n\n```{r}\n1+1\n```",
//!     "format": { ... },
//!     "tempDir": "/tmp/quarto-xxx",
//!     "resourceDir": "/path/to/resources",
//!     "handledLanguages": ["ojs", "mermaid", "dot"]
//!   },
//!   "results": "/tmp/r-results-xxx.json",
//!   "wd": "/project"
//! }
//! ```
//!
//! # Response Format
//!
//! ```json
//! {
//!   "engine": "knitr",
//!   "markdown": "# Hello\n\n::: {.cell}\n...\n:::",
//!   "supporting": ["/path/to/doc_files"],
//!   "filters": ["rmarkdown/pagebreak.lua"],
//!   "includes": { "include-in-header": ["/tmp/header.html"] },
//!   "postProcess": false
//! }
//! ```

use std::fmt;
use std::path::PathBuf;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::format::KnitrFormatConfig;

/// Parameters for the knitr execute action.
///
/// This is serialized to JSON and sent to R via stdin.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnitrExecuteParams {
    /// Path to the input document
    pub input: PathBuf,

    /// Markdown content (with YAML frontmatter removed and inline R resolved)
    pub markdown: String,

    /// Format configuration
    pub format: KnitrFormatConfig,

    /// Directory for temporary files
    pub temp_dir: PathBuf,

    /// Library directory for output files (e.g., lib/ for self-contained: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lib_dir: Option<PathBuf>,

    /// Whether to compute dependencies
    pub dependencies: bool,

    /// Current working directory
    pub cwd: PathBuf,

    /// Document parameters (from YAML params key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,

    /// Path to Quarto resources directory
    pub resource_dir: PathBuf,

    /// Languages handled by Quarto (pass-through, don't execute)
    pub handled_languages: Vec<String>,

    /// Record each chunk's evaluate() pieces next to the markdown (KNR-1
    /// capture; `execute.R` then returns `pieces`). Omitted from the wire
    /// when off, so existing requests are unchanged. Turning it on also
    /// turns knitr's cache off for the run.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub capture: bool,
}

/// Result from the knitr execute action.
///
/// This is read from the JSON results file after R execution.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnitrExecuteResult {
    /// Engine name (always "knitr")
    #[allow(dead_code)]
    pub engine: String,

    /// Processed markdown output
    pub markdown: String,

    /// Supporting files/directories (e.g., doc_files/)
    #[serde(default)]
    pub supporting: Vec<String>,

    /// Pandoc filters to apply
    #[serde(default)]
    pub filters: Vec<String>,

    /// Include files for Pandoc
    #[serde(default, deserialize_with = "deserialize_includes")]
    pub includes: Option<KnitrIncludes>,

    // TODO: Processing not yet implemented. See analysis in:
    // claude-notes/plans/2026-01-15-workspace-warnings-cleanup.md (Section 2.4)
    // These fields are deserialized from R but the processing logic
    // (e.g., htmlwidgets dependency handling) is not yet implemented.
    /// Engine-specific dependencies (e.g., htmlwidgets)
    #[serde(default)]
    #[allow(dead_code)]
    pub engine_dependencies: Option<Value>,

    // TODO: Processing not yet implemented. See analysis in:
    // claude-notes/plans/2026-01-15-workspace-warnings-cleanup.md (Section 2.4)
    // Content preservation during post-processing is not yet implemented.
    /// Content to preserve during post-processing
    #[serde(default)]
    #[allow(dead_code)]
    pub preserve: Option<Value>,

    /// Whether post-processing is needed
    #[serde(default)]
    pub post_process: bool,

    /// One record per chunk that carried a `quarto-unit-id` option, in
    /// execution order (EXR-11). Absent from results that predate it.
    #[serde(default)]
    pub chunks: Vec<KnitrChunkRecord>,

    /// One record per brace-spelling inline span that carried an id statement, in
    /// evaluation order (EXR-12). Absent from results that predate it.
    #[serde(default)]
    pub spans: Vec<KnitrSpanRecord>,

    /// One entry per executed chunk, with knitr's pieces (source, output,
    /// plots, ...) before `sew` turns them into markdown (KNR-1 S2). Parallel
    /// to `chunks`, not an extension of it. Absent from results that predate it.
    #[serde(default)]
    pub pieces: Vec<KnitrCapturedChunk>,

    /// The option names the knitr that ran knows (`names(knitr::opts_chunk$get())`, read before
    /// any chunk runs, so a setup chunk\'s `opts_chunk$set` cannot make an unknown name known).
    /// Present only on a captured run; becomes the record\'s `known_options` (KNR-3).
    #[serde(default)]
    pub known_options: Option<Vec<String>>,

    /// The labels of chunks that asked for knitr\'s cache and had it forced off by the capture
    /// (KNR-1 S4). Empty on an uncaptured run.
    #[serde(default)]
    pub cache_disabled: Vec<String>,
}

/// A brace-spelling inline expression recorded by the `evaluate.inline` and `inline`
/// hooks (EXR-12).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnitrSpanRecord {
    /// The unit id the span carried in its `.QuartoInlineId` statement.
    pub id: String,

    /// The markdown knitr inserted for the span: empty for a value knitr inserts
    /// nothing for (`invisible`, a zero-length vector).
    pub markdown: String,

    /// The value as a captured piece when knitr read it as `knit_asis` or image paths (an
    /// `I(...)`-wrapped htmlwidget or `include_graphics()`): the same wire shape as a chunk
    /// piece, so the record can hold the mime knitr produced, not only the flattened text
    /// (KNR-4 decision 6). Absent for a plain value.
    #[serde(default)]
    pub piece: Option<KnitrPiece>,
}

/// A chunk's captured pieces (KNR-1 S2 wire shape).
// Read by KNR-1's mapper (integration tests) and KNR-3's adapter, not by the lib yet.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct KnitrCapturedChunk {
    /// 1-based position in knitr's own chunk sequence. Harness-local: not a record id.
    pub ordinal: u32,
    pub label: String,
    /// The code `evaluate()` received.
    #[serde(default)]
    pub code: String,
    /// The resolved chunk options (KNR-1 S3).
    #[serde(default)]
    pub options: Value,
    pub pieces: Vec<KnitrPiece>,
    /// Files the chunk wrote, as knitr addresses them.
    #[serde(default)]
    pub files: Vec<String>,
    /// The first knit hook found replaced by the document (`output`, `source`, ...), kept on
    /// every chunk from the one that replaced it on: the presenter cannot see what the
    /// override did, so those chunks are fallback units (KNR-3, KNR-1 S5).
    #[serde(default)]
    pub hook_override: Option<String>,
}

/// One piece of a chunk's evaluation, tagged by `class` on the wire.
// The fields are read by KNR-1's mapper (integration tests) and KNR-3's adapter,
// not by the lib yet, like `KnitrExecuteResult::chunks`.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum KnitrPiece {
    Source {
        text: String,
    },
    Output {
        text: String,
    },
    Message {
        text: String,
    },
    Warning {
        text: String,
        #[serde(default)]
        call: Option<String>,
    },
    Error {
        text: String,
        ename: String,
        evalue: String,
        #[serde(default)]
        call: Option<String>,
    },
    Plot {
        files: Vec<String>,
    },
    ImagePaths {
        paths: Vec<String>,
        /// The width knitr computed for each path (`raster_dpi_width`), where it did.
        #[serde(default, deserialize_with = "numbers_or_none")]
        widths: Vec<Option<f64>>,
    },
    Asis {
        text: String,
        mime: String,
        #[serde(default)]
        widget: bool,
        #[serde(default)]
        knit_meta: Option<String>,
    },
    /// Any class not listed: the whole chunk becomes a fallback unit.
    Other {
        classes: Vec<String>,
    },
}

/// A knitr chunk recorded by the chunk hook (EXR-11).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnitrChunkRecord {
    /// The unit id the chunk carried in its `quarto-unit-id` option.
    pub id: String,

    /// The chunk's finished markdown: what lands in the document (empty, or
    /// the indent, for `include: false`).
    pub markdown: String,

    /// Stable digest of the resolved chunk options.
    pub options_digest: String,

    /// Files the chunk wrote under its figure directory, as knitr addresses
    /// them (relative to the knit working directory unless `base.dir` is set).
    #[serde(default)]
    pub files: Vec<String>,
}

/// Include files from knitr execution.
///
/// Each slot is a list of files whose contents go into the named location
/// of the final document. `execute.R`'s `create_pandoc_includes` writes one
/// file per slot and wraps its path in `I()`, so `jsonlite::toJSON(auto_unbox
/// = TRUE)` emits a **one-element array**, never a bare string — Quarto 1
/// declares the same slot as `string[]`. A bare string is also accepted
/// (bd-gy2ozix3 / GH #683: typing the slot as a single path made every
/// document with an HTML dependency fail to render).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct KnitrIncludes {
    /// Files to include in the document header (e.g., CSS, JS)
    #[serde(default, deserialize_with = "deserialize_string_or_seq")]
    pub include_in_header: Vec<PathBuf>,

    /// Files to include before the body
    #[serde(default, deserialize_with = "deserialize_string_or_seq")]
    pub include_before_body: Vec<PathBuf>,

    /// Files to include after the body
    #[serde(default, deserialize_with = "deserialize_string_or_seq")]
    pub include_after_body: Vec<PathBuf>,
}

impl KnitrIncludes {
    /// True when no slot names any file.
    pub fn is_empty(&self) -> bool {
        self.include_in_header.is_empty()
            && self.include_before_body.is_empty()
            && self.include_after_body.is_empty()
    }
}

/// Deserialize an include slot from a bare path string, a list of path
/// strings, or `null`.
///
/// Implemented as a visitor rather than via an intermediate
/// `serde_json::Value` so that a type error keeps its position in the
/// document: `serde_path_to_error` can then report the offending field as
/// `includes.include-in-header` instead of just `includes`.
fn deserialize_string_or_seq<'de, D>(deserializer: D) -> Result<Vec<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    struct SlotVisitor;

    impl<'de> Visitor<'de> for SlotVisitor {
        type Value = Vec<PathBuf>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a path string or a list of path strings")
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            d.deserialize_any(SlotVisitor)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(vec![PathBuf::from(v)])
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut paths = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            while let Some(path) = seq.next_element::<String>()? {
                paths.push(PathBuf::from(path));
            }
            Ok(paths)
        }
    }

    deserializer.deserialize_any(SlotVisitor)
}

/// Deserialize the `includes` field from an object, an empty array, or
/// `null`.
///
/// The R scripts return an empty array `[]` instead of an empty object `{}`
/// when there are no includes (an empty R `list()` serializes as `[]`).
/// An object with no populated slot also yields `None`. Visitor-based for
/// the same reason as [`deserialize_string_or_seq`]: field errors keep
/// their path.
fn deserialize_includes<'de, D>(deserializer: D) -> Result<Option<KnitrIncludes>, D::Error>
where
    D: Deserializer<'de>,
{
    struct IncludesVisitor;

    impl<'de> Visitor<'de> for IncludesVisitor {
        type Value = Option<KnitrIncludes>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an includes object, an empty array, or null")
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            d.deserialize_any(IncludesVisitor)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            if seq.next_element::<de::IgnoredAny>()?.is_some() {
                return Err(de::Error::custom(
                    "expected an includes object or an empty array, got a non-empty array",
                ));
            }
            Ok(None)
        }

        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
            let includes = KnitrIncludes::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(if includes.is_empty() {
                None
            } else {
                Some(includes)
            })
        }
    }

    deserializer.deserialize_any(IncludesVisitor)
}

/// Request wrapper sent to R via stdin.
///
/// This wraps the action-specific params with metadata needed by rmd.R.
#[derive(Debug, Clone, Serialize)]
pub struct KnitrRequest<T: Serialize> {
    /// Action to perform ("execute", "dependencies", etc.)
    pub action: String,

    /// Action-specific parameters
    pub params: T,

    /// Path to write results JSON
    pub results: PathBuf,

    /// Working directory for R
    pub wd: PathBuf,
}

impl<T: Serialize> KnitrRequest<T> {
    /// Create a new request.
    pub fn new(action: impl Into<String>, params: T, results: PathBuf, wd: PathBuf) -> Self {
        Self {
            action: action.into(),
            params,
            results,
            wd,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_params_serialization() {
        let params = KnitrExecuteParams {
            input: PathBuf::from("/project/doc.qmd"),
            markdown: "# Hello".to_string(),
            format: KnitrFormatConfig::new("html"),
            temp_dir: PathBuf::from("/tmp/quarto"),
            lib_dir: None,
            dependencies: true,
            cwd: PathBuf::from("/project"),
            params: None,
            resource_dir: PathBuf::from("/usr/share/quarto"),
            handled_languages: crate::engine::HANDLED_LANGUAGES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            capture: false,
        };

        let json = serde_json::to_string(&params).unwrap();

        assert!(json.contains("\"input\":\"/project/doc.qmd\""));
        assert!(json.contains("\"markdown\":\"# Hello\""));
        assert!(json.contains("\"tempDir\":\"/tmp/quarto\""));
        assert!(json.contains("\"dependencies\":true"));
        assert!(json.contains("\"handledLanguages\":[\"ojs\",\"mermaid\",\"dot\"]"));

        // lib_dir is None, should not appear
        assert!(!json.contains("\"libDir\""));
    }

    #[test]
    fn execute_result_without_chunks_field_has_no_chunks() {
        let json = r##"{"engine": "knitr", "markdown": "x"}"##;
        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        assert!(result.chunks.is_empty());
    }

    #[test]
    fn pieces_field_deserializes_with_and_without() {
        let without = r##"{"engine": "knitr", "markdown": "x"}"##;
        let result: KnitrExecuteResult = serde_json::from_str(without).unwrap();
        assert!(result.pieces.is_empty());

        // The S2 example from KNR-1: every piece class on the wire.
        let with = r##"{
            "engine": "knitr",
            "markdown": "x",
            "pieces": [{
                "ordinal": 1,
                "label": "plain",
                "code": "x <- 1:3\nx\n",
                "options": {},
                "pieces": [
                    {"class": "source", "text": "x <- 1:3\n"},
                    {"class": "output", "text": "[1] 1 2 3\n"},
                    {"class": "message", "text": "hi\n"},
                    {"class": "warning", "text": "Warning in f(): boom", "call": "f()"},
                    {"class": "error", "text": "Error in f(): boom\n", "ename": "simpleError",
                     "evalue": "boom", "call": "f()"},
                    {"class": "plot", "files": ["doc_files/figure-html/plain-1.png"]},
                    {"class": "image_paths", "paths": ["made.png"]},
                    {"class": "asis", "text": "<div>..</div>", "mime": "text/html",
                     "widget": true, "knit_meta": "[]"},
                    {"class": "other", "classes": ["weird"]}
                ],
                "files": ["doc_files/figure-html/plain-1.png"]
            }, {"ordinal": 2, "label": "bare", "pieces": []}]
        }"##;
        let result: KnitrExecuteResult = serde_json::from_str(with).unwrap();
        assert_eq!(result.pieces.len(), 2);
        let first = &result.pieces[0];
        assert_eq!(first.ordinal, 1);
        assert_eq!(first.label, "plain");
        assert_eq!(first.pieces.len(), 9);
        assert!(matches!(&first.pieces[0], KnitrPiece::Source { text } if text == "x <- 1:3\n"));
        assert!(matches!(
            &first.pieces[3],
            KnitrPiece::Warning { call: Some(c), .. } if c == "f()"
        ));
        assert!(matches!(
            &first.pieces[7],
            KnitrPiece::Asis {
                widget: true,
                knit_meta: Some(_),
                ..
            }
        ));
        // Optional fields default.
        let second = &result.pieces[1];
        assert_eq!(second.code, "");
        assert!(second.options.is_null());
        assert!(second.files.is_empty());
    }

    #[test]
    fn execute_result_with_chunks_field_deserializes_records() {
        let json = r##"{
            "engine": "knitr",
            "markdown": "x",
            "chunks": [
                {"id": "a1", "markdown": "m1", "optionsDigest": "d1", "files": ["f/a-1.png"]},
                {"id": "a2", "markdown": "", "optionsDigest": "d2"}
            ]
        }"##;
        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        assert_eq!(
            result.chunks,
            vec![
                KnitrChunkRecord {
                    id: "a1".into(),
                    markdown: "m1".into(),
                    options_digest: "d1".into(),
                    files: vec!["f/a-1.png".into()],
                },
                KnitrChunkRecord {
                    id: "a2".into(),
                    markdown: String::new(),
                    options_digest: "d2".into(),
                    files: vec![],
                },
            ]
        );
    }

    #[test]
    fn test_execute_result_deserialization() {
        let json = r##"{
            "engine": "knitr",
            "markdown": "# Output",
            "supporting": ["/path/to/doc_files"],
            "filters": ["rmarkdown/pagebreak.lua"],
            "includes": {},
            "postProcess": false
        }"##;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();

        assert_eq!(result.engine, "knitr");
        assert_eq!(result.markdown, "# Output");
        assert_eq!(result.supporting, vec!["/path/to/doc_files"]);
        assert_eq!(result.filters, vec!["rmarkdown/pagebreak.lua"]);
        assert!(!result.post_process);
    }

    #[test]
    fn test_includes_empty_object() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "includes": {}
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        assert!(result.includes.is_none());
    }

    #[test]
    fn test_includes_empty_array() {
        // R sometimes returns [] instead of {}
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "includes": []
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        assert!(result.includes.is_none());
    }

    #[test]
    fn test_includes_null() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "includes": null
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        assert!(result.includes.is_none());
    }

    #[test]
    fn test_includes_with_values() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "includes": {
                "include-in-header": "/tmp/header.html",
                "include-before-body": "/tmp/before.html"
            }
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        let includes = result.includes.unwrap();

        assert_eq!(
            includes.include_in_header,
            vec![PathBuf::from("/tmp/header.html")]
        );
        assert_eq!(
            includes.include_before_body,
            vec![PathBuf::from("/tmp/before.html")]
        );
        assert!(includes.include_after_body.is_empty());
    }

    #[test]
    fn test_includes_kebab_case() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "includes": {
                "include-in-header": "/tmp/h.html",
                "include-before-body": "/tmp/b.html",
                "include-after-body": "/tmp/a.html"
            }
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();
        let includes = result.includes.unwrap();

        assert_eq!(
            includes.include_in_header,
            vec![PathBuf::from("/tmp/h.html")]
        );
        assert_eq!(
            includes.include_before_body,
            vec![PathBuf::from("/tmp/b.html")]
        );
        assert_eq!(
            includes.include_after_body,
            vec![PathBuf::from("/tmp/a.html")]
        );
    }

    // ── T2: every slot shape the wire can carry (bd-gy2ozix3) ────────────

    fn parse_includes(includes_json: &str) -> Option<KnitrIncludes> {
        let json = format!(r#"{{"engine":"knitr","markdown":"","includes":{includes_json}}}"#);
        serde_json::from_str::<KnitrExecuteResult>(&json)
            .unwrap_or_else(|e| panic!("includes {includes_json} must deserialize: {e}"))
            .includes
    }

    #[test]
    fn include_slot_accepts_two_element_array_in_order() {
        let includes = parse_includes(r#"{"include-in-header":["/tmp/a","/tmp/b"]}"#).unwrap();
        assert_eq!(
            includes.include_in_header,
            vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]
        );
    }

    #[test]
    fn include_slot_accepts_bare_string() {
        let includes = parse_includes(r#"{"include-after-body":"/tmp/after"}"#).unwrap();
        assert_eq!(
            includes.include_after_body,
            vec![PathBuf::from("/tmp/after")]
        );
        assert!(includes.include_in_header.is_empty());
    }

    #[test]
    fn include_slot_empty_array_counts_as_absent() {
        assert!(parse_includes(r#"{"include-in-header":[]}"#).is_none());
    }

    #[test]
    fn include_slot_null_counts_as_absent() {
        assert!(parse_includes(r#"{"include-in-header":null}"#).is_none());
    }

    #[test]
    fn include_slot_rejects_non_string_element_naming_the_slot() {
        let json = r#"{"engine":"knitr","markdown":"","includes":{"include-in-header":[42]}}"#;
        let err = serde_json::from_str::<KnitrExecuteResult>(json).unwrap_err();
        assert!(
            err.to_string()
                .contains("invalid type: integer `42`, expected a string"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn includes_non_empty_array_is_rejected() {
        let json = r#"{"engine":"knitr","markdown":"","includes":["/tmp/x"]}"#;
        let err = serde_json::from_str::<KnitrExecuteResult>(json).unwrap_err();
        assert!(
            err.to_string().contains("got a non-empty array"),
            "unexpected error: {err}"
        );
    }

    /// bd-gy2ozix3 / GH #683: the results file exactly as `execute.R`
    /// writes it when the document attaches an HTML dependency (an
    /// htmlwidget, or any `htmltools::htmlDependency`). Captured verbatim
    /// from a failing render on 2026-09-18, with only the `markdown` field
    /// elided and paths shortened. `create_pandoc_includes` wraps each
    /// include path in `I()`, so jsonlite's `auto_unbox = TRUE` leaves it
    /// as a one-element array — Quarto 1's declared type is `string[]`.
    /// Revert binding: typing a slot as a single path makes this fail with
    /// `invalid type: sequence, expected path string`.
    const CAPTURED_HTML_DEPENDENCY_RESULT: &str = r#"{"engine":"knitr","markdown":"","supporting":["/work/doc_files"],"filters":["rmarkdown/pagebreak.lua"],"includes":{"include-in-header":["/tmp/quarto-pipeline_46gJNU/file65026e45f832"]},"engineDependencies":{},"preserve":{},"postProcess":true}"#;

    #[test]
    fn captured_html_dependency_result_deserializes() {
        let result: KnitrExecuteResult = serde_json::from_str(CAPTURED_HTML_DEPENDENCY_RESULT)
            .expect("the shape execute.R actually writes must deserialize");
        let includes = result.includes.expect("one include slot is populated");
        assert_eq!(
            includes.include_in_header,
            vec![PathBuf::from(
                "/tmp/quarto-pipeline_46gJNU/file65026e45f832"
            )]
        );
        assert!(includes.include_before_body.is_empty());
        assert!(includes.include_after_body.is_empty());
        assert!(result.post_process);
    }

    #[test]
    fn test_execute_result_missing_optional_fields() {
        // Minimal response
        let json = r##"{
            "engine": "knitr",
            "markdown": "# Output"
        }"##;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();

        assert_eq!(result.markdown, "# Output");
        assert!(result.supporting.is_empty());
        assert!(result.filters.is_empty());
        assert!(result.includes.is_none());
        assert!(!result.post_process);
    }

    #[test]
    fn test_knitr_request_serialization() {
        let params = KnitrExecuteParams {
            input: PathBuf::from("/doc.qmd"),
            markdown: "test".to_string(),
            format: KnitrFormatConfig::new("html"),
            temp_dir: PathBuf::from("/tmp"),
            lib_dir: None,
            dependencies: true,
            cwd: PathBuf::from("/project"),
            params: None,
            resource_dir: PathBuf::from("/resources"),
            handled_languages: vec![],
            capture: false,
        };

        let request = KnitrRequest::new(
            "execute",
            params,
            PathBuf::from("/tmp/results.json"),
            PathBuf::from("/project"),
        );

        let json = serde_json::to_string(&request).unwrap();

        assert!(json.contains("\"action\":\"execute\""));
        assert!(json.contains("\"results\":\"/tmp/results.json\""));
        assert!(json.contains("\"wd\":\"/project\""));
        assert!(json.contains("\"params\":{"));
    }

    #[test]
    fn test_engine_dependencies_preserved() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "engineDependencies": {
                "htmlwidgets": {
                    "version": "1.5.4"
                }
            }
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();

        assert!(result.engine_dependencies.is_some());
        let deps = result.engine_dependencies.unwrap();
        assert!(deps.get("htmlwidgets").is_some());
    }

    #[test]
    fn test_preserve_field() {
        let json = r#"{
            "engine": "knitr",
            "markdown": "test",
            "preserve": {
                "uuid1": "<div>preserved content</div>"
            },
            "postProcess": true
        }"#;

        let result: KnitrExecuteResult = serde_json::from_str(json).unwrap();

        assert!(result.preserve.is_some());
        assert!(result.post_process);
    }
}

/// R writes a missing number as the string `"NA"`; anything that is not a number is `None`.
fn numbers_or_none<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Option<f64>>, D::Error> {
    let values = Vec::<serde_json::Value>::deserialize(d)?;
    Ok(values.iter().map(|v| v.as_f64()).collect())
}
