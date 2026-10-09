---
title: 'Plan: Runtime gates (pandoc-host H0)'
date: 2026-10-01
description: 'Checks in browsers that `pandoc.wasm` runs in WASI command mode and that the vendored Lua filters give output equal to native pandoc, as a stop-or-continue gate before the host is built.'
---

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (Standing decision 5, D2, D4, D9)
**Depends on:** request R0 (capture wrapper, constants file, extractor CLI). **Unblocks:** request R1 and host H1 (human checkpoint); H7.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** R0's tasks through the extractor CLI are ticked on `feature/pandoc-wasm` (R0 itself stays open: its hashable-request task runs meanwhile). If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H1 (its Start gate decides whether it can begin).

## Overview

Retire the two mechanism risks before the seam and the host are built: whether the 3.11 wasm runs in WASI command mode (`_start`) in a real browser, and whether the vendored Q1 Lua runs under wasm pandoc on real pandoc-profile inputs. Node already shows both working (design: What the spikes and the audit established); this phase repeats them in browsers on captured native inputs and answers the open questions. Mostly throwaway verification code in the spike area.

## Decisions

- Pass criteria and the stop limit below are fixed in advance. **Fallback if command mode is unusable:** reactor mode with the Lua env preamble, i.e. D2(b) becomes the baseline.
- The Rust request owns the whole argv including `argv[0]` and any RTS options.

## Checklist

### Tasks
- [x] **Command-mode check in a browser, with numbers** (Chromium and WebKit via Playwright 1.60, results in the evidence file §11; Firefox is the separate by-hand box below). The 3.11 wasm exports `memory, __wasm_call_ctors, _start, convert, query, malloc, hs_init_with_rtsopts` (no `_initialize`); `@bjorn3/browser_wasi_shim` 0.4.2's `start()` converts `WASIProcExit` into the exit code, and its `poll_oneoff` supports one clock subscription only. Run `_start` with real argv and env in a Chromium Worker with `-L main.lua` and the nested share tree, then in WebKit and Firefox by hand. Pass criteria (Chromium; WebKit and Firefox by hand need only the observable ones, docx produced and the exit code): the docx is produced; the exit code arrives through `WASIProcExit`; stderr is captured whole by a custom `Fd` (the spike's `ConsoleStdout.lineBuffered` loses a final partial line); `+RTS -M` is honoured (exit 251 when exceeded); `wasi.wasiImport.poll_oneoff` is wrapped to record return codes and call counts, and zero non-zero returns is the criterion (with `{debug:false}` the shim fails silently); a document with a non-ASCII name in argv and mounted paths works with `args_sizes_get` overridden to UTF-8 byte lengths (the shim's counts UTF-16 units). Record fresh-instance first-docx latency, Lua startup and memory (`instance.exports.memory.buffer.byteLength` after the run) in the browser; these numbers are informational, not pass criteria (the reactor numbers are not a comparable baseline). A pre-plan run on a hand-wrapped native docx capture already passed every observable criterion in Chromium 148 and WebKit 26.4 (evidence §10); the replay needs `argv[0]` and the empty dependency file, and `_start` runs once per instance. H0 repeats it on R0's recordings, pptx, epub and typst included.
- [x] **Lua-under-wasm gate** (24 of 24 recordings equal in Chromium and in WebKit; evidence §11). Replay the captured native inputs from R0 (the pandoc-profile pipeline, *not* the preview-format AST the earlier byte-identical result used) through pandoc.wasm with `-L main.lua`: docx first, then pptx, epub and typst (compared as `.typ` text), over the R0 fixtures. Pass = R0's extractor reports equal under the normalization the P7 goldens use, over a listed fixture set; any other diff fails. A failed Lua gate (as opposed to command mode) is reported and a human decides between a smaller Lua subset and hiding the feature. The native environment decides SVG fixtures (with `rsvg-convert` installed native embeds a PNG), so SVG is excluded from the byte comparison or the native env is pinned.
- [x] **Open questions the gate settles** (all but the Word-open question, which is its own box below): whether Word opens a docx with an SVG image (D9: strip, rasterize in Rust or JS, or accept alt text; default alt text if Word opens the file); whether the crossref-index filter writes `.quarto/crossref-index.json` (a docx project replay did not see it; `writeIndex` looked gated off); `mediabag_filter` (`main.lua:598`) and `writeIndex` (`main.lua:714`) gating; Lua startup time for ~1.3 MB of filters in the browser.
- [x] Write the numbers and decisions into the evidence file (`research/2026-10-01-pandoc-wasm-evidence.md`).
- [x] **Firefox by hand** (Firefox 157, run 2026-10-01; 24 of 24 equal, evidence §11; Playwright's Firefox cannot launch here): `WASM=<pandoc.wasm> node serve.mjs` in `claude-notes/research/2026-10-01-pandoc-wasm-spike/h0/`, open `http://localhost:8140/?b=firefox`, wait for `FINISHED`, then `node compare.mjs firefox`. Pass = the observable criteria: docx produced and the exit codes.
- [x] **Word opens the SVG docx** (Gordon, 2026-10-01: opens, shows alt text in place of the image; D9 = accept alt text; evidence §11): open `h0/out/chromium/svg-docx/svg.docx` in Word (regenerate with `node drive.mjs chromium callouts-docx` if `out/` is gone); `png-control.docx` is the control. 

**Stop/rethink gate:** a bounded effort of 3 working days (or an equivalent attempt budget for an agent) to make docx run; if it does not, **STOP** and revisit (smaller Lua subset, or hide the feature) before request R1 and H1.

## Verification

The pass criteria above, on Chromium (Worker) and by hand on WebKit and Firefox; results recorded. No CI changes in this phase.

## Exit

Human review of the results, including the Word-open, WebKit and Firefox checks (and R0's hashable-request recommendation) before request R1 and H1 start.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS). This phase changed no Rust and no hub-client or ts-package code (harness under the research spike dir, notes), so no suite applies; say so in the report.
- [x] **STOP:** human review of the H0 results, the manual Word-open, WebKit and Firefox checks and R0's hashable-request recommendation, before R1 and H1 start.
- [x] Checklist reconciled (re-read 2026-10-01: evidence §11, the h0 harness and every task tick are on `feature/pandoc-wasm`; no tick needed correcting): this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/h0-runtime-gates` in workspace-7 (cut from `feature/pandoc-wasm`); the task work is landed on `feature/pandoc-wasm` (fast-forwarded at `4d7500b12`), the branch stays open for the manual boxes and the STOP.
- Tasks ticked: command-mode check (Chromium, WebKit), Lua-under-wasm gate, open questions (except Word-open), evidence file §11. Firefox 157 by hand passed. Word opens the SVG docx (alt text shown; D9 = accept alt text). Open: the Close-out STOP (also R0's hashable-request recommendation, which is still open on the R side).
- State and gotchas: harness is `claude-notes/research/2026-10-01-pandoc-wasm-spike/h0/` (README has the commands). It needs pandoc 3.11 wasm (`gh release download 3.11 -R jgm/pandoc`, SHA matches `resources/pandoc-wasm.json`) and `cargo build --release -p quarto-output-extract`. Results: 24/24 equal in both browsers, exit codes 0/83/6/1/251 as in Node, `poll_oneoff` never called, UTF-8 `args_sizes_get` override needed for long non-ASCII argv, Lua startup ~70 ms in browsers. `npm install` at the repo root rewrites `package-lock.json` (drops platform-specific optional entries): `git checkout --` it. The SVG docx has an `svgBlip` and no PNG fallback.
- Next step: Gordon reviews the evidence §11 together with R0's recommendation, then says the STOP passed; then tick the STOP, reconcile the checklist, rebase onto `feature/pandoc-wasm`, `git -C ../workspace-3 merge --ff-only pandoc-wasm/h0-runtime-gates`, and retire the branch to `parking/workspace-7`.
- **STOP passed (Gordon, 2026-10-01: "i approve")** for the H0 results and R0's hashable-request recommendation. H0 is closed and landed; branch retired to `parking/workspace-7`. Next in lane: H1 (its Start gate: R1's contract landed; R1 is lane R's). R0's own STOP box is lane R's to tick.
