//! Tests for round-trip serialization, forward compatibility, and build metadata.

use quarto_trace::read::{list_traces, read_trace};
use quarto_trace::write::write_trace;
use quarto_trace::{
    BUILD_GIT_HASH, EngineCapture, RenderInfo, SCHEMA_VERSION, StageErrorInfo, StageStatus,
    TraceDocument, TraceEntry,
};
use serde_json::json;

fn sample_doc() -> TraceDocument {
    let render = RenderInfo {
        input_path: Some("doc.qmd".into()),
        output_path: Some("doc.html".into()),
        format_target: Some("html".into()),
        started_at_unix_ms: Some(1_799_200_496_000.0),
        git_hash: Some("abc1234-dirty".into()),
        total_duration_ms: Some(123.4),
    };

    let mut doc = TraceDocument::new(render);
    doc.pipeline.push(TraceEntry {
        stage: "parse".into(),
        index: 0,
        data_kind: Some("DocumentAst".into()),
        data: Some(json!({"blocks": []})),
        duration_ms: Some(1.2),
        status: StageStatus::Ok,
        error: None,
    });
    doc.pipeline.push(TraceEntry {
        stage: "engine-execution".into(),
        index: 1,
        data_kind: None,
        data: None,
        duration_ms: None,
        status: StageStatus::Error,
        error: Some(StageErrorInfo {
            message: "jupyter kernel died".into(),
        }),
    });
    doc.pipeline.push(TraceEntry {
        stage: "render-html-body".into(),
        index: 2,
        data_kind: None,
        data: None,
        duration_ms: None,
        status: StageStatus::Skipped,
        error: None,
    });
    doc
}

#[test]
fn test_roundtrip_through_disk() {
    let tmp = std::env::temp_dir().join("quarto-trace-roundtrip");
    let _ = std::fs::remove_dir_all(&tmp);
    let path = tmp.join("latest.json");

    let doc = sample_doc();
    write_trace(&doc, &path).unwrap();

    let read_back = read_trace(&path).unwrap();

    assert_eq!(read_back.schema_version, SCHEMA_VERSION);
    assert_eq!(read_back.render.input_path, doc.render.input_path);
    assert_eq!(read_back.render.git_hash, doc.render.git_hash);
    assert_eq!(read_back.pipeline.len(), 3);

    assert_eq!(read_back.pipeline[0].stage, "parse");
    assert_eq!(read_back.pipeline[0].status, StageStatus::Ok);
    assert!(read_back.pipeline[0].data.is_some());

    assert_eq!(read_back.pipeline[1].stage, "engine-execution");
    assert_eq!(read_back.pipeline[1].status, StageStatus::Error);
    assert!(read_back.pipeline[1].data.is_none());
    assert_eq!(
        read_back.pipeline[1].error.as_ref().unwrap().message,
        "jupyter kernel died"
    );

    assert_eq!(read_back.pipeline[2].status, StageStatus::Skipped);
}

#[test]
fn test_forward_compat_unknown_status() {
    // A trace written by a future version that includes a status variant we
    // don't recognize yet should deserialize with `Unknown`, not fail.
    let json_text = r#"{
      "schema_version": 1,
      "render": {},
      "pipeline": [
        { "stage": "future-stage", "index": 0, "status": "partially-executed" }
      ]
    }"#;
    let doc: TraceDocument = serde_json::from_str(json_text).unwrap();
    assert_eq!(doc.pipeline[0].status, StageStatus::Unknown);
}

#[test]
fn test_forward_compat_unknown_fields() {
    // Unknown fields at any level should not cause deserialization to fail —
    // future writers can add new metadata without breaking today's readers.
    let json_text = r#"{
      "schema_version": 2,
      "render": {"new_future_field": 42},
      "pipeline": [
        { "stage": "parse", "index": 0, "status": "ok",
          "speculative_delta_base": 0 }
      ],
      "new_top_level_field": "hi"
    }"#;
    let doc: TraceDocument = serde_json::from_str(json_text).unwrap();
    assert_eq!(doc.schema_version, 2);
    assert_eq!(doc.pipeline[0].stage, "parse");
}

#[test]
fn test_legacy_trace_without_status_defaults_to_ok() {
    // Pre-status traces should default to Ok.
    let json_text = r#"{
      "schema_version": 1,
      "render": {},
      "pipeline": [ { "stage": "parse", "index": 0 } ]
    }"#;
    let doc: TraceDocument = serde_json::from_str(json_text).unwrap();
    assert_eq!(doc.pipeline[0].status, StageStatus::Ok);
}

/// bd-5qnj Phase 1a: writer emits compact JSON on disk.
///
/// The on-disk artifact must not be pretty-printed — pretty-print accounts
/// for ~80% of bytes on real traces (`claude-notes/plans/5qnj-trace-size-investigation/measurements.md`).
/// Humans who want a pretty view use `quarto trace show` (which formats
/// from the parsed `TraceDocument`) or `jq` on the file.
#[test]
fn test_writer_emits_compact_json_on_disk() {
    let tmp = std::env::temp_dir().join(format!(
        "quarto-trace-compact-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("latest.json");

    write_trace(&sample_doc(), &path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let s = std::str::from_utf8(&bytes).unwrap();

    // Compact JSON has no `\n` between top-level keys and no leading
    // indentation. `serde_json::to_writer_pretty` writes one token per
    // line with two-space indentation; both signatures are absent in
    // compact output.
    assert!(
        !s.contains("\n  "),
        "trace file appears to be pretty-printed (found indented line); first 200 bytes: {:?}",
        &s.chars().take(200).collect::<String>()
    );
    // Pretty output also starts with `{\n  "schema_version"`; compact
    // starts with `{"schema_version"`.
    assert!(
        s.starts_with("{\"schema_version\""),
        "expected compact start, got: {:?}",
        &s.chars().take(40).collect::<String>()
    );

    // Sanity: still parses back to an equivalent doc.
    let read_back = read_trace(&path).unwrap();
    assert_eq!(read_back.pipeline.len(), sample_doc().pipeline.len());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// bd-5qnj Phase 1b: writer emits gzipped bytes when the path ends in
/// `.gz`, and `read_trace` transparently inflates them.
#[test]
fn test_roundtrip_through_gzipped_disk() {
    let tmp = std::env::temp_dir().join(format!(
        "quarto-trace-gz-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("latest.json.gz");

    let doc = sample_doc();
    write_trace(&doc, &path).unwrap();

    // Bytes on disk must look like a gzip stream (magic 0x1f 0x8b).
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b,
        "expected gzip magic at start, got first 4 bytes = {:x?}",
        &bytes[..bytes.len().min(4)]
    );

    // Reader recognizes the .gz extension and transparently inflates.
    let read_back = read_trace(&path).unwrap();
    assert_eq!(read_back.schema_version, SCHEMA_VERSION);
    assert_eq!(read_back.pipeline.len(), doc.pipeline.len());
    assert_eq!(read_back.pipeline[0].stage, "parse");
    assert_eq!(read_back.pipeline[1].status, StageStatus::Error);
    assert_eq!(read_back.pipeline[2].status, StageStatus::Skipped);

    let _ = std::fs::remove_dir_all(&tmp);
}

/// bd-5qnj Phase 1b: legacy (pre-Phase-1) `latest.json` files written by
/// older `quarto` versions must still be readable. Pretty or compact —
/// either is valid input.
#[test]
fn test_read_legacy_uncompressed_json() {
    let tmp = std::env::temp_dir().join(format!(
        "quarto-trace-legacy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("latest.json");

    // Hand-write a pretty-printed legacy trace as if produced by an older
    // `quarto` version (mirrors the pre-Phase-1 on-disk format).
    let pretty = serde_json::to_string_pretty(&sample_doc()).unwrap();
    std::fs::write(&path, &pretty).unwrap();

    let read_back = read_trace(&path).unwrap();
    assert_eq!(read_back.schema_version, SCHEMA_VERSION);
    assert_eq!(read_back.pipeline.len(), sample_doc().pipeline.len());

    let _ = std::fs::remove_dir_all(&tmp);
}

/// bd-5qnj Phase 1b: `list_traces` discovers both `latest.json` and
/// `latest.json.gz` artifacts. New traces are gzipped; old uncompressed
/// traces co-existing with new ones must still be listed.
#[test]
fn test_list_traces_finds_gzipped_and_uncompressed() {
    let tmp = std::env::temp_dir().join(format!(
        "quarto-trace-list-mix-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // doc-a: gzipped (the new default).
    let dir_a = tmp.join("a");
    std::fs::create_dir_all(&dir_a).unwrap();
    write_trace(&sample_doc(), &dir_a.join("latest.json.gz")).unwrap();

    // doc-b: legacy uncompressed (simulates an existing trace dir).
    let dir_b = tmp.join("b");
    std::fs::create_dir_all(&dir_b).unwrap();
    let pretty = serde_json::to_string_pretty(&sample_doc()).unwrap();
    std::fs::write(dir_b.join("latest.json"), &pretty).unwrap();

    let listings = list_traces(&tmp);
    let stems: std::collections::BTreeSet<_> =
        listings.iter().map(|l| l.doc_stem.clone()).collect();
    assert!(
        stems.contains("a"),
        "missing gzipped trace listing: {:?}",
        stems
    );
    assert!(
        stems.contains("b"),
        "missing uncompressed trace listing: {:?}",
        stems
    );

    // Each listing's path must round-trip through read_trace.
    for l in &listings {
        let _ = read_trace(&l.latest_path).expect("listed trace must be readable");
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

/// bd-5qnj Phase 1: when both `latest.json.gz` and a stale `latest.json`
/// exist in the same directory, `list_traces` must prefer the `.gz`
/// (newer) artifact. This guards against a future regression where a
/// pre-Phase-1 trace lingers next to a freshly-written gzipped one.
#[test]
fn test_list_traces_prefers_gz_when_both_present() {
    let tmp = std::env::temp_dir().join(format!(
        "quarto-trace-prefer-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let dir = tmp.join("doc");
    std::fs::create_dir_all(&dir).unwrap();

    // Stale uncompressed file with a marker we can recognize.
    let mut stale = sample_doc();
    stale.render.input_path = Some("STALE".into());
    let pretty = serde_json::to_string_pretty(&stale).unwrap();
    std::fs::write(dir.join("latest.json"), &pretty).unwrap();

    // Fresh gzipped file is what should be reported.
    let mut fresh = sample_doc();
    fresh.render.input_path = Some("FRESH".into());
    write_trace(&fresh, &dir.join("latest.json.gz")).unwrap();

    let listings = list_traces(&tmp);
    let entry = listings.iter().find(|l| l.doc_stem == "doc").unwrap();
    let read_back = read_trace(&entry.latest_path).unwrap();
    assert_eq!(
        read_back.render.input_path.as_deref(),
        Some("FRESH"),
        "expected list_traces to prefer the .gz file"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_engine_capture_roundtrip_through_disk() {
    // bd-45yw: traces double as replay fixtures. A TraceDocument's
    // engine_capture must round-trip through disk losslessly.
    let tmp = std::env::temp_dir().join("quarto-trace-engine-capture");
    let _ = std::fs::remove_dir_all(&tmp);
    let path = tmp.join("latest.json");

    let result_json = json!({
        "markdown": "# Hello\n\nWorld\n",
        "supporting_files": ["fig1.png", "data/table.csv"],
        "filters": ["quarto"],
        "includes": {
            "header_includes": ["<style>.x{}</style>"],
            "include_before": [],
            "include_after": ["<script>foo()</script>"],
        },
        "needs_postprocess": false,
    });

    let mut doc = TraceDocument::new(RenderInfo::default());
    doc.engine_captures = vec![EngineCapture {
        engine_name: "jupyter".into(),
        input_qmd: "---\nengine: jupyter\n---\n\n# Hello\n".into(),
        result: result_json.clone(),
        files: Vec::new(),
    }];

    write_trace(&doc, &path).unwrap();
    let read_back = read_trace(&path).unwrap();

    assert_eq!(read_back.engine_captures.len(), 1);
    let capture = &read_back.engine_captures[0];
    assert_eq!(capture.engine_name, "jupyter");
    assert_eq!(capture.input_qmd, "---\nengine: jupyter\n---\n\n# Hello\n");
    assert_eq!(capture.result, result_json);
}

#[test]
fn test_engine_capture_absent_by_default() {
    // Existing traces without engine captures should still deserialize
    // cleanly (empty vector).
    let json_text = r#"{
      "schema_version": 1,
      "render": {},
      "pipeline": []
    }"#;
    let doc: TraceDocument = serde_json::from_str(json_text).unwrap();
    assert!(doc.engine_captures.is_empty());
}

#[test]
fn test_legacy_single_engine_capture_folds_into_vec() {
    // A pre-bd-5yff4 trace with a single `engine_capture` object must
    // fold into the one-element `engine_captures` vector on read.
    let json_text = r#"{
      "schema_version": 2,
      "render": {},
      "pipeline": [],
      "engine_capture": {
        "engine_name": "knitr",
        "input_qmd": "---\nengine: knitr\n---\n",
        "result": {"markdown": "out\n", "supporting_files": [], "filters": [],
                   "includes": {"header_includes": [], "include_before": [], "include_after": []},
                   "needs_postprocess": false}
      }
    }"#;
    let doc: TraceDocument = serde_json::from_str(json_text).unwrap();
    assert_eq!(doc.engine_captures.len(), 1);
    assert_eq!(doc.engine_captures[0].engine_name, "knitr");
}

#[test]
fn test_build_git_hash_populated() {
    // The env! captured at build time should never be empty.
    assert!(!BUILD_GIT_HASH.is_empty());
    // In a normal dev/CI build the hash looks like 7 hex chars, optionally
    // with `-dirty`. In tarball builds it's `unknown`. All three are OK.
    let is_unknown = BUILD_GIT_HASH == "unknown";
    let core = BUILD_GIT_HASH.trim_end_matches("-dirty");
    let looks_like_hash = core.len() >= 7 && core.chars().all(|c| c.is_ascii_hexdigit());
    assert!(
        is_unknown || looks_like_hash,
        "BUILD_GIT_HASH = {:?}",
        BUILD_GIT_HASH
    );
}

// ─── Phase 2 (bd-5qnj): schema_version 2 dedup ───────────────────────────────

/// Build a fixture document where the same AST appears in many entries —
/// the situation that motivated dedup. Emulates the "many no-op
/// transforms" pattern: 36 of 42 DocumentAst entries on a real trace
/// were byte-identical to the previous one
/// (see `claude-notes/plans/5qnj-trace-size-investigation/measurements.md`).
fn doc_with_repeated_asts() -> TraceDocument {
    let render = RenderInfo {
        input_path: Some("doc.qmd".into()),
        format_target: Some("html".into()),
        ..Default::default()
    };
    let mut doc = TraceDocument::new(render);
    // A "fat" AST sub-value used many times.
    let fat_ast = json!({
        "pandoc-api-version": [1, 23, 0],
        "meta": {},
        "blocks": (0..50).map(|i| json!({
            "t": "Para",
            "c": [{"t": "Str", "c": format!("block-{}", i)}],
        })).collect::<Vec<_>>(),
    });
    // Three entries share the same AST nested under data.ast (wrapped shape).
    for stage in ["metadata-merge", "include-expansion", "unwrap-profile"] {
        doc.pipeline.push(TraceEntry {
            stage: stage.into(),
            index: 0,
            data_kind: Some("DocumentAst".into()),
            data: Some(json!({
                "path": "doc.qmd",
                "ast": fat_ast.clone(),
                "warnings_count": 0,
            })),
            duration_ms: Some(1.0),
            status: StageStatus::Ok,
            error: None,
        });
    }
    // Two transform entries where data IS the AST directly (no wrapper)
    // — that's how `on_transform_data` writes it today.
    for stage in ["transform:callout", "transform:sectionize"] {
        doc.pipeline.push(TraceEntry {
            stage: stage.into(),
            index: 0,
            data_kind: Some("DocumentAst".into()),
            data: Some(fat_ast.clone()),
            duration_ms: None,
            status: StageStatus::Ok,
            error: None,
        });
    }
    // One AtProfile entry that wraps the same AST plus a profile field.
    doc.pipeline.push(TraceEntry {
        stage: "document-profile".into(),
        index: 0,
        data_kind: Some("AtProfile".into()),
        data: Some(json!({
            "path": "doc.qmd",
            "ast": fat_ast.clone(),
            "warnings_count": 0,
            "profile": {"profile_version": 3, "title": "x"},
        })),
        duration_ms: None,
        status: StageStatus::Ok,
        error: None,
    });
    doc
}

/// bd-5qnj Phase 2: when the writer encounters repeated ASTs across
/// entries, the on-disk JSON must factor them out into a top-level
/// `asts` map and reference them via `$ref` sentinels.
///
/// This test inspects the *raw on-disk JSON* (not the rehydrated
/// `TraceDocument`) so we can assert the wire format directly.
#[test]
fn test_v2_writer_dedups_repeated_asts() {
    let tmp = std::env::temp_dir().join(format!(
        "quarto-trace-v2-dedup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("latest.json");

    write_trace(&doc_with_repeated_asts(), &path).unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

    // schema bumped to 2.
    assert_eq!(raw["schema_version"], 2);

    // top-level asts map exists and contains exactly one entry — the
    // shared AST appears in 6 entries but is stored once.
    let asts = raw["asts"]
        .as_object()
        .expect("v2 wire format must include a top-level `asts` map");
    assert_eq!(
        asts.len(),
        1,
        "expected 1 unique AST (all entries share); got {}: {:?}",
        asts.len(),
        asts.keys().collect::<Vec<_>>()
    );
    let (hash, _ast_value) = asts.iter().next().unwrap();
    assert!(!hash.is_empty(), "hash key must not be empty");

    // Every entry's data references the AST via `$ref` instead of inlining it.
    let pipeline = raw["pipeline"].as_array().unwrap();
    assert_eq!(pipeline.len(), 6);

    // Wrapped DocumentAst entries: data.ast is a $ref, not an inline AST.
    for stage in ["metadata-merge", "include-expansion", "unwrap-profile"] {
        let entry = pipeline.iter().find(|e| e["stage"] == stage).unwrap();
        let data = &entry["data"];
        let ast_field = &data["ast"];
        assert_eq!(
            ast_field["$ref"],
            serde_json::Value::String(hash.clone()),
            "stage {}: expected data.ast to be {{$ref: <hash>}}, got {:?}",
            stage,
            ast_field
        );
        // The wrapper's other fields are preserved inline.
        assert_eq!(data["path"], "doc.qmd");
        assert_eq!(data["warnings_count"], 0);
    }

    // Transform entries: data IS the $ref (no wrapper).
    for stage in ["transform:callout", "transform:sectionize"] {
        let entry = pipeline.iter().find(|e| e["stage"] == stage).unwrap();
        assert_eq!(
            entry["data"]["$ref"],
            serde_json::Value::String(hash.clone()),
            "stage {}: expected data to be {{$ref: <hash>}}, got {:?}",
            stage,
            entry["data"]
        );
    }

    // AtProfile entry: data.ast is $ref; profile is preserved.
    let prof = pipeline
        .iter()
        .find(|e| e["stage"] == "document-profile")
        .unwrap();
    assert_eq!(
        prof["data"]["ast"]["$ref"],
        serde_json::Value::String(hash.clone())
    );
    assert_eq!(prof["data"]["profile"]["profile_version"], 3);

    let _ = std::fs::remove_dir_all(&tmp);
}

/// bd-5qnj Phase 2: round-tripping a v2-written trace through `read_trace`
/// must yield a `TraceDocument` byte-equivalent to the original — the
/// dedup is a wire-format detail, invisible to consumers.
#[test]
fn test_v2_roundtrip_yields_rehydrated_doc() {
    let tmp = std::env::temp_dir().join(format!(
        "quarto-trace-v2-rt-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("latest.json.gz");

    let doc = doc_with_repeated_asts();
    write_trace(&doc, &path).unwrap();
    let read_back = read_trace(&path).unwrap();

    // pipeline length and entry shapes preserved
    assert_eq!(read_back.pipeline.len(), doc.pipeline.len());
    for (i, (got, want)) in read_back
        .pipeline
        .iter()
        .zip(doc.pipeline.iter())
        .enumerate()
    {
        assert_eq!(got.stage, want.stage, "entry {} stage", i);
        assert_eq!(got.data_kind, want.data_kind, "entry {} data_kind", i);
        assert_eq!(
            got.data, want.data,
            "entry {} ({}): rehydrated data must equal original",
            i, got.stage
        );
    }
    // The reader must NOT leak the $ref machinery to consumers — the
    // top-level asts map should be folded back into the entries (or
    // exposed as an empty map at minimum, never with $ref-laden data
    // inside the pipeline).
    for (i, entry) in read_back.pipeline.iter().enumerate() {
        if let Some(data) = &entry.data {
            assert!(
                !contains_dollar_ref(data),
                "entry {} ({}) still contains a $ref after read: {:?}",
                i,
                entry.stage,
                data
            );
        }
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

fn contains_dollar_ref(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(map) => {
            if map.contains_key("$ref") {
                return true;
            }
            map.values().any(contains_dollar_ref)
        }
        serde_json::Value::Array(arr) => arr.iter().any(contains_dollar_ref),
        _ => false,
    }
}

/// bd-5qnj Phase 2: a v1 trace (no `asts` map, no `$ref` sentinels)
/// must continue to read correctly. v1 readers and v2 readers must
/// both interpret existing v1 files; only the writer side bumps.
#[test]
fn test_v2_reader_handles_v1_input() {
    let tmp = std::env::temp_dir().join(format!(
        "quarto-trace-v2-v1-input-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("latest.json");

    // Hand-write a v1-shaped JSON file: schema_version 1, no asts, no $ref.
    let inline_ast = json!({"blocks": [{"t": "Para", "c": [{"t": "Str", "c": "hello"}]}]});
    let v1_text = serde_json::json!({
        "schema_version": 1,
        "render": {},
        "pipeline": [
            {
                "stage": "metadata-merge",
                "index": 0,
                "data_kind": "DocumentAst",
                "data": {"path": "doc.qmd", "ast": inline_ast, "warnings_count": 0},
                "status": "ok"
            },
            {
                "stage": "transform:callout",
                "index": 0,
                "data_kind": "DocumentAst",
                "data": inline_ast,
                "status": "ok"
            }
        ]
    })
    .to_string();
    std::fs::write(&path, &v1_text).unwrap();

    let doc = read_trace(&path).unwrap();
    assert_eq!(doc.schema_version, 1);
    assert_eq!(doc.pipeline.len(), 2);
    // Inline AST values come through unchanged.
    assert_eq!(
        doc.pipeline[0].data.as_ref().unwrap()["ast"]["blocks"][0]["c"][0]["c"],
        "hello"
    );
    assert_eq!(
        doc.pipeline[1].data.as_ref().unwrap()["blocks"][0]["c"][0]["c"],
        "hello"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

/// bd-5qnj Phase 2: dedup actually shrinks the file when ASTs repeat —
/// a regression gate against accidentally bypassing the dedup path.
///
/// We compare *uncompressed* sizes (writing to a `.json` path) so the
/// signal isn't blunted by gzip's own ability to collapse repeated
/// content. End-to-end gzipped numbers from real fixtures are tracked
/// in `claude-notes/plans/5qnj-trace-size-investigation/measurements.md`
/// and don't belong in a unit test.
#[test]
fn test_v2_dedup_actually_shrinks_repeated_asts() {
    let tmp = std::env::temp_dir().join(format!(
        "quarto-trace-v2-shrink-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("latest.json");

    // 20 entries sharing the same fat AST — a stress version of the
    // repeated-no-op-transforms pattern. Uncompressed sizes here:
    // each entry inlines the AST in v1 form (~baseline_per_entry KB);
    // v2 emits one stored copy plus 20 cheap $ref wrappers.
    let render = RenderInfo {
        input_path: Some("doc.qmd".into()),
        ..Default::default()
    };
    let mut doc = TraceDocument::new(render);
    let fat_ast = json!({
        "pandoc-api-version": [1, 23, 0],
        "meta": {},
        "blocks": (0..200).map(|i| json!({
            "t": "Para",
            "c": [{"t": "Str", "c": format!("block-{}-pad-pad-pad-pad-pad", i)}],
        })).collect::<Vec<_>>(),
    });
    for i in 0..20 {
        doc.pipeline.push(TraceEntry {
            stage: format!("transform:no-op-{}", i),
            index: i,
            data_kind: Some("DocumentAst".into()),
            data: Some(fat_ast.clone()),
            duration_ms: Some(0.1),
            status: StageStatus::Ok,
            error: None,
        });
    }

    write_trace(&doc, &path).unwrap();
    let dedup_size = std::fs::metadata(&path).unwrap().len();

    // Non-deduped baseline: serialize the in-memory doc directly (no
    // dedup pass). v1-shaped pipeline with inline ASTs.
    let baseline_path = tmp.join("baseline.json");
    {
        let baseline = serde_json::json!({
            "schema_version": 1,
            "render": doc.render,
            "pipeline": doc.pipeline.iter().map(|e| serde_json::json!({
                "stage": e.stage, "index": e.index,
                "data_kind": e.data_kind, "data": e.data,
                "duration_ms": e.duration_ms,
                "status": match e.status {
                    StageStatus::Ok => "ok",
                    StageStatus::Error => "error",
                    StageStatus::Skipped => "skipped",
                    StageStatus::Unknown => "unknown",
                },
            })).collect::<Vec<_>>()
        });
        std::fs::write(&baseline_path, serde_json::to_vec(&baseline).unwrap()).unwrap();
    }
    let baseline_size = std::fs::metadata(&baseline_path).unwrap().len();

    // Expect at least 10x reduction: the AST is ~big, stored once
    // instead of 20 times.
    assert!(
        dedup_size * 10 < baseline_size,
        "expected dedup'd size to be at least 10x smaller than non-dedup baseline; \
         dedup: {} bytes, baseline: {} bytes",
        dedup_size,
        baseline_size
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// ── bd-qbhp2cvv: EngineCapture.files wire-format compatibility ─────────

/// A capture JSON that predates the `files` field must deserialize
/// with an empty `files` vec (serde default).
#[test]
fn test_engine_capture_without_files_field_deserializes() {
    let old_wire = serde_json::json!({
        "engine_name": "knitr",
        "input_qmd": "```{r}\n1\n```\n",
        "result": {"markdown": "output", "supporting_files": []},
    });
    let capture: EngineCapture = serde_json::from_value(old_wire).unwrap();
    assert!(capture.files.is_empty());
}

/// A capture with no files must serialize WITHOUT the `files` key —
/// byte-identical wire shape to the pre-field format, so existing
/// snapshots and hash-keyed caches are unaffected.
#[test]
fn test_engine_capture_empty_files_serializes_without_key() {
    let capture = EngineCapture {
        engine_name: "knitr".into(),
        input_qmd: "```{r}\n1\n```\n".into(),
        result: serde_json::json!({"markdown": "output"}),
        files: Vec::new(),
    };
    let value = serde_json::to_value(&capture).unwrap();
    assert!(
        value.get("files").is_none(),
        "empty files must not appear on the wire; got: {value}"
    );
}

/// Files round-trip: path + base64 contents survive serialize →
/// deserialize unchanged.
#[test]
fn test_engine_capture_files_roundtrip() {
    use quarto_trace::CaptureFile;
    let capture = EngineCapture {
        engine_name: "knitr".into(),
        input_qmd: "```{r}\nplot(1)\n```\n".into(),
        result: serde_json::json!({"markdown": "![](doc_files/figure-html/f.png)"}),
        files: vec![CaptureFile {
            path: "doc_files/figure-html/f.png".into(),
            contents_base64: "iVBORw0KGgo=".into(),
        }],
    };
    let json = serde_json::to_vec(&capture).unwrap();
    let back: EngineCapture = serde_json::from_slice(&json).unwrap();
    assert_eq!(back.files.len(), 1);
    assert_eq!(back.files[0].path, "doc_files/figure-html/f.png");
    assert_eq!(back.files[0].contents_base64, "iVBORw0KGgo=");
}
