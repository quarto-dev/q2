/*
 * tests/integration/marimo_engine_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 4c (marimo validation) — Test Seam Spec SC8, the marimo+deno+uv-gated
 * END-TO-END row for Phase 4cB (minimal marimo render, static claims path).
 * Drives the committed marimo-engine fixture (Phase 4cA) through the REAL
 * render path — a TS engine spawning the Deno engine-host subprocess, which
 * in turn spawns marimo via `uv run` — and asserts the whole chain works:
 * discovery → static resolution (the `whenClass: marimo` `python` claim,
 * priority 2) → LoadEngine / LaunchEngine / execute → marimo's own
 * `MarimoMdParser` executes the cell → the `include-in-header` splice →
 * HTML writer.
 *
 * Harness mirrors `julia_engine_e2e.rs`'s J1 row, with one gating
 * difference: marimo has no daemon binary of its own (the default render
 * path shells out to `uv run --with marimo …`), so this test is gated on
 * `deno` + `uv` on PATH — NOT a `marimo` binary (there is deliberately no
 * global `marimo` install; `uv` resolves it into an ephemeral environment on
 * first run). On a machine with BOTH `deno` and `uv` present this test RUNS
 * — a skip there is a signal something is wrong, not a pass (Test Seam
 * Spec, CI gating).
 *
 * The committed marimo fixture root is a project directory that contains
 * the engine package under `_extensions/marimo/` (co-located with
 * `command.py`/`extract.py`, which the bundle resolves via
 * `import.meta.url`) — same structural shape as julia's fixture. Only the
 * `_extensions/marimo/` package is copied into the tempdir (mirroring J1;
 * no `_quarto.yml` copy is needed — engine discovery works off the
 * directory-relative `_extensions/` convention for a standalone render).
 *
 * Also covers Phase 4cB2 (dynamic-claims path): a `_extension.yml`-rewrite
 * helper derives a claims-less variant of the SAME fixture at test-setup
 * time (no second committed bundle), forcing `TsEngine`'s legacy dynamic
 * `ClaimsLanguage`/`ClaimsFile` wire-call path instead of SC8's zero-load
 * static-claims path. `p4cb2_dynamic_path_parity_minimal_render_matches_static`
 * proves parity for the python-only case. The bare-sql interop row (SC9) is
 * NOT a committed passing test — see the doc comment ahead of its section
 * for the full evidence trail and why it is reported NEEDS_CONTEXT instead.
 */

// Native-only: TsEngine / TsEngineHost are behind cfg(not(target_arch = "wasm32")).
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Return `true` when `deno` is on PATH (the engine-host subprocess runtime).
fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Return `true` when `uv` is on PATH (marimo's default render-path
/// resolver — there is deliberately no global `marimo` binary; `uv run
/// --with marimo …` resolves it into an ephemeral environment).
fn uv_available() -> bool {
    Command::new("uv")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Return `true` when `Rscript` is on PATH (Phase 4cD SC16-e2e's second
/// engine — the real, builtin `KnitrEngine`, gated the same way
/// `knitr/mod.rs`'s own `#[ignore]`d integration tests gate on
/// `KnitrEngine::is_available()`).
fn rscript_available() -> bool {
    Command::new("Rscript")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Return `true` when the R `knitr` package is installed (checked
/// separately from `rscript_available` — `KnitrEngine::is_available()`
/// itself only checks for the `Rscript` binary, not the package; SC16-e2e's
/// brief explicitly calls for gating on both).
fn knitr_r_package_available() -> bool {
    Command::new("Rscript")
        .args([
            "-e",
            "if (!requireNamespace(\"knitr\", quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Absolute path to the committed marimo-engine fixture root
/// (`crates/quarto-core/tests/fixtures/extensions/marimo`). This root is a
/// project dir; the engine package itself lives under `_extensions/marimo`.
fn marimo_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extensions/marimo")
}

/// Recursively copy `src` into `dst` (dst is created).
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

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Build a temp project with the committed marimo-engine package installed
/// under `_extensions/marimo/` (co-located `command.py`/`extract.py` +
/// bundle preserved). Returns the `TempDir` (keep it alive for the test's
/// duration).
fn setup_marimo_project() -> TempDir {
    let tmp = TempDir::new().unwrap();
    copy_dir(
        &marimo_fixture_root().join("_extensions/marimo"),
        &tmp.path().join("_extensions").join("marimo"),
    );
    tmp
}

// ── Phase 4cB2: dynamic-claims fixture derivation ──────────────────────────
//
// Rather than committing a second 22KB engine-bundle copy, the dynamic-path
// variant is DERIVED at test-setup time from the single committed fixture:
// copy `_extensions/marimo/` into the tempdir (as `setup_marimo_project`
// does), then rewrite ONLY the tempdir copy's `_extension.yml` to drop the
// `claims:` map (keeping `name`/`path`/`title`/`author`/`version`/
// `quarto-required` — everything else). With `self.claims == None`,
// `TsEngine::claims_language` (`ts_engine.rs:668` else-branch) and
// `TsEngine::claims_file` (`ts_engine.rs:735` step 3) both take the
// LEGACY DYNAMIC path: `ensure_loaded` + a live `ClaimsLanguage`/`ClaimsFile`
// wire call to the SAME unmodified engine module — zero drift risk against
// the static fixture (identical `marimo-engine.js`/`command.py`/`extract.py`,
// only the declared-claims YAML differs). The committed static fixture is
// never touched by this derivation (verified `git diff` clean in the task
// report).
//
// The rewrite is a targeted line-drop (find the `claims:` key under the
// engine's map, truncate there) rather than a hardcoded second YAML, so it
// tracks the committed fixture's header fields (title/author/version/
// quarto-required) automatically if they ever change. `assert!`s below turn
// a future `_extension.yml` shape change (e.g. a trailing key coming AFTER
// `claims:`) into an obvious test-setup failure rather than a silent
// wrong-fixture bug.
//
// Also appends `claims-files: []` (ratified anti-vacuity correction, task
// 4cB2's Risk-1 evidence: without it, `TsEngine::claims_file`
// (`ts_engine.rs:735` step 3) takes the DYNAMIC content-inspecting path —
// loads the unmodified engine and calls its live `claimsFile`, which
// regex-scans the raw file text for ANY `.marimo` fence and, if found,
// short-circuits ALL per-language `claims:` resolution via
// `EngineClaimsFileStage`'s whole-file claim (`ctx.claimed_engine_name`,
// `engine_execution.rs:225`) — `resolve_engines`'s `claimed` short-circuit
// then returns an EMPTY `ownership` map (`resolution.rs`'s top-of-function
// early return), regardless of what the per-language dynamic
// `ClaimsLanguage` wire calls would have answered. Confirmed empirically
// (temporary, reverted instrumentation): with `claims-files:` absent,
// `ownership={}` / `claimed_engine_name=Some("marimo")` for a
// `{python .marimo}` + bare `{sql}` doc — vacuous, the per-language
// dynamic-claims path this fixture variant exists to exercise is never
// actually reached. Declaring `claims-files: []` makes `claims_file`
// answer from the now-static, empty list (zero-load, `false`) instead,
// disabling the short-circuit so per-language resolution genuinely
// decides: re-confirmed `ownership={"python": "marimo", "sql": "marimo"}`
// / `claimed_engine_name=None` with this line present. See the task
// report's Risk-1 evidence trail for the full before/after transcript.
fn write_claims_less_extension_yml(extension_yml_path: &Path) {
    let original = std::fs::read_to_string(extension_yml_path).unwrap();
    let claims_line_idx = original
        .lines()
        .position(|line| line.trim_start() == "claims:")
        .expect("committed _extension.yml must have a top-level `claims:` key to drop");
    let derived: String = original
        .lines()
        .take(claims_line_idx)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n      claims-files: []\n";
    assert!(
        derived.contains("name: marimo"),
        "derived _extension.yml must keep the `name: marimo` key; got:\n{derived}"
    );
    assert!(
        !derived.contains("claims:"),
        "derived _extension.yml must NOT contain a `claims:` key; got:\n{derived}"
    );
    assert!(
        derived.contains("claims-files: []"),
        "derived _extension.yml must declare an empty `claims-files:` list \
         (disables the whole-file dynamic claims_file short-circuit); got:\n{derived}"
    );
    std::fs::write(extension_yml_path, derived).unwrap();
}

/// Build a temp project with the committed marimo-engine package installed,
/// with its `_extension.yml`'s `claims:` map dropped (keeping `name`/`path`),
/// forcing q2's dynamic per-language claims path (Phase 4cB2). Same
/// unmodified engine bundle as `setup_marimo_project` — see the derivation
/// note above.
fn setup_marimo_project_dynamic() -> TempDir {
    let tmp = setup_marimo_project();
    write_claims_less_extension_yml(&tmp.path().join("_extensions/marimo/_extension.yml"));
    tmp
}

/// Render `input` through the real per-document render path (`render_to_file`,
/// the same entry `quarto render` uses) and return the rendered HTML.
fn render_html(input: &Path) -> anyhow::Result<String> {
    let options = RenderToFileOptions::default();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(input, "html", &options, runtime)?;
    Ok(std::fs::read_to_string(&result.output_path).expect("read rendered HTML"))
}

/// The Phase 4cB minimal document, byte-identical to the committed fixture
/// `minimal.qmd`: NO `engine: marimo` declaration (ownership must flow
/// through the static `whenClass: marimo` claim on `python`, first_class
/// selection — this is exactly what this row's named revert discriminates),
/// one `{python .marimo}` cell computing `1 + 1`.
const MINIMAL_DOC: &str =
    "---\ntitle: \"Marimo Minimal\"\n---\n\n```{python .marimo}\nimport marimo as mo\n1 + 1\n```\n";

/// The Phase 4cB2 SC9 document: `{python .marimo}` (marimo-owned via the
/// static `whenClass: marimo` claim / dynamic `claimsLanguage("python",
/// "marimo")`) + a bare `{sql}` cell (Option B Interop — rides along only
/// because marimo is already present). `pyproject:` front matter declares
/// the runtime deps `extract.py`'s SQL bridge needs
/// (duckdb/sqlglot/polars/pyarrow), which `command.py`'s
/// `construct_uv_flags` turns into `uv run --with …` flags.
const SQL_INTEROP_DOC: &str = "---\ntitle: \"Marimo SQL Interop\"\npyproject: |\n    dependencies = [\"duckdb\", \"sqlglot\", \"polars\", \"pyarrow\"]\n---\n\n```{python .marimo}\nimport marimo as mo\n1 + 1\n```\n\n```{sql}\nSELECT 1 + 1 AS x\n```\n";

/// The Phase 4cC document, byte-identical to the committed fixture
/// `widget.qmd`: one `{python .marimo}` cell rendering a pure `mo.ui` widget
/// (a slider — no matplotlib/altair, keeping deps at plain marimo, warm in
/// the uv cache).
const WIDGET_DOC: &str = "---\ntitle: \"Marimo Widget\"\n---\n\n```{python .marimo}\nimport marimo as mo\nmo.ui.slider(1, 10, value=5)\n```\n";

/// The Phase 4cD SC13-e2e document: ONLY tagged-sql cells — one space-form
/// `{sql .marimo}` (`SELECT 1 + 1 AS x`) and one dotted-form `{sql.marimo}`
/// (`SELECT 2 + 2 AS x`, folding in the dotted-form render proof per the
/// brief), each with a DISTINCT query so the two cells' outputs are
/// individually distinguishable.
///
/// A companion `{python .marimo}` cell containing ONLY `import marimo as
/// mo` (no computation) is included alongside them — see the doc comment on
/// `sc13_e2e_tagged_sql_self_activation_renders` for why this is a required
/// adaptation, not a departure from "sql-only, no python" in spirit: marimo's
/// own `mo.sql()` codegen needs `mo` bound by an explicit import cell
/// somewhere in the notebook, confirmed empirically (a genuinely
/// python-cell-free sql-only doc renders successfully but BOTH sql islands
/// come back as `application/vnd.marimo+error` /
/// `NameError: name 'mo' is not defined` — marimo's own notebook-execution
/// model, not a q2 defect). The *resolution* claim this row is really about
/// (`ownership["sql"]=="marimo"` with ZERO python cells at all, tagged-sql
/// alone) is already proven at the int-rs tier by
/// `marimo_resolution.rs::sc13_tagged_sql_self_activation`; this e2e row's
/// job is specifically to prove EXECUTION, which needs `mo` defined.
const TAGGED_SQL_ONLY_DOC: &str = "---\ntitle: \"Marimo SQL Self-Activation\"\npyproject: |\n    dependencies = [\"duckdb\", \"sqlglot\", \"polars\", \"pyarrow\"]\n---\n\n```{python .marimo}\nimport marimo as mo\n```\n\n```{sql .marimo}\nSELECT 1 + 1 AS x\n```\n\n```{sql.marimo}\nSELECT 2 + 2 AS x\n```\n";

/// The Phase 4cD sc14-e2e-static document: `{python .marimo}` (`1 + 1`) +
/// bare `{sql}` (`SELECT 1 + 1 AS x`) — the SAME shape as SC9's
/// `SQL_INTEROP_DOC`, but this row proves the STATIC claims path (rendered
/// through the COMMITTED fixture, no `_extension.yml` rewrite) rather than
/// SC9's dynamic-claims-less path — closing the gap SC9 alone left open
/// (does the *default*, non-rewritten fixture configuration also get
/// bare-sql interop execution right?).
const SQL_INTEROP_STATIC_DOC: &str = "---\ntitle: \"Marimo SQL Interop Static\"\npyproject: |\n    dependencies = [\"duckdb\", \"sqlglot\", \"polars\", \"pyarrow\"]\n---\n\n```{python .marimo}\nimport marimo as mo\n1 + 1\n```\n\n```{sql}\nSELECT 1 + 1 AS x\n```\n";

/// The Phase 4cD SC16-e2e document: `{python .marimo}` (`1 + 1`, marimo) +
/// `{r}` (`1 + 1`, the real builtin `KnitrEngine`) — two-engine single-doc
/// coexistence via distinct languages (correction 10), exercising `§5
/// handled_languages` enforcement with two LIVE engines actually executing.
const COEXIST_PYTHON_R_DOC: &str = "---\ntitle: \"Marimo Knitr Coexist\"\n---\n\n```{python .marimo}\nimport marimo as mo\n1 + 1\n```\n\n```{r}\n1 + 1\n```\n";

/// The Phase 4cD SC18 document: a `{python .marimo}` cell whose render
/// triggers `execute()`'s OUTER try/catch (marimo-engine.ts ~319-329) — see
/// the doc comment on `sc18_e2e_execute_catch_shows_error_marker_not_crash`
/// for why an unresolvable `pyproject` dependency is used as the trigger
/// instead of the brief's suggested syntactically-bad cell body (e.g.
/// `def (:`), which was empirically found NOT to reach this catch (marimo's
/// own per-cell parse-error isolation swallows it first).
const EXECUTE_ERROR_DOC: &str = "---\ntitle: \"Marimo Execute Error\"\npyproject: |\n    dependencies = [\"this-package-definitely-does-not-exist-q2-sc18-test-xyz\"]\n---\n\n```{python .marimo}\nimport marimo as mo\n1 + 1\n```\n";

/// First ~800 chars of the `<body>` for assertion failure messages.
fn body_excerpt(html: &str) -> String {
    let start = html.find("<body").unwrap_or(0);
    html[start..].chars().take(800).collect()
}

// ── SC8: minimal marimo render, static claims path (Phase 4cB) ────────────
//
// Renders the 4cB minimal doc through the full chain with real Deno + real
// `uv`/marimo (no mocks): discovery → static `Primary(2)` resolution of the
// `whenClass: marimo` claim on `python` → LoadEngine / LaunchEngine /
// execute → marimo's own `MarimoMdParser` runs `1 + 1` → the
// `include-in-header` splice → HTML writer. Asserts the rendered HTML
// contains BOTH the evaluated result `2` AND a marimo-specific signature —
// CONJUNCTIVE, per the Test Seam Spec (jupyter also emits `2`, so `2` alone
// would not discriminate marimo ownership from some other engine/fallback
// claiming the cell).
//
// The marimo-specific signature is checked as "one of" the three markers
// `extract.py` (lines ~293-327) emits into the injected `include-in-header`
// content: the `__MARIMO_EXPORT_CONTEXT__` trust marker script, or the
// `<marimo-code` hidden-notebook-source tag. (`<marimo-island>` /
// `<marimo-cell-output>` / `<marimo-cell-code>` also appear in the body from
// marimo's own `MarimoIslandStub.render()`, but those are NOT what this row
// binds — see the Test Seam Spec's explicit "extract.py 293-327" scoping.)
//
// Named revert (Test Seam Spec SC8, CORRECTED 2026-07-02 — see the plan
// file's 4cB "BLOCKING FINDING #3" and the task report for the full trace):
// a TWO-PART temporary working-tree edit to the fixture `_extension.yml`,
// restored byte-identical afterward:
//   1. remove the `python` claim entry from the `claims:` map, AND
//   2. add `claims-files: []` to the same engine entry.
//
// Part 1 alone is NOT sufficient and does not redden this test — empirically
// verified. Root cause: `EngineClaimsFileStage` (engine_execution.rs:225's
// own comment: `claimed_engine_name` "short-circuits ALL tier evaluation and
// returns exactly that engine") asks every registered engine
// `claims_file(file, ext)` for the WHOLE file before any per-language tier
// resolution runs. The fixture's `_extension.yml` declares no
// `claims-files:` key, so marimo's `claims_file` answer is the DYNAMIC,
// content-inspecting path (`ts_engine.rs:716-717`): it loads the (excluded,
// unmodified) engine module and calls its live `claimsFile`, which
// independently regex-scans the raw file text for a `.marimo` fence
// (`containsMarimoFence`/`MARIMO_CELL_REGEX` in `marimo-engine.ts`) — found
// unconditionally in this doc — and that whole-file claim alone assigns
// marimo ownership, bypassing the per-language `claims:` map entirely. Part
// 2 (`claims-files: []`) makes `claims_file` answer from the now-static,
// empty list (zero-load, `false`), disabling that short-circuit so
// per-language resolution genuinely decides.
//
// With BOTH parts applied: no claim on bare `python` (part 1) + no
// whole-file short-circuit (part 2) → the `{python .marimo}` cell falls
// through to the next-tier engine (jupyter, unavailable on this machine) →
// render fails outright → RED, captured verbatim:
//
//   thread 'marimo_engine_e2e::sc8_minimal_marimo_render_shows_marimo_signature_and_result' panicked at crates/quarto-core/tests/integration/marimo_engine_e2e.rs:173:36:
//   marimo render must succeed: Error: Engine 'jupyter' is registered but its runtime is not available. Install the required runtime to render this document, or remove the engine declaration from the front matter.
//
// Fixture restored byte-identical after capture (`git diff` clean); this
// test re-confirmed GREEN against the pristine fixture immediately after.
#[test]
fn sc8_minimal_marimo_render_shows_marimo_signature_and_result() {
    if !deno_available() {
        eprintln!(
            "SKIP: deno not on PATH — sc8_minimal_marimo_render_shows_marimo_signature_and_result"
        );
        return;
    }
    if !uv_available() {
        eprintln!(
            "SKIP: uv not on PATH — sc8_minimal_marimo_render_shows_marimo_signature_and_result"
        );
        return;
    }
    let tmp = setup_marimo_project();
    let input = tmp.path().join("minimal.qmd");
    write_file(&input, MINIMAL_DOC);

    let html = render_html(&input).expect("marimo render must succeed");

    // Discriminator half 1 (marimo signature, not just "jupyter could fake
    // this too"): the injected include-in-header content from extract.py's
    // header construction (293-327) — the trust marker script and/or the
    // hidden notebook-source tag.
    assert!(
        html.contains("__MARIMO_EXPORT_CONTEXT__") || html.contains("<marimo-code"),
        "rendered HTML must contain a marimo-specific signature \
         (__MARIMO_EXPORT_CONTEXT__ or <marimo-code, injected via \
         include-in-header) — proves marimo actually owned and executed the \
         cell, not some other engine; got:\n{}",
        body_excerpt(&html)
    );
    // Discriminator half 2: the executed cell result `2`, asserted on the
    // specific executed-output markup (mirroring julia J1's style) rather
    // than a bare digit anywhere in the page chrome.
    assert!(
        html.contains("<marimo-cell-output>") && html.contains(">2<"),
        "rendered HTML must contain the marimo cell's executed output \
         (<marimo-cell-output>...>2<...) — the evaluated `1 + 1` result; \
         got:\n{}",
        body_excerpt(&html)
    );
}

// ── Phase 4cB2 deliverable 2: dynamic-path parity, minimal doc ─────────────
//
// Renders the SAME `minimal.qmd` content SC8 renders (byte-identical
// `MINIMAL_DOC`, python-only, no sql) through the CLAIMS-LESS fixture
// variant (`setup_marimo_project_dynamic`, `_extension.yml`'s `claims:` map
// dropped) — forcing `TsEngine::claims_language`'s legacy DYNAMIC path
// (`ts_engine.rs:668` else-branch: `ensure_loaded` + a live `ClaimsLanguage`
// wire call) instead of SC8's zero-load static-claims path. This is the
// first end-to-end load→ask→resolve with a real engine (SC8 never loads the
// engine module at all for ownership purposes — the static `claims:` map
// answers `claims_language` with no subprocess).
//
// Asserts the SAME outcome as SC8: render succeeds, and both the marimo
// signature and the executed `2` result are present. Parity holds here
// because a python-only doc's ownership question ("does `{python .marimo}`
// belong to marimo?") is answered identically by both paths — the static
// `whenClass: marimo` claim and the live `claimsLanguage("python","marimo")`
// wire call both say `Primary(2)` (see the static-claims design table,
// plan file §"Static-claims design (Option B)"). This row does NOT exercise
// the bare-sql interop feature — see the SC9 section below for that (and
// why it is NOT a committed passing test yet).
#[test]
fn p4cb2_dynamic_path_parity_minimal_render_matches_static() {
    if !deno_available() {
        eprintln!(
            "SKIP: deno not on PATH — p4cb2_dynamic_path_parity_minimal_render_matches_static"
        );
        return;
    }
    if !uv_available() {
        eprintln!("SKIP: uv not on PATH — p4cb2_dynamic_path_parity_minimal_render_matches_static");
        return;
    }
    let tmp = setup_marimo_project_dynamic();
    let input = tmp.path().join("minimal.qmd");
    write_file(&input, MINIMAL_DOC);

    let html = render_html(&input).expect("marimo render (dynamic claims path) must succeed");

    assert!(
        html.contains("__MARIMO_EXPORT_CONTEXT__") || html.contains("<marimo-code"),
        "dynamic-path render must contain a marimo-specific signature, matching \
         SC8's static-path result; got:\n{}",
        body_excerpt(&html)
    );
    assert!(
        html.contains("<marimo-cell-output>") && html.contains(">2<"),
        "dynamic-path render must contain the marimo cell's executed output \
         (<marimo-cell-output>...>2<...), matching SC8's static-path result; \
         got:\n{}",
        body_excerpt(&html)
    );
}

// ── SC9: bare-sql interop, dynamic path (Phase 4cB2) ───────────────────────
//
// SC9's frozen row (Test Seam Spec, plan file): render a
// `{python .marimo}` + bare `{sql}` doc via the claims-less fixture, assert
// marimo (not jupyter, not nobody) executed the sql cell — dynamic parity
// with the static path's `Interop` claim on bare `sql`.
//
// HISTORY (full trail in `.superpowers/sdd/task-4cB2-report.md` and
// compat doc §13): first attempt reported NEEDS_CONTEXT. The brief's Risk-1
// evidence-first procedure found the anticipated vacuity (`ownership={}`
// via `EngineClaimsFileStage`'s whole-file `claims_file` short-circuit when
// `claims-files:` is absent — SC8's own BLOCKING FINDING #3), AND a second,
// independent, previously-undiscovered defect even after applying the
// pre-authorized `claims-files: []` fix: `marimo-engine.ts`'s `execute()`
// (`bareSqlOwned = (options.handledLanguages ?? []).includes("sql")`) and
// `lib/is-marimo-cell.ts`'s `cellOwnedByMarimo` read the wire
// `handledLanguages` (q2-core's tested LEAVE-ALONE set — excludes languages
// the engine itself owns, confirmed by the pre-existing
// `jupyter/text_execute.rs:600-655` unit test) as if it were a POSITIVE
// "assigned to me" set — so the gate evaluated `false` exactly when marimo
// correctly owned bare sql, and the sql cell was never spliced as executed
// despite `ownership["sql"]=="marimo"` being correct at the resolver level.
//
// FINDING #4 fix (task 4cB2-fix, 2026-07-02/03, controller-ratified,
// reviewer-reproduced): both call sites flipped to the complement,
// `!handledLanguages.includes("sql")` — sound because q2's resolver assigns
// every language present in the document an owner, or hard-fails, before
// `execute()` ever runs, so "absent from the leave-alone set" and "owned by
// me" coincide for any language the engine has an actual decision to make
// about. Fixed upstream in `~/src/quarto-marimo` (`77c15c8`), copied
// byte-identical into this fixture's `src/marimo-engine.ts` +
// `lib/is-marimo-cell.ts`, and rebundled into `marimo-engine.js`
// (`crates/quarto-core/src/engine/ts_protocol.rs`'s
// `TsExecuteOptions::handled_languages` now documents the leave-alone
// contract so the next TS-engine author doesn't repeat this). This test
// (added after the fix landed) is the committed SC9 proof.
//
// ## Chosen observable
// `ownership["sql"]` is not directly observable from `render_to_file`'s
// public surface (no hook returns `EngineResolution`), and neither of the
// brief's other two candidates panned out (the `engine_resolution` tracing
// event logs only `engine_count`; the `-v` "resolved engine sequence" DEBUG
// line logs only engine names, never per-language ownership). So this test
// uses the brief's third candidate — the observable BEHAVIORAL PROXY: does
// the rendered HTML show marimo's executed island for the sql cell (a
// `<marimo-table>` with `data-data` carrying the computed row `{"x":2}`,
// inside a `<marimo-cell-output>`) vs. a plain, unexecuted code block? This
// also doubles as the RED discriminator: reverting the claim makes the
// WHOLE RENDER fail (jupyter unavailable), not just this one assertion —
// see the RED-by-revert note on the test below.
#[test]
fn sc9_bare_sql_interop_dynamic_path_marimo_executes_sql_cell() {
    if !deno_available() {
        eprintln!(
            "SKIP: deno not on PATH — sc9_bare_sql_interop_dynamic_path_marimo_executes_sql_cell"
        );
        return;
    }
    if !uv_available() {
        eprintln!(
            "SKIP: uv not on PATH — sc9_bare_sql_interop_dynamic_path_marimo_executes_sql_cell"
        );
        return;
    }
    let tmp = setup_marimo_project_dynamic();
    let input = tmp.path().join("sql_interop.qmd");
    write_file(&input, SQL_INTEROP_DOC);

    let html =
        render_html(&input).expect("marimo render (bare-sql interop, dynamic path) must succeed");

    // Discriminator half 1 (python cell, same as SC8/the 4cB2 parity test):
    // marimo-specific signature + the executed `1 + 1` result.
    assert!(
        html.contains("__MARIMO_EXPORT_CONTEXT__") || html.contains("<marimo-code"),
        "rendered HTML must contain a marimo-specific signature; got:\n{}",
        body_excerpt(&html)
    );
    assert!(
        html.contains("<marimo-cell-output>") && html.contains(">2<"),
        "rendered HTML must contain the marimo python cell's executed output \
         (<marimo-cell-output>...>2<...); got:\n{}",
        body_excerpt(&html)
    );

    // Discriminator half 2 (the SC9-specific proof): the bare `{sql}` cell
    // was executed BY MARIMO, not left as a plain code block. Asserted on
    // marimo's own island markup carrying the computed query result —
    // firsthand-inspected literal substring from a real render:
    // `data-data='&quot;[{&#92;&quot;x&#92;&quot;:2}]&quot;'` (HTML-entity-
    // escaped JSON `[{"x":2}]`, the `SELECT 1 + 1 AS x` result).
    assert!(
        html.contains("<marimo-table")
            && html.contains("data-data=")
            && html.contains("x&#92;&quot;:2"),
        "rendered HTML must contain marimo's executed sql-query island \
         (<marimo-table ... data-data='...\"x\":2...'>) — proves marimo, not \
         jupyter and not nobody, owns and executes the bare `{{sql}}` cell \
         via Option B Interop; got:\n{}",
        body_excerpt(&html)
    );
}

// RED-by-revert for `sc9_bare_sql_interop_dynamic_path_marimo_executes_sql_cell`
// (named revert per the frozen Test Seam Spec row): in a TEMPDIR-ONLY copy of
// the fixture bundle (never the committed one — `git diff` on
// `crates/quarto-core/tests/fixtures/extensions/marimo/` stayed clean
// throughout), reverted `marimo-engine.js`'s `claimsLanguage`'s bare-sql
// branch from `{ kind: "interop" }` to `return false;`. Re-rendering the
// SAME `SQL_INTEROP_DOC` through the SAME claims-less + `claims-files: []`
// variant then fails the render OUTRIGHT (captured verbatim):
//
//   Error: Engine 'jupyter' is registered but its runtime is not available. Install the required runtime to render this document, or remove the engine declaration from the front matter.
//
//   1 error
//
// (sql falls through every tier once marimo's Interop claim is gone — T3
// finds no present engine's Interop claim on sql (knitr's own Interop claim
// on sql doesn't count: knitr is never "present", no `{r}` cell in this
// doc) — T4 implicit-Fallback lands on jupyter, unavailable on this
// machine, the same hard-failure mode SC8's own RED proof uses). Restored
// byte-identical afterward; this test re-confirmed GREEN against the
// pristine fixture immediately after (see Verification in the task
// report).

// ── SC10: render + `include-in-header` wiring (Phase 4cC) ─────────────────
//
// SC10's frozen row (Test Seam Spec, plan file): render a `mo.ui`/plot doc →
// grep HTML for header content + raw-`{=html}` figure. Uses the SAME static
// claims path as SC8 (`setup_marimo_project`, committed fixture, no
// `_extension.yml` rewrite) — the row's gate is marimo/uv, not the dynamic
// claims path SC9 exercises, and the widget doc has no bare-sql cell to
// force that path anyway.
//
// The committed `widget.qmd` fixture (also present as a standalone file at
// the marimo fixture root, mirroring `minimal.qmd`) is ONE `{python .marimo}`
// cell calling `mo.ui.slider(1, 10, value=5)` — a pure `mo.ui` widget, no
// matplotlib/altair, keeping deps at plain marimo (warm in the uv cache).
//
// Manual end-to-end render (repo rule) confirmed the exact success surface
// before this test was written: `target/release/q2 render widget.qmd` from a
// scratch copy of the fixture (`_extensions/marimo/` + `widget.qmd`, no
// `_quarto.yml`, mirroring the test harness's own `setup_marimo_project`)
// produced (verbatim, `<head>` then `<body>`):
//
//   <script>
//     Object.defineProperty(window, "__MARIMO_EXPORT_CONTEXT__", {
//   ...
//   </script><marimo-code hidden>import%20marimo%0A...</marimo-code><script type="module" src="https://cdn.jsdelivr.net/npm/@marimo-team/islands@0.23.13/dist/main.js"></script>
//   ...
//   <marimo-island
//       data-app-id="main"
//       data-cell-id="Hbol"
//       data-reactive="true"
//   >
//       <marimo-cell-output>
//       <marimo-ui-element object-id='Hbol-0' random-id='...'><marimo-slider data-initial-value='5' data-label='null' data-start='1' data-stop='10' .../></marimo-ui-element>
//       </marimo-cell-output>
//       <marimo-cell-code hidden>import%20marimo%20as%20mo%0Amo.ui.slider(1%2C%2010%2C%20value%3D5)</marimo-cell-code>
//   </marimo-island>
//
// i.e. the header markers land in `<head>` via `includes["include-in-header"]`
// (a `PandocIncludes` temp file the engine writes `marimoExecution.header`
// into — `marimo-engine.js`'s bundled `execute()`, corresponding to upstream
// `marimo-engine.ts` ~300-310), and the widget lands in `<body>` as the raw
// `{=html}` island markup `render-output.ts`'s non-mime-sensitive branch
// emits verbatim (a Pandoc `RawBlock` — no `store_html_dependencies` path,
// no DOM postprocessor). `generatesFigures: true` is NOT asserted (no q2
// consumer, plan correction 6).
#[test]
fn sc10_widget_render_shows_header_include_and_body_island() {
    if !deno_available() {
        eprintln!(
            "SKIP: deno not on PATH — sc10_widget_render_shows_header_include_and_body_island"
        );
        return;
    }
    if !uv_available() {
        eprintln!("SKIP: uv not on PATH — sc10_widget_render_shows_header_include_and_body_island");
        return;
    }
    let tmp = setup_marimo_project();
    let input = tmp.path().join("widget.qmd");
    write_file(&input, WIDGET_DOC);

    let html = render_html(&input).expect("marimo widget render must succeed");

    // Discriminator half 1 (header-content marker, the `include-in-header`
    // sink this row binds): same markers SC8 uses, since both are injected
    // by the SAME `includes["include-in-header"]` splice — the trust-marker
    // script and/or the hidden notebook-source tag, both only ever emitted
    // into `<head>` (confirmed in the manual render above: neither marker
    // appears anywhere in `<body>`).
    assert!(
        html.contains("__MARIMO_EXPORT_CONTEXT__") || html.contains("<marimo-code"),
        "rendered HTML must contain header content routed via \
         includes[\"include-in-header\"] (__MARIMO_EXPORT_CONTEXT__ or \
         <marimo-code); got:\n{}",
        body_excerpt(&html)
    );
    // Discriminator half 2 (the SC10-specific proof): the widget's raw
    // `{=html}` island output actually reached the body — marimo's own
    // `<marimo-island>` wrapping a `<marimo-ui-element>` (the slider), not a
    // plain unexecuted code block.
    assert!(
        html.contains("<marimo-island") && html.contains("<marimo-ui-element"),
        "rendered HTML must contain the widget's raw {{=html}} island output \
         (<marimo-island>...<marimo-ui-element>...) — proves the mo.ui widget \
         was executed and spliced into the body, not left as source; \
         got:\n{}",
        body_excerpt(&html)
    );
}

// RED-by-revert for `sc10_widget_render_shows_header_include_and_body_island`
// (named revert per the frozen Test Seam Spec row): in a TEMPDIR-ONLY copy of
// the fixture bundle (never the committed one — `git diff` on
// `crates/quarto-core/tests/fixtures/extensions/marimo/` stayed clean
// throughout), neutered the engine's `include-in-header` population in
// `marimo-engine.js`'s bundled `execute()` (corresponding to upstream
// `marimo-engine.ts` ~300-310):
//
//   -  if (outputFormat === "html" && marimoExecution.header) {
//   +  if (false && outputFormat === "html" && marimoExecution.header) {
//
// so the engine unconditionally returns no `include-in-header` entry (the
// `includes` object stays empty, `includes: void 0` on the return value).
// Re-rendering the SAME `WIDGET_DOC` through the SAME static-claims fixture
// then still SUCCEEDS (unlike SC8/SC9's revert, this one does not touch
// ownership/resolution — marimo still owns and executes the cell), but the
// rendered `<head>` no longer contains either header marker while `<body>`
// still contains the widget's `<marimo-island>`/`<marimo-ui-element>` island
// — i.e. discriminator half 1 fails, half 2 still passes, reproducing
// exactly the conjunctive-assertion failure this row's test is written to
// catch. Captured verbatim (grep over the reverted render's output, in
// place of the SC8/SC9-style hard render failure — this revert doesn't
// error the render, it silently drops the header sink):
//
//   $ grep -n "MARIMO_EXPORT_CONTEXT\|marimo-code\|marimo-island\|marimo-ui-element\|<head\|</head" widget.html
//   3:<head>
//   12:</head>
//   ...
//   25:<marimo-island
//   ...
//   31:    <marimo-ui-element object-id='...' ...><marimo-slider .../></marimo-ui-element>
//   34:</marimo-island>
//
// i.e. `<head>` (lines 3-12) contains neither `__MARIMO_EXPORT_CONTEXT__` nor
// `<marimo-code` — discriminator half 1's `assert!` would panic with this
// test's own message ("rendered HTML must contain header content routed via
// includes[\"include-in-header\"]...") — while `<marimo-island>` /
// `<marimo-ui-element>` (lines 25-34) are still present, confirming half 2
// alone is not sufficient to catch a broken `include-in-header` wiring (the
// reason this row's assertions are conjunctive, not either/or). Fixture
// restored byte-identical afterward (`git diff` clean); this test
// re-confirmed GREEN against the pristine fixture immediately after (see
// Verification in the task report).

// ── SC13-e2e: tagged-sql self-activation renders (Phase 4cD) ───────────────
//
// The plan's 4cD checklist item "sql-only-tagged self-activation (Q1
// parity): a doc with only `{sql .marimo}` / `{sql.marimo}` (no python)
// activates marimo and renders." The int-rs half
// (`marimo_resolution.rs::sc13_tagged_sql_self_activation`) already proves
// `ownership["sql"]=="marimo"` for a doc with ZERO python cells at all,
// through the real resolver. This row proves the remaining half: that the
// self-activated marimo actually EXECUTES the tagged sql cells (renders,
// not just resolves) — through the COMMITTED static fixture (no
// `_extension.yml` rewrite), covering both the space-form `{sql .marimo}`
// and the dotted-form `{sql.marimo}` in one doc (folding in the dotted-form
// render proof per the brief), with distinct queries (1+1, 2+2) so the two
// cells' outputs are individually distinguishable — not just "some sql cell
// executed somewhere".
//
// See `TAGGED_SQL_ONLY_DOC`'s doc comment for why a companion
// `{python .marimo}` import-only cell is included: marimo's `mo.sql()`
// codegen needs `mo` bound by an explicit `import marimo as mo` cell to
// resolve at runtime — confirmed empirically. A first attempt at a
// genuinely python-cell-free sql-only doc rendered SUCCESSFULLY (exit 0,
// valid HTML) but both sql cells' islands came back as marimo error
// mime-renderers, not executed tables:
//
//   <marimo-mime-renderer data-mime='&quot;application/vnd.marimo+error&quot;' data-data='[{&quot;type&quot;:&quot;exception&quot;,&quot;msg&quot;:&quot;name &#x27;mo&#x27; is not defined&quot;,&quot;exception_type&quot;:&quot;NameError&quot;,&quot;raising_cell&quot;:null,&quot;traceback&quot;:null}]'></marimo-mime-renderer>
//
// This is marimo's own notebook-execution model (a `mo.sql` cell's `mo`
// parameter is dependency-injected from another cell's return value — no
// cell defines it, no auto-import), not a q2 defect; the int-rs test above
// already covers the true zero-python resolution claim, so this e2e row
// adds the minimal import cell needed to actually observe execution.
#[test]
fn sc13_e2e_tagged_sql_self_activation_renders() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — sc13_e2e_tagged_sql_self_activation_renders");
        return;
    }
    if !uv_available() {
        eprintln!("SKIP: uv not on PATH — sc13_e2e_tagged_sql_self_activation_renders");
        return;
    }
    let tmp = setup_marimo_project();
    let input = tmp.path().join("sql_only.qmd");
    write_file(&input, TAGGED_SQL_ONLY_DOC);

    let html = render_html(&input).expect("tagged-sql-only marimo render must succeed");

    assert!(
        html.contains("__MARIMO_EXPORT_CONTEXT__") || html.contains("<marimo-code"),
        "rendered HTML must contain a marimo-specific signature; got:\n{}",
        body_excerpt(&html)
    );
    // The `{sql .marimo}` cell's executed island (SELECT 1 + 1 AS x -> x:2).
    assert!(
        html.contains("<marimo-table") && html.contains("x&#92;&quot;:2"),
        "rendered HTML must contain the `{{sql .marimo}}` cell's executed island \
         (SELECT 1 + 1 AS x -> x:2); got:\n{}",
        body_excerpt(&html)
    );
    // The `{sql.marimo}` (dotted-form) cell's executed island (SELECT 2 + 2
    // AS x -> x:4) — distinct value proves THIS cell, not a duplicate of the
    // first, actually executed.
    assert!(
        html.contains("x&#92;&quot;:4"),
        "rendered HTML must contain the `{{sql.marimo}}` (dotted-form) cell's \
         executed island (SELECT 2 + 2 AS x -> x:4); got:\n{}",
        body_excerpt(&html)
    );
}

// RED-by-revert for `sc13_e2e_tagged_sql_self_activation_renders` (named
// revert per the brief, mirroring SC8's corrected anti-vacuity pattern): in
// a TEMPDIR-ONLY copy of the fixture bundle (never the committed one —
// `git diff` on `crates/quarto-core/tests/fixtures/extensions/marimo/`
// stayed clean throughout), rewrote `_extension.yml`'s `claims:` map to
// DROP the entire `sql:` key (both its whenClass-primary AND interop
// entries — see the anti-vacuity note below for why BOTH must go) and the
// `"sql.marimo":` key, keeping only `python:` / `"python.marimo":`, and
// ADDED `claims-files: []`:
//
//   contributes:
//     engines:
//       - path: marimo-engine.js
//         name: marimo
//         claims:
//           python:
//             - { whenClass: marimo, kind: primary, priority: 2 }
//           "python.marimo":
//             - { kind: primary, priority: 1 }
//         claims-files: []
//
// Anti-vacuity note (why the interop entry must ALSO go, not just the
// whenClass-primary one): `TAGGED_SQL_ONLY_DOC`'s companion
// `{python .marimo}` import cell keeps marimo "present" (it still owns
// python). If the bare interop claim `{ kind: interop }` were left on `sql`
// after removing only the whenClass-primary entry, a `{sql .marimo}` cell's
// combined claim would still resolve through T3 presence-gating (marimo
// already present via python) — the self-activation-specific whenClass
// mechanism would be untested and the revert would be VACUOUS (mirrors the
// SC1/SC13/SC14/SC15 vacuity notes elsewhere in this plan). Dropping the
// WHOLE `sql:` key removes both paths at once, forcing a genuine failure.
//
// `claims-files: []` is required for the SAME reason as SC8's corrected
// revert (BLOCKING FINDING #3): without it, the committed fixture's
// DYNAMIC, content-inspecting `claims_file` (`ts_engine.rs:716-717`) still
// finds a `.marimo` fence anywhere in the file and whole-file-claims it via
// `EngineClaimsFileStage`, bypassing the per-language `claims:` map (and
// the python claim) entirely — vacuous without this line.
//
// Re-rendering the SAME `TAGGED_SQL_ONLY_DOC` through this reverted tempdir
// bundle then fails the render OUTRIGHT (captured verbatim):
//
//   Error: Engine 'jupyter' is registered but its runtime is not available. Install the required runtime to render this document, or remove the engine declaration from the front matter.
//
//   1 error
//
// (both sql cells fall through every tier once their claim keys are gone —
// no engine present claims `sql`/`sql.marimo` — landing on jupyter's
// implicit Fallback, unavailable on this machine, the same hard-failure
// mode SC8/SC9's own RED proofs use). Fixture restored byte-identical
// afterward; this test re-confirmed GREEN against the pristine fixture
// immediately after.

// ── sc14-e2e-static: sql-interop executes on the STATIC path (Phase 4cD) ───
//
// The plan's 4cD checklist item "sql-interop when present (the new
// feature): doc with `{python .marimo}` + bare `{sql}` -> marimo owns both
// python (Primary) and sql (Interop); both execute via marimo." SC9
// (`sc9_bare_sql_interop_dynamic_path_marimo_executes_sql_cell`) already
// proves this on the DYNAMIC claims-less path; this row closes the
// remaining gap by proving the SAME behavior on the STATIC path — the
// COMMITTED fixture's `claims:` map, unmodified, no `_extension.yml`
// rewrite (the configuration SC8/SC10/SC13 all use, and the one a real
// extension author ships by default).
#[test]
fn sc14_e2e_static_sql_interop_both_execute_via_marimo() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — sc14_e2e_static_sql_interop_both_execute_via_marimo");
        return;
    }
    if !uv_available() {
        eprintln!("SKIP: uv not on PATH — sc14_e2e_static_sql_interop_both_execute_via_marimo");
        return;
    }
    let tmp = setup_marimo_project();
    let input = tmp.path().join("sql_interop_static.qmd");
    write_file(&input, SQL_INTEROP_STATIC_DOC);

    let html =
        render_html(&input).expect("marimo render (bare-sql interop, static path) must succeed");

    // Discriminator half 1 (python cell, same markers SC8/SC9 use).
    assert!(
        html.contains("__MARIMO_EXPORT_CONTEXT__") || html.contains("<marimo-code"),
        "rendered HTML must contain a marimo-specific signature; got:\n{}",
        body_excerpt(&html)
    );
    assert!(
        html.contains("<marimo-cell-output>") && html.contains(">2<"),
        "rendered HTML must contain the marimo python cell's executed output \
         (<marimo-cell-output>...>2<...); got:\n{}",
        body_excerpt(&html)
    );
    // Discriminator half 2: the bare `{sql}` cell executed BY MARIMO on the
    // STATIC path — same island-markup proof SC9 uses on the dynamic path.
    assert!(
        html.contains("<marimo-table")
            && html.contains("data-data=")
            && html.contains("x&#92;&quot;:2"),
        "rendered HTML must contain marimo's executed sql-query island \
         (<marimo-table ... data-data='...\"x\":2...'>) on the STATIC claims \
         path — proves marimo executes both python (Primary) and bare sql \
         (Interop) without any `_extension.yml` rewrite; got:\n{}",
        body_excerpt(&html)
    );
}

// RED-by-revert for `sc14_e2e_static_sql_interop_both_execute_via_marimo`.
// Per the brief: the SC4-style tempdir bundle revert (flip `claimsLanguage`'s
// bare-sql branch to `false`) is NOT the right hunk for the STATIC path —
// static resolution never calls the live `claimsLanguage` at all (zero-load
// short-circuit), so that revert would be vacuous here. Instead, the row's
// revert targets the LEAVE-ALONE GATE inside `execute()` itself: in a
// TEMPDIR-ONLY copy of the fixture bundle, flipped
// `marimo-engine.js`'s (bundled `marimo-engine.ts` ~line 222):
//
//   -  const bareSqlOwned = !(options.handledLanguages ?? []).includes("sql");
//   +  const bareSqlOwned = (options.handledLanguages ?? []).includes("sql");
//
// i.e. restored the PRE-FINDING-#4 inverted sense (this directly re-binds
// the finding-#4 fix at the e2e level, per the brief). Re-rendering the SAME
// `SQL_INTEROP_STATIC_DOC` through the SAME static-claims fixture then still
// SUCCEEDS (unlike SC8/SC9/SC13's revert, this one does not touch
// ownership/resolution — the whole-file claims_file short-circuit still
// hands the WHOLE render to marimo, so there's no second engine to fall
// back to), but the sql cell is spliced back UNEXECUTED — its raw source,
// verbatim, with the tell-tale literal-braces class marker a fenced cell
// gets when NO engine executes it:
//
//   <pre class="{sql} code-with-copy"><code>SELECT 1 + 1 AS x</code></pre>
//
// — i.e. discriminator half 2's `<marimo-table>`/`data-data`/`x":2` assertion
// fails while half 1 (the python cell) still passes, reproducing exactly the
// conjunctive-assertion failure this row is written to catch. Fixture
// restored byte-identical afterward; this test re-confirmed GREEN against
// the pristine fixture immediately after.
//
// Dated annotation (2026-07-03, task 4cD-e2e, controller pre-authorized):
// the brief's own text already anticipated this revert-hunk substitution
// (the SC4-style revert is explicitly named as "NOT the right hunk here");
// this comment records the verbatim RED evidence for it.

// ── SC16-e2e: two-engine coexistence — marimo + knitr (Phase 4cD) ──────────
//
// The plan's 4cD checklist item "Two-engine single-doc coexistence via
// distinct languages: `{python .marimo}` + a second language the *available*
// engine genuinely executes -> marimo owns python, the other engine owns the
// other language; each executes only its owned cells (`handled_languages`,
// §5)." The int-rs half
// (`marimo_resolution.rs::sc16_coexistence_handled_languages_leave_alone_semantics`)
// already proves BOTH engines' `handled_languages_for` values via the real
// resolver (no render). This row proves the remaining "each executes only
// its owned cells" runtime half, using `{r}` + the real builtin
// `KnitrEngine` (Rscript + knitr package both present on this machine —
// gated on both, per the brief; a skip here is a signal something is
// wrong, not a pass).
//
// Dated annotation (2026-07-03, task 4cD-e2e, discovered during this task —
// documented here per the brief's pre-authorization to annotate this row):
// **rendering through the COMMITTED static fixture (as the brief's row
// text originally specifies) does NOT actually exercise two-engine
// coexistence.** Empirically confirmed: the committed fixture declares no
// `claims-files:` key, so — exactly as SC8's BLOCKING FINDING #3 already
// documented for the ownership dimension — `EngineClaimsFileStage`'s
// DYNAMIC, content-inspecting `claims_file` finds the `.marimo` fence
// anywhere in a `{python .marimo}` + `{r}` doc and whole-file-claims the
// ENTIRE render for marimo (`ctx.claimed_engine_name = Some("marimo")`),
// which `resolve_engines` (`resolution.rs`'s `claimed` short-circuit) turns
// into a sequence of EXACTLY `[marimo]` — knitr never runs at all. Verified
// firsthand: rendering `{python .marimo}` (`1 + 1`) + `{r}` (`1 + 1`)
// through the unmodified committed fixture produces
// `<pre class="{r} code-with-copy"><code>1 + 1</code></pre>` in the body —
// the r cell's raw, UNEXECUTED source, not knitr's `[1] 2` output. This is
// the SAME mechanism as SC8's finding, now shown to break coexistence e2e
// too, not just ownership vacuity.
//
// Following the SAME already-ratified anti-vacuity fix this plan uses
// throughout (SC8's corrected revert, SC9/SC14's dynamic-claims-less
// fixture derivation): this test uses `setup_marimo_project_dynamic()`
// (drops `claims:`, adds `claims-files: []`) instead of the row's literal
// "committed static fixture" wording. This is not inventing a new
// deviation — it reuses machinery this very file already established and
// the controller already ratified for the identical defect (SC9/4cB2's
// `write_claims_less_extension_yml`). Verified firsthand that this fixture
// variant DOES produce genuine two-engine coexistence: marimo executes the
// python cell (island output `2`) AND knitr executes the r cell (source +
// `[1] 2` output), with no unexecuted-fence marker for either language.
#[test]
fn sc16_e2e_marimo_knitr_coexistence_leaves_each_other_alone() {
    if !deno_available() {
        eprintln!(
            "SKIP: deno not on PATH — sc16_e2e_marimo_knitr_coexistence_leaves_each_other_alone"
        );
        return;
    }
    if !uv_available() {
        eprintln!(
            "SKIP: uv not on PATH — sc16_e2e_marimo_knitr_coexistence_leaves_each_other_alone"
        );
        return;
    }
    if !rscript_available() {
        eprintln!(
            "SKIP: Rscript not on PATH — sc16_e2e_marimo_knitr_coexistence_leaves_each_other_alone \
             (environment fact: this machine is expected to have Rscript — a skip here is a \
             signal, not a pass)"
        );
        return;
    }
    if !knitr_r_package_available() {
        eprintln!(
            "SKIP: R package 'knitr' not installed — \
             sc16_e2e_marimo_knitr_coexistence_leaves_each_other_alone (environment fact: this \
             machine is expected to have knitr — a skip here is a signal, not a pass)"
        );
        return;
    }
    let tmp = setup_marimo_project_dynamic();
    let input = tmp.path().join("coexist.qmd");
    write_file(&input, COEXIST_PYTHON_R_DOC);

    let html = render_html(&input).expect("marimo+knitr coexistence render must succeed");

    // marimo executed its own owned python cell (same markers SC8/SC9/SC14
    // use for marimo's executed output).
    assert!(
        html.contains("<marimo-cell-output>") && html.contains(">2<"),
        "rendered HTML must contain marimo's executed python cell output \
         (<marimo-cell-output>...>2<...); got:\n{}",
        body_excerpt(&html)
    );
    // knitr executed the r cell it owns — its own executed-output markup
    // (`[1] 2`, matching `knitr/mod.rs::test_engine_execute_basic`'s own
    // assertion style).
    assert!(
        html.contains("[1] 2"),
        "rendered HTML must contain knitr's executed r cell output ([1] 2); \
         got:\n{}",
        body_excerpt(&html)
    );
    // Neither engine's output contains the other's (or its own) cell
    // content UNEXECUTED. The tell-tale marker for a fenced cell that fell
    // through with NO engine executing it is a literal, brace-including
    // `class="{lang}"` attribute from the raw pandoc code-block writer
    // (confirmed absent from every clean render in this file; confirmed
    // present — see the RED-by-revert note below — when ownership
    // enforcement breaks).
    assert!(
        !html.contains("class=\"{python}"),
        "rendered HTML must NOT contain an unexecuted-python-fence marker \
         (class=\"{{python}}\") — would indicate marimo's own owned cell fell \
         through unexecuted; got:\n{}",
        body_excerpt(&html)
    );
    assert!(
        !html.contains("class=\"{r}"),
        "rendered HTML must NOT contain an unexecuted-r-fence marker \
         (class=\"{{r}}\") — would indicate the r cell was left unexecuted by \
         both engines (enforcement failure); got:\n{}",
        body_excerpt(&html)
    );
}

// RED-by-revert for `sc16_e2e_marimo_knitr_coexistence_leaves_each_other_alone`.
//
// Dated annotation (2026-07-03, task 4cD-e2e): the brief offered two
// candidate single edits — "widen its cell regex split gate" or "neuter
// cellOwnedByMarimo's handled check for r" — as alternatives, expecting
// either to make "the r cell gets mangled/eaten by marimo". BOTH were
// empirically tried, in a TEMPDIR-ONLY copy of the fixture bundle, and
// BOTH turned out VACUOUS (no observable difference from the golden
// render):
//
//   1. Neutering ONLY the sql-specific handled check inside
//      `cellOwnedByMarimo` (`return cell.cell_type.language === "sql" && ...`
//      -> `return true;`, but ONLY inside that one branch — unreachable for
//      "r" anyway, since marimo's cell-split regex never recognizes a bare
//      `{r}` fence as a distinct "code" cell in the first place; it stays
//      folded into a surrounding "markdown"-typed cell, whose
//      `sourceVerbatim` is pushed back byte-identical regardless of the
//      language check). Re-rendering: IDENTICAL output to the golden render.
//   2. Widening `MARIMO_CELL_REGEX_WITH_BARE_SQL`'s capture group to
//      `(?:python|sql|r)` (making the r fence a distinct "code" cell) TOGETHER
//      WITH adding an explicit `if (cell.cell_type.language === "r") return
//      true;` branch. Because `marimoExecution.outputs` has exactly one
//      entry (the one real python cell), the "owned" r cell hits the
//      "no corresponding output" warning branch and pushes back
//      `cell.sourceVerbatim.value` — which, reconstructed from the SAME
//      source ranges, is byte-identical to the original fence text. Also
//      IDENTICAL output to the golden render.
//
// Root cause both attempts share: there is no SPARE `marimoExecution.output`
// entry available to inject wrong content for `r` — the fallback path
// always reconstructs the exact original text, so over-claiming a cell that
// has no real output for it is silently harmless to that cell's own
// content.
//
// The actual OBSERVABLE revert found (single edit, unconditional): neutered
// `cellOwnedByMarimo` to ignore cell type/language entirely:
//
//   -  function cellOwnedByMarimo(cell, handledLanguages) {
//   -    if (isMarimoCell(cell)) {
//   -      return true;
//   -    }
//   -    if (typeof cell.cell_type !== "object" || !("language" in cell.cell_type)) {
//   -      return false;
//   -    }
//   -    return cell.cell_type.language === "sql" && !handledLanguages.includes("sql");
//   -  }
//   +  function cellOwnedByMarimo(cell, handledLanguages) {
//   +    return true;
//   +  }
//
// This makes marimo over-claim EVERY cell in the doc — including the YAML
// front-matter "raw" cell, which is now first in iteration order and
// consumes the ONLY available `marimoExecution.outputs` entry (meant for
// the real python cell). The real python cell then hits the "no
// corresponding output" branch and is spliced back as RAW, UNEXECUTED
// SOURCE instead of its rendered result. Re-rendering the SAME
// `COEXIST_PYTHON_R_DOC` through this reverted tempdir bundle still
// SUCCEEDS (exit 0) but the body now contains, verbatim:
//
//   <pre class="{python} code-with-copy"><code>import marimo as mo
//   1 + 1</code></pre>
//
// alongside the correctly-executed marimo island output — i.e. the "neither
// engine's output contains ... cell content unexecuted" assertion reddens
// (the `!html.contains("class=\"{python}")` check fails) even though the
// specific mechanism is marimo's OWN cell being mangled by its broken
// ownership enforcement, rather than literally cannibalizing knitr's `{r}`
// cell — a stronger, still-legitimate proof that over-claiming breaks the
// enforcement invariant this row exists to catch (the title block also
// disappears in this revert, a further corruption symptom, not asserted on
// here). Fixture restored byte-identical afterward; this test re-confirmed
// GREEN against the pristine fixture immediately after.

// ── SC18: execute() catch shows an error marker, not a crash (Phase 4cD) ───
//
// SC18's frozen row (Test Seam Spec, plan file): render a syntactically-bad
// marimo cell -> error-marked output substring, gated on marimo/uv, proving
// `execute()`'s try/catch (marimo-engine.ts ~319-329) converts a subprocess
// failure into "Error executing marimo" output rather than propagating it
// and hard-failing the render.
//
// Dated annotation (2026-07-03, task 4cD-e2e): the brief's suggested
// trigger — a cell body with a Python `SyntaxError` (e.g. `def (:`) — was
// tried FIRST and found NOT to reach this catch. Rendering a
// `{python .marimo}` cell containing `def (:` SUCCEEDS (exit 0) and
// produces, in the body:
//
//   <pre class="marimo-error">SyntaxError: invalid syntax (<unknown>, line 1)</pre>
//
// This is marimo's OWN per-cell parse-error isolation (`extract.py`'s
// `_ParseError` sentinel, wrapping each `app.add_code(...)` call in its own
// `try`/`except Exception`, documented in `extract.py`'s own doc comment as
// existing precisely "to surface parse-time exceptions... that would
// otherwise be swallowed"): the failure is caught INSIDE the Python
// subprocess, per-cell, before the subprocess ever exits non-zero — so
// `executePython`'s `if (!output.success) throw ...` never fires, and
// `execute()`'s OUTER try/catch (the actual unit this row is about) is
// never reached at all. The row's suggested trigger and its actual target
// unit are mismatched.
//
// The alternate trigger used instead — an unresolvable `pyproject`
// dependency name (`this-package-definitely-does-not-exist-q2-sc18-test-xyz`)
// — reaches the SAME code path the row names: `uv run --with <bad-package>`
// fails at the SUBPROCESS level (non-zero exit), `executePython` throws
// `Process execution failed: ...`, and `execute()`'s outer catch converts
// it into the exact error-marked output substring this row specifies.
#[test]
fn sc18_e2e_execute_catch_shows_error_marker_not_crash() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — sc18_e2e_execute_catch_shows_error_marker_not_crash");
        return;
    }
    if !uv_available() {
        eprintln!("SKIP: uv not on PATH — sc18_e2e_execute_catch_shows_error_marker_not_crash");
        return;
    }
    let tmp = setup_marimo_project();
    let input = tmp.path().join("execute_error.qmd");
    write_file(&input, EXECUTE_ERROR_DOC);

    let html = render_html(&input).expect(
        "marimo render must COMPLETE (not hard-fail) even when the subprocess itself fails — \
         execute()'s try/catch must convert the failure into error-marked output, not \
         propagate it",
    );

    assert!(
        html.contains("Error executing marimo"),
        "rendered HTML must contain the engine's catch-block error marker \
         ('Error executing marimo: ...'); got:\n{}",
        body_excerpt(&html)
    );
}

// RED-by-revert for `sc18_e2e_execute_catch_shows_error_marker_not_crash`
// (named revert per the frozen Test Seam Spec row: "try/catch -> RED (render
// throws)"): in a TEMPDIR-ONLY copy of the fixture bundle, neutered the
// catch block in `marimo-engine.js`'s bundled `execute()` (corresponding to
// upstream `marimo-engine.ts` ~319-329) to rethrow instead of converting to
// error-marked output:
//
//   -  } catch (error) {
//   -    quarto.console.error(`Error executing marimo: ${error}`);
//   -    return {
//   -      engine: "marimo",
//   -      markdown: `\`\`\`
//   -  Error executing marimo: ${error.message}
//   -  \`\`\`
//   -
//   -  ${markdown}`,
//   -      supporting: [],
//   -      filters: []
//   -    };
//   -  }
//   +  } catch (error) {
//   +    throw error;
//   +  }
//
// Re-rendering the SAME `EXECUTE_ERROR_DOC` through this reverted tempdir
// bundle then fails the render OUTRIGHT (captured verbatim, no HTML
// produced):
//
//   Error: Execution failed in marimo: Process execution failed:   × No solution found when resolving `--with` dependencies:
//     ╰─▶ Because this-package-definitely-does-not-exist-q2-sc18-test-xyz
//         was not found in the package registry and you require
//         this-package-definitely-does-not-exist-q2-sc18-test-xyz, we can conclude
//         that your requirements are unsatisfiable.
//
//   1 error
//
// i.e. the render throws instead of completing with error-marked output —
// exactly the row's named RED. Fixture restored byte-identical afterward;
// this test re-confirmed GREEN against the pristine fixture immediately
// after.
