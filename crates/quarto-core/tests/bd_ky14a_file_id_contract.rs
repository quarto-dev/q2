//! Contract tests for bd-ky14a — pampa's hash-based FileId migration.
//!
//! These tests pin down the new invariant: pampa's `ASTContext` and
//! `quarto_yaml` must use the same `FileId` for a given filename, so
//! `SourceInfo`s produced by either parser can be cross-referenced
//! and rendered against any `SourceContext` populated via
//! `quarto_yaml::file_id_for_filename`.
//!
//! All four start RED on `main` (pampa uses `FileId(0)` everywhere)
//! and turn GREEN after the migration. The asymmetry between the
//! two schemes is exactly what the bd-ky14a refactor eliminates.
//!
//! See `claude-notes/plans/2026-05-22-pampa-hash-fileids.md`.

use std::io::sink;
use std::path::PathBuf;

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_source_map::{SourceContext, SourceInfo};

/// Contract #2 — cross-parser agreement.
///
/// Parsing the same filename via `quarto_yaml::file_id_for_filename`
/// and via `pampa::readers::qmd::read` (the real CLI entry point)
/// must produce the same `FileId`. This is what makes the bridge
/// layer (sass_error_to_parse_error and friends) work without
/// out-of-band binding.
#[test]
fn bd_ky14a_pampa_qmd_read_uses_quarto_yaml_file_id() {
    let filename = "doc.qmd";
    let yaml_fid = quarto_yaml::file_id_for_filename(filename);

    let (_pandoc, ast_context, _warnings) = pampa::readers::qmd::read(
        b"# Title\n",
        false,
        filename,
        &mut sink(),
        true, // prune_errors
        None, // parent_source_info
    )
    .expect("parse should succeed");

    assert_eq!(
        ast_context.current_file_id(),
        yaml_fid,
        "pampa::readers::qmd::read must use quarto_yaml::file_id_for_filename for the parsed file's FileId",
    );
}

/// Contract #3 — include-expansion sub-document.
///
/// When a document includes a sub-document, the sub-document's
/// content gets registered in the parent's `SourceContext`. After
/// the migration, that registration must use the *hash-based*
/// FileId for the sub-document's path, not a sequentially-assigned
/// one. This is what lets the include_expansion code drop its
/// `remap_file_ids(FileId(0) → new_sequential_id)` workaround.
///
/// We exercise this at the lowest layer that demonstrates the
/// contract: parsing a sub-document via pampa and asserting its
/// ASTContext's FileId matches what
/// `quarto_yaml::file_id_for_filename` would produce. The
/// include_expansion stage subsequently lifts this FileId into the
/// parent's SourceContext; the migration eliminates the remap.
#[test]
fn bd_ky14a_sub_document_file_id_is_hash_based() {
    let sub_filename = "sub.qmd";
    let expected_fid = quarto_yaml::file_id_for_filename(sub_filename);

    let (_pandoc, ast_context, _warnings) = pampa::readers::qmd::read(
        b"sub-document content\n",
        false,
        sub_filename,
        &mut sink(),
        true,
        None,
    )
    .expect("parse should succeed");

    assert_eq!(
        ast_context.current_file_id(),
        expected_fid,
        "an included sub-document must land at FileId(hash(sub_path)) so include_expansion can merge SourceContexts without a FileId remap",
    );
    assert!(
        ast_context.source_context.get_file(expected_fid).is_some(),
        "the sub-doc's content must be reachable in its SourceContext via the hash FileId",
    );
}

/// Contract #4 — fresh-SourceContext rendering.
///
/// Take a pampa-produced `SourceInfo`, populate a *fresh*
/// `SourceContext` using only `add_file_with_id(file_id_for_filename(p), p, content)`,
/// and render an ariadne diagnostic against it. The renderer must
/// resolve the SourceInfo's FileId to the right file. This is the
/// no-out-of-band-binding property that downstream bridge layers
/// (theme_diagnostic, future hub-client diagnostics, q2-preview
/// endpoint) need.
#[test]
fn bd_ky14a_fresh_source_context_renders_pampa_source_info() {
    let filename = "doc.qmd";
    let content = "# Heading\n\nBody text.\n";

    // Parse via pampa so the SourceInfo we capture is real and
    // came from the parser.
    let (pandoc, _ast_context, _warnings) =
        pampa::readers::qmd::read(content.as_bytes(), false, filename, &mut sink(), true, None)
            .expect("parse should succeed");

    // Find the first block's SourceInfo. We don't care what kind
    // of block it is — only that it carries an Original SourceInfo
    // with the filename's FileId.
    let first_block_si =
        first_block_source_info(&pandoc).expect("first block must carry a source info");
    let first_block_fid = first_block_si
        .resolve_byte_range()
        .expect("first block must resolve to an Original")
        .0;

    // Build a SourceContext from scratch using the canonical
    // hash-based FileId. After the migration, this should match
    // the FileId pampa put on the SourceInfo.
    let expected_fid = quarto_yaml::file_id_for_filename(filename);
    assert_eq!(
        first_block_fid, expected_fid.0,
        "pampa's first-block FileId must equal quarto_yaml::file_id_for_filename — this is the property the bridge layer relies on",
    );

    let mut fresh_ctx = SourceContext::new();
    fresh_ctx.add_file_with_id(
        expected_fid,
        filename.to_string(),
        Some(content.to_string()),
    );

    let diag = DiagnosticMessageBuilder::error("test diagnostic")
        .with_code("Q-14-1") // any existing code; we're not testing the catalog here
        .problem("synthetic")
        .with_location(first_block_si.clone())
        .build();

    let opts = quarto_error_reporting::TextRenderOptions {
        enable_hyperlinks: false,
    };
    let rendered = diag.to_text_with_options(Some(&fresh_ctx), &opts);

    // The ariadne render should include the filename and at least
    // one of the line markers — i.e. it found the file in the
    // SourceContext via the FileId on the SourceInfo.
    assert!(
        rendered.contains("doc.qmd"),
        "expected fresh-SourceContext render to include the filename:\n{}",
        strip_ansi(&rendered),
    );
}

/// Return the first block's `SourceInfo`, if any. Used by contract
/// #4 to capture a real pampa-produced SourceInfo without depending
/// on a specific AST shape.
fn first_block_source_info(pandoc: &quarto_pandoc_types::pandoc::Pandoc) -> Option<SourceInfo> {
    pandoc.blocks.first().map(|b| b.source_info().clone())
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for nc in chars.by_ref() {
                    if nc.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
            if chars.peek() == Some(&']') {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc == '\x07' {
                        break;
                    }
                    if nc == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

// Suppress the unused-import warning on PathBuf if the test grows
// to need filesystem paths and we add them back. Until then keep
// the import declaration list minimal.
#[allow(dead_code)]
fn _path_buf_keepalive(_: PathBuf) {}
