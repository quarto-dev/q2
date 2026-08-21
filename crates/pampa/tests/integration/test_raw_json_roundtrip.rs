/*
 * test_raw_json_roundtrip.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Roundtrip contract tests for the pampa-native `raw-json` format
 * (GH issue #11, bd-en2hvrwn, plan
 * claude-notes/plans/2026-07-17-raw-json-format.md).
 *
 * Contract under test: `raw_json::write` then `raw_json::read` is the
 * identity (structural equality) on the pampa AST — including every
 * extension Pandoc JSON cannot represent — plus targeted rejection
 * diagnostics when the wrong format is fed to either reader.
 */

use std::sync::Arc;

use hashlink::LinkedHashMap;
use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::attr::AttrSourceInfo;
use pampa::pandoc::{
    Block, CaptionBlock, CustomNode, Delete, Div, EditComment, Highlight, Inline, InlineAttr,
    Insert, MetaBlock, NoteDefinitionFencedBlock, NoteDefinitionPara, NoteReference, Pandoc,
    Paragraph, Shortcode, ShortcodeArg, Slot, Str,
};
use pampa::readers;
use pampa::readers::json::JsonReadError;
use pampa::writers::{json, raw_json};
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
use quarto_source_map::{FileId, SourceInfo};

/// Original-file source info spanning `start..end` in FileId(0).
fn si(start: usize, end: usize) -> SourceInfo {
    SourceInfo::original(FileId(0), start, end)
}

fn str_inline(text: &str, start: usize, end: usize) -> Inline {
    Inline::Str(Str {
        text: text.to_string(),
        source_info: si(start, end),
    })
}

fn para(content: Vec<Inline>, start: usize, end: usize) -> Block {
    Block::Paragraph(Paragraph {
        content,
        source_info: si(start, end),
    })
}

fn empty_attr() -> quarto_pandoc_types::Attr {
    (String::new(), vec![], LinkedHashMap::new())
}

/// Write `doc` as raw-json, read it back, and return the parsed document
/// and context.
fn roundtrip(doc: &Pandoc, context: &ASTContext) -> (Pandoc, ASTContext) {
    let mut out = Vec::new();
    raw_json::write(doc, context, &mut out).expect("raw-json write should succeed");
    let mut cursor = std::io::Cursor::new(out);
    readers::raw_json::read(&mut cursor).expect("raw-json read should succeed")
}

fn assert_roundtrip_identity(doc: &Pandoc, context: &ASTContext) {
    let (parsed, _parsed_context) = roundtrip(doc, context);
    assert_eq!(doc, &parsed, "raw-json write→read must be the identity");
}

// ---------------------------------------------------------------------------
// Envelope shape
// ---------------------------------------------------------------------------

#[test]
fn test_raw_json_marker_is_first_key_and_no_pandoc_api_version() {
    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(vec![str_inline("hi", 0, 2)], 0, 2)],
    };
    let context = ASTContext::new();
    let mut out = Vec::new();
    raw_json::write(&doc, &context, &mut out).expect("write should succeed");
    let text = String::from_utf8(out).expect("valid UTF-8");

    // The marker must be the first key so it is visible in truncated
    // previews / the first line of output.
    assert!(
        text.starts_with(r#"{"pampa-json-format":{"version":1}"#),
        "output must start with the pampa-json-format marker, got: {}",
        &text[..text.len().min(80)]
    );
    // Machine consumers of Pandoc JSON key on pandoc-api-version; it must
    // be absent so they fail fast.
    assert!(
        !text.contains("pandoc-api-version"),
        "raw-json must not contain pandoc-api-version"
    );

    let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(value["pampa-json-format"]["version"], 1);
}

// ---------------------------------------------------------------------------
// Extension inlines: the constructs Pandoc JSON rejects or desugars
// ---------------------------------------------------------------------------

/// The issue #11 repro: a standalone attribute must roundtrip through
/// raw-json (while the Pandoc-superset writer errors with Q-3-32).
///
/// The Attr sits mid-paragraph: a *trailing* paragraph Attr is
/// representable in the Pandoc-superset format via the para-`attr` hoist
/// (bd-aeyss6p5) and would not exercise the Q-3-32 path.
#[test]
fn test_raw_json_roundtrip_standalone_attr() {
    let mut kvs = LinkedHashMap::new();
    kvs.insert("key".to_string(), "value".to_string());
    let attr = (
        "free-floating".to_string(),
        vec!["cls-a".to_string(), "cls-b".to_string()],
        kvs,
    );
    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(
            vec![
                str_inline("Hello.", 0, 6),
                Inline::Attr(InlineAttr::new(attr, AttrSourceInfo::empty(), si(8, 32))),
                str_inline("Here?", 34, 39),
            ],
            0,
            39,
        )],
    };
    let context = ASTContext::new();

    // Sanity: the Pandoc-superset JSON writer refuses this document.
    let mut pandoc_json = Vec::new();
    let err = json::write(&doc, &context, &mut pandoc_json)
        .expect_err("pandoc-superset json must reject standalone Attr");
    assert!(
        err.iter().any(|d| d.code.as_deref() == Some("Q-3-32")),
        "expected Q-3-32 from the pandoc-superset writer, got: {:?}",
        err.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
    );

    // The raw writer accepts it and roundtrips identically.
    assert_roundtrip_identity(&doc, &context);
}

#[test]
fn test_raw_json_roundtrip_note_reference() {
    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(
            vec![
                str_inline("See", 0, 3),
                Inline::NoteReference(NoteReference {
                    id: "note-1".to_string(),
                    source_info: si(4, 12),
                }),
            ],
            0,
            12,
        )],
    };
    assert_roundtrip_identity(&doc, &ASTContext::new());
}

#[test]
fn test_raw_json_roundtrip_critic_markup() {
    let mut kvs = LinkedHashMap::new();
    kvs.insert("author".to_string(), "cs".to_string());
    let attr = ("crit-1".to_string(), vec!["review".to_string()], kvs);

    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(
            vec![
                Inline::Insert(Insert {
                    attr: attr.clone(),
                    content: vec![str_inline("added", 3, 8)],
                    source_info: si(0, 11),
                    attr_source: AttrSourceInfo::empty(),
                }),
                Inline::Delete(Delete {
                    attr: empty_attr(),
                    content: vec![str_inline("removed", 14, 21)],
                    source_info: si(11, 24),
                    attr_source: AttrSourceInfo::empty(),
                }),
                Inline::Highlight(Highlight {
                    attr: empty_attr(),
                    content: vec![str_inline("marked", 27, 33)],
                    source_info: si(24, 36),
                    attr_source: AttrSourceInfo::empty(),
                }),
                Inline::EditComment(EditComment {
                    attr: empty_attr(),
                    content: vec![str_inline("why?", 39, 43)],
                    source_info: si(36, 46),
                    attr_source: AttrSourceInfo::empty(),
                }),
            ],
            0,
            46,
        )],
    };
    assert_roundtrip_identity(&doc, &ASTContext::new());
}

#[test]
fn test_raw_json_roundtrip_shortcode() {
    let mut keyword_args = LinkedHashMap::new();
    keyword_args.insert(
        "width".to_string(),
        ShortcodeArg::String("100%".to_string()),
    );
    keyword_args.insert("autoplay".to_string(), ShortcodeArg::Boolean(true));

    let nested = Shortcode {
        is_escaped: false,
        name: "meta".to_string(),
        positional_args: vec![ShortcodeArg::String("title".to_string())],
        keyword_args: LinkedHashMap::new(),
        source_info: si(20, 34),
    };

    let mut kv_map = std::collections::HashMap::new();
    kv_map.insert("a".to_string(), ShortcodeArg::Number(1.5));
    kv_map.insert("b".to_string(), ShortcodeArg::String("two".to_string()));

    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(
            vec![Inline::Shortcode(Shortcode {
                is_escaped: true,
                name: "video".to_string(),
                positional_args: vec![
                    ShortcodeArg::String("intro.mp4".to_string()),
                    ShortcodeArg::Number(2.0),
                    ShortcodeArg::Shortcode(nested),
                    ShortcodeArg::KeyValue(kv_map),
                ],
                keyword_args,
                source_info: si(0, 40),
            })],
            0,
            40,
        )],
    };
    assert_roundtrip_identity(&doc, &ASTContext::new());
}

// ---------------------------------------------------------------------------
// Extension blocks
// ---------------------------------------------------------------------------

#[test]
fn test_raw_json_roundtrip_caption_block() {
    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![
            Block::CaptionBlock(CaptionBlock {
                content: vec![str_inline("A caption", 0, 9)],
                source_info: si(0, 9),
            }),
            para(vec![str_inline("body", 10, 14)], 10, 14),
        ],
    };
    assert_roundtrip_identity(&doc, &ASTContext::new());
}

#[test]
fn test_raw_json_roundtrip_note_definitions_and_block_metadata() {
    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![
            Block::NoteDefinitionPara(NoteDefinitionPara {
                id: "fn-1".to_string(),
                content: vec![str_inline("a footnote", 0, 10)],
                source_info: si(0, 10),
            }),
            Block::NoteDefinitionFencedBlock(NoteDefinitionFencedBlock {
                id: "fn-2".to_string(),
                content: vec![para(vec![str_inline("fenced note", 11, 22)], 11, 22)],
                source_info: si(11, 22),
            }),
            Block::BlockMetadata(MetaBlock {
                meta: ConfigValue {
                    value: ConfigValueKind::Map(vec![ConfigMapEntry {
                        key: "layout".to_string(),
                        key_source: si(23, 29),
                        value: ConfigValue {
                            value: ConfigValueKind::scalar(yaml_rust2::Yaml::String(
                                "wide".to_string(),
                            )),
                            source_info: si(31, 35),
                            merge_op: MergeOp::default(),
                        },
                    }]),
                    source_info: si(23, 35),
                    merge_op: MergeOp::default(),
                },
                source_info: si(23, 35),
            }),
        ],
    };
    assert_roundtrip_identity(&doc, &ASTContext::new());
}

#[test]
fn test_raw_json_roundtrip_custom_nodes() {
    let block_custom = CustomNode::new("Callout", empty_attr(), si(0, 30))
        .with_slot("title", Slot::Inlines(vec![str_inline("Watch out", 5, 14)]))
        .with_slot(
            "content",
            Slot::Blocks(vec![para(vec![str_inline("body", 15, 19)], 15, 19)]),
        )
        .with_data(serde_json::json!({"type": "warning", "appearance": "default"}));

    let inline_custom = CustomNode::new("Kbd", empty_attr(), si(31, 40))
        .with_slot("keys", Slot::Inline(Box::new(str_inline("Ctrl", 33, 37))));

    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![
            Block::Custom(block_custom),
            para(vec![Inline::Custom(inline_custom)], 31, 40),
        ],
    };
    assert_roundtrip_identity(&doc, &ASTContext::new());
}

// ---------------------------------------------------------------------------
// Metadata fidelity
// ---------------------------------------------------------------------------

/// Everything the Pandoc-superset meta encoding loses, raw-json must keep:
/// Path/Glob/Expr kinds, merge_op, non-string scalar types, and map entry
/// order.
#[test]
fn test_raw_json_meta_fidelity() {
    let entry = |key: &str, ks: (usize, usize), value: ConfigValue| ConfigMapEntry {
        key: key.to_string(),
        key_source: si(ks.0, ks.1),
        value,
    };
    let scalar = |yaml: yaml_rust2::Yaml, s: (usize, usize)| ConfigValue {
        value: ConfigValueKind::scalar(yaml),
        source_info: si(s.0, s.1),
        merge_op: MergeOp::default(),
    };

    let meta = ConfigValue {
        value: ConfigValueKind::Map(vec![
            // Deliberately NOT in alphabetical order: order must survive.
            entry(
                "zebra",
                (0, 5),
                scalar(yaml_rust2::Yaml::String("stripes".to_string()), (7, 14)),
            ),
            entry(
                "resources",
                (15, 24),
                ConfigValue {
                    value: ConfigValueKind::Glob("images/*.png".to_string()),
                    source_info: si(26, 38),
                    merge_op: MergeOp::Concat,
                },
            ),
            entry(
                "logo",
                (39, 43),
                ConfigValue {
                    value: ConfigValueKind::Path("assets/logo.svg".to_string()),
                    source_info: si(45, 60),
                    // Non-default merge op (default is Concat): exercises the
                    // `m` key emission path.
                    merge_op: MergeOp::Prefer,
                },
            ),
            entry(
                "date",
                (61, 65),
                ConfigValue {
                    value: ConfigValueKind::Expr("Sys.Date()".to_string()),
                    source_info: si(67, 77),
                    merge_op: MergeOp::default(),
                },
            ),
            entry(
                "count",
                (78, 83),
                scalar(yaml_rust2::Yaml::Integer(42), (85, 87)),
            ),
            entry(
                "ratio",
                (88, 93),
                scalar(yaml_rust2::Yaml::Real("0.75".to_string()), (95, 99)),
            ),
            entry(
                "draft",
                (100, 105),
                scalar(yaml_rust2::Yaml::Boolean(false), (107, 112)),
            ),
            entry(
                "abstract",
                (113, 121),
                scalar(yaml_rust2::Yaml::Null, (122, 123)),
            ),
            entry(
                "alpha",
                (124, 129),
                ConfigValue {
                    value: ConfigValueKind::Array(vec![
                        scalar(yaml_rust2::Yaml::Integer(1), (131, 132)),
                        scalar(yaml_rust2::Yaml::Integer(2), (133, 134)),
                    ]),
                    source_info: si(130, 135),
                    merge_op: MergeOp::Concat,
                },
            ),
            entry(
                "title",
                (136, 141),
                ConfigValue {
                    value: ConfigValueKind::PandocInlines(vec![
                        str_inline("Hello", 143, 148),
                        str_inline("world", 149, 154),
                    ]),
                    source_info: si(143, 154),
                    merge_op: MergeOp::default(),
                },
            ),
        ]),
        source_info: si(0, 154),
        merge_op: MergeOp::default(),
    };

    let doc = Pandoc {
        meta,
        blocks: vec![para(vec![str_inline("x", 155, 156)], 155, 156)],
    };
    let context = ASTContext::new();
    let (parsed, _) = roundtrip(&doc, &context);

    // Full identity, which covers kinds, merge ops, scalar types, and
    // source infos in one shot.
    assert_eq!(doc.meta, parsed.meta, "meta must roundtrip identically");

    // Entry order asserted explicitly so a future "helpful" sort is caught
    // even if equality semantics ever change.
    if let ConfigValueKind::Map(entries) = &parsed.meta.value {
        let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "zebra",
                "resources",
                "logo",
                "date",
                "count",
                "ratio",
                "draft",
                "abstract",
                "alpha",
                "title"
            ],
            "meta entry order must be preserved"
        );
    } else {
        panic!("expected Map meta after roundtrip");
    }
}

/// A `Scalar` holding a non-scalar YAML value (Array/Hash) cannot be
/// represented faithfully; the raw writer must fail loudly rather than
/// silently degrade the value.
#[test]
fn test_raw_json_rejects_non_scalar_yaml_in_scalar() {
    let doc = Pandoc {
        meta: ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: "bad".to_string(),
                key_source: si(0, 3),
                value: ConfigValue {
                    value: ConfigValueKind::scalar(yaml_rust2::Yaml::Array(vec![
                        yaml_rust2::Yaml::Integer(1),
                    ])),
                    source_info: si(5, 8),
                    merge_op: MergeOp::default(),
                },
            }]),
            source_info: si(0, 8),
            merge_op: MergeOp::default(),
        },
        blocks: vec![],
    };
    let mut out = Vec::new();
    raw_json::write(&doc, &ASTContext::new(), &mut out)
        .expect_err("raw-json must not silently degrade non-scalar YAML in Scalar");
}

// ---------------------------------------------------------------------------
// Source-info preservation
// ---------------------------------------------------------------------------

/// Two nodes sharing one `Substring` parent through the same `Arc` must
/// come back with equal (structurally shared) parent chains.
#[test]
fn test_raw_json_preserves_shared_substring_parents() {
    let parent = Arc::new(si(0, 100));
    let shared_a = SourceInfo::Substring {
        parent: parent.clone(),
        start_offset: 0,
        end_offset: 5,
    };
    let shared_b = SourceInfo::Substring {
        parent: parent.clone(),
        start_offset: 6,
        end_offset: 11,
    };

    let concat = SourceInfo::concat(vec![(shared_a.clone(), 0), (shared_b.clone(), 5)]);
    let generated = SourceInfo::generated(quarto_source_map::By::filter("f.lua".to_string(), 3));

    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(
            vec![
                Inline::Str(Str {
                    text: "one".to_string(),
                    source_info: shared_a,
                }),
                Inline::Str(Str {
                    text: "two".to_string(),
                    source_info: shared_b,
                }),
                Inline::Str(Str {
                    text: "cat".to_string(),
                    source_info: concat,
                }),
                Inline::Str(Str {
                    text: "gen".to_string(),
                    source_info: generated,
                }),
            ],
            0,
            100,
        )],
    };
    assert_roundtrip_identity(&doc, &ASTContext::new());
}

/// ASTContext state must roundtrip: filenames and the example-list counter.
#[test]
fn test_raw_json_roundtrips_ast_context() {
    let context = ASTContext::with_filename("doc.qmd");
    context.example_list_counter.set(7);

    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(vec![str_inline("hi", 0, 2)], 0, 2)],
    };
    let (_, parsed_context) = roundtrip(&doc, &context);
    assert_eq!(parsed_context.filenames, vec!["doc.qmd"]);
    assert_eq!(
        parsed_context.example_list_counter.get(),
        7,
        "example_list_counter must be carried in the raw envelope"
    );
}

// ---------------------------------------------------------------------------
// Format self-identification / cross-format rejection
// ---------------------------------------------------------------------------

#[test]
fn test_raw_json_reader_rejects_pandoc_style_json() {
    // Real output of the Pandoc-superset writer.
    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(vec![str_inline("hi", 0, 2)], 0, 2)],
    };
    let mut pandoc_json = Vec::new();
    json::write(&doc, &ASTContext::new(), &mut pandoc_json).expect("pandoc json write");

    let mut cursor = std::io::Cursor::new(pandoc_json);
    let err =
        readers::raw_json::read(&mut cursor).expect_err("raw reader must reject Pandoc-style JSON");
    let msg = err.to_string();
    assert!(
        msg.contains("-f json"),
        "rejection must point the user at `-f json`, got: {}",
        msg
    );
}

#[test]
fn test_raw_json_reader_rejects_unmarked_json() {
    let input = br#"{"blocks": [], "meta": {}}"#;
    let mut cursor = std::io::Cursor::new(&input[..]);
    let err =
        readers::raw_json::read(&mut cursor).expect_err("raw reader must reject unmarked JSON");
    let msg = err.to_string();
    assert!(
        msg.contains("pampa-json-format"),
        "rejection must name the missing marker, got: {}",
        msg
    );
}

#[test]
fn test_raw_json_reader_rejects_wrong_version() {
    let input = br#"{"pampa-json-format": {"version": 999}, "blocks": [], "meta": {}}"#;
    let mut cursor = std::io::Cursor::new(&input[..]);
    let err =
        readers::raw_json::read(&mut cursor).expect_err("raw reader must reject unknown versions");
    let msg = err.to_string();
    assert!(
        msg.contains("999"),
        "version rejection must name the offending version, got: {}",
        msg
    );
}

#[test]
fn test_pandoc_json_reader_rejects_raw_json() {
    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(vec![str_inline("hi", 0, 2)], 0, 2)],
    };
    let mut raw_out = Vec::new();
    raw_json::write(&doc, &ASTContext::new(), &mut raw_out).expect("raw write");

    let mut cursor = std::io::Cursor::new(raw_out);
    let err = readers::json::read(&mut cursor)
        .expect_err("pandoc json reader must reject raw-json input");
    let msg = err.to_string();
    assert!(
        msg.contains("raw-json"),
        "rejection must point the user at the raw-json reader, got: {}",
        msg
    );
    assert!(
        matches!(err, JsonReadError::UnexpectedRawJsonMarker),
        "expected UnexpectedRawJsonMarker, got: {:?}",
        err
    );
}

// ---------------------------------------------------------------------------
// Stability across generations
// ---------------------------------------------------------------------------

/// AST-level idempotence: read(write(read(write(doc)))) == read(write(doc)).
/// (Byte-level stability of the pool is explicitly NOT promised — see the
/// plan's roundtrip-contract section.)
#[test]
fn test_raw_json_ast_stable_across_generations() {
    let parent = Arc::new(si(0, 50));
    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(
            vec![
                Inline::Str(Str {
                    text: "a".to_string(),
                    source_info: SourceInfo::Substring {
                        parent: parent.clone(),
                        start_offset: 0,
                        end_offset: 1,
                    },
                }),
                Inline::Str(Str {
                    text: "b".to_string(),
                    source_info: SourceInfo::Substring {
                        parent: parent.clone(),
                        start_offset: 1,
                        end_offset: 2,
                    },
                }),
            ],
            0,
            50,
        )],
    };
    let context = ASTContext::new();
    let (gen1, ctx1) = roundtrip(&doc, &context);
    let (gen2, _) = roundtrip(&gen1, &ctx1);
    assert_eq!(gen1, gen2, "second-generation roundtrip must be stable");
}

// ---------------------------------------------------------------------------
// End-to-end from qmd: the issue #11 document
// ---------------------------------------------------------------------------

/// Parse the exact document from GH issue #11 with the qmd reader — which
/// produces a standalone `Inline::Attr` — and roundtrip the parser-produced
/// AST (real Substring/Concat source infos) through raw-json.
#[test]
fn test_raw_json_roundtrip_issue_11_document() {
    let input = "Hello. {#free-floating-attribute} Here?\n";
    let (doc, context, diagnostics) = readers::qmd::read(
        input.as_bytes(),
        false,
        "<stdin>",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("qmd parse should succeed");
    assert!(
        diagnostics.is_empty(),
        "expected clean parse, got: {:?}",
        diagnostics
    );

    // Confirm the fixture exercises what we think it does.
    let has_standalone_attr = doc.blocks.iter().any(|b| match b {
        Block::Paragraph(p) => p.content.iter().any(|i| matches!(i, Inline::Attr(_))),
        _ => false,
    });
    assert!(
        has_standalone_attr,
        "issue #11 fixture should produce a standalone Inline::Attr; blocks: {:?}",
        doc.blocks
    );

    assert_roundtrip_identity(&doc, &context);
}

/// Corpus sweep: every document in the existing json-writer fixture
/// corpus (`tests/writers/json/*.md`), parsed by the real qmd reader,
/// must roundtrip identically through raw-json. Identity assertions
/// subsume snapshots here, so raw-json has no parallel snapshot corpus —
/// this sweep provides the breadth instead.
#[test]
fn test_raw_json_roundtrip_writer_fixture_corpus() {
    let dir = std::path::Path::new("tests/writers/json");
    let mut count = 0;
    for entry in std::fs::read_dir(dir).expect("fixture corpus dir must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let content = std::fs::read(&path).expect("readable fixture");
        let (doc, context, _diagnostics) = readers::qmd::read(
            &content,
            false,
            &path.to_string_lossy(),
            &mut std::io::sink(),
            true,
            None,
        )
        .unwrap_or_else(|e| panic!("qmd parse failed for {}: {:?}", path.display(), e));

        let (parsed, _) = roundtrip(&doc, &context);
        assert_eq!(
            doc,
            parsed,
            "raw-json roundtrip must be the identity for fixture {}",
            path.display()
        );
        count += 1;
    }
    assert!(count > 0, "corpus sweep found no fixtures — path wrong?");
}

// ---------------------------------------------------------------------------
// Interop sanity: constructs that already roundtrip through pandoc json
// must also roundtrip through raw-json
// ---------------------------------------------------------------------------

/// Sidecar source infos that the shared reader used to drop on read-back
/// (found by the corpus sweep): Link/Image `target_source` and Citation
/// `id_source` must survive.
#[test]
fn test_raw_json_roundtrip_sidecar_source_infos() {
    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![para(
            vec![
                Inline::Link(pampa::pandoc::Link {
                    attr: empty_attr(),
                    content: vec![str_inline("a link", 1, 7)],
                    target: ("./hello".to_string(), "out".to_string()),
                    source_info: si(0, 46),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: pampa::pandoc::attr::TargetSourceInfo {
                        url: Some(si(9, 16)),
                        title: Some(si(17, 22)),
                    },
                }),
                Inline::Cite(pampa::pandoc::Cite {
                    citations: vec![pampa::pandoc::Citation {
                        id: "knuth1984".to_string(),
                        prefix: vec![str_inline("see", 48, 51)],
                        suffix: vec![str_inline("p. 42", 62, 67)],
                        mode: pampa::pandoc::CitationMode::NormalCitation,
                        note_num: 0,
                        hash: 0,
                        id_source: Some(si(52, 61)),
                    }],
                    content: vec![str_inline("[@knuth1984]", 47, 68)],
                    source_info: si(47, 68),
                }),
            ],
            0,
            68,
        )],
    };
    assert_roundtrip_identity(&doc, &ASTContext::new());
}

#[test]
fn test_raw_json_roundtrip_standard_constructs() {
    let mut kvs = LinkedHashMap::new();
    kvs.insert("style".to_string(), "border: 1px".to_string());
    let doc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![
            para(
                vec![
                    str_inline("plain", 0, 5),
                    Inline::Emph(pampa::pandoc::Emph {
                        content: vec![str_inline("emph", 6, 10)],
                        source_info: si(5, 11),
                    }),
                ],
                0,
                11,
            ),
            Block::Div(Div {
                attr: ("d1".to_string(), vec!["note".to_string()], kvs),
                content: vec![para(vec![str_inline("inner", 12, 17)], 12, 17)],
                source_info: si(11, 18),
                attr_source: AttrSourceInfo::empty(),
            }),
            Block::CodeBlock(pampa::pandoc::CodeBlock {
                attr: (
                    "cb".to_string(),
                    vec!["rust".to_string()],
                    LinkedHashMap::new(),
                ),
                text: "fn main() {}".to_string(),
                source_info: si(19, 31),
                attr_source: AttrSourceInfo::empty(),
            }),
        ],
    };
    assert_roundtrip_identity(&doc, &ASTContext::new());
}
