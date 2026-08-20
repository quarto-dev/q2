# Plan 10: engine `checkInstallation` → real `q2 check` (bd-4qflzhwh)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the engine `checkInstallation` capability into q2's TS-engine
protocol and implement a real `q2 check` command whose engine section matches
Quarto 1's `quarto check` user-visibly, including full Q1-fidelity native
knitr/jupyter checks.

**Architecture:** A new discovery-tier wire verb (`checkInstallation`) whose
response is **streamed**: the host forwards each engine console line as an
interim `checkProgress` frame (same correlation id) and closes with
`checkInstallationResult`. An optional `check_installation` method on the Rust
`ExecutionEngine` trait (default NotSupported, mirroring Plan 9's
`call_engine_command`) emits `CheckLine`s through a sink callback; `TsEngine`
forwards over the wire, native knitr/jupyter reproduce Q1's decision trees.
`q2 check` enumerates `project.registry`, validates targets Q1-style, and
prints lines as they arrive.

**Tech Stack:** Rust (quarto-core, quarto CLI crate), serde wire enums,
Deno host (`ts-packages/quarto-engine-host-deno`), vendored Q1 probe scripts
(`knitr.R`, `jupyter.py`).

**Spec:** `claude-notes/research/2026-07-03-plan10-check-installation-research.md`
(Q1 verbatim behavior = Part 1a–1c; ratified decision points DP1–8, Q9, Q10,
consequences C1–C6). **Q1 is the binding user-visible spec**; every deviation
is in the Deviations ledger below — none may be added silently.

## Global Constraints

- **NEVER push to the remote.** Commit locally; pushing needs Gordon's explicit approval.
- Branch: `braid/bd-4qflzhwh-wire-ts-engine-checkinstallation` (based on `feature/ts-engine-extensions`).
- Commits are **path-scoped** (`git add <files>`), never `git add -A` (concurrent epic agent).
- `cargo nextest run` (never `cargo test`, never piped through `tail`).
- New Rust integration tests go in `tests/integration/<name>.rs` + registration in
  `tests/integration/main.rs` (one binary per crate — see `.claude/rules/integration-tests.md`).
- Cross-platform: no unconditional `std::os::unix`; paths via `PathBuf`; `lines()` for iteration.
- Wire payloads fully typed — **no `serde_json::Value`** (plan-1a rule).
- Probe scripts are **copied** from `external-sources/quarto-cli/src/resources/capabilities/`
  into local resources; never referenced from `external-sources/` at build time.
- Trait stays sync + object-safe (`Send + Sync`); async helpers bridged via
  current-thread tokio `block_on` (precedent `jupyter/text_execute.rs:229-234`).
- Before final commit: `cargo xtask verify --skip-hub-build` minimum; full
  `cargo xtask verify` if anything under `quarto-core` could affect the WASM leg (it does — run full).
- Environment: deno, uv (python), julia, R present. Deno-gated / R-gated / python-gated
  tests must SKIP (not fail) when the runtime is absent, following the existing
  `deno_is_available()` gating pattern (`registry.rs:482`).

## Deviations ledger (numbered, ratified 2026-07-03/04 — Q1 is the spec)

| # | Deviation from Q1 | Disposition |
|---|---|---|
| D-1 | Engine check output is **streamed frames** printed by the Rust side; no in-terminal ANSI spinner animation from the engine itself (q2's `withSpinner` is neutral: start line + `[✓]` completion). A Rust-side TTY spinner MAY animate between lines (polish task, accepted-untested). | ratified (DP2 revised) |
| D-2 | Fixed targets `install`/`info`/`versions` validate and print a **minimal placeholder section** (Quarto version + path); full Q1 section content deferred to a follow-up strand. | ratified (DP1) |
| D-3 | No `--output` (JSON mode) and no `--no-strict` flags yet; wire `conf` carries `strict: true` and is designed to grow (`output`/`jsonResult` later). Engines receive the console path (valid per Q1 contract). | ratified (DP3, DP4) |
| D-4 | Q1's literal `R succesfully found at` typo is corrected to `successfully`. | ratified (DP7) |
| D-5 | TS-engine `quarto.system.checkRender` stays a stub (no host→Rust render callback); **native** engines DO run Q1's test-render sub-checks. Follow-up strand filed at close-out. | ratified (DP5) |
| D-6 | q2-only case (no Q1 analog): extension engines registered but `deno` missing → per-engine `Checking <name> installation....(None)` + indented `Unable to locate deno (required to run extension engines).`, **exit 0**. Broken engine bundle (LoadEngine error) → abort with error. | ratified (Q10) |
| D-7 | Jupyter check replicates Q1's package lines (`jupyter_core`/`nbformat`/`nbclient`/`ipykernel`/`shiny`) even though q2 executes via ZeroMQ and does not require them all; the test render is the ground truth. | ratified (Q9) |
| D-8 | Failure semantics exactly Q1: missing runtime → `(None)` + hint, **exit 0**; an engine check that returns `Err` → abort remaining checks, non-zero exit. | ratified (DP6) |
| D-9 | Whole report prints to **stderr** (Q1's `info()` writes to stderr). | consequence C2 |

## File structure

| File | Role |
|---|---|
| `crates/quarto-core/src/engine/ts_protocol.rs` (modify) | `ToEngine::CheckInstallation`, `FromEngine::CheckProgress`/`CheckInstallationResult`, `TsCheckConfiguration`, `TsCheckLine`, `LoadEngineResult.has_check_installation`, `FromEngine::is_interim()` |
| `crates/quarto-core/src/engine/ts_process.rs` (modify) | unbounded pending-slot channel, `request_streaming` (interim-frame callback + idle timeout) |
| `crates/quarto-core/src/engine/check.rs` (create) | `CheckLine`, `CheckLineKind`, `CheckContext`, message constants shared by native checks |
| `crates/quarto-core/src/engine/traits.rs` (modify) | `supports_check_installation` + `check_installation` defaults |
| `crates/quarto-core/src/engine/ts_engine.rs` (modify) | overrides forwarding over the wire |
| `crates/quarto-core/src/engine/mod.rs` (modify) | `pub mod check;` + re-exports |
| `crates/quarto-core/src/engine/knitr/{mod.rs,check.rs,resources/capabilities.R}` | native knitr check (Q1 decision tree) |
| `crates/quarto-core/src/engine/jupyter/{mod.rs,check.rs,python.rs,resources/capabilities.py}` | python resolution + native jupyter check |
| `ts-packages/quarto-engine-host-deno/src/host.ts` (modify) | `hasCheckInstallation` in loaded payload; `checkInstallation` dispatch case with sink capture→frames |
| `ts-packages/quarto-engine-host-deno/src/deno-host.ts` (modify) | swappable log sink indirection |
| `ts-packages/quarto-engine-host-deno/src/types.ts` (modify) | wire type mirrors |
| `crates/quarto/src/main.rs` (modify) | thread `target` into `commands::check::execute(target)` |
| `crates/quarto/src/commands/check.rs` (rewrite) | real command: banner, target validation, engine loop, line printer |
| `crates/quarto-core/tests/fixtures/extensions/check-fail/` (create) | hand-authored fixture engine whose `checkInstallation` throws (D-8 abort test) |
| `crates/quarto/tests/integration/check_command.rs` (create) | e2e tests C1–C8 |
| `crates/quarto-core/tests/integration/ts_engine_check.rs` (create) | TsEngine round-trip test E1 |

## Frozen Test Seam Spec (prevalidated 2026-07-05)

Tiers: `unit-rs` (in-crate `#[cfg(test)]`), `deno` (host `*.deno-test.ts`),
`e2e-rs` (integration binary driving `q2` / real subprocess; deno/R/python-gated
skips). Byte-equality is used only for **fixed message literals**; lines
containing machine-dependent versions/paths use prefix/contains assertions
(unlike Plan 9's cliffy oracles, Q1 check output embeds local versions — whole-
output byte oracles would be machine-unstable).

| ID | Phase | Tier | Real unit / seam | Assertion | Mocks | Named revert hunk → RED |
|---|---|---|---|---|---|---|
| W1 | 0 | unit-rs | `ToEngine::CheckInstallation` serde | serializes to `{"type":"checkInstallation","engine":"e","conf":{"strict":true,"target":"all"}}` (exact JSON) + round-trips | none | Remove `#[serde(rename = "checkInstallation")]` → tag `CheckInstallation` → exact-JSON RED |
| W2 | 0 | unit-rs | `FromEngine::CheckProgress` serde | round-trips; tag `checkProgress`; `line` field camelCase; `FromEngine::CheckInstallationResult` tag `checkInstallationResult` | none | Rename `line`→`Line` via serde attr / drop variant rename → RED |
| W3 | 0 | unit-rs | `LoadEngineResult.has_check_installation` compat | legacy JSON **without** the field deserializes with `false`; JSON with `"hasCheckInstallation":true` → `true` | none | Remove `#[serde(default)]` → legacy-payload deserialization errors → RED |
| W4 | 0 | unit-rs | `FromEngine::is_interim()` | `true` for `CheckProgress`, `false` for all 10 other variants (exhaustive match) | none | Make it return `false` for `CheckProgress` → RED |
| T1 | 1 | unit-rs | `request_streaming` via mock transport (existing `spawn_into`-bypassing mock, see ts_process.rs test mocks) | mock replies 3× `CheckProgress` then `CheckInstallationResult` → callback receives the 3 lines **in order**, return is the final frame, pending map empty after | mock `EngineTransport` only | In the reader/delivery path, treat interim frames as terminal (remove slot on first frame) → callback count 1 & hang/Err → RED |
| T2 | 1 | unit-rs | idle-timeout reset | mock emits a `CheckProgress` every 100 ms for 5 frames, final at +100 ms; `idle_window = 300 ms` → completes Ok. Control: same mock with first frame delayed 400 ms → `ExecutionError::Timeout` | mock transport | Replace idle-deadline reset (`deadline = now + window` on each interim frame) with fixed total deadline → 600 ms run times out → first assertion RED |
| H1 | 2 | deno | `loadEngine` payload | julia fixture → `loaded.discovery.hasCheckInstallation === true`; echo fixture → `false` | none (real fixtures) | Drop the `hasCheckInstallation:` property from the loaded payload → `undefined !== true` → RED |
| H2 | 2 | deno | `checkInstallation` dispatch on marimo fixture | frames received: ≥1 `checkProgress` with `line.text` containing `Checking Marimo installation...`, THEN exactly one `checkInstallationResult`; progress frames precede the result frame | none | Buffer sink lines and discard instead of `writeFrame` per line → zero `checkProgress` frames → RED |
| H3 | 2 | deno | `checkInstallation` on non-implementing engine (echo) | reply is a `FromEngine` `error` frame (message contains `does not implement checkInstallation`) — Rust gates on the flag, so reaching here is protocol misuse | none | Optional-chain `discovery.checkInstallation?.(conf)` (silently resolves) → `checkInstallationResult` instead of `error` → RED |
| H4 | 2 | deno | log-sink restoration | after a check completes, a subsequent `host.log.info` goes to stderr (spy), NOT into frames | spy on stderr writer | Remove the `finally { restoreSink() }` → post-check log captured as frame → RED |
| R1 | 3 | unit-rs | trait defaults | struct implementing only `name`/`execute`: `supports_check_installation() == false`; `check_installation(...)` → `Err(NotSupported("check_installation"))` | none | Change default to `Ok(())` → `matches!(… Err(NotSupported(_)))` RED |
| E1 | 3 | e2e-rs (deno-gated) | `TsEngine::check_installation` full round trip | julia fixture project: collected `CheckLine`s include one with text containing `Checking Julia installation...`; result `Ok(())`; `supports_check_installation() == true`; echo-legacy engine → `supports == false` | none | Delete the `TsEngine` override (fall to trait default) → `Err(NotSupported)` → RED |
| C1 | 4 | e2e-rs (deno-gated) | `q2 check` happy path | temp project with marimo fixture: stderr contains line starting `Quarto ` (banner) and a line containing `Checking Marimo installation...`; exit 0 | none | Remove the engine loop from `check::execute` → marimo line absent → RED |
| C2 | 4 | e2e-rs | unknown target | `q2 check nonexistent` → exit ≠ 0; stderr contains `Unknown check target: nonexistent` and `Available targets: install, info, versions, all` (+ engine names when fixtures present) | none | Accept any target (skip `enforce_target`) → exit 0 → RED |
| C3 | 4 | e2e-rs (deno-gated) | capability-filtered targets | in echo-fixture project, `q2 check echo` → the C2 unknown-target error (echo lacks checkInstallation ⇒ not a target, Q1 `getTargets()` parity) | none | Build the target list from ALL engine names (drop the `supports_check_installation` filter) → echo accepted → RED |
| C4 | 4 | e2e-rs (deno-gated) | silent skip | `q2 check` (all) in a project with echo + marimo: marimo section present, **no** line mentioning `echo`, exit 0 | none | Emit `Engine echo has no installation check` for non-supporting engines → line present → RED (asserts absence, Q1 silent-skip parity) |
| C5 | 4 | e2e-rs (deno-gated) | D-8 abort semantics | project with `check-fail` + marimo fixtures where `check-fail` sorts first (registration order): exit ≠ 0, stderr contains the thrown message `boom from check-fail`, and NO `Checking Marimo installation...` line (later engine not reached) | none | Wrap the engine loop body in per-engine `match`+continue → marimo line present & exit 0 → RED |
| C6 | 4 | e2e-rs | D-2 placeholder sections | `q2 check versions` (no fixtures needed) → exit 0, stderr contains `Quarto ` banner + the placeholder line `(full versions section not yet implemented in q2)` | none | Route unknown fixed targets to the C2 error path (drop the placeholder branch) → exit ≠ 0 → RED |
| C7 | 4 | e2e-rs | D-6 deno missing | marimo-fixture project, spawn `q2 check` with `PATH` stripped of deno (keep system dirs sans deno; set `QUARTO_TEST_HIDE_DENO=1`-free — pure PATH surgery): stderr contains `Checking marimo installation....(None)` and `Unable to locate deno (required to run extension engines).`; **exit 0** | none | Treat deno-missing as an error (propagate `Err`) → exit ≠ 0 → RED |
| C8 | 4 | e2e-rs | D-9 stream | banner + engine lines appear on **stderr**; stdout is empty in C1's run | none | Print report via `println!` → stdout non-empty → RED |
| K1 | 5 | e2e-rs (R-gated) | knitr check, R present | `q2 check knitr` in plain temp dir: stderr contains `Checking R installation...........OK`, indented `Version:`, `knitr:`, `rmarkdown:` lines | none | Skip the capabilities probe (emit only the OK header) → `Version:` absent → RED |
| K2 | 5 | e2e-rs | knitr check, R absent | spawn with `PATH` stripped of Rscript and `QUARTO_R` unset → `Checking R installation...........(None)` + `Unable to locate an installed version of R.` + `Install R from https://cloud.r-project.org/`; exit 0 (D-8) | none | Change the not-found branch to return `Err` → exit ≠ 0 → RED |
| K3 | 5 | e2e-rs (R-gated) | knitr test render | with knitr+rmarkdown healthy: stderr contains `Checking Knitr engine render......OK` | none | Remove the `ctx.render_probe` invocation from the version-ok branch → line absent → RED |
| P1 | 6 | unit-rs | `find_python` env override | temp dir with executable `python3` stub; `QUARTO_PYTHON=<stub>` → returns stub path (highest priority, beats PATH) | none (real tempdir, `#[cfg(unix)]` chmod helper per cross-platform rule) | Reorder resolution to try PATH before `QUARTO_PYTHON` → returns PATH python → RED |
| P2 | 6 | unit-rs | `find_python` fallback | empty `QUARTO_PYTHON`, `PATH` = tempdir containing only `python3` stub → returns it; `PATH` = empty tempdir → `None` | none | Drop the `python3` PATH probe (keep only conda) → `None` for first case → RED |
| J1 | 6 | e2e-rs (python-gated) | jupyter check, python present | `q2 check jupyter`: stderr contains `Checking Python 3 installation....OK` + indented `Version:` + `Path:` + `Jupyter:` lines | none | Skip capabilities-message emission after OK → `Version:` absent → RED |
| J2 | 6 | e2e-rs | jupyter check, python absent | `PATH` stripped of python*, `QUARTO_PYTHON` unset → `Checking Python 3 installation....(None)` + `Unable to locate an installed version of Python 3.` + `Install Python 3 from https://www.python.org/downloads/`; exit 0 | none | Not-found branch returns `Err` → exit ≠ 0 → RED |
| J3 | 6 | e2e-rs (python+kernel-gated) | jupyter test render | with jupyter_core + python kernelspec present: `Checking Jupyter engine render....OK` | none | Remove `ctx.render_probe` call from the kernel-present branch → line absent → RED |

**Missing-test pass (logged, accepted-untested with rationale):**

- **Windows py-launcher branch** of `find_python` (`PY_PYTHON`) — cannot execute
  on macOS/Linux CI; code is `#[cfg(windows)]`-gated and reviewed by inspection (C5 consequence).
- **TTY spinner animation** (D-1 polish) — visual behavior on a live TTY; tests
  run non-TTY and assert the plain-line degenerate output (C1/C8 cover it).
  Rationale: animation is presentation-only; all *content* is bound by C1–C8.
- **`unactivatedEnvMessage` warning** (jupyter, venv-not-activated scan) —
  requires constructing a python env fixture; the message constants are bound
  by unit assertion in Task 12's message-constants test; the trigger scan is
  accepted-untested (matches Q1, where it is also untested).
- **Sink-capture concurrency** (a render request racing a check on the shared
  host) — `q2 check` is a dedicated CLI invocation; no concurrent requests
  exist in the process. Documented invariant in host.ts comment (H4 binds the
  restore path, which is the failure that could actually corrupt state).
- **Idle-timeout against a real slow engine** — T2 binds the logic against the
  mock clock; a real 60s-idle engine test would be wall-clock-hostile.

---

## Phase 0 — Wire protocol

### Task 1: `checkInstallation` verb pair + payload types + serde tests (W1–W4)

**Files:**
- Modify: `crates/quarto-core/src/engine/ts_protocol.rs`
- Tests: same file, `#[cfg(test)] mod tests` (52 existing round-trip tests to pattern-match)

**Interfaces (Produces):**
```rust
// ToEngine gains:
#[serde(rename = "checkInstallation")]
CheckInstallation { engine: String, conf: TsCheckConfiguration },

// FromEngine gains:
#[serde(rename = "checkProgress")]
CheckProgress { line: TsCheckLine },
#[serde(rename = "checkInstallationResult")]
CheckInstallationResult,

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsCheckConfiguration {
    pub strict: bool,
    pub target: String,
    // growth points (D-3): output / jsonResult land with the JSON-mode strand
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsCheckLine {
    pub kind: TsCheckLineKind,
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum TsCheckLineKind { Info, Warning, Error }

impl FromEngine {
    /// Interim (non-terminal) frames of a streaming request. The transport
    /// keeps the pending slot alive across these; everything else terminates.
    pub fn is_interim(&self) -> bool {
        matches!(self, FromEngine::CheckProgress { .. })
    }
}

// LoadEngineResult gains (after quarto_required):
#[serde(default)]
pub has_check_installation: bool,
```

- [ ] **Step 1: failing tests W1–W4** — add to the ts_protocol test module,
  following the existing `test_to_engine_*_tag` style:

```rust
#[test]
fn test_to_engine_check_installation_tag() {
    let msg = ToEngine::CheckInstallation {
        engine: "e".to_string(),
        conf: TsCheckConfiguration { strict: true, target: "all".to_string() },
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(
        json,
        r#"{"type":"checkInstallation","engine":"e","conf":{"strict":true,"target":"all"}}"#
    );
    let back: ToEngine = serde_json::from_str(&json).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn test_from_engine_check_progress_and_result_tags() {
    let p = FromEngine::CheckProgress {
        line: TsCheckLine { kind: TsCheckLineKind::Info, text: "Checking...".to_string() },
    };
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(json, r#"{"type":"checkProgress","line":{"kind":"info","text":"Checking..."}}"#);
    assert_eq!(serde_json::from_str::<FromEngine>(&json).unwrap(), p);

    let r = FromEngine::CheckInstallationResult;
    assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"type":"checkInstallationResult"}"#);
}

#[test]
fn test_load_engine_result_has_check_installation_default() {
    // legacy payload (pre-Plan-10 host) — field absent
    let legacy = r#"{"name":"x","validExtensions":[]}"#;
    let d: LoadEngineResult = serde_json::from_str(legacy).unwrap();
    assert!(!d.has_check_installation);
    let with = r#"{"name":"x","validExtensions":[],"hasCheckInstallation":true}"#;
    let d: LoadEngineResult = serde_json::from_str(with).unwrap();
    assert!(d.has_check_installation);
}

#[test]
fn test_from_engine_is_interim_exhaustive() {
    // Exhaustive: CheckProgress is the ONLY interim variant.
    let interim = FromEngine::CheckProgress {
        line: TsCheckLine { kind: TsCheckLineKind::Info, text: String::new() },
    };
    assert!(interim.is_interim());
    assert!(!FromEngine::CheckInstallationResult.is_interim());
    assert!(!FromEngine::Cancelled.is_interim());
    // (and one representative per remaining variant, all false)
}
```

- [ ] **Step 2: run, verify FAIL** —
  `cargo nextest run -p quarto-core -E 'test(check_installation) | test(check_progress) | test(is_interim)'`
  → compile errors (variants absent) count as the RED.
- [ ] **Step 3: add the variants/types/impl** exactly as in Interfaces above
  (variants at the END of each enum; `has_check_installation` after
  `quarto_required` with `#[serde(default)]`).
- [ ] **Step 4: run, verify PASS** — same filter, then the whole protocol module:
  `cargo nextest run -p quarto-core -E 'binary_id(quarto-core) & test(ts_protocol)'` —
  all pre-existing round-trip tests must stay green (additive change).
- [ ] **Step 5: commit** —
  `git add crates/quarto-core/src/engine/ts_protocol.rs && git commit -m "feat(ts-protocol): checkInstallation verb pair + streamed checkProgress frames + hasCheckInstallation discovery flag (bd-4qflzhwh W1-W4)"`

---

## Phase 1 — Transport: multi-response requests

### Task 2: unbounded pending slots + `request_streaming` (T1–T2)

**Files:**
- Modify: `crates/quarto-core/src/engine/ts_process.rs`

**Context an implementer needs:** today `PendingSlot.tx` is a
`sync_channel(1)` and `reader_loop` **removes the slot before sending**
(ts_process.rs:929-934, the "slot-delivery invariant": capacity-1, exactly one
send). Streaming breaks "exactly one send," so:

**Interfaces (Produces):**
```rust
/// Like `request`, but delivers interim frames (`FromEngine::is_interim()`)
/// to `on_progress` and returns the terminal frame. `idle_window` is reset
/// on every interim frame (C4 consequence: idle timeout, not total budget).
pub fn request_streaming(
    &self,
    msg: ToEngine,
    idle_window: Option<Duration>,
    cancellation: &CancellationToken,
    on_progress: &mut dyn FnMut(FromEngine),
) -> Result<FromEngine, ExecutionError>
```

Design (all in this task):
1. Change `PendingSlot.tx` from `sync_channel(1)` to unbounded
   `std::sync::mpsc::channel()`. Update the invariant comment: the reader now
   sends **without removing** when `msg.is_interim()`; it removes-then-sends
   (preserving the old ordering) for terminal frames. Unbounded channel keeps
   "reader's send never blocks".
2. `reader_loop`: replace the unconditional `remove(&id)` with:
   ```rust
   let is_interim = matches!(&msg, m if m.is_interim());
   let slot = if is_interim {
       pending.lock().unwrap().get(&id).map(|s| s.tx.clone())
   } else {
       pending.lock().unwrap().remove(&id).map(|s| s.tx)
   };
   if let Some(tx) = slot { let _ = tx.send(Ok(msg)); }
   ```
   (Late interim frames after cancel/timeout hit the removed-slot path and are
   dropped — same as today's late terminal replies.)
3. `request_streaming`: clone of `request`'s recv loop with two changes:
   on `Ok(frame)` where `frame.is_interim()` → `on_progress(frame)`, reset
   `deadline = Instant::now() + window`, continue; terminal frame → existing
   error-mapping (`FromEngine::Error` → `execution_failed`) and return.
4. `request` itself is re-expressed as
   `self.request_streaming(msg, window, c, &mut |_| {})` **only if** the diff
   stays behavior-identical (interim frames were impossible before this plan,
   so a stray interim frame reaching old `request` callers is new protocol —
   dropping it via the empty callback is correct); otherwise keep `request`
   untouched and duplicate the loop.

- [ ] **Step 1: failing tests T1–T2** in the existing ts_process test module,
  using the existing mock-transport infrastructure (the mocks that pass
  `None` for stderr — see `start_with_transport_for_tests`, ts_process.rs:481-498):

```rust
#[test]
fn request_streaming_delivers_interim_frames_in_order() {
    // Mock transport scripted to reply to the next request id with:
    //   3 × CheckProgress(line text "l1"/"l2"/"l3"), then CheckInstallationResult.
    let host = mock_host_with_scripted_replies(vec![
        interim("l1"), interim("l2"), interim("l3"),
        terminal_check_result(),
    ]);
    let mut seen = Vec::new();
    let out = host.request_streaming(
        check_installation_msg("julia"),
        Some(Duration::from_secs(2)),
        &CancellationToken::new(),
        &mut |f| if let FromEngine::CheckProgress { line } = f { seen.push(line.text) },
    ).unwrap();
    assert_eq!(seen, vec!["l1", "l2", "l3"]);
    assert!(matches!(out, FromEngine::CheckInstallationResult));
    assert!(host.pending_is_empty_for_tests());
}

#[test]
fn request_streaming_idle_window_resets_per_frame() {
    // frames at t=100/200/300/400/500ms, terminal at 600ms; idle window 300ms
    let host = mock_host_with_timed_replies(/* per above */);
    let ok = host.request_streaming(..., Some(Duration::from_millis(300)), ..., &mut |_| {});
    assert!(ok.is_ok()); // total 600ms > 300ms window — only passes if idle-reset
    // control: first frame at 400ms, window 300ms → Timeout
    let host2 = mock_host_with_first_reply_after(Duration::from_millis(400));
    assert!(matches!(host2.request_streaming(...), Err(ExecutionError::Timeout { .. })));
}
```
  (Adapt helper names to the actual mock API in the test module — the mocks
  exist; T1/T2's *assertions and revert hunks* are the frozen part.)

- [ ] **Step 2: run, verify FAIL** —
  `cargo nextest run -p quarto-core -E 'test(request_streaming)'` → compile RED.
- [ ] **Step 3: implement** points 1–4 above.
- [ ] **Step 4: run streaming tests + the FULL ts_process module** (the reader
  invariants are heavily tested — J6/J9 exactly-one-event counts must stay green):
  `cargo nextest run -p quarto-core -E 'test(ts_process)'`
- [ ] **Step 5: commit** —
  `git add crates/quarto-core/src/engine/ts_process.rs && git commit -m "feat(ts-process): multi-response request_streaming with idle-window timeout; unbounded pending slots (bd-4qflzhwh T1-T2)"`

---

## Phase 2 — Deno host

### Task 3: `hasCheckInstallation` in the loaded payload (H1)

**Files:**
- Modify: `ts-packages/quarto-engine-host-deno/src/host.ts` (loadEngine case, ~line 370)
- Modify: `ts-packages/quarto-engine-host-deno/src/types.ts` (LoadEngineResult mirror)
- Test: `ts-packages/quarto-engine-host-deno/src/host.deno-test.ts` (or the existing fixture-driven deno test file — follow where the current loadEngine tests live)

- [ ] **Step 1: failing test H1** — drive `loadEngine` against the julia and
  echo fixtures (paths as in existing deno tests):
```ts
Deno.test("loadEngine reports hasCheckInstallation", async () => {
  const julia = await loadFixture("julia-engine");   // existing helper pattern
  assertEquals(julia.discovery.hasCheckInstallation, true);
  const echo = await loadFixture("echo-engine");
  assertEquals(echo.discovery.hasCheckInstallation, false);
});
```
- [ ] **Step 2: run, FAIL** — `deno test --allow-all src/host.deno-test.ts` (from
  `ts-packages/quarto-engine-host-deno`) → `undefined !== true`.
- [ ] **Step 3: implement** — in the `loadEngine` payload (host.ts:370-373 area):
```ts
hasCheckInstallation: typeof discovery.checkInstallation === "function",
```
  plus the `types.ts` field `hasCheckInstallation: boolean`.
- [ ] **Step 4: run, PASS.**
- [ ] **Step 5: commit** (both files + test).

### Task 4: `checkInstallation` dispatch case + swappable log sink (H2–H4)

**Files:**
- Modify: `ts-packages/quarto-engine-host-deno/src/deno-host.ts` (log object ~line 251)
- Modify: `ts-packages/quarto-engine-host-deno/src/host.ts` (new switch case)
- Test: same deno test file as Task 3

**Interfaces (Produces, host-internal):**
```ts
// deno-host.ts — the log methods route through a swappable sink:
export type LogLine = { kind: "info" | "warning" | "error"; text: string };
export type LogSink = (line: LogLine) => void;
// default sink = today's stderr writer ("[INFO] " prefixes etc.)
export function setLogSink(sink: LogSink | null): void;  // null → restore default
```

- [ ] **Step 1: failing tests H2–H4** (shape):
```ts
Deno.test("checkInstallation streams progress frames then result (marimo)", ...);
Deno.test("checkInstallation on engine without the method → error frame (echo)", ...);
Deno.test("log sink restored after check", ...);  // H4: spy stderr, log after check
```
  H2 asserts: collected frames = N×`checkProgress` (one with
  `line.text.includes("Checking Marimo installation...")`) followed by exactly
  one `checkInstallationResult`, in that order.
- [ ] **Step 2: run, FAIL** (unknown message type `checkInstallation`).
- [ ] **Step 3: implement the case** — discovery-tier lookup (like
  `claimsLanguage`, host.ts:470), NOT the launched path:
```ts
case "checkInstallation": {
  const name: string = msg.engine;
  const entry = engineByName.get(name);
  if (!entry) throw new Error(`engine not loaded: ${name}`);
  const discovery = entry.discovery;
  if (typeof discovery.checkInstallation !== "function") {
    // Rust gates on hasCheckInstallation; reaching here is protocol misuse.
    throw new Error(`engine ${name} does not implement checkInstallation`);
  }
  const services = makeCheckServices();          // tempContext-backed, below
  const conf = {
    strict: msg.conf.strict,
    target: msg.conf.target,
    output: undefined,
    services,
    jsonResult: undefined,                       // D-3: console path
  };
  // NOTE (accepted invariant): q2 check drives one request at a time; no
  // concurrent request writes console output while the sink is swapped.
  setLogSink((line) =>
    writeFrame(writer, { id, msg: { type: "checkProgress", line } }));
  try {
    await discovery.checkInstallation(conf);
  } finally {
    setLogSink(null);
    services.cleanup();
  }
  await writeFrame(writer, { id, msg: { type: "checkInstallationResult" } });
  return;
}
```
  `makeCheckServices()`: build `CheckRenderServiceWithLifetime` from the
  existing tempContext factory (`quarto-api` system namespace uses
  `host.fs.makeTempDir/makeTempFile` — deno-host.ts:91-95); `cleanup()`
  delegates to the temp context's cleanup; `extension`/`notebook`/`lifetime`
  stay `undefined` (the vendored type marks them placeholders).
- [ ] **Step 4: run all deno tests, PASS.**
- [ ] **Step 5: rebuild the embedded host bundle** (the Rust side embeds it):
  `cargo xtask build-engine-host-bundle` if that xtask exists — otherwise the
  build command used by Plan 1b/9 commits ("build(engine-host): rebuild embedded
  bundle…"): check `git log --grep "rebuild embedded bundle" -1 --stat` for the
  exact artifact path and regenerate the same way. Commit code + bundle together.
- [ ] **Step 6: commit** —
  `git add ts-packages/quarto-engine-host-deno/src/{host.ts,deno-host.ts,types.ts,*.deno-test.ts} <bundle artifact> && git commit -m "feat(engine-host): checkInstallation dispatch — conf synthesis, streamed console sink, hasCheckInstallation (bd-4qflzhwh H1-H4)"`

---

## Phase 3 — Rust trait + TsEngine

### Task 5: `check.rs` types + trait defaults (R1)

**Files:**
- Create: `crates/quarto-core/src/engine/check.rs`
- Modify: `crates/quarto-core/src/engine/traits.rs`, `crates/quarto-core/src/engine/mod.rs`

**Interfaces (Produces):**
```rust
// crates/quarto-core/src/engine/check.rs
use std::path::Path;

/// One user-facing line of a check report. TS lines arrive pre-formatted
/// (makeConsole embeds the "[✓] " completion prefix in the text); native
/// checks use `Complete` and the printer adds the same prefix — identical
/// visual output either way.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckLine {
    pub kind: CheckLineKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckLineKind { Info, Complete, Warning, Error }

impl CheckLine {
    pub fn info(text: impl Into<String>) -> Self { Self { kind: CheckLineKind::Info, text: text.into() } }
    pub fn complete(text: impl Into<String>) -> Self { Self { kind: CheckLineKind::Complete, text: text.into() } }
}

/// Outcome of a check test-render probe.
pub type RenderProbe<'a> = &'a dyn Fn(&str /*qmd content*/, &str /*language*/) -> Result<(), String>;

pub struct CheckContext<'a> {
    pub strict: bool,
    pub target: String,
    /// In-process test render (D-5: native engines only). None ⇒ the engine
    /// skips its render sub-check (used by unit tests; the CLI always passes Some).
    pub render_probe: Option<RenderProbe<'a>>,
}

/// Q1's 6-space indent for detail lines (jupyter.ts/rmd.ts kIndent).
pub const K_INDENT: &str = "      ";
```

```rust
// traits.rs — after is_available():

/// Whether this engine implements an installation check (Q1's
/// `checkInstallation`). Gates `q2 check` target enumeration and the
/// check loop (non-implementing engines are silently skipped — Q1
/// `getTargets()` / check-loop parity). Default: `false`.
fn supports_check_installation(&self) -> bool { false }

/// Run the engine's installation check, emitting user-facing lines in
/// order. Q1 semantics: report through lines (or `conf.jsonResult`, later);
/// return `Err` only for hard failure (aborts `q2 check`, non-zero exit).
/// Default: NotSupported (callers must gate on `supports_check_installation`).
fn check_installation(
    &self,
    _ctx: &CheckContext,
    _emit: &mut dyn FnMut(CheckLine),
) -> Result<(), ExecutionError> {
    Err(ExecutionError::not_supported("check_installation"))
}
```

- [ ] **Step 1: failing test R1** (traits.rs test module, next to Plan 9's
  `call_engine_command_defaults_to_not_supported`):
```rust
#[test]
fn check_installation_defaults() {
    let e = MinimalEngine;   // the existing test struct implementing name/execute only
    assert!(!e.supports_check_installation());
    let mut lines = Vec::new();
    let r = e.check_installation(
        &CheckContext { strict: true, target: "all".into(), render_probe: None },
        &mut |l| lines.push(l),
    );
    assert!(matches!(r, Err(ExecutionError::NotSupported(_))));
    assert!(lines.is_empty());
}
```
- [ ] **Step 2: run, FAIL (compile).**
- [ ] **Step 3: implement** check.rs + trait defaults + `pub mod check;` /
  re-export `CheckLine, CheckLineKind, CheckContext` from `engine/mod.rs`.
- [ ] **Step 4: run, PASS** + `cargo build --workspace` (trait default ⇒ no
  implementor breaks; FixtureEngine/ReplayEngine/test impls inherit).
- [ ] **Step 5: commit.**

### Task 6: `TsEngine` overrides + `check-fail` fixture (E1)

**Files:**
- Modify: `crates/quarto-core/src/engine/ts_engine.rs`
- Create: `crates/quarto-core/tests/fixtures/extensions/check-fail/_extensions/check-fail/{_extension.yml,check-fail-engine.js}`
- Create: `crates/quarto-core/tests/integration/ts_engine_check.rs` (+ register in `tests/integration/main.rs`, alphabetized)

**Interfaces (Consumes):** Task 1 wire types, Task 2 `request_streaming`, Task 5 types.

- [ ] **Step 1: the fixture** — hand-authored JS (no build step; loader accepts
  any prebuilt lowercase `.js`), modeled on echo-legacy's minimal shape but
  with a throwing check:
```js
// check-fail-engine.js — minimal discovery object whose checkInstallation throws.
let quarto;
export default {
  name: "checkfail",
  init: (q) => { quarto = q; },
  defaultExt: ".qmd",
  defaultYaml: () => [],
  defaultContent: () => [],
  validExtensions: () => [],
  claimsFile: () => false,
  claimsLanguage: () => false,
  canFreeze: false,
  generatesFigures: false,
  checkInstallation: async () => {
    quarto.console.info("Checking checkfail installation...");
    throw new Error("boom from check-fail");
  },
  launch: () => { throw new Error("check-fail fixture never launches"); },
};
```
```yaml
# _extension.yml
title: check-fail
contributes:
  engines:
    - path: check-fail-engine.js
      name: checkfail
```
- [ ] **Step 2: failing test E1** (deno-gated like registry.rs:482):
```rust
// tests/integration/ts_engine_check.rs
#[test]
fn ts_engine_check_installation_round_trip() {
    if !deno_is_available() { eprintln!("skipping: deno not on PATH"); return; }
    let (registry, _tmp) = registry_with_fixtures(&["julia-engine", "echo-legacy"]); // existing helper pattern from ts-engine e2e tests
    let julia = registry.get("julia").unwrap();
    assert!(julia.supports_check_installation());
    let mut lines = Vec::new();
    let ctx = CheckContext { strict: true, target: "julia".into(), render_probe: None };
    julia.check_installation(&ctx, &mut |l| lines.push(l)).unwrap();
    assert!(lines.iter().any(|l| l.text.contains("Checking Julia installation...")));
    let echo = registry.get("echolegacy").unwrap();
    assert!(!echo.supports_check_installation());
}
```
- [ ] **Step 3: run, FAIL** (supports returns false — no override yet).
- [ ] **Step 4: implement the overrides** in ts_engine.rs (discovery-tier —
  `ensure_loaded`, NOT `ensure_launched`, mirroring `claims_language`):
```rust
fn supports_check_installation(&self) -> bool {
    let c = CancellationToken::new();
    if self.ensure_loaded(&c).is_err() { return false; }
    self.discovery.get().map(|d| d.has_check_installation).unwrap_or(false)
}

fn check_installation(
    &self,
    ctx: &CheckContext,
    emit: &mut dyn FnMut(CheckLine),
) -> Result<(), ExecutionError> {
    let c = CancellationToken::new();
    self.ensure_loaded(&c)?;
    let msg = ToEngine::CheckInstallation {
        engine: self.wire_name(),
        conf: TsCheckConfiguration { strict: ctx.strict, target: ctx.target.clone() },
    };
    // Idle window (C4): 60s of silence = dead check; resets on every frame.
    let out = self.host.request_streaming(msg, Some(Duration::from_secs(60)), &c,
        &mut |frame| {
            if let FromEngine::CheckProgress { line } = frame {
                emit(CheckLine {
                    kind: match line.kind {
                        TsCheckLineKind::Info => CheckLineKind::Info,
                        TsCheckLineKind::Warning => CheckLineKind::Warning,
                        TsCheckLineKind::Error => CheckLineKind::Error,
                    },
                    text: line.text,
                });
            }
        })?;
    match out {
        FromEngine::CheckInstallationResult => Ok(()),
        other => Err(ExecutionError::other(format!(
            "unexpected response to CheckInstallation: {other:?}"
        ))),
    }
}
```
  (Adapt the `discovery` OnceLock accessor name to the field at ts_engine.rs:117.)
- [ ] **Step 5: run E1 + full quarto-core suite, PASS. Commit** (fixture + code + test).

---

## Phase 4 — the `q2 check` command

### Task 7: command implementation (C1–C8 scaffolding)

**Files:**
- Modify: `crates/quarto/src/main.rs:744` — `Commands::Check { target } => commands::check::execute(target.as_deref())`
- Rewrite: `crates/quarto/src/commands/check.rs`
- Test: `crates/quarto/tests/integration/check_command.rs` (+ register in `main.rs` of that integration binary, alphabetized)

**Interfaces (Consumes):** `ProjectContext::discover` (project/mod.rs:867) →
`project.registry: Arc<EngineRegistry>` (project/mod.rs:849);
`registry.engines_in_order()` (registry.rs:159); Task 5 types;
`render_document_to_file` (render_to_file.rs:201); `deno_is_available()` (ts_process.rs:151).

**Behavior spec (from research §1a/1c + deviations):**

```text
q2 check [target]           # default target: "all"
stderr:
  Quarto <CARGO_PKG_VERSION>
  [fixed-target placeholder if target ∈ {install, info, versions} or "all"]   (D-2)
  [engine sections, registry order, only engines with supports_check_installation]
exit: 0 normally; ≠0 for unknown target or Err from an engine check (D-8)
```

- Target validation: build `targets = ["install","info","versions","all"] + <supporting engine names>`;
  unknown → stderr `Unknown check target: <t>` + `Available targets: <joined>`, exit 1.
  (Fixed names shadow engines — C6 consequence.)
- Placeholder sections (D-2), one line each, e.g. for `versions`:
  `Checking versions of quarto binary dependencies... (full versions section not yet implemented in q2)`.
- Engine loop (Q1 check.ts:112-119 parity): `for e in registry.engines_in_order()`,
  run when `e.supports_check_installation() && (target == e.name() || target == "all")`.
- **D-6 deno gate:** before the loop, if any TS engine is registered
  (`project` has external engines) and `!deno_is_available()`: for each such
  engine that would have been checked, print
  `Checking <name> installation....(None)` + `K_INDENT + "Unable to locate deno (required to run extension engines)."`,
  skip invoking, continue, exit 0. (Detect TS engines via the registry's
  contribution order / a `is_ts_engine` discriminator — use the same mechanism
  `project/mod.rs` uses to decide `needs_host`, exposed as needed.)
- Printer: `CheckLineKind::Complete` → `eprintln!("[✓] {text}")` (matching
  makeConsole's completeMessage format so TS + native render identically);
  Info/Warning/Error → `eprintln!("{text}")`. All output stderr (D-9).
- Render probe closure (for Phases 5/6):
```rust
let probe = |content: &str, _language: &str| -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let input = dir.path().join("check.qmd");
    std::fs::write(&input, content).map_err(|e| e.to_string())?;
    let runtime = default_system_runtime();   // whatever render.rs uses
    render_document_to_file(&input, "html", &RenderToFileOptions::default(),
        None, runtime, None, None, None)
        .map(|_| ()).map_err(|e| e.to_string())
};
```
  (Copy the exact runtime/options construction from `commands/render.rs`'s
  simplest path — the implementer reads render.rs and mirrors it; flags
  equivalent to Q1's `quiet: true`.)

- [ ] **Step 1: failing tests C1–C8** — integration test spawning the built `q2`
  binary (`env!("CARGO_BIN_EXE_q2")` if the binary target is `q2`, else the
  crate's established `assert_cmd`/`Command::new(cargo_bin)` pattern — copy
  from `smoke_all.rs`). Fixture projects: temp dir + copied
  `crates/quarto-core/tests/fixtures/extensions/<name>/_extensions` tree +
  a stub `_quarto.yml`. Deno-gated tests skip when `deno` absent. Each test
  asserts exactly its Seam Spec row (assertions frozen).
- [ ] **Step 2: run, FAIL** — C1 currently gets `Command not yet implemented: check`.
- [ ] **Step 3: implement** `check.rs` per the behavior spec. Suggested shape:
  `execute(target: Option<&str>) -> Result<()>`; internal
  `fn run_check(target: &str, cwd: &Path) -> Result<i32>` returning the exit
  code so unit tests can drive it without a subprocess.
- [ ] **Step 4: run C1–C8, PASS**; also `q2 check` **manually** in
  `crates/quarto-core/tests/fixtures/extensions/marimo/` and paste the observed
  stderr into the PR/plan notes (end-to-end verification rule — the plan's
  close-out task records it).
- [ ] **Step 5: TTY spinner (D-1 ratified polish).** When
  `std::io::stderr().is_terminal()` (std `IsTerminal`, no new dependency):
  while an engine check is in flight, a small helper animates
  `\r<frame> <last emitted line's text>` on stderr (braille or `|/-\` frames,
  ~80 ms tick, spawned thread + `AtomicBool` stop flag); every arriving
  `CheckLine` clears the animation line (`\r` + spaces + `\r`), prints the
  line, and the spinner resumes on the new text. The first frame arrives
  immediately (the engine's own `withSpinner` start message), so the animated
  text is **engine-authored**. Non-TTY: helper is inert (tests C1–C8 see plain
  lines — this step must not change any test's observed output).
- [ ] **Step 6: commit** (main.rs + check.rs + tests + fixture-copy helper).

---

## Phase 5 — native knitr check

### Task 8: vendored probe + knitr `check_installation` (K1–K3)

**Files:**
- Create: `crates/quarto-core/src/engine/knitr/resources/capabilities.R` —
  **verbatim copy** of `external-sources/quarto-cli/src/resources/capabilities/knitr.R`
  (YAML between `--- YAML_START/END ---` markers; R version, `R.home()`,
  `.libPaths()`, knitr/rmarkdown versions-or-null).
- Create: `crates/quarto-core/src/engine/knitr/check.rs`
- Modify: `crates/quarto-core/src/engine/knitr/mod.rs` (module + trait overrides)
- Test: K1–K3 rows in `crates/quarto/tests/integration/check_command.rs`

**Message constants (Q1-verbatim, from research §1b — the ONLY approved texts):**
```rust
pub const K_MSG_R: &str = "Checking R installation...........";
pub const K_MSG_KNITR_RENDER: &str = "Checking Knitr engine render......";
// (None)-branch:
//   "Unable to locate an installed version of R."
//   "Install R from https://cloud.r-project.org/"
// caps-failed branch (D-4 typo corrected):
//   "R successfully found at {rbin}."
//   "However, a problem was encountered when checking configurations of packages."
//   "Please check your installation of R."
// package messages (knitrInstallationMessage parity):
//   "The {pkg} package is not available in this R installation."
//   "Install with install.packages(\"{pkg}\")"
//   outdated variant: "The {pkg} package is outdated in this R installation."
//   "Update with update.packages(\"{pkg}\")"
// capabilities block (knitrCapabilitiesMessage parity, K_INDENT prefix each line):
//   "Version: {maj}.{min}.{patch}" / "Path: {home}" / "LibPaths:" / "  - {p}" /
//   "knitr: {v|(None)}" / "rmarkdown: {v|(None)}"
//   "NOTE: knitr version {v} is too old. Please upgrade to 1.30 or later."
//   "NOTE: rmarkdown version {v} is too old. Please upgrade to 2.3 or later."
```
Version gates (Q1 `pkgVersRequirement`): knitr `>= 1.30`, rmarkdown `>= 2.3`
(semver-compare on coerced versions; use the `semver` crate already in the tree
or a 2-component numeric compare — match Q1's `coerce` leniency for versions
like `1.50.1`).

**Decision tree (Q1 rmd.ts:89-222, exact):**
1. probe: `find_rscript()`; if found, run
   `Rscript <extracted capabilities.R>` (extract from the include_dir the same
   way `KNITR_RESOURCES.path()` does), parse the between-markers YAML.
2. `rscript == None` → emit `complete(K_MSG_R + "(None)\n")`, the two
   not-found info lines (K_INDENT), `Ok(())` (D-8: exit 0).
3. R found, parse failed → `complete("(None)\n")` + the three D-4 lines, `Ok(())`.
4. Caps OK → `complete(K_MSG_R + "OK")` + capabilities block + blank line.
   - both gates pass → if `ctx.render_probe` is Some: run it with Q1's doc
     (below), lang `"r"`; Ok → `complete(K_MSG_KNITR_RENDER + "OK\n")`;
     Err(e) → return `Err(ExecutionError::other(e))` (Q1 throws → D-8 abort).
   - gate fails → per-package install/update message lines, `Ok(())`.

Test-render doc (Q1-verbatim, rmd.ts:104-116):
```text
---
title: "Title"
---

## Header

```{r}
1 + 1
```
```

`supports_check_installation()` → `true` for KnitrEngine.

- [ ] **Step 1: failing K1–K3** (per Seam Spec; K2 strips PATH of Rscript in the
  spawned env and unsets `QUARTO_R` — note `find_rscript` is OnceLock-cached
  per process, which is fine because each e2e row spawns a fresh `q2`).
- [ ] **Step 2: run, FAIL** (knitr not a valid target yet → C2-style error).
- [ ] **Step 3: implement** `knitr/check.rs` + overrides; probe runs
  `Command::new(rscript).arg(script_path)` with piped stdout, no timeout
  (Q1 parity; the CLI is interactive — Ctrl-C works).
- [ ] **Step 4: run K1–K3 + full workspace tests, PASS.**
- [ ] **Step 5: commit** (resources + check.rs + mod.rs + tests). Note in the
  commit body: capabilities.R copied verbatim from quarto-cli @ the submodule/
  checkout revision, per External Sources Policy.

---

## Phase 6 — native jupyter check

### Task 9: `find_python` + vendored probe (P1–P2)

**Files:**
- Create: `crates/quarto-core/src/engine/jupyter/python.rs`
- Create: `crates/quarto-core/src/engine/jupyter/resources/capabilities.py` —
  **verbatim copy** of Q1's `capabilities/jupyter.py` (emits YAML: version
  fields, conda flag, exec paths, then `jupyter_core`/`nbformat`/`nbclient`/
  `ipykernel`/`shiny` versions-or-null).
- Modify: `crates/quarto-core/src/engine/jupyter/mod.rs`

**Interfaces (Produces):**
```rust
/// Q1 resolution order (capabilities.ts): QUARTO_PYTHON → [windows py
/// launcher when PY_PYTHON set] → conda `python` → `python3` (windows: `py`).
/// NOT OnceLock-cached: `q2 check` is one-shot and tests vary the env.
pub fn find_python() -> Option<PathBuf>;

pub struct PythonCapabilities {           // parsed from capabilities.py YAML
    pub version_major: u32, pub version_minor: u32, pub version_patch: u32,
    pub conda: bool, pub executable: String,
    pub jupyter_core: Option<String>, pub nbformat: Option<String>,
    pub nbclient: Option<String>, pub ipykernel: Option<String>,
    pub shiny: Option<String>,
}
pub fn python_capabilities(python: &Path) -> Option<PythonCapabilities>; // rejects major < 3 (Q1)
```
- [ ] **Step 1: failing P1–P2** (unit tests in python.rs; `#[cfg(unix)]` chmod
  helper + `#[cfg(not(unix))]` variant per cross-platform rule; env mutation via
  the crate's existing env-lock test helper if one exists — check for a
  `serial_test`/mutex pattern in quarto-core first, else mark `#[serial]`).
- [ ] **Step 2: FAIL. Step 3: implement. Step 4: PASS. Step 5: commit.**

### Task 10: jupyter `check_installation` (J1–J3)

**Files:**
- Create: `crates/quarto-core/src/engine/jupyter/check.rs`
- Modify: `crates/quarto-core/src/engine/jupyter/mod.rs`
- Test: J1–J3 rows in `check_command.rs`

**Message constants (Q1-verbatim, research §1b):**
```rust
pub const K_MSG_PY: &str = "Checking Python 3 installation....";
pub const K_MSG_JUPYTER_RENDER: &str = "Checking Jupyter engine render....";
// (None)-branch: "Unable to locate an installed version of Python 3."
//                "Install Python 3 from https://www.python.org/downloads/"
// capabilities block: "Version: {maj}.{min}.{patch}[ (Conda)]" / "Path: {executable}"
//                     "Jupyter: {jupyter_core|(None)}" / "Kernels: {names, comma-joined}"
// jupyter-missing: "Jupyter is not available in this Python installation."
//                  conda ⇒ "Install with conda install jupyter"
//                  else  ⇒ "Install with {python} -m pip install jupyter"
// no-kernel NOTE:  K_INDENT + "NOTE: No Jupyter kernel for Python found"
```

**Decision tree (Q1 jupyter.ts:124-246, exact; D-7 replicate):**
1. `find_python()` → None: `complete(K_MSG_PY + "(None)\n")` + install lines, `Ok(())`.
2. caps → `complete(K_MSG_PY + "OK")` + capabilities block (Kernels line via
   `list_kernelspecs()`, bridged with a current-thread tokio runtime exactly as
   `text_execute.rs:229-234`) + blank line.
   - `jupyter_core` present + `find_kernelspec_for_language("python")` Some →
     render probe with Q1's `{python} 1 + 1` doc (same shape as knitr's, lang
     `python`); Ok → `complete(K_MSG_JUPYTER_RENDER + "OK\n")`; Err → `Err` (D-8).
   - jupyter present, no python kernel → the no-kernel NOTE, `Ok(())`.
   - `jupyter_core` None → jupyter-missing lines (pip/conda variant), `Ok(())`.
     (unactivated-env scan: accepted-untested — port the message only if the
     scan is trivial; otherwise SKIP the scan entirely this plan and log it in
     the close-out — do NOT half-port.)

`supports_check_installation()` → `true` for JupyterEngine.

- [ ] **Step 1: failing J1–J3. Step 2: FAIL. Step 3: implement.
  Step 4: J1–J3 + workspace PASS. Step 5: commit.**

---

## Phase 7 — close-out

### Task 11: verification, ledger reconcile, strand bookkeeping

- [ ] **Full verification:** `cargo xtask verify` (full — quarto-core changed,
  WASM leg affected; capture to a log file per the long-command convention:
  `cargo xtask verify > /tmp/plan10-verify.log 2>&1` then inspect tail/grep).
- [ ] **End-to-end record (mandatory):** run
  `cargo run --bin q2 -- check` inside a marimo-fixture temp project AND a
  plain dir (native engines only); paste both observed stderr transcripts into
  this plan file under "## E2E transcripts", with the note that output was
  inspected. Verify against Q1's structure (research §1c).
- [ ] **Deviations reconcile:** re-read D-1…D-9 against the implementation;
  any new deviation discovered during implementation gets a D-number here and
  a line in the research doc — never silent.
- [ ] **Checklist reconcile:** verify every `- [ ]` above against reality
  (per the finishing-a-plan rule), correct stale ticks, commit the plan file.
- [ ] **Follow-up strands** (braid, `discovered-from:bd-4qflzhwh`):
  (a) `quarto check` fixed sections (info/versions/install) full content — D-2;
  (b) `--output` JSON mode + `--no-strict` — D-3;
  (c) TS-engine `checkRender` host→Rust callback — D-5;
  (d) Q1 hidden `capabilities` command reusing `find_python`/`python_capabilities`
      (research: q1-engine-cli-survey recommends it ride Plan 10's probes).
- [ ] **Strand:** `braid comment bd-4qflzhwh "<plan path + status>"`; close only
  when all tests pass (test-suite rule).
- [ ] **CLAUDE.local.md:** set the Plan line to this file.
- [ ] **Do NOT push, do NOT merge to the epic branch** — report to Gordon for
  review + merge decision (finishing-a-development-branch).

## E2E transcripts

*(filled by Task 11)*
