//! D2 (YAML content-provenance epic, obligation 8): guard the byte offsets
//! `qmd-syntax-helper` splices at against the `AttrSourceInfo` meaning change.
//!
//! Task D1 changed `AttrSourceInfo.attributes[i].1` (and
//! `TargetSourceInfo.title`) from a raw file span to **content provenance**.
//! For a value containing a collapsed escape that provenance is a
//! `SourceInfo::Concat`, and `Concat` answers the two accessors this crate
//! uses to compute splice positions in *content* space, not file space:
//!
//! - `start_offset()` → `0`
//! - `end_offset()`   → `length()` (the decoded length, not a file offset)
//!
//! Those are the correct answers in content space and stay unhardened
//! upstream on purpose. So the defect, if it existed, would live here: a
//! conversion that read such a span as a file offset would splice at byte
//! `0` of the user's file — silently corrupting it, because this crate
//! writes to the files it is pointed at.
//!
//! The reachability argument for why that cannot happen is that
//! `pampa::readers::qmd::read` returns its parse-error diagnostics from the
//! tree-sitter log observer *before* `treesitter_to_pandoc` runs, so no
//! `AttrSourceInfo` exists yet. These tests do not restate that argument —
//! they pin it:
//!
//! 1. `diagnostic_locations_are_always_raw_file_spans` asserts the shape
//!    directly, on both arms of `read`'s `Result` and on detail locations
//!    (`q_2_7.rs` reads `details[0].location`), for a fixture built to
//!    maximise the chance of a content-provenance span leaking in.
//! 2. The round-trip tests splice for real and assert the rewritten bytes.
//!    A span silently collapsed to `0` corrupts the output immediately and
//!    visibly, which is a stronger check than asserting a symptom's absence.

use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use quarto_source_map::SourceInfo;
use std::fs;

/// Every attribute-carrying construct whose value slot D1 retyped, in one
/// document: a div `key="value"` with two collapsed `` \` `` escapes, a
/// bracketed span with a collapsed `\"`, a link title and an image title
/// (the `TargetSourceInfo.title` leg), and a heading attribute.
const ESCAPED_ATTRS: &str = concat!(
    "---\n",
    "title: D2 attribute provenance\n",
    "---\n",
    "\n",
    "::: {.callout-note title=\"Use \\`renv\\` today\"}\n",
    "Body text.\n",
    ":::\n",
    "\n",
    "A [span]{.cls note=\"Say \\\"hi\\\" now\"} and a\n",
    "[link](https://example.com \"Say \\\"hi\\\" first\") and an\n",
    "![img](https://example.com/a.png \"Cite \\`x\\` here\").\n",
    "\n",
    "# Heading {#h1 .cls key=\"a \\* b\"}\n",
);

fn write(rm: &ResourceManager, name: &str, content: &str) -> std::path::PathBuf {
    let path = rm.temp_dir().join(name);
    fs::write(&path, content).unwrap();
    path
}

/// Assert a diagnostic location is a raw file span — the only shape a byte
/// offset may legitimately be read out of. `Concat` is the shape D1 can
/// produce for a decoded attribute value, and it is precisely the shape that
/// would answer `start_offset()` with `0`.
fn assert_raw_file_span(loc: &SourceInfo, what: &str) {
    match loc {
        SourceInfo::Original { .. } => {}
        SourceInfo::Concat { .. } => panic!(
            "{what} carries a Concat — content provenance in the diagnostic \
             channel. start_offset() on it returns 0, so any conversion \
             splicing at it would write at byte 0 of the user's file. \
             Got: {loc:?}"
        ),
        SourceInfo::Substring { .. } | SourceInfo::Generated { .. } => panic!(
            "{what} is neither a raw file span nor a recognised safe shape; \
             this crate reads byte offsets straight out of it. Got: {loc:?}"
        ),
    }
}

/// The mechanism guard: the diagnostics this crate reads offsets out of are
/// always raw file spans, never the content provenance D1 introduced.
///
/// Covers both arms of `read`'s `Result`, because they are not the same
/// channel: the `Err` arm is the tree-sitter parse-error path (which returns
/// before the AST — and therefore before any `AttrSourceInfo` — is built),
/// while the `Ok` arm's warnings come from `treesitter_to_pandoc`'s collector,
/// which runs *after* attribute decoding. `q_2_28.rs:60` is the one
/// conversion that reads the `Ok` arm, and it reads `end_offset()`.
#[test]
fn diagnostic_locations_are_always_raw_file_spans() {
    // (a) The clean fixture: parses, so this exercises the `Ok` arm's
    //     warnings — the channel that is downstream of attribute decoding.
    let mut sink = std::io::sink();
    let clean = pampa::readers::qmd::read(
        ESCAPED_ATTRS.as_bytes(),
        false,
        "escaped_attrs.qmd",
        &mut sink,
        false, // don't prune — see every diagnostic, as q_2_28 does
        None,
    );

    let ok_warnings = match clean {
        Ok((_doc, _ctx, warnings)) => warnings,
        Err(diags) => panic!(
            "the escaped-attribute fixture was expected to parse cleanly, so \
             that the Ok-arm warning channel is what gets exercised; it \
             failed with {} diagnostic(s): {:?}",
            diags.len(),
            diags.iter().map(|d| &d.title).collect::<Vec<_>>()
        ),
    };

    for diag in &ok_warnings {
        let tag = format!("Ok-arm warning {:?} location", diag.code);
        if let Some(loc) = diag.location.as_ref() {
            assert_raw_file_span(loc, &tag);
        }
        for (i, detail) in diag.details.iter().enumerate() {
            if let Some(loc) = detail.location.as_ref() {
                assert_raw_file_span(loc, &format!("{tag} details[{i}]"));
            }
        }
    }

    // (b) The same attributes plus a parse error, so the `Err` arm — the
    //     channel every other conversion reads — is exercised too.
    let mut broken = ESCAPED_ATTRS.to_string();
    broken.push_str("\nAn unclosed emphasis: *never closed\n");

    let mut sink = std::io::sink();
    let err_diags = match pampa::readers::qmd::read(
        broken.as_bytes(),
        false,
        "escaped_attrs_broken.qmd",
        &mut sink,
        true,
        None,
    ) {
        Ok(_) => panic!("the broken fixture was expected to fail to parse"),
        Err(diags) => diags,
    };

    assert!(
        !err_diags.is_empty(),
        "expected at least one parse diagnostic to inspect"
    );

    for diag in &err_diags {
        let tag = format!("Err-arm diagnostic {:?} location", diag.code);
        if let Some(loc) = diag.location.as_ref() {
            assert_raw_file_span(loc, &tag);
        }
        // `q_2_7.rs:81` splices at `details[0].location.start_offset()`, so
        // detail locations are a live byte-offset source too.
        for (i, detail) in diag.details.iter().enumerate() {
            if let Some(loc) = detail.location.as_ref() {
                assert_raw_file_span(loc, &format!("{tag} details[{i}]"));
            }
        }
    }
}

/// The last two byte-offset sources in the crate are not diagnostic locations
/// at all: `q_2_30.rs:92` and `:115` read `Paragraph.source_info` off the
/// parsed AST, on the `Ok` arm — downstream of attribute decoding, and so in
/// principle exposed to the same meaning change.
///
/// They are a different shape from the diagnostic sites and need their own
/// check: a block's `source_info` is the paragraph node's own range, not a
/// decoded attribute value's provenance. This asserts that directly over the
/// escaped-attribute fixture, so the claim rests on a measurement rather than
/// on where the field happens to be built today.
#[test]
fn paragraph_source_infos_are_always_raw_file_spans() {
    use pampa::pandoc::Block;

    let mut sink = std::io::sink();
    let (doc, _ctx, _warnings) = pampa::readers::qmd::read(
        ESCAPED_ATTRS.as_bytes(),
        false,
        "escaped_attrs.qmd",
        &mut sink,
        false,
        None,
    )
    .expect("the escaped-attribute fixture must parse");

    // `q_2_30` walks top-level blocks looking for Paragraph after
    // NoteDefinitionPara, so top-level blocks are the reachable set.
    let mut seen = 0usize;
    for block in &doc.blocks {
        if let Block::Paragraph(para) = block {
            assert_raw_file_span(&para.source_info, "Paragraph.source_info");
            // `para_starts_with_indent` slices `content[..para_start]`, so the
            // offset must also be in range for the file it came from.
            assert!(
                para.source_info.start_offset() <= ESCAPED_ATTRS.len(),
                "Paragraph.source_info start offset {} is past the end of the \
                 {}-byte source it indexes",
                para.source_info.start_offset(),
                ESCAPED_ATTRS.len()
            );
            seen += 1;
        }
    }
    assert!(
        seen > 0,
        "expected at least one top-level Paragraph to inspect"
    );
}

/// Round trip: a real splice, over a document whose attributes contain
/// collapsed escapes, with the rewritten bytes asserted exactly.
///
/// `q-2-12` appends the missing `*` at the end of the unclosed emphasis. The
/// assertion is byte-for-byte on the whole file, so a span that had silently
/// become `0` would put the `*` in front of the front-matter fence and fail
/// here rather than in production.
#[test]
fn q_2_12_splice_is_byte_correct_with_escaped_attributes() {
    let rm = ResourceManager::new().unwrap();
    let input = format!("{ESCAPED_ATTRS}\nAn unclosed emphasis: *never closed\n");
    let path = write(&rm, "q212_escaped.qmd", &input);

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-12").unwrap();

    let result = rule.convert(&path, true, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1, "expected exactly one splice");

    let expected = format!("{ESCAPED_ATTRS}\nAn unclosed emphasis: *never closed*\n");
    let actual = fs::read_to_string(&path).unwrap();
    assert_eq!(
        actual, expected,
        "the splice must land at the end of the unclosed emphasis and leave \
         every escaped attribute byte untouched"
    );
}

/// The same round trip through the *other* accessor, and through a
/// `replace_range` rather than an `insert`. `q-2-33` percent-encodes the space
/// in a link target, computing the replaced range from **both**
/// `start_offset()` and `end_offset()` — and `end_offset()` on a `Concat`
/// returns the decoded *length*, a small, plausible-looking, entirely wrong
/// file offset. A range built from a contaminated span would overwrite the
/// front matter instead of the link target.
#[test]
fn q_2_33_replace_range_is_byte_correct_with_escaped_attributes() {
    let rm = ResourceManager::new().unwrap();
    let input = format!("{ESCAPED_ATTRS}\nA link with spaces: [docs](my file.qmd).\n");
    let path = write(&rm, "q233_escaped.qmd", &input);

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-33").unwrap();

    let result = rule.convert(&path, true, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1, "expected exactly one splice");

    let expected = format!("{ESCAPED_ATTRS}\nA link with spaces: [docs](my%20file.qmd).\n");
    let actual = fs::read_to_string(&path).unwrap();
    assert_eq!(
        actual, expected,
        "the replaced range must cover exactly the space in the link target \
         and leave every escaped attribute byte untouched"
    );
}

/// `q_2_7.rs:81` is the only site that reads `details[0].location` rather than
/// the diagnostic's own `location`, so it is a separate byte-offset source and
/// needs its own round trip.
#[test]
fn q_2_7_detail_location_splice_is_byte_correct_with_escaped_attributes() {
    let rm = ResourceManager::new().unwrap();
    let input = format!("{ESCAPED_ATTRS}\nThe 'tis a fine day for it.\n");
    let path = write(&rm, "q27_escaped.qmd", &input);

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("q-2-7").unwrap();

    let result = rule.convert(&path, true, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1, "expected exactly one splice");

    let expected = format!("{ESCAPED_ATTRS}\nThe \\'tis a fine day for it.\n");
    let actual = fs::read_to_string(&path).unwrap();
    assert_eq!(
        actual, expected,
        "the escape must be inserted immediately before the apostrophe and \
         leave every escaped attribute byte untouched"
    );
}

/// `attribute_ordering.rs:74` is the site whose *name* makes it the obvious
/// suspect, and it is genuinely different from the others: the diagnostic
/// offset is not the splice position but a **search pivot** handed to
/// `find_attribute_block`, which scans the raw content outward for `{` / `}`.
///
/// This pins the pivot without needing the conversion itself, whose
/// normalisation step shells out to `pandoc` (which is why this crate's
/// attribute-ordering *conversion* tests are `#[ignore]`d). `check` is enough
/// — it is the code path containing the `start_offset()` read, and it reports
/// the recovered block text, so the assertion is positive: the block is found
/// and delimited exactly. A pivot of `0` would make `find_attribute_block`
/// return `Err` (byte 0 of the fixture is `-`, not `{`), yielding zero
/// violations instead of one.
#[test]
fn attribute_ordering_check_recovers_the_block_around_an_escaped_value() {
    let rm = ResourceManager::new().unwrap();
    let block = "{title=\"Use \\`renv\\` today\" .callout-note #my-id}";
    let input = format!("---\ntitle: D2 pivot\n---\n\n::: {block}\nBody text.\n:::\n");
    let path = write(&rm, "attr_order_escaped.qmd", &input);

    let registry = RuleRegistry::new().unwrap();
    let rule = registry.get("attribute-ordering").unwrap();

    let results = rule.check(&path, false).unwrap();
    assert_eq!(
        results.len(),
        1,
        "expected exactly one attribute-ordering violation; zero would mean \
         find_attribute_block rejected the pivot (which is what a pivot of 0 \
         produces), got: {results:?}"
    );

    let hit = &results[0];
    let message = hit.message.as_deref().unwrap_or_default();
    assert!(
        message.contains(block),
        "the reported block must be the exact source text of the attribute \
         block, escapes and all — that text is content[block_start..block_end], \
         so recovering it verbatim pins both ends of the pivot-derived range. \
         Got: {message:?}"
    );

    let loc = hit
        .location
        .as_ref()
        .expect("the violation must carry a location");
    // Row 4 (0-indexed) is the `::: {...}` line; a span collapsed to byte 0
    // reports row 0, column 0.
    assert_eq!(
        loc.row, 4,
        "the violation must be reported on the `::: {{...}}` line (0-indexed \
         row 4), not at row 0 — which is where a span collapsed to byte 0 \
         would report"
    );
    assert!(
        loc.column > 0,
        "the violation must be reported inside the attribute block, not at \
         column 0; got column {}",
        loc.column
    );
}

/// A document that is clean apart from carrying escaped attributes must come
/// out of every registered rule byte-identical. Any rule that manufactured a
/// violation out of an attribute-derived span would splice here, and the
/// byte-identity assertion catches it whatever the offset turned out to be.
///
/// This sweeps the whole registry rather than the handful of codes the
/// round-trip tests name, so a conversion added later is covered by default.
#[test]
fn no_registered_rule_rewrites_a_clean_escaped_attribute_document() {
    let rm = ResourceManager::new().unwrap();
    let registry = RuleRegistry::new().unwrap();

    let mut checked = 0usize;
    for rule in registry.all() {
        let name = rule.name().to_string();
        let path = write(
            &rm,
            &format!("clean_{}.qmd", name.replace('-', "_")),
            ESCAPED_ATTRS,
        );

        // `convert` is the writing path; `in_place = true` is what the CLI
        // does, so this is the shape that would corrupt a real file.
        // An `Err` is tolerated and counted as "no fixes": a rule that bails
        // out did not write, and the byte-identity assertion below still runs.
        // (`attribute-ordering` bails when `pandoc` is absent, which is why
        // this crate's attribute-ordering conversion tests are `#[ignore]`d.)
        let fixes = rule
            .convert(&path, true, false, false)
            .map_or(0, |r| r.fixes_applied);
        assert_eq!(
            fixes, 0,
            "rule {name} reported {fixes} fix(es) on a document whose only \
             unusual feature is escaped attribute values"
        );

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(
            after, ESCAPED_ATTRS,
            "rule {name} rewrote a clean escaped-attribute document"
        );
        checked += 1;
    }

    assert!(
        checked >= 20,
        "expected the sweep to cover the whole registry, only saw {checked} rule(s)"
    );
}
