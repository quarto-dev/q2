/*
 * tiling_corpus_tests.rs
 *
 * Corpus driver for the Plan 7g source-range auditor (bd-1d6io).
 *
 * `audit_source_range_tiling` exhaustively checks the provenance invariants in
 * `claude-notes/designs/provenance-contract.md` — P1/P3 tightness, P4
 * containment and sibling disjointness — over a whole document. Everything it
 * needs to catch bd-1d6io's attribute-key defect was already implemented and
 * correct: `audit_attr_source` calls `check_tightness` on every attr-key range.
 *
 * The defect still lived for a year, because the auditor was only ever pointed
 * at eleven hand-written snippets in `tiling_phase3_tests.rs` — none of which
 * contained a multi-kv attribute. The check existed one function call away from
 * an assertion that would have named it exactly.
 *
 * This is that function call: run the auditor over every real document in the
 * tree and assert it stays silent. It asserts a *property*, which is what makes
 * it strictly stronger than a snapshot comparison — regenerating a fixture
 * cannot launder a violation past it.
 *
 * **Scope limit — read before trusting a green run.** `audit_source_range_tiling`
 * walks `ast.blocks` only; it never visits `ast.meta`. Document *metadata*
 * provenance is therefore entirely unguarded by this test, and there is a known
 * live defect there (bd-mxa44voa: quarto-yaml's span is quote-inclusive while
 * the decoded scalar is what gets re-parsed, so `author: "Dr. Alice Smith"`
 * yields inline sub-ranges that are shifted and would be reported as
 * TightnessViolations if meta were walked). Extending the walk to metadata is
 * tracked separately. Until then, "green" means "no block-content violations".
 *
 * Copyright (c) 2026 Posit, PBC
 */

use std::path::{Path, PathBuf};

use pampa::writers::incremental::{TilingFinding, TilingFindingKind, audit_source_range_tiling};

/// Corpus roots, relative to the repo root. Realistic documents only — the
/// deliberately-malformed trees (`invalid-syntax/`, `error-corpus/`) are out of
/// scope: they exist to exercise diagnostics, and a document that fails to
/// parse has no AST to audit.
const CORPUS_ROOTS: &[&str] = &[
    "ts-packages/annotated-qmd/examples",
    "crates/pampa/tests/pandoc-match-corpus",
    "crates/pampa/tests/smoke",
    "crates/pampa/tests/writers",
    "crates/pampa/tests/claude-examples",
];

/// Documents in the corpus that do not parse. Asserted as an exact set rather
/// than a count, so a *new* unparseable document names itself instead of
/// disappearing into a tolerance. All of these are deliberate parse-error
/// fixtures that return `Err`; none panics.
const EXPECTED_UNPARSEABLE: &[&str] = &[
    "crates/pampa/tests/smoke/001.qmd",
    "crates/pampa/tests/smoke/008.qmd",
    "crates/pampa/tests/smoke/009.qmd",
    "crates/pampa/tests/smoke/010.qmd",
    "crates/pampa/tests/smoke/014.qmd",
    "crates/pampa/tests/smoke/016.qmd",
];

/// Known findings, each with the strand that owns it. An entry here is a
/// promise to come back, not a permanent exemption — keep it keyed as tightly
/// as the underlying issue allows so an *unrelated* regression in the same file
/// still fails.
///
/// `(repo-relative path suffix, finding kind, why)`
const KNOWN: &[(&str, TilingFindingKind, &str)] = &[(
    "pandoc-match-corpus/markdown/034.qmd",
    TilingFindingKind::AttrAlignmentSkipped,
    "Autolink `<https://example.com>` carries a synthesized class in Attr.1 \
     with no matching AttrSourceInfo.classes entry, so the alignment guard \
     skips attr auditing for that node. Census finding, not a range defect \
     (bd-3aolj / bd-1e6a5).",
)];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/pampa should be two levels below the repo root")
        .to_path_buf()
}

fn collect_qmd(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("qmd") {
                out.push(path);
            }
        }
    }
    out
}

/// Parse, tolerating failure. Some corpus documents are pinned parser bugs or
/// deliberately odd; a document we cannot parse simply has no AST to audit and
/// is not this test's business.
fn try_parse(src: &str, name: &str) -> Option<pampa::pandoc::Pandoc> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pampa::readers::qmd::read(
            src.as_bytes(),
            false,
            name,
            &mut std::io::sink(),
            true,
            None,
        )
        .ok()
        .map(|parsed| parsed.0)
    }))
    .ok()
    .flatten()
}

fn is_known(rel: &str, finding: &TilingFinding) -> bool {
    KNOWN
        .iter()
        .any(|(suffix, kind, _)| finding.kind == *kind && rel.ends_with(suffix))
}

#[test]
fn corpus_has_no_source_range_violations() {
    let root = repo_root();

    // Per-root, not just in total: `collect_qmd` skips an unreadable directory
    // silently, so a moved root would otherwise vanish under the total floor.
    // Losing `ts-packages/annotated-qmd/examples` — the 20 fixtures this test
    // exists for — still leaves ~155 files, comfortably over any total floor.
    let mut files: Vec<PathBuf> = Vec::new();
    for corpus_root in CORPUS_ROOTS {
        let found = collect_qmd(&root.join(corpus_root));
        assert!(
            !found.is_empty(),
            "corpus root `{corpus_root}` contributed no .qmd files — it has \
             probably moved or been renamed. A silently empty root makes this \
             test vacuous for everything it was supposed to cover."
        );
        files.extend(found);
    }
    files.sort();

    let mut violations: Vec<String> = Vec::new();
    let mut audited = 0usize;
    let mut unparseable: Vec<String> = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();

    for file in &files {
        // Normalize to forward slashes: `Path::display` yields backslashes on
        // Windows, which would stop every `KNOWN`/`EXPECTED_UNPARSEABLE` suffix
        // from matching and turn this test red for Windows developers only
        // (CI is ubuntu + macOS).
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .display()
            .to_string()
            .replace('\\', "/");
        let Ok(src) = std::fs::read_to_string(file) else {
            // Don't skip silently — that is the very hole the exact-set
            // assertion below closes for parse failures. A corpus document that
            // becomes non-UTF-8 or unreadable must name itself rather than
            // quietly leaving the audited set.
            unreadable.push(rel.clone());
            continue;
        };
        let Some(ast) = try_parse(&src, &rel) else {
            unparseable.push(rel.clone());
            continue;
        };
        audited += 1;

        for finding in audit_source_range_tiling(&ast, &src) {
            // `GeneratedNoInvocation` is a census tally, not a defect: the node
            // makes no contiguous source claim (see TilingFindingKind docs).
            if finding.kind == TilingFindingKind::GeneratedNoInvocation {
                continue;
            }
            if is_known(&rel, &finding) {
                continue;
            }
            violations.push(format!("{rel}: {}", finding.message));
        }
    }

    assert!(
        unreadable.is_empty(),
        "could not read {} corpus document(s): {:?}. A .qmd that is not valid \
         UTF-8 (or is unreadable) would otherwise drop out of the audit without \
         a trace.",
        unreadable.len(),
        unreadable,
    );

    // Exact set, not a count: a change that makes documents panic or fail to
    // parse mid-run would otherwise be absorbed as a smaller `audited` number
    // and still clear a floor. This names the newcomer.
    unparseable.sort();
    let expected: Vec<String> = EXPECTED_UNPARSEABLE.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        unparseable,
        expected,
        "the set of unparseable corpus documents changed. If you added a \
         parse-error fixture, add it to EXPECTED_UNPARSEABLE; if a document \
         that used to parse no longer does, that is a parser regression and \
         this test just found it. ({audited} of {} documents audited.)",
        files.len(),
    );

    assert!(
        violations.is_empty(),
        "audit_source_range_tiling reported {} finding(s) over {audited} documents:\n\n{}\n\n\
         These are provenance-contract violations — see \
         claude-notes/designs/provenance-contract.md (P1 tightness, P3 symmetry, \
         P4 tiling). Fix the producer; do NOT add to KNOWN unless you have a \
         strand explaining why the finding is legitimate. Note that the \
         abbreviation NBSP substitution is already excluded (a `Str` whose text \
         keeps the source space as U+00A0, in matching quantity), so a \
         TightnessViolation here means the range really does claim a byte the \
         node does not own.",
        violations.len(),
        violations.join("\n"),
    );
}
