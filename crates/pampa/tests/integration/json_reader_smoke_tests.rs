use pampa::pandoc::{Block, Inline, Pandoc, Plain, Str};
use pampa::readers::json;
use pampa::writers::json as json_writer;
use quarto_source_map::{Anchor, AnchorRole, By, FileId, SourceInfo};
use smallvec::SmallVec;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn test_read_all_json_files_in_tests_readers() {
    let test_dir = PathBuf::from("tests/readers");

    if !test_dir.exists() {
        eprintln!("Warning: tests/readers directory does not exist, skipping test");
        return;
    }

    let mut json_files = Vec::new();
    collect_json_files(&test_dir, &mut json_files);

    if json_files.is_empty() {
        eprintln!("Warning: No JSON files found in tests/readers directory");
        return;
    }

    for json_file in json_files {
        println!("Testing JSON reader with: {}", json_file.display());

        let mut file = fs::File::open(&json_file)
            .unwrap_or_else(|_| panic!("Failed to open file: {}", json_file.display()));

        // Pandoc-format fixtures under tests/readers/json/ predate q2's
        // `s:` extension. Route through the completing reader with
        // `By::unknown()` (plan 7f Phase 4).
        match json::read_completing_source_info(&mut file, By::unknown()) {
            Ok((pandoc, _context)) => {
                println!("  ✓ Successfully read {}", json_file.display());
                // Basic validation - ensure we got some content
                assert!(
                    !pandoc.blocks.is_empty() || !pandoc.meta.is_empty(),
                    "File {} produced empty document",
                    json_file.display()
                );
            }
            Err(e) => {
                panic!("Failed to read JSON file {}: {}", json_file.display(), e);
            }
        }
    }
}

fn collect_json_files(dir: &PathBuf, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_json_files(&path, files);
            } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
                files.push(path);
            }
        }
    }
}

#[test]
fn test_manybullets_json_specifically() {
    let json_file = PathBuf::from("tests/readers/json/manybullets.json");

    if !json_file.exists() {
        eprintln!("Warning: manybullets.json not found, skipping test");
        return;
    }

    let mut file = fs::File::open(&json_file).expect("Failed to open manybullets.json");

    let (pandoc, _context) = json::read_completing_source_info(&mut file, By::unknown())
        .expect("Failed to read manybullets.json");

    // Verify the content matches what we expect
    assert_eq!(pandoc.blocks.len(), 1, "Should have exactly one block");

    match &pandoc.blocks[0] {
        pampa::pandoc::Block::OrderedList(list) => {
            assert_eq!(list.content.len(), 12, "Should have 12 list items");
            assert_eq!(list.attr.0, 1, "List should start at 1");
        }
        _ => panic!("Expected OrderedList block"),
    }
}

// ----------------------------------------------------------------
// Plan 5 — End-to-end round-trip through the streaming writer
//          and the public reader API.
//
// These tests exercise the *production* JSON path:
//   `pampa::writers::json::write` → bytes → `pampa::readers::json::read`.
// The writer's streaming arm (`stream_write_source_info_pool`) is what
// the orchestrator uses, so a regression here is exactly what bd-3odjm
// surfaced. The hand-constructed reader/writer unit tests live next to
// their respective modules; these tests guard the wire.
// ----------------------------------------------------------------

/// Round-trip a single `Pandoc` through the streaming writer and the
/// reader. Returns the recovered `source_info` of the inner `Str`.
fn roundtrip_str_source_info(str_source_info: SourceInfo) -> SourceInfo {
    let mut pandoc = Pandoc::default();
    let inner = Inline::Str(Str {
        text: "hi".to_string(),
        source_info: str_source_info,
    });
    let plain = Plain {
        content: vec![inner],
        source_info: SourceInfo::for_test(),
    };
    pandoc.blocks.push(Block::Plain(plain));

    let context = pampa::pandoc::ASTContext::anonymous();
    let mut buf = Vec::new();
    json_writer::write(&pandoc, &context, &mut buf).expect("write_pandoc");

    let mut cursor = Cursor::new(&buf);
    let (round, _ctx) = json::read(&mut cursor).expect("read_pandoc");

    let Block::Plain(plain) = &round.blocks[0] else {
        panic!("Expected Plain block")
    };
    let Inline::Str(str_node) = &plain.content[0] else {
        panic!("Expected Str inline")
    };
    str_node.source_info.clone()
}

#[test]
fn roundtrip_generated_no_anchors_via_public_api() {
    let original = SourceInfo::generated(By::sectionize());
    let recovered = roundtrip_str_source_info(original.clone());
    assert_eq!(original, recovered);
}

#[test]
fn roundtrip_generated_filter_with_data_via_public_api() {
    let original = SourceInfo::generated(By::filter("/x.lua", 42));
    let recovered = roundtrip_str_source_info(original.clone());
    assert_eq!(original, recovered);
}

#[test]
fn roundtrip_generated_with_invocation_anchor_via_public_api() {
    let target = Arc::new(SourceInfo::Original {
        file_id: FileId(0),
        start_offset: 5,
        end_offset: 12,
    });
    let mut from = SmallVec::<[Anchor; 2]>::new();
    from.push(Anchor::invocation(Arc::clone(&target)));
    let original = SourceInfo::Generated {
        by: By::shortcode("meta"),
        from,
    };
    let recovered = roundtrip_str_source_info(original.clone());
    assert_eq!(original, recovered);
}

#[test]
fn roundtrip_generated_with_all_anchor_roles_via_public_api() {
    let mk_target = |start: usize, end: usize| {
        Arc::new(SourceInfo::Original {
            file_id: FileId(0),
            start_offset: start,
            end_offset: end,
        })
    };
    let mut from = SmallVec::<[Anchor; 2]>::new();
    from.push(Anchor::invocation(mk_target(0, 5)));
    from.push(Anchor::value_source(mk_target(10, 20)));
    from.push(Anchor {
        role: AnchorRole::Other("ext/foo/bar".to_string()),
        source_info: mk_target(30, 35),
    });
    let original = SourceInfo::Generated {
        by: By::shortcode("meta"),
        from,
    };
    let recovered = roundtrip_str_source_info(original.clone());
    assert_eq!(original, recovered);
}

#[test]
fn roundtrip_concat_of_generated_via_public_api() {
    let g1 = SourceInfo::generated(By::filter("/a.lua", 1));
    let g2 = SourceInfo::generated(By::filter("/b.lua", 2));
    let original = SourceInfo::concat(vec![(g1, 5), (g2, 7)]);
    let recovered = roundtrip_str_source_info(original.clone());
    assert_eq!(original, recovered);
}

#[test]
fn roundtrip_substring_of_generated_via_public_api() {
    let parent = Arc::new(SourceInfo::generated(By::filter("/x.lua", 1)));
    let original = SourceInfo::Substring {
        parent: Arc::clone(&parent),
        start_offset: 0,
        end_offset: 4,
    };
    let recovered = roundtrip_str_source_info(original.clone());
    assert_eq!(original, recovered);
}

#[test]
fn roundtrip_original_via_public_api() {
    let original = SourceInfo::Original {
        file_id: FileId(0),
        start_offset: 7,
        end_offset: 12,
    };
    let recovered = roundtrip_str_source_info(original.clone());
    assert_eq!(original, recovered);
}

#[test]
fn roundtrip_substring_via_public_api() {
    let parent = Arc::new(SourceInfo::Original {
        file_id: FileId(0),
        start_offset: 0,
        end_offset: 100,
    });
    let original = SourceInfo::Substring {
        parent: Arc::clone(&parent),
        start_offset: 10,
        end_offset: 20,
    };
    let recovered = roundtrip_str_source_info(original.clone());
    assert_eq!(original, recovered);
}

/// Streaming-writer parity: the streaming writer emits a code-4 entry
/// whose payload reads back as the same `Generated` value the writer
/// was given. Specifically guards `stream_write_source_info_pool`'s
/// match arms, which are independent from `to_json`'s.
#[test]
fn streaming_writer_generated_round_trip_preserves_by_data() {
    let target = Arc::new(SourceInfo::Original {
        file_id: FileId(0),
        start_offset: 0,
        end_offset: 5,
    });
    let mut from = SmallVec::<[Anchor; 2]>::new();
    from.push(Anchor::invocation(Arc::clone(&target)));
    let original = SourceInfo::Generated {
        by: By::raw(
            "ext/example/foo",
            serde_json::json!({
                "nested": {
                    "n": 7,
                    "flag": true,
                    "items": [1, 2, "three"],
                    "empty": null
                }
            }),
        ),
        from,
    };
    let recovered = roundtrip_str_source_info(original.clone());
    assert_eq!(original, recovered);
}
