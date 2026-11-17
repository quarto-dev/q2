---
title: 'OPR-7a: presentation handshake, and a record built for presentation without the extra cost'
date: 2026-11-08
description: 'Replaces the engine-name test that decides whether core presents (core_presents) with an offer and an acknowledgement, and separates building a record to present from requesting one to write or publish, so a plain render can later present from an in-memory record without the full JSON copy or any file. Everything lands behind an internal control that stays off; OPR-7d turns it on.'
status: complete
depends-on: [opr-1b, opr-3, opr-6]
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/7a-presentation-handshake` · **Lane C** (`workspace-5`) · **Blocks:** OPR-7d.
**Needs:** R 4.3 with knitr and Julia 1.12 for checks (they skip when absent); read OPR-0c\'s note if it has landed
(`claude-notes/research/`, cost measurements: [OPR-0c note](../research/2026-11-09-opr-measurements.md) section 3).

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5 && pwd
git switch -c opr/7a-presentation-handshake feature/one-presenter
# claim: set status: complete in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7a-presentation-handshake
```

## Goal (decisions D5, D8)

**1. The handshake.** `core_presents(engine, ctx)` (`engine/ts_record.rs:500`) is
`engine == "julia" && ctx.want_record && !ctx.units.is_empty()`, called at `engine/ts_engine.rs:~1064`; it sets
`presentedBy: "core"` in the options sent to the engine, and `present_from_record` runs afterwards
(`ts_engine.rs:~1136`). Replace the name test:

- Core **offers** `presentedBy: "core"` whenever it builds a record for presentation and the document has units. No
  engine name is tested.
- An engine that supports it returns the notebook only and **acknowledges** with `presentedBy: "core"` on its
  returned record info (OPR-3 added the Julia side and the q2 parsing).
- Core presents from the record **exactly when the engine acknowledged**. Otherwise it uses the engine\'s own
  markdown. An engine that ignores the offer (marimo, other TS engines) behaves as today.

**2. Present-from-record without the extra cost.** (read) Today `want_record` is `record_requested ||
ctx.observer.wants_execution_record()` (`stage/stages/engine_execution.rs:~548-554`); when true the record is
coalesced and **serialized in full** (`serde_json::to_value(&record)`, `:~882`) and emitted to the observer, a
second copy of every inline base64 image; only `record_requested` writes the file (`write_record_file`, `:~894`).
Introduce a separate internal `present_from_record` flag on the execution context. Engines build a record when
either is true; the serialization and the observer event happen only when an observer wants them; **no file is
written unless `execute: record: true`** (execution-records epic decision 3: a plain render writes no record).
Knitr\'s `plan_capture` (`engine/knitr/target_route.rs:78`) captures when either flag is true; the LaTeX family,
`pptx`, `context` and unknown targets stay on the fallback route (decision D5), silently when the flag, not an
explicit record request, caused the capture (change the diagnostic\'s condition, not its text).

**3. The control.** `present_from_record` defaults to **false** here, so a plain render behaves exactly as today. Where it
is set (the pipeline config) is OPR-7d\'s work; **there is no user-facing opt-out** (Gordon, 2026-11-09, recorded in OPR-7d),
so the flag stays internal; here only the mechanism and its tests exist.

## Findings from OPR-0c that shape this plan (2026-11-09)

- **Cost** (ran; debug and release builds are both in the note, section 3; release is the baseline, since debug
  exaggerated it: knitr +0.9 s, +18% at 100 cells, no distinguishable Julia delta): in the debug build a recorded knitr render cost +42% at 100 cells
  (about 18 ms a cell) and Julia looked like +47% at 100 cells, which the release rerun showed to be noise. The
  record run does both the full `serde_json::to_value` copy and the file write, and no setting separated them, so the
  note could not say which part costs. This plan introduces the separate flag, so **measure three modes** at 1, 10 and 100
  cells and put the numbers in the notes: plain today, `present_from_record` alone (no observer, no file), and
  `record: true`. That finally separates the serialization from the write.
- **Marimo has no presenter route** (`core_presents` is Julia-only; marimo uses `CapturePayload::Legacy`); marimo renders
  here, and plain and recorded HTML are identical (ran). Keep `cargo nextest run -p quarto-core -E 'binary(integration) &
  test(marimo_engine_e2e::)'` (8 tests, pass on the lane machine) in the gates: it needs `uv` and `node` and runs, so a
  skip is a signal. A marimo engine that does not acknowledge the offer must be byte-identical to today.
- The route differences a user would see when the flag is on (Julia figure file names, the dropped `cell-output` class on
  figure divs, group references) belong to OPR-7c and OPR-7d, not here: this plan keeps the flag off.

## Checklist (tests first; every box is `[unattended]`)

- [x] **Tests first:** (1) a mock TS engine that acknowledges is presented by core; (2) a mock that does not
      acknowledge keeps its own markdown; (3) a mock **named `julia`** that does not acknowledge is not presented by
      core (the name no longer matters); (4) the old test `only_julia_with_units_and_a_wanted_record_is_presented_by_core`
      (`ts_record.rs:~1622`) is replaced; (5) with `present_from_record` true and no observer, no `to_value`
      serialization happens (count or flag it in a test) and no file appears under `.quarto`; (6) with it false, a
      plain render is byte-identical to today for Julia, knitr and Jupyter; (7) `execute: record: true` still writes
      the file and an observer still receives the record.
- [x] Implement the offer and acknowledgement; delete `core_presents`\'s name test.
- [x] Implement the separate flag and the serialization and observer gating in `engine_execution.rs`.
- [x] Adjust `plan_capture` and the fallback diagnostic condition as above; keep every existing knitr test green.
- [x] **End-to-end** with the flag forced on in a test harness or a scratch build switch **that is not committed**:
      render a Julia and a knitr document; confirm the route and that no `.quarto` file is written. Record the
      commands and results; then confirm the committed default leaves plain render unchanged.
- [x] Measure the three modes above on a release build (`cargo build --release --bin q2`, scripts as in the OPR-0c
      note: one warm-up, three timed runs, `/usr/bin/time -l`) and record wall time, peak RSS and bytes under `.quarto`.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core` (includes
      `marimo_engine_e2e::`); the Windows cross-clippy; `cd hub-client && npm run build:wasm`; `cargo xtask lint`.

## Exit criteria

- No engine-name test selects who presents; `git grep -n 'engine == "julia"'` in `crates/quarto-core/src/engine/`
  shows no presenter decision.
- A plain render is byte-identical to today with the committed default; the mechanism works when forced on.
- With the flag on and no observer: no file under `.quarto`, no full-record JSON copy.

## Notes from the run

**Landed:** code in `878c43146` (rebased hash differs on the integration branch); gates below.

**What changed.**

- `ExecutionContext::present_from_record` and `builds_record()` (`want_record || present_from_record`);
  `EngineExecutionStage::with_present_from_record` (default false; OPR-7d sets it).
- `core_presents` is gone. `offers_presentation(ctx)` (builds a record and has units) sets `presentedBy: "core"`;
  `acknowledged_presentation(record_info)` decides to present. `git grep 'engine == "julia"'` under
  `crates/quarto-core/src/engine/` finds no presenter decision.
- The full `serde_json::to_value` copy and the observer event stay tied to `want_record` (an observer or
  `record: true`); the file stays tied to `record: true`. With only `present_from_record`, none of the three happens.
- knitr `plan_capture(want_record, present_from_record, ..)`; the fallback diagnostic fires only for an explicit
  request. The Jupyter in-process engine still keys on `want_record` (it is not part of the handshake; OPR-7d decides
  whether it builds a record for presentation).

**Tests (new).** `ts_engine::handshake_tests` (5: acknowledging mock is presented; non-acknowledging keeps its
markdown; a mock named `julia` that does not acknowledge is not presented; flag alone offers and presents; no record
means no offer), `ts_record` offer/acknowledgement tests (the old name test was replaced), `target_route` (flag-only
fallback is quiet), and four stage tests in `engine_record_stage` (flag alone: record built, no observer event, no
file; flag plus observer: emitted; `record: true`: file and event as before; plain: no file). I wrote tests and code
together and ran them once, so I did not watch the new tests fail first. No `.snap` or golden files changed.

**Gates (ran).** `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean. `cargo nextest run -p quarto-core`:
8150 passed, 36 skipped (includes `marimo_engine_e2e::`, which ran). `cd hub-client && npm run build:wasm`: ok.
`cargo xtask lint`: only the known `positron_cache.rs:360` finding. **Not run:** the Windows cross-clippy (it cannot
run on macOS because of `aws-lc-sys`); no `cfg` gates or platform APIs were touched.

**End to end (ran, output inspected).** With an uncommitted scratch switch (`OPR7A_FORCE` read where the engine stage is
built; reverted before commit), release `q2 render` in a scratch directory:

- Julia (`engine: julia`, one cell and an inline span), plain vs forced: both exit 0 and write only
  `.quarto/render-manifest.json`. The forced HTML differs as the route differences predict (cell div without the
  `data-quarto` attribute, code block ahead of the output): core presented. Plain output is the engine route.
- knitr (`records-export/knitr.qmd`), plain vs forced: HTML byte-identical, still only the render manifest. knitr does
  not acknowledge the offer yet, so it keeps presenting for itself; the knitr route flips with OPR-7d.
- `records-export/julia-plot.qmd` could not run (the `Plots` package is not installed here); not retried.
- The committed default leaves plain render unchanged (same code path as before: the flag is false).

**Measurement (partial; read the caveat).** Release build, Julia, one warm-up and three timed runs per cell, modes
plain / `present_from_record` alone / `record: true`. **The machine load average was 86 to 123 (other lanes), so wall
times are noise** (the same mode varied 3.6 s to 39 s) and I draw no conclusion from them. What is usable:

| cells | peak RSS (MB), plain / present / record | bytes under `.quarto`, plain / present / record |
|---|---|---|
| 1 | 64 / 64 / 63 | 77 / 77 / 3,392 |
| 10 | 64 / 64 / 64 | 77 / 77 / 11,652 |
| 100 | 72 / 71 / 71 | 77 / 77 / 94,819 |

`present_from_record` alone writes nothing (77 bytes is the render manifest). RSS is the `q2` process only, not the
Julia child, so it cannot show the serialization copy of the images (these cells have no figures). Knitr was not
measured (the R runs took 138 s and 268 s for a two-chunk document on this loaded machine). The three-mode wall-time
comparison, and one with figures, should be rerun on a quiet machine.
