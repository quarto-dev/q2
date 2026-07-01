/*
 * tests/integration/marimo_resolution.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Phase 4cD (Plan 4c, Stream B — int-rs half) — the resolve-level (Pass-1,
 * STATIC) seams SC11-SC15, SC17, and SC16's int-rs half. See
 * `claude-notes/plans/2026-07-02-plan4c-marimo-validation.md`, the Test Seam
 * Spec table + vacuity notes, and the "Static-claims design (Option B)"
 * table.
 *
 * These are pure resolver tests: **no engine load, no subprocess, no deno,
 * no uv, no render.** They bind 4c0's Vec-per-language claims-combine
 * machinery (`crates/quarto-core/src/extension/types.rs`) + the committed
 * marimo fixture's Option B claims map (`_extension.yml`) through the REAL
 * `resolve_engines` resolver.
 *
 * ## int-rs faithfulness (B-1/B-2)
 *
 * Every test here builds its registry via `ProjectContext::discover` over a
 * temp dir seeded with the COMMITTED marimo fixture's `_extensions/marimo/`
 * package (same `copy_dir` idiom `julia_engine_e2e.rs` uses). That is the
 * REAL production path: `ProjectContext::discover` →
 * `discover_extensions_and_build_registry` → `crate::extension::discover_extensions`
 * (real directory walk) + `read_extension` (real YAML parse of the fixture's
 * `_extension.yml`, including the 4c0 Vec-per-language `claims:` map) →
 * `build_engine_registry` (real, private to `project::mod` — reached only
 * through this public entry point, never reimplemented) → a real `TsEngine`
 * registered under `marimo`, backed by a real (but never-started)
 * `TsEngineHost`. `EngineRegistry::new()` (called internally) also registers
 * the REAL builtins `JupyterEngine`/`KnitrEngine` — no `MockEngine` closures
 * anywhere in this file.
 *
 * `TsEngine::claims_language` answers from its parsed static `claims` Vec
 * with ZERO subprocess — the short-circuit at `ts_engine.rs` (`if let
 * Some(map) = &self.claims { ... }`, proven non-load-triggering by
 * `test_p1_12_static_zero_load` in `ts_engine.rs`) — so every assertion below
 * exercises the real `read.rs` parse + `types.rs` combine/`whenClass`/interop
 * reducer, never a closure.
 *
 * Documents are parsed via the REAL qmd reader (`pampa::readers::qmd::read`)
 * — the same parse path `EngineExecutionStage`/`parse_qmd_to_ast` consume —
 * so `walk_block_for_langs` (`resolution.rs`) sees production-shaped
 * `CodeBlock` attrs, not hand-built ones. (Verified empirically via `cargo
 * run -p pampa --bin pampa -- -t json`: `` ```{python .marimo} `` ``
 * lowers to `CodeBlock` classes `["{python}", "marimo"]` → lang=`"python"`,
 * first_class=`Some("marimo")`; `` ```{python.marimo} `` `` lowers to a
 * SINGLE class `["{python.marimo}"]` → lang=`"python.marimo"`,
 * first_class=`None`.)
 */

// Native-only: TsEngine / TsEngineHost are behind cfg(not(target_arch = "wasm32")).
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use quarto_core::engine::EngineResolution;
use quarto_core::engine::resolve_engines;
use quarto_core::project::ProjectContext;
use quarto_pandoc_types::Pandoc;
use quarto_system_runtime::NativeRuntime;

/// Absolute path to the committed marimo fixture root
/// (`crates/quarto-core/tests/fixtures/extensions/marimo`). The engine
/// package itself lives under `_extensions/marimo/` (co-located `.js`
/// bundle + `_extension.yml` with the Option B `claims:` map).
fn marimo_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extensions/marimo")
}

/// Recursively copy `src` into `dst` (dst is created). Mirrors
/// `julia_engine_e2e.rs`'s helper of the same name.
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// Build a temp single-file "project" with the committed marimo package
/// installed under `_extensions/marimo/`, discover it (real
/// `ProjectContext::discover` → `discover_extensions_and_build_registry` →
/// `read_extension` + `build_engine_registry`), and return the `TempDir`
/// (keep alive for the test's duration) plus the discovered
/// `ProjectContext`. `project.registry` is the REAL registry: builtins
/// (`markdown`/`knitr`/`jupyter`) + the real marimo `TsEngine` built from the
/// fixture's parsed static claims.
///
/// No `.qmd` content matters here beyond existing — `discover` needs an
/// anchor file to find (parent dir holds `_extensions/`); the documents
/// actually resolved in each test are separate in-memory parses (see
/// `resolve_doc`), reusing this one discovered registry (pure fn of
/// `meta`/`ast`/`registry`, so registry reuse across docs is faithful — same
/// pattern as `julia_engine_e2e.rs`'s J4 shared-host reuse).
fn setup_marimo_project() -> (TempDir, ProjectContext) {
    let tmp = TempDir::new().unwrap();
    copy_dir(
        &marimo_fixture_root().join("_extensions/marimo"),
        &tmp.path().join("_extensions").join("marimo"),
    );
    let anchor = tmp.path().join("anchor.qmd");
    std::fs::write(&anchor, "anchor\n").unwrap();

    let runtime: NativeRuntime = NativeRuntime::new();
    let project = ProjectContext::discover(&anchor, &runtime).expect(
        "project discovery must succeed: real read_extension parse of the marimo fixture's \
         _extension.yml + real build_engine_registry construction",
    );
    (tmp, project)
}

/// Parse `qmd` through the REAL qmd reader (`pampa::readers::qmd::read` —
/// the same parse path `EngineExecutionStage`/`parse_qmd_to_ast` consume) and
/// return the `Pandoc` AST. Real fenced-code-block attrs, not hand-built
/// `Block::CodeBlock` values.
fn parse_qmd(qmd: &str) -> Pandoc {
    let (ast, _ctx, _warnings) = pampa::readers::qmd::read(
        qmd.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("qmd parse must succeed for a well-formed fixture doc");
    ast
}

/// Resolve `qmd` against `project.registry` with no file-claim seed (pure
/// Pass-1 static resolution — `claimed: None`, matching every row's "Pass-1
/// static-resolution" framing: no engine load, no subprocess, no render).
fn resolve_doc(qmd: &str, project: &ProjectContext) -> EngineResolution {
    let ast = parse_qmd(qmd);
    resolve_engines(&ast.meta, &ast, &project.registry, None)
}

// ── SC11: first_class selection via two separate docs ───────────────────────
//
// Doc A: `{python .marimo}`-only → `ownership["python"] == "marimo"` (the
// whenClass-conditioned Primary(2) claim applies because first_class ==
// "marimo").
// Doc B: plain `{python}`-only → `ownership["python"] == "jupyter"` (the SAME
// claim's `whenClass: marimo` guard does NOT match — first_class is `None`
// — so it falls through jupyter's implicit-Fallback tier). Doc B is the real
// discriminator: without it, an unconditional python-Primary claim would
// pass doc A alone and the test would be theater (Test Seam Spec vacuity
// note).
//
// RED evidence (revert (a) — drop the fixture's `python:` claim entirely,
// i.e. delete its two lines from
// `crates/quarto-core/tests/fixtures/extensions/marimo/_extensions/marimo/_extension.yml`,
// restored byte-identical afterward):
//
// ```text
// thread 'marimo_resolution::sc11_first_class_selection_two_docs' panicked at crates/quarto-core/tests/integration/marimo_resolution.rs:188:5:
// assertion `left == right` failed: doc A ({python .marimo}) must resolve python ownership to marimo
//   left: Some("jupyter")
//  right: Some("marimo")
// ```
//
// RED evidence (revert (b) — remove the `whenClass` guard in
// `crates/quarto-core/src/extension/types.rs`'s
// `static_claim_to_language_claim` — i.e. delete the
// `if let Some(ref required) = claim.when_class && first_class != Some(...) { return LanguageClaim::None; }`
// block so every claim applies unconditionally, restored byte-identical
// afterward):
//
// ```text
// thread 'marimo_resolution::sc11_first_class_selection_two_docs' panicked at crates/quarto-core/tests/integration/marimo_resolution.rs:196:5:
// assertion `left == right` failed: doc B (bare {python}) must resolve python ownership to jupyter, NOT marimo (whenClass gate: marimo's python claim only applies when first_class==marimo)
//   left: Some("marimo")
//  right: Some("jupyter")
// ```
#[test]
fn sc11_first_class_selection_two_docs() {
    let (_tmp, project) = setup_marimo_project();

    let doc_a = "```{python .marimo}\n1 + 1\n```\n";
    let resolution_a = resolve_doc(doc_a, &project);
    assert_eq!(
        resolution_a.ownership.get("python").map(String::as_str),
        Some("marimo"),
        "doc A ({{python .marimo}}) must resolve python ownership to marimo"
    );

    let doc_b = "```{python}\n1 + 1\n```\n";
    let resolution_b = resolve_doc(doc_b, &project);
    assert_eq!(
        resolution_b.ownership.get("python").map(String::as_str),
        Some("jupyter"),
        "doc B (bare {{python}}) must resolve python ownership to jupyter, NOT marimo \
         (whenClass gate: marimo's python claim only applies when first_class==marimo)"
    );
}

// ── SC12: dotted syntax `{python.marimo}` / `{sql.marimo}` ───────────────────
//
// A single dotted token (no space before `.marimo`) lowers to ONE literal
// language string ("python.marimo" / "sql.marimo") with first_class `None`
// (verified via `pampa -t json`, see module doc). The fixture's claims map
// carries a matching, unconditional Primary(1) claim under exactly those
// dotted keys, so both resolve to marimo purely through key lookup — no
// `whenClass` involved.
//
// RED evidence (revert — drop the two dotted claim keys `"python.marimo"`
// and `"sql.marimo"` from the fixture's `_extension.yml`, restored
// byte-identical afterward):
//
// ```text
// thread 'marimo_resolution::sc12_dotted_syntax_primary_keys' panicked at crates/quarto-core/tests/integration/marimo_resolution.rs:230:5:
// assertion `left == right` failed: {python.marimo} cell must resolve ownership via its dotted Primary(1) claim key
//   left: Some("jupyter")
//  right: Some("marimo")
// ```
#[test]
fn sc12_dotted_syntax_primary_keys() {
    let (_tmp, project) = setup_marimo_project();

    let doc = "```{python.marimo}\n1 + 1\n```\n\n```{sql.marimo}\nselect 1\n```\n";
    let resolution = resolve_doc(doc, &project);

    assert_eq!(
        resolution
            .ownership
            .get("python.marimo")
            .map(String::as_str),
        Some("marimo"),
        "{{python.marimo}} cell must resolve ownership via its dotted Primary(1) claim key"
    );
    assert_eq!(
        resolution.ownership.get("sql.marimo").map(String::as_str),
        Some("marimo"),
        "{{sql.marimo}} cell must resolve ownership via its dotted Primary(1) claim key"
    );
}

// ── SC13: tagged-sql self-activation (no python present) ─────────────────────
//
// A doc with ONLY `{sql .marimo}` (no python cell at all) still activates
// marimo: the whenClass-conditioned Primary(2) sql claim matches
// first_class=="marimo" directly — self-activation doesn't need python to be
// present first (Q1 parity).
//
// RED evidence (revert — drop the sql whenClass-primary claim entry (the
// FIRST element of the fixture's `sql:` Vec, `{ whenClass: marimo, kind:
// primary, priority: 2 }`), restored byte-identical afterward):
//
// ```text
// thread 'marimo_resolution::sc13_tagged_sql_self_activation' panicked at crates/quarto-core/tests/integration/marimo_resolution.rs:269:5:
// assertion `left == right` failed: {sql .marimo}-only doc (no python) must self-activate marimo via the whenClass-primary sql claim
//   left: Some("jupyter")
//  right: Some("marimo")
// ```
#[test]
fn sc13_tagged_sql_self_activation() {
    let (_tmp, project) = setup_marimo_project();

    let doc = "```{sql .marimo}\nselect 1\n```\n";
    let resolution = resolve_doc(doc, &project);

    assert_eq!(
        resolution.ownership.get("sql").map(String::as_str),
        Some("marimo"),
        "{{sql .marimo}}-only doc (no python) must self-activate marimo via the \
         whenClass-primary sql claim"
    );
}

// ── SC14: bare sql rides along when marimo is already present ───────────────
//
// A doc with `{python .marimo}` (marimo activates via python's
// whenClass-Primary) PLUS a bare `{sql}` cell (no tag): sql's combined claim
// for first_class=None is the unconditional Interop(0) entry (the
// whenClass-primary entry mismatches and is dropped by `combine_claims`).
// Interop is presence-gated (T3) — marimo is already `present` from owning
// python, so it wins sql too.
//
// RED evidence (revert — drop the sql interop claim entry (the SECOND
// element of the fixture's `sql:` Vec, `{ kind: interop }`), restored
// byte-identical afterward):
//
// ```text
// thread 'marimo_resolution::sc14_bare_sql_rides_along_when_present' panicked at crates/quarto-core/tests/integration/marimo_resolution.rs:308:5:
// assertion `left == right` failed: bare {sql} must ride along to marimo via the interop claim when marimo is already present (python .marimo)
//   left: Some("jupyter")
//  right: Some("marimo")
// ```
#[test]
fn sc14_bare_sql_rides_along_when_present() {
    let (_tmp, project) = setup_marimo_project();

    let doc = "```{python .marimo}\n1 + 1\n```\n\n```{sql}\nselect 1\n```\n";
    let resolution = resolve_doc(doc, &project);

    assert_eq!(
        resolution.ownership.get("python").map(String::as_str),
        Some("marimo"),
        "exercised-guard: python must own marimo first (interop is presence-gated on it)"
    );
    assert_eq!(
        resolution.ownership.get("sql").map(String::as_str),
        Some("marimo"),
        "bare {{sql}} must ride along to marimo via the interop claim when marimo is \
         already present (python .marimo)"
    );
}

// ── SC15: bare sql alone does NOT activate marimo (presence-gating negative) ─
//
// A bare `{sql}`-only doc (no python, no `.marimo` tag anywhere): marimo is
// never made `present` (nothing claims it as Primary/Fallback), so its
// Interop(0) sql claim is presence-gated OUT at T3 — jupyter's unconditional
// implicit-Fallback(0) wins instead. SC13+SC14+SC15 bind presence-gating AS A
// SET (Test Seam Spec vacuity note): this row is the one that would redden
// if the sql interop claim were wrongly a Primary (a Primary claim isn't
// presence-gated).
//
// RED evidence (revert — change the fixture's bare-sql `{ kind: interop }`
// entry to `{ kind: primary }`, restored byte-identical afterward):
//
// ```text
// thread 'marimo_resolution::sc15_bare_sql_alone_does_not_activate_marimo' panicked at crates/quarto-core/tests/integration/marimo_resolution.rs:341:5:
// bare {sql}-only doc (no marimo tag anywhere) must NOT activate marimo (presence-gating negative) — got Some("marimo")
// ```
#[test]
fn sc15_bare_sql_alone_does_not_activate_marimo() {
    let (_tmp, project) = setup_marimo_project();

    let doc = "```{sql}\nselect 1\n```\n";
    let resolution = resolve_doc(doc, &project);

    let owner = resolution.ownership.get("sql").map(String::as_str);
    assert!(
        owner != Some("marimo"),
        "bare {{sql}}-only doc (no marimo tag anywhere) must NOT activate marimo \
         (presence-gating negative) — got {:?}",
        owner
    );
}

// ── SC16 (int-rs half): coexistence — handled_languages_for both engines ────
//
// `{python .marimo}` + `{r}` doc: marimo owns python (Primary via whenClass),
// the REAL `KnitrEngine` owns r (Primary(1), unconditional). Asserts BOTH
// engines' `handled_languages_for` values.
//
// ⚠️ SEMANTICS CORRECTION (dated annotation, appended to the frozen Test Seam
// Spec row per the task brief's pre-authorization — see
// `claude-notes/plans/2026-07-02-plan4c-marimo-validation.md` SC16 row):
// `handled_languages_for` returns the LEAVE-ALONE set (`HANDLED_LANGUAGES ∪
// {lang : ownership[lang] != engine}`, `resolution.rs`'s
// `EngineResolution::handled_languages_for`) — NOT the owned set. So the
// correct assertion is: marimo's handled set CONTAINS "r" (marimo leaves the
// knitr-owned r cell alone) and knitr's handled set CONTAINS "python" (knitr
// leaves the marimo-owned python cell alone). The original row phrasing
// ("marimo `handled` excludes the other lang") predates finding #4
// (`crates/quarto-core/src/engine/jupyter/text_execute.rs:600-655`) and reads
// inverted against the real, already-pinned API semantics; this test asserts
// the real (leave-alone) semantics, matching `test_first_class_passed_to_claim`
// and its siblings in `resolution.rs`.
//
// The e2e half (render actually leaves the non-owned `{r}` cell's markdown
// untouched by marimo / untouched by knitr for the python cell) is NOT
// covered here — it remains for the e2e stream (marimo_engine_e2e.rs).
//
// RED evidence (revert — the enforcement mechanism itself:
// `EngineResolution::handled_languages_for` in
// `crates/quarto-core/src/engine/resolution.rs`, flip `owner.as_str() !=
// engine` to `owner.as_str() == engine` so it computes the OWNED set instead
// of the leave-alone set, restored byte-identical afterward):
//
// ```text
// thread 'marimo_resolution::sc16_coexistence_handled_languages_leave_alone_semantics' panicked at crates/quarto-core/tests/integration/marimo_resolution.rs:403:5:
// marimo's handled_languages_for must CONTAIN "r" (leave-alone semantics: marimo leaves the knitr-owned r cell alone) — got ["dot", "mermaid", "ojs", "python"]
// ```
//
// (the revert computes the OWNED set instead of leave-alone: marimo owns
// python, so `["dot", "mermaid", "ojs"]` (HANDLED_LANGUAGES) plus the
// wrongly-included owned "python" — "r" is absent either way, which is what
// reddens the assertion)
#[test]
fn sc16_coexistence_handled_languages_leave_alone_semantics() {
    let (_tmp, project) = setup_marimo_project();

    let doc = "```{python .marimo}\n1 + 1\n```\n\n```{r}\n1 + 1\n```\n";
    let resolution = resolve_doc(doc, &project);

    assert_eq!(
        resolution.ownership.get("python").map(String::as_str),
        Some("marimo"),
        "exercised-guard: marimo must own python"
    );
    assert_eq!(
        resolution.ownership.get("r").map(String::as_str),
        Some("knitr"),
        "exercised-guard: the real KnitrEngine must own r (Primary(1), unconditional)"
    );

    let marimo_handled = resolution.handled_languages_for("marimo");
    assert!(
        marimo_handled.iter().any(|l| l == "r"),
        "marimo's handled_languages_for must CONTAIN \"r\" (leave-alone semantics: \
         marimo leaves the knitr-owned r cell alone) — got {:?}",
        marimo_handled
    );

    let knitr_handled = resolution.handled_languages_for("knitr");
    assert!(
        knitr_handled.iter().any(|l| l == "python"),
        "knitr's handled_languages_for must CONTAIN \"python\" (leave-alone semantics: \
         knitr leaves the marimo-owned python cell alone) — got {:?}",
        knitr_handled
    );
}

// ── SC17: first-occurrence-wins (v1 single-owner limitation, pinned) ────────
//
// A doc mixing `{python .marimo}` THEN a later plain `{python}`: q2's
// resolver de-duplicates languages by FIRST occurrence
// (`resolution.rs::walk_block_for_langs`'s `if !seen_set.contains(lang)`
// guard) and resolves the WHOLE document's `python` ownership from the
// FIRST cell's `first_class` alone — there is no per-cell re-resolution.
// This pins the plan's documented v1 single-owner limitation (correction
// 10): mixing marimo-tagged and plain cells of the SAME language in one doc
// resolves to a SINGLE owner (whichever the first cell selects), not a
// per-cell split.
//
// RED evidence (revert — flip first-wins to last-wins: in
// `crates/quarto-core/src/engine/resolution.rs::walk_block_for_langs`,
// change the `if !seen_set.contains(lang) { ... }` guard so `first_classes`
// is updated on EVERY occurrence (not just the first), restored
// byte-identical afterward):
//
// ```text
// thread 'marimo_resolution::sc17_first_occurrence_wins_v1_single_owner' panicked at crates/quarto-core/tests/integration/marimo_resolution.rs:455:5:
// assertion `left == right` failed: mixed {python .marimo} THEN plain {python} must resolve python ownership to marimo (FIRST occurrence's first_class wins, v1 single-owner limitation)
//   left: Some("jupyter")
//  right: Some("marimo")
// ```
#[test]
fn sc17_first_occurrence_wins_v1_single_owner() {
    let (_tmp, project) = setup_marimo_project();

    let doc = "```{python .marimo}\n1 + 1\n```\n\n```{python}\n2 + 2\n```\n";
    let resolution = resolve_doc(doc, &project);

    assert_eq!(
        resolution.ownership.get("python").map(String::as_str),
        Some("marimo"),
        "mixed {{python .marimo}} THEN plain {{python}} must resolve python ownership to \
         marimo (FIRST occurrence's first_class wins, v1 single-owner limitation)"
    );
}
