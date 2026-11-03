---
title: 'RIU-0 spikes: findings'
date: 2026-11-03
description: 'Per-spike results for the records-in-use spikes (S0 to S7): the question, the exact invocation, how the output was inspected, a go or no-go, and what was not tested.'
status: done  # S0 to S7 done 2026-11-03; the plans RIU-1 to RIU-6 are not yet rewritten (see the plan, After the spikes)
---

**Plan:** [RIU-0](../plans/2026-11-02-riu-00-spikes.md)
**Epic:** [records-in-use](../plans/2026-11-02-records-in-use-epic.md)

## S0: environment

- **q2 build:** `cargo build --bin q2` in `.worktrees/workspace-4`, dev profile
  (optimized + debuginfo), commit `421fb165d` on `feature/records-in-use`, cold build 6m36s.
  Binary: `target/debug/q2` (`q2 (quarto 2) 0.39.0`). This build has **not** had the preview
  WASM chain run, so it is valid for `q2 render` only, not for judging `q2 preview`.
- **How inspected:** each trivial document was rendered with `target/debug/q2 render <doc>.qmd`
  from a scratch directory, and the HTML was grepped for the expected output cell.

| Runtime | Version | How enabled | Trivial render |
|---|---|---|---|
| Python (Jupyter) | 3.12 in a uv venv (`jupyter-core ipykernel matplotlib pandas`) | `uv venv` in the scratchpad; `ipykernel install --prefix <venv>` so `~/Library/Jupyter` is untouched; venv `bin` on `PATH` | `jupyter: python3`, `print(1+1)` gives `<code>2</code>` |
| Julia | 1.12.7 (juliaup); kernelspec `julia-1.11` exists | `engine: julia` | `1+1` gives `<code>2</code>` |
| R (knitr) | R 4.3.2, knitr 1.50, rmarkdown 2.29 | `engine: knitr` | `1+1` gives `<code>[1] 2</code>` |
| R as a Jupyter kernel | Ark kernelspec at `~/Library/Jupyter/kernels/ark` (binary exists); IRkernel not installed | tried `jupyter: ark`, with and without `engine: jupyter` | **Not reachable.** q2 sends `{r}` chunks to knitr regardless (`processing file: ir2.rmarkdown`) |
| Browser (Playwright) | 1.60.0, Chromium 148.0.7778.96 | already installed in `~/Library/Caches/ms-playwright` | Launched headless from `hub-client`, set content, read it back |
| Positron dev build | `/Users/gordon/src/positron-spike-s3/.build/electron/Positron.app` present, `out/` built | not launched | **Not run.** See below |

Other runtimes already on this machine: `uv`, Node 24.5.0, `jupyter` only inside the scratch
venv (not on the global `PATH`; each shell that runs Jupyter needs the venv `bin` first).

### Consequences for later spikes

- **U1:** Python, Julia and R (through knitr) all run. R-through-Jupyter is not a scenario:
  with q2 as it is, an R chunk is always knitr. S1's mixed-engine document (B10) is therefore
  Jupyter plus knitr (a Python cell and an R chunk), which is the case the EXR-15 note flags.
- **U2:** an agent can drive a headless Chromium. It cannot be assumed to drive the Positron
  Electron window; S5, S6 and S7 steps that need to see the Positron window need Gordon at the
  keyboard (or a separate attempt through Electron's remote debugging, not yet tried).
- **Not tested in S0:** that the hub-client e2e Playwright setup itself launches
  (`hub-client/e2e/`; only a bare Chromium launch was checked), and whether the Positron dev
  build launches.
- **Go or no-go:** go. Nothing blocks S1.

## S1: records through the real q2 preview and hub

- **Build:** `target/debug/q2` and `target/debug/hub` at `595ced696` (dev profile). The preview WASM
  chain was run first (`npm run build:wasm`, `cargo xtask build-q2-preview-spa`,
  `cargo build --bin q2`). The hub client for B12 came from `npm run build:local-prod` on the
  same tree at 01:41 local time (B13: not the stale `hub-client/dist` of Sep 3 that was there
  before). The D1 fix below was built into `q2` afterwards; the WASM does not contain it
  (native-only change).
- **How inspected.** Browser: headless Chromium through Playwright, reading the preview
  iframe's DOM text, image and table counts, and the host page's banner and buttons; one
  screenshot of the hub client. File: cache files decompressed with `gunzip -c | jq`. Log:
  `-v` output of each server. Scratch drivers (`steps.mjs`, `pty_provide.py`) live in the
  session scratchpad and are not committed.
- **Fixtures (kept):** `crates/quarto-preview/tests/fixtures/records-in-use/` (`basic`, `echo`,
  `mixed`, `knitr`, `julia`, `marimo` (see B10), `project/`).

| # | Scenario | Result |
|---|---|---|
| B1 | Gate explicitly off (`QUARTO_ENGINE_RECORD_WRITE=0`), first open | **Not the control the plan expected.** `captures/` holds `<key>.bin` **and** `<key>.record.bin`. The gate decides only the hub document payload (`capture_driver.rs:388`, `execute.rs:361`); `cache.rs` writes the record file whatever the gate says. Not decoded: which payload the local preview's hub doc held with the gate off. |
| B2 | Gate on, first open | Text, figure (1 `img`), HTML table, error with traceback, and the inline span (`42`) all show. One kernel start. Record: nbformat 4.5, `quarto_record_version` 1, five units (4 `cell` + 1 `inline`), content and exec hashes on each, outputs `stream`, `display_data` (`image/png`, `text/plain`), `execute_result` (`text/html`, `text/plain`), `error`, `execute_result`. Stream `text` is a **string**, not an array of lines. |
| B3 | Prose-only edit | Outputs stay, no banner, no button, no kernel, no new cache file. Reuse in a live session comes from the hub's path-keyed `CaptureRef`; the content key is not consulted. With no ref (a cache copied into a fresh `--data-dir`), the same prose edit changes the key, misses, and **re-runs the kernel** (cache 2 files to 4). U4 first half: yes. |
| B4 | Code edit, then Re-execute | Edited cell loses its output (source only), the other cells keep theirs, banner "Code has changed since the last capture." with a **Re-execute** button. Clicking it refreshes the cell and writes a second cache entry. |
| B5 | `echo: false` to `echo: true` | Code toggles on, output stays, no banner, no new cache file, no new kernel. |
| B6 | Inline span edit | Span reverts to raw `` `{python} 6 * 8` ``, cells keep outputs, banner shows. |
| B7 | Restart, same `--data-dir` | No kernel start, outputs shown from the persisted hub doc and capture ref. **Defect D1 (below).** Without `--data-dir`, the store is a tempdir and is gone on exit (not run separately; it is the default `TempDir` at `preview.rs:248-255`). |
| B8 | `preview.engine` | `off` with an existing ref: records show, no kernel, no banner, an edited cell loses its output silently. `off`, fresh: no capture, inert source. `POST /api/preview/re-execute` under `off` returns 202 and **starts a kernel and writes a capture**; the SPA just does not offer the button (handed to RIU-4). `auto`: a code edit re-executes and shows the new output. |
| B9 | `record: true` | Works from front matter (`execute: {record: true}`): writes `.quarto/execution-records/basic.ipynb`. The plan's `-M 'execute:{record:true}'` and `-M execute.record:true` wrote nothing; use front matter. `q2 preview` ignores the file and starts a kernel (the known gap). |
| B10 | Engine matrix, gate on vs off | Knitr-only, Python plus knitr (mixed), and Julia: identical pages either way, every output shown; the gate-on records carry the right engine per cell (`jupyter` and `knitr` in one record). The EXR-15 concern (an engine with no record loses output in a mixed document) did **not** reproduce. Marimo needs the extension engine (the committed `extensions/marimo` fixture, with `node` and `uv`); a bare `python.marimo` fence runs as Jupyter, so the first marimo attempt was invalid and was redone. Marimo: output `2` with the gate on and off; its record output is `display_data` `text/x-quarto-markdown` (matters for S7). |
| B11 | Failure modes | Unwritable cache dir: warns "write failed; capture still emitted", page complete. Corrupt `<key>.record.bin`, and a record with `quarto_record_version` 99: warns "unreadable; using the captures alone", page complete, no kernel. `<key>.bin` deleted, record kept: miss, kernel runs, both files rewritten. `.quarto` a regular file: `q2 render` exits 1 with "Failed to create .quarto directory", with or without `record: true` (earlier than the `engine_execution.rs:864` write the plan expected). `.quarto/execution-records/` read-only with `record: true`: exit 1, "writing the execution record ... Permission denied", **no HTML written** (handed to RIU-1). |
| B12 | Hub path | Local `hub` (project mode, `--allow-insecure-auth`) with the proxy and sandboxed-preview servers from `scripts/`; `provide-hub --token dev --file basic.qmd` through a pty, consent answered `1` after the driver checked the review file for the fixture's code. Gate **off** on the provider: the stored doc is the legacy capture array; hub client shows all outputs except the inline span (raw). Gate **on**: stored doc is the record (nbformat 4.5, five units, decoded from the hub's storage); hub client shows all outputs including `42`. Prose edit keeps every output; code edit drops only the edited cell's output. The hub client offers no Re-run, only "Clear results...". MIME `application/x-engine-record+gzip` is the constant at `quarto-hub/src/resource.rs:168`; **not observed on the wire** (automerge compresses the text). Share URL for the client needs `server`, `file`, `name` and `ephemeral=true`. |
| B13 | Hub client from this commit | Yes; see Build above. |

### Defect D1 (fixed): staleness not recomputed at startup

With a persisted `--data-dir`, `record_eager_captures` skipped any path that already had a
capture ref, and `recompute_staleness` ran only from file-change events. So code edited while
the server was stopped rendered as fresh (the edited cell correctly had no output, but no
"Code has changed" banner and no Re-execute button), and a stale flag left from the previous
session survived after the file was reverted.

- **Tests first:** `restart_marks_existing_capture_stale_when_code_changed_offline` and
  `restart_clears_persisted_stale_flag_when_content_matches`
  (`crates/quarto-preview/src/capture_driver.rs`). Both failed as intended before the fix
  (`Some(false)` where `Some(true)` was expected, and the reverse).
- **Fix:** the "capture already recorded" branch calls `recompute_staleness` for that path.
- **After:** `cargo nextest run -p quarto-preview`: 314 passed, 1 skipped.
  `cargo clippy -p quarto-preview --all-targets -- -D warnings`: clean.
- **End to end:** rebuilt `q2`; started preview with the gate on, stopped it, edited a code
  cell, restarted on the same `--data-dir`: no kernel, banner "Code has changed since the last
  capture." and the Re-execute button present. Output inspected in the browser.

### Handed to RIU-1

- The gate does not cover the local cache: `<key>.record.bin` is written regardless
  (B1). Decide whether the gate should cover it, and say so when describing "gate off".
- A failed `record: true` write aborts the whole render with no HTML (B11). The option is
  opt-in, so failing loudly is defensible; decide and document it.
- The writer flip: nothing about the local preview needs a hold-back (B2 to B8, B10, B11 all
  behave with the gate on), so the local preview is safe to flip. The hub path is S3's call.

### Handed to RIU-2

- A record-only entry loses everything if the record is unreadable: B11 (corrupt record or a
  future version) survives only because the legacy `<key>.bin` is still there.
- Without a persisted ref, the cache is content-keyed and any prose edit misses (B3).
  A persisted `--data-dir` hides this behind the path-keyed ref.

### Handed to RIU-4

- `POST /api/preview/re-execute` is not gated by `preview.engine: off` (B8).

### Not tested

- The hub client's own staleness for an inline span edit (B6 was local preview only).
- A real browser other than headless Chromium; the Positron path (S5 to S7).
- Without `--data-dir` across a restart (relied on `preview.rs:248-255`).
- R as a Jupyter kernel (S0).
- **Go or no-go:** go. Records work in the real preview and hub with the gate on; D1 is fixed.

## S2: a record-only entry through the whole preview path

- **Branch:** `spike/riu-s2-record-only`, cut from `feature/records-in-use` at `dbe1d0315`, never
  merged. Commit `861176227` is (a) the record-only entry; `ce3961571` is (b) the path-keyed
  pointer, stacked on it. Build: `target/debug/q2` (dev), gate on
  (`QUARTO_ENGINE_RECORD_WRITE=1`). No WASM rebuild: nothing in the change reaches the SPA.
- **How inspected.** Same Playwright and file inspection as S1; test runs from nextest.

### (a) The record-only change

What was changed (`cache.rs`, `capture_driver.rs`, `re_execute.rs`, `quarto-hub-provider/src/execute.rs`):

- `record_capture_run_cached`: a hit is a readable `<key>.bin` **or** a record with engines in
  `<key>.record.bin` (the capture list is then empty). A run is stored when it has captures or a
  record with engines. With the gate on and a record present, the legacy `.bin` is not written.
- Three new-style guards replace `!run.captures.is_empty()`: `record_one`
  (`capture_driver.rs`), `perform_re_execute` (`re_execute.rs`) and `run_and_store`
  (`execute.rs`), via `run_has_content` (captures non-empty, or a record with engines). The
  `engines=` log line in `record_one` reads the record's engine names when there are no
  captures.

| Check (gate on, `basic.qmd`) | Observed |
|---|---|
| First open | One kernel start. `captures/` holds `<key>.record.bin` only. Page: every output, 1 image, 1 table, inline `42`. |
| Restart on the same `--data-dir` (ref path) | No kernel, `captures/` unchanged. |
| Record-only cache copied into a fresh `--data-dir` with no ref | No kernel; page shows every output. A record-only entry is a hit with no legacy file. |
| Prose edit, then code edit (live) | Prose: all outputs stay, no banner. Code: the edited cell loses its output, banner "Code has changed", others keep theirs. |
| Re-execute click (record-only store) | Refreshes the cell; `captures/` holds two `.record.bin` and no `.bin`. |

**Tests** (`cargo nextest run -p quarto-preview -p quarto-hub-provider --no-fail-fast`, 365 run,
5 skipped):

- Spike, gate off: 365 passed.
- Baseline (no spike), gate on: 365 passed. So the gate by itself breaks no existing test.
- Spike, gate on: **one failure**, `cache::tests::cached_wrapper_stores_and_returns_the_record`,
  at its last assertion (`second.captures.len() == first.captures.len()`: 0 against 1). A
  record-only hit has no legacy captures by construction; the test encodes the old assumption.
  With the pointer added, one more: `cache_hit::second_eager_run_with_same_cache_dir_hits_cache_skips_engine`
  counts the entries of the cache directory (2, not 1), because of the `by-path/` directory.
- **The survey's list did not materialize.** `eager_capture.rs`, `cache_hit.rs` (apart from the
  count above), the provider's `fetch_captures` helper and the `capture_driver.rs` unit tests all
  use the passthrough engine, which builds no record, so a record-only entry never arises in
  them. They would break only under a change that also stops producing legacy captures, which
  this spike did not do (the engine still returns them in memory; only the cache file and the
  guards changed). `gate_defaults_off_and_parses_overrides` is a pure parse test and is
  unaffected.

**What a record-only entry requires of the rest of the code (the scope RIU-2 inherits):**

- `encode_capture_payload` writes a record doc only when the gate is on; with the gate off and
  no captures it writes an **empty legacy list**. So a record-only entry is meaningful only
  with the gate on, and the flip is what makes it the normal case.
- `recompute_staleness`: its Legacy arm (`capture_driver.rs:264-272`) assumes captures; a
  record-only entry is a `Record` payload and takes the Record arm, so no change was needed.
  It is reached only when the hub doc itself is a legacy payload, which the gate rules out.
- The `Err` that `read_cached` returns for a record or empty payload is unchanged and now
  means "no legacy file", not "miss".
- A corrupt record file is now a miss with a warning that re-runs the engine (S1 B11 survived
  that case only because the legacy file was still there).

### (b) The path-keyed pointer

- **Prototype** (`ce3961571`): `<cache>/by-path/<sha256(abs path)>` holds the last content key
  stored or hit for that path (written in `record_capture_run_cached`). `record_one`, on a miss
  of the current content key, loads that record without running the engine, writes the hub doc,
  and calls `recompute_staleness` (the D1 hook), so a prose edit stays fresh and a code edit
  shows stale. About 45 lines in `cache.rs` and 40 in `record_one`, plus passing `policy` into
  `record_one`.

| Scenario (cache copied into a fresh `--data-dir`, no ref) | Observed |
|---|---|
| Prose edited while stopped | 0 kernels, log "reused the last record for this path", all outputs show, no banner. |
| A code cell edited while stopped | 0 kernels (`manual`), banner and Re-execute button, edited cell has no output, others keep theirs. |

- **Is it needed?** Only if the cache outlives the hub index. Today it does not: the CLI always
  passes `cache_dir: None` and both live under `data_dir`, so a persisted `--data-dir` keeps
  the `CaptureRef` too, and with D1 fixed the `CaptureRef` alone reuses the record across a
  restart (S1 B7 and the D1 test). It becomes necessary if the cache moves to a per-project
  location while the store stays a tempdir. Decision for RIU-2: either leave the cache where it
  is (no pointer), or move the cache and add this pointer (and its garbage collection, which
  the prototype does not have).
- **Go or no-go:** go on (a). Record-only is valid end to end through the preview and a restart;
  the change is five functions and one test assertion. (b) is a go if RIU-2 moves the cache, a
  no-go otherwise.

### Not tested

- `provide-hub` with a record-only store (only its guard was changed; it does not use the
  cache), the share and join paths, `hub admin` classification, and the hub client reading a
  record-only entry (the hub doc is a record doc either way, so unchanged).
- A record-only entry with the gate off (rejected above: it writes an empty legacy list).
- Pointer garbage collection, and two documents sharing a path across projects.

## S3: where record docs can be stranded, and consent

- **Method.** Reading, plus one run: the `q2` on PATH (`0.36.0-nightly.20261027`, older than the
  record reader) pointed at a copy of a `--data-dir` that a gate-on build had written.
  `v0.39.0` (the latest tag) has no `CapturePayload::Record` in `quarto-core/src` (`git grep` on
  the tag found none), so no release contains the reader. Two things not done: nothing was run with
  two q2 builds as host and guest, and the hosted hub's deployed `q2_ref` was not read (Gordon
  has the `cat /etc/quarto-hub/version.json` item open).
- **Observed old-reader behavior** (the one real run): the old `q2 preview` on a store holding a
  record doc shows the whole page as **"Render Error: Failed to parse capture: JSON parse failed:
  invalid type: map, expected a sequence at line 1 column 0"**. That is worse than the runbook's
  "they show as source" (the page is replaced by an error). The runbook now says so. Inspected in
  headless Chromium. The compat matrix test (`capture_compat_matrix.rs`) models the same failure.
- **Build note:** `target/debug/q2` at this point is the S2 spike build (record-only and the
  pointer), because the branch was switched back without a rebuild. It does not affect the
  old-reader result, which depends only on the old binary and the record doc.

| Writer | Store it writes | Who reads it | Strandable by a version mismatch? | Evidence |
|---|---|---|---|---|
| Local `q2 preview`, default | Embedded hub store under `data_dir`, a `TempDir` deleted on exit (`preview.rs:248-255`) | The SPA embedded in the same binary, whose WASM comes from the `quarto-wasm-embed` copy in that same binary | **No.** Writer and reader are one binary, and the store does not outlive it | Code reading. A dev build can carry a stale WASM (AGENTS.md), which is a build-process issue, not a release one |
| Local `q2 preview --data-dir` reused across versions | The same store, kept | Whatever binary opens it next | **Yes, observed.** A downgrade (or any older binary) gets the Render Error above | The old-reader run above |
| `q2 preview --share`, host with `--join` guest | The host's store | The host's SPA, or a guest copy that is byte-identical: the guest serves assets locally only when its embedded manifest hash equals the host's, and otherwise tunnels everything from the host (`asset_manifest.rs`, `join_frontend.rs`) | **No by version.** An older guest tunnels the host's UI, so the reader is always the host's | Code reading. **Not run** with two binaries; the manifest is described as exact bytes, and the WASM filename is content-hashed |
| `q2 hub` (local) with the hub-client served beside it | The hub holds docs opaquely; the writers are the preview and `provide-hub` | The hub-client WASM served with that hub | **Yes, if the writer is newer than the client.** In `local-prod` they are built together | S1 B12 happy path (same commit). The mismatch case is the next row |
| `q2 provide-hub` to a remote hub (hosted quarto-hub.com) | The hosted hub's storage | The hub-client at the hub's deployed `q2_ref` | **Yes.** The provider is whatever `q2` the user runs; the reader is whatever was deployed by hand | The runbook; the deployment facts in the epic. The deployed `q2_ref` is **not known** to me |
| `hub admin` scan and classify | Reads only | The classifier in `quarto-hub/src/admin/classify.rs` (record-aware on this branch) | Not a writer; not strandable | Not run |

**Consequence for RIU-1.** The per-surface gate is justified, not a single global flip:
the only surfaces where an older reader can be handed a record doc are `provide-hub` and
`q2 hub` writes to a hub whose client is older, and a persisted `--data-dir` across a
downgrade. The default local preview and a `--share` session cannot be stranded by version.
So the local preview can flip as soon as RIU-1 is done; `provide-hub` needs a hold until the
hosted hub is on a `q2_ref` that has the reader.

### Consent (U13): can an import-only provider skip the gate?

**Yes, without touching the execution path.**

- The gate is consulted in exactly one place: `self.consent.review(rel_path, &review_file)`
  at `quarto-hub-provider/src/execute.rs:313`, inside `run_and_store`, after
  `materialize_project` (`:275`) and `write_review_file` (`:302`) and before any engine runs.
  `execute_document` (`:235`) only checks the path and calls `run_and_store`; the one-shot and
  watch paths both reach it through `execute_document`.
- Nothing else references consent: a search of `crates/` finds it only in `execute.rs`,
  `consent.rs`, `lib.rs`, the CLI's `provide_hub.rs` (which builds the gate from flags and the
  TTY), and the tests. The beacon and the exec channel (`exec_channel.rs`) never consult it.
- So an import path is a separate method that writes a record doc and the sidecar without
  calling `run_and_store`, so no engine runs and the gate is never reached. The CLI can build
  that provider with `AlwaysReject`, which is never consulted and fails closed if some path
  did reach it.

**Verified:** by reading and a code search. A test of an import method does not exist.

### Not tested

- The two-binary share check; the hosted hub's `q2_ref`; `hub admin` on a record doc; whether
  `q2 mcp` or any other reader opens capture docs (the TypeScript packages only carry the MIME
  constants "for tests and tooling": the payload is dispatched by shape in Rust).
- **Go or no-go:** go for a per-surface gate with the local preview first. No-go for flipping
  `provide-hub` until the hosted hub's reader is confirmed.

## S4: a Positron cache spliced in the real preview

- **Branch:** `spike/riu-s4-positron-record`, stacked on `spike/riu-s2-record-only`, never merged.
  Commit `803608487` (S2 is `861176227` and `ce3961571`). Build: `target/debug/q2`, dev, gate
  on, with the S2 record-only preview. The preview WASM was **not** rebuilt for the browser
  runs: the splice code is unchanged, so the SPA from the S0 chain is what ran.
- **How inspected.** Playwright on the preview iframe (text, image and table counts); nextest
  for the library checks; `npm run build:wasm` for the WASM check.
- **Real data.** A Positron cache file from this machine
  (`~/Library/Application Support/Positron/User/globalStorage/quarto-inline-outputs/2d70fef49b77f134.ipynb`,
  copied to the scratchpad; the original untouched, nothing committed) and its source document
  `ks3.qmd` (an R document, three cells: stream plus image, error, result). The other cache file
  there belongs to a document whose cells have since changed, so it was not used.

### What was built (all scratch)

- `crates/quarto-core/src/record/positron_record.rs`:
  `record_from_positron(&PositronImport, &[Unit], &AuthorTexts, source: Option<String>,
  owner: &dyn Fn(&str) -> String) -> Record`.
  Per unit: the occurrence is counted per content hash **across all units**, then the
  language gate (`PositronImport::outputs_for(hash, occurrence, owner(lang), lang)`) decides;
  a gated-in unit becomes a `RecordCell` with `quarto_engine: None`, `quarto_lang` from the unit,
  the real content hash, `quarto_exec_hash: ""`, `quarto_kind` cell or inline, the label from
  `author_label`, the nbformat cell id (hash, then `-2`, `-3`), the author text as `source`, and
  the outputs read from `DisplayOnlyOutputs::outputs()` (no `From` or `Into`). The record has no
  engines, `quarto_cache_version: None`, `quarto_record_version: 1`.
- **Provenance marker:** `quarto_provenance: "positron"` in the record's flattened `extra` and in
  each imported cell's `extra`. It survives a write and read (tested) and is excluded from a
  cell's options (those are the non-`quarto_` keys).
- `Record::describes_something()` (engines non-empty **or** cells non-empty), used in place of the
  "has engines" test in `encode_capture_payload` (`capture_files.rs:303`) and in the preview
  cache's hit and write filters. Without it an engineless record is treated as "no record": the
  hub doc would be written as an empty legacy list, and the cache would miss and run the engine.
- `crates/quarto-preview/examples/riu_s4_import.rs`: `hashes` and `convert` over a real document
  and cache, writing `<data-dir>/captures/<key>.record.bin` as the record-only entry.

### Results

| Check | Observed |
|---|---|
| Hash compatibility on real data | The three `quarto_content_hash` values in the real cache equal q2's content hashes for `ks3.qmd` (`7a3388dc…`, `17c954c6…`, `94baef38…`). |
| Real cache in the real preview, no kernel | `q2 preview --data-dir <entry>` on `ks3.qmd`: log "recorded engine capture(s) engines=" (empty), **0 kernel starts**, no R process. Page shows the stream (`hi`), the printed table, **1 image**, the error, and `[1] 2`. Inspected in the browser. |
| Language gate, real owner | With R owned by knitr (`auto`): 0 record cells, nothing shown. With a Jupyter owner (what an Ark route would give): 3 cells. |
| Edited cell | `1 + 1` changed to `1 + 2` in the live preview: that cell shows source only, the image and the error stay, no banner, no kernel. |
| `python.marimo` cell (synthetic document, hashes from q2) | Gets nothing: 4 record cells for 5 units. |
| Duplicate-hash pair | FIRST and SECOND outputs pair by occurrence. Editing the **first** copy drops its output and the surviving `x = 1` then shows the FIRST output (SECOND is orphaned): the occurrence rule, and the same rule Positron uses. Recorded, not hidden. |
| HTML output, image output | A `text/html` table renders as a table; an `image/png` renders as an image. |
| An edited cell and its orphan (library) | After an edit, the unit is `Stale`, the old record cell is in `Matching::orphaned`. Checked with `match_units` in a unit test; the SPA does not display the orphan list. |
| Gated-out duplicate (library) | A marimo `x = 1` followed by a python `x = 1`: the python unit gets the **second** cached cell, because the occurrence is counted before the gate. |
| Tests | 4 new scratch tests in `positron_record.rs`, all pass; `cargo nextest run -p quarto-core -E 'test(record) | test(positron) | test(capture_files) | test(capture_compat)'`: 486 passed, which includes the conformance probe `imported_outputs_convert_to_nothing_a_reuse_decision_takes`. |
| WASM | `cd hub-client && npm run build:wasm` succeeds with the new module in `quarto-core`. A bare `cargo check --target wasm32-unknown-unknown` fails in C build scripts (`mlua-sys`, `tree-sitter-*`), which is why the repo says to use the npm script. |

**Observed by the way (for U20 and RIU-5):** the error output shows raw ANSI escape sequences
(`\u001b[1m...`) in the preview, because Positron's traceback carries them. Rendering or
stripping them is an RIU-5 and S7 question.

### Answers

- **U14, the design works.** A record with no engines, empty execution hashes and the real content
  hash splices in the unchanged viewer through the content-hash-only key. No engine entry is
  needed, as the survey said.
- **U15, the gate and the occurrence count belong at conversion** with a caller-supplied owner
  function. The splice has no registry and the record has no engines, so it cannot know the owner;
  the count must precede the gate (tested).
- **U16, WASM-clean:** yes, the conversion and `import_positron_cache` build for the WASM target
  through the npm script.
- **Document-level staleness for an import is "fresh, never stale".** `record_is_stale` returns
  `Ok(false)` with no engines (observed: no banner ever appeared), and it is what the preview's
  file-change hook calls. That is harmless here (nothing to re-execute), and per-unit staleness
  comes from the splice. But `rehash_record` (`overlay.rs:38-45`) stamps a hash of an empty list
  and must never be called on an imported record; no caller does today.

### What the viewer needs to show "outputs from Positron"

Nothing in the preview or the hub client reads the provenance marker, so today a Positron-sourced
page looks the same as an executed one. The smallest addition is to carry
`record.metadata.extra["quarto_provenance"]` in the splice report's aux payload
(`SpliceReport::to_json`, `record_splice.rs:62`) and have the SPA read it to choose a banner
("outputs imported from Positron" in place of "Showing executed output"). **Not built, not tested.**

### Final signature, and what RIU-3 inherits

```rust
pub fn record_from_positron(
    import: &PositronImport,
    units: &[Unit],           // ids filled
    authors: &AuthorTexts,
    source: Option<String>,   // project-relative document path
    owner: &dyn Fn(&str) -> String,
) -> Record
```

- The three engines-assumption sites to change for real: `encode_capture_payload`,
  `cache.rs` (hit filter, write filter, `run_has_record`), and the `quarto-hub-provider`
  guard (unchanged in this spike because an import path does not use it).
- The owner function in the preview comes from the engine registry (a Jupyter-owned plain
  language only); the spike used a stand-in.
- **A question for Gordon, not settled here:** under real q2 an `{r}` chunk is owned by knitr, so a
  real Ark-produced cache like the one used here gets **no outputs** with the real owner function;
  it showed only because the spike forced a Jupyter owner. Whether R through Ark should route to
  Jupyter in q2 decides whether Positron imports help R documents at all.

### Not tested

- The imported record through the hub path (`provide-hub` or the hub client), and `auto`
  policy with an imported record.
- The "outputs from Positron" banner.
- A Python document with a real Positron cache (the only real caches were an R document, whose
  cells are Ark-run, and a stale one); the Python, HTML and image cases used a cache generated
  from q2's own hashes, so they test the pairing and rendering, not Positron's real output shapes.
- **Go or no-go:** go. Option ii (a second splice input) is not needed.

## S5: pushing outputs into an open Positron document

**Question (U17, U18).** What is the smallest change that makes cached outputs appear in an
already-open document, and where does the cache live?

**Branch:** `spike/q2-s3-inline-exec` in `/Users/gordon/src/positron-spike-s3`, commit
`6b0649ff1c` ("Push Outputs From File"). Scratch; never merged. It is stacked on the earlier S3
spike commits there. Nothing in `/Users/gordon/src/positron` was touched.

**What was built (the smaller of the two options in the plan).**

- `QuartoOutputCacheService.spikeParseIpynb(uri, text)`: validates and parses a cache-format
  ipynb with the existing private `_ipynbToCachedDocument`, without touching the cache.
- `QuartoOutputContribution.spikePushCells(cells)`: for each cell, `findCellByContentHash`
  (the same lookup the restore path uses), then `_clearCellOutputs`, add each output to the view
  zone, and `saveOutput` so it persists. Cells with no hash match are counted as skipped.
- Command **Quarto: Spike: Push Outputs From File**
  (`positronQuarto.spike.pushOutputsFromFile`, in `quartoInlineExprCommands.ts`): reads the
  path, parses, finds the editor's `QuartoOutputContribution` through
  `editor.getContribution`, calls `spikePushCells`, and reports `N matched, M skipped`.
- Not built: `reloadCachedOutputs()` and a cache-service `importCachedDocument`. The direct push
  made them unnecessary, so the comparison the plan asked for was not run.

**Invocation.** The dev build from `scripts/code.sh`, `~/docs/inline-output/python.qmd` run in
it (two code cells, `sum: 10` in the first). The pushed file was
`~/docs/inline-output/pushed-python.ipynb`: a copy of the dev build's own cache file
(`bcfd94bdc5437bc3.ipynb`) with `sum: 10` replaced by `sum: PUSHED-FROM-FILE`, so the hashes
match and the text is distinguishable. `tsc --noEmit` on the Positron tree was clean.

**How it was inspected.** Gordon watched the dev-build window and reported each result; I
inspected the cache directory and the converted record on disk. I did not drive the window.

| Check | Result (reported by Gordon) |
|---|---|
| Outputs appear in the open document without closing it | Yes |
| They persist across a close and reopen | Yes |
| A dirty (unsaved) document does not overwrite them wrongly | Yes |
| A code cell whose content changed is skipped, not given stale output | Yes |

**Where the cache lives (U18).** Verified on disk. Running `python.qmd` in the dev build
created `~/Library/Application Support/code-oss-dev/User/globalStorage/quarto-inline-outputs/bcfd94bdc5437bc3.ipynb`
(1,123 bytes, nbformat 4.5, two code cells, per-cell `quarto_cell_id` and `quarto_content_hash`
metadata, top-level `last_updated`, `quarto_cache_version`, `quarto_source`). The installed
release build keeps its cache under `~/Library/Application Support/Positron/User/globalStorage/quarto-inline-outputs/`
and that directory did not change. The file name is **not** `sha256(path)[:16]` of the
document's path (`1e7644de6b7e5d3e` for this one), so a q2 command cannot compute a document's
cache path by itself; `QuartoOutputCacheService._getCachePath` decides it.

**Same file, through q2.** The dev build's cache file converts through `record_from_positron`
(the S4 spike branch, `riu_s4_import convert ... jupyter`): `units=3 import_hashes=2
record_cells=2 notices=[]`, no kernel.

**Go or no-go.** Go. A push into an open document is one method and a parse helper; RIU-6's
first command needs no reload plumbing.

**Not tested.**

- `--user-data-dir` and `POSITRON_APPDATA` effects on the cache path.
- Outputs richer than streams (images, tables, errors, HTML): the pushed file had two stream
  outputs only. S7 covers rich ones.
- A document open in two editors (a split view), where each editor has its own contribution.
- Untitled documents and the cache transfer on save.
- Concurrent push while a cell is running.
- The `reloadCachedOutputs` and `importCachedDocument` alternatives.
- Positron's own code-quality hygiene beyond `tsc`; the commit hook raised three lint warnings in
  existing `quartoOutputManager.ts` code, none in the added lines.

## S6: how a Positron command runs q2

**Question (U19).** What is the least-effort way for a Positron command to run `q2` and get a
result back, and will the GUI app find `q2`?

**Branch:** `spike/q2-s3-inline-exec` in `/Users/gordon/src/positron-spike-s3`, commit
`afd8c75204`. Scratch; never merged. Nothing in `/Users/gordon/src/positron` was touched.

**What was built (route B in the plan; route A, the terminal, was not tried).**

- A scratch extension `extensions/q2-spike` (TypeScript, compiled to `out/extension.js`, which is
  gitignored). Command `q2spike.run`, titled "Spike: Run q2 (S6)", and a setting
  `q2spike.q2Path` (empty means resolve `q2` on PATH). It runs, with `execFile` in the extension
  host: `q2 --version`, `q2 render ~/docs/q2-spike-s6/hello.qmd`, and a binary that does not
  exist.
- A workbench command `_positronQuarto.spikeReceive` (in `quartoInlineExprCommands.ts`, through
  `CommandsRegistry.registerCommand`) that receives one JSON object and shows a notification.
  The extension calls it with `vscode.commands.executeCommand`.
- The repository pre-commit hook rejects new `.js` files (it requires TypeScript, or an entry in
  `.eslint-allowed-javascript-files` that needs CODEOWNERS review), a copyright header, and
  non-ASCII characters in source. The extension was rewritten in TypeScript to satisfy it
  rather than bypass it.

**Invocation.** The dev build from `scripts/code.sh`, command run from the command palette, in
three launches:

| Launch | Result (read by Gordon from the notification) |
|---|---|
| From a terminal, shell PATH inherited, `q2spike.q2Path` empty | `q2 --version` exit 0 in 44 ms (`q2 (quarto 2) 0.36.0-nightly.20261027`); `q2 render` exit 0 in 148 ms; missing binary `exit=null ENOENT`. I confirmed `hello.html` was written. |
| `env -i HOME=$HOME PATH=<node dir>:/usr/bin:/bin ./scripts/code.sh`, setting empty | `q2 --version` and `q2 render` both `exit=null ENOENT`. Not found by name. |
| Same minimal launch, `q2spike.q2Path` set to `/Users/gordon/.local/bin/q2` | `--version` exit 0 in 22 ms; `render` exit 0 in 97 ms; missing binary `ENOENT`. The setting was read without a restart. |

The first attempt at the minimal launch, with PATH `/usr/bin:/bin` alone, failed in
`scripts/code.sh` itself (`node: command not found`), so node must stay on PATH for the dev
build; that does not apply to an installed Positron.

**What this shows.**

- The mechanism needs no new infrastructure: `execFile` in the extension host returns the exit
  code, stdout and stderr, and the result reaches the workbench through an internal command. A
  failed lookup (`ENOENT`) is distinguishable from a failed run (a numeric exit code).
- `q2` is found by name only when the shell PATH is inherited. RIU-6 needs a setting for the
  `q2` path, and its error message must name the setting when the lookup returns `ENOENT`.
- The `q2` on this machine PATH is `0.36.0-nightly.20261027`, the same version that showed the
  full-page Render Error on a record in S3. A command that runs whatever `q2` is on PATH must
  check `q2 --version` (or a capability probe) before using any record feature.

**Go or no-go.** Go. Mechanism: `child_process.execFile` in an extension, result handed to the
workbench through an internal `_` command, `q2` path from a setting.

**Not tested.**

- A real Dock or `open -a` launch of an installed Positron. The minimal-environment terminal
  launch only approximates it; macOS resolves a login-shell environment for GUI launches, which
  that launch does not reproduce.
- Argument marshalling for the nested object, the newline and the non-ASCII characters: the
  workbench command ran and showed its notification, but I could not find the log line with the
  full payload, so I did not check those fields.
- Route A, `ITerminalService`.
- Windows and Linux, `q2.exe`, quoting of paths with spaces, and `q2` being killed on timeout.
- Streaming progress and cancellation of a long run; the spike ran two short commands.
- A configured `q2` path that is not executable (a different error than `ENOENT`).

## S7: export for Positron

**Question (U20).** What does Positron actually display from an exported record, and what is the
final transform?

**Branches.** q2: `spike/riu-s7-export`, commit `9156265db` (scratch, never merged, cut from
`feature/records-in-use`). Positron: the S5 command on `spike/q2-s3-inline-exec` in
`/Users/gordon/src/positron-spike-s3`, no new Positron code.

**What was built.** `quarto_core::record::export_for_positron(&Record, source, last_updated)` in
`record/positron_export.rs`, and an example `riu_s7_export <record> <out> [source]` that runs it.
The transform: skip inline units, units not owned by `jupyter`, extension languages, and units
whose output carries `text/x-quarto-markdown`; skip every member of a duplicate-hash group when
one member is skipped; join array data and stringify object data; drop per-output
`quarto_format`; reduce cell metadata to `quarto_cell_id`, `quarto_content_hash` and
`quarto_label`; reduce notebook metadata to `quarto_cache_version`, `quarto_source` and
`last_updated`; always emit `outputs`.

**Invocation.**

- Two real records: `q2 render basic.qmd` and `q2 render dup.qmd` with `execute: {record: true}`
  in the front matter and the Jupyter venv first on PATH. They wrote
  `.quarto/execution-records/basic.ipynb` and `dup.ipynb`. `basic` has a stream, a matplotlib
  figure, a pandas table, a caught error with a traceback, and one inline span. `dup` has two
  identical cells and a labeled cell.
- `riu_s7_export` on each, giving `basic-export.ipynb` (4 cells exported, the inline span
  skipped; `quarto_source` a `file://` URI) and `dup-export.ipynb` (3 cells; `quarto_source` a
  relative path). The record itself was also pushed unchanged as `basic-raw.ipynb`.
- In the dev build, the S5 command "Spike: Push Outputs From File" with each file, onto an open
  document with matching cells (`s7-basic.qmd`, `s7-dup.qmd`, which were not run).

**How it was inspected.** Gordon watched the dev-build window and reported each result. I
inspected the files with `jq`: the types of every data value, cell ids and hashes. I did not
drive the window.

| Output | What Positron showed (reported by Gordon) |
|---|---|
| Stream (`print`) | Text, correct |
| `display_data` with `image/png` | An image, correct |
| `execute_result` with `text/html` | A rendered table, correct |
| `error` with a traceback | `ValueError: boom` with the traceback and colors, correct |
| Inline span (`{python} 6 * 7`) | Nothing, as intended (not exported) |
| Labels | Correct on the labeled cell in `dup` |
| Two identical cells | Both showed their output |
| `basic-export` | 4 matched, 0 skipped |
| `basic-raw` (record unchanged) | 5 cells, 4 matched, 1 skipped (the inline span); the display was the same as the export |

**What the exporter changed.** Nothing visible for this document. Every data value in the real
records was already a string, because the kernel gives q2 strings rather than nbformat line
arrays, and Positron reads only the three per-cell keys and two notebook keys. So the exporter is
needed only for outputs that arrive as an array or an object: Positron calls `JSON.stringify` on
a non-string, which would put quotes around an HTML array. No such output was tested.

**Settled transform list.** Skip inline units (and any unit that is not a plain-language
Jupyter cell); skip a whole duplicate-hash group if one member is skipped; keep the three cell
keys; always emit `outputs`; stringify non-string data values; set `quarto_cache_version` to 1.
`quarto_source` does not matter on load (Positron only writes it; the push worked with a
`file://` URI and with a relative path), so it can be a relative path.

**What cannot be exported, and why.** Cells owned by an engine other than Jupyter (knitr R,
marimo, Julia) are skipped, because Positron would have its own kernel produce those outputs or
none. Inline spans are not cached cells in Positron. Anything with a rendered
`text/x-quarto-markdown` output is a q2 construct Positron cannot display.

**Go or no-go.** Go. For Jupyter Python the record is already close to Positron cache format.

**Not tested.**

- Array-valued and JSON-valued outputs (Plotly, widgets, IPython JSON mimes), where the exporter
  does change the bytes.
- Positron loading the exported file through its normal open path, instead of the S5 push
  command. That needs the document cache path, which a q2 command cannot compute (S5).
- Outputs with `quarto_webview_metadata` or per-image `width` and `height` metadata.
- Large records (this one was 30 KB; Positron has a 100 MB cache cap).
- Whether the `positron_loader` mirror in `positron_conformance.rs` agrees; the real window was
  used as the oracle instead.
- R, Julia and marimo documents, and R through Ark.
- A document where one of two identical cells was edited, so the group is partly stale.

## Unknowns register: answers (U3 to U20)

U1 and U2 are in S0. U17 and U18 are answered by S5, U19 by S6, U20 by S7.

| # | Answer | Section |
|---|---|---|
| U3 | Yes. With the gate on, records work through the real `q2 preview` and through `provide-hub` and the hub client: every output type shows, code edits go stale per cell. | S1 B2, B4, B12 |
| U4 | A live session reuses the record through the path-keyed `CaptureRef`. The content-keyed cache **misses** on any prose edit (the key is the input text) and re-runs the engine, but a persisted `--data-dir` keeps the ref so that only matters for a new session with no ref. | S1 B3; S2(b) |
| U5 | `off` skips the eager capture and the staleness hook; an existing ref still shows its record, an edited cell silently loses its output, and the SPA hides the button. The re-execute POST is **not** gated and runs a kernel under `off`. | S1 B8 |
| U6 | A persisted `--data-dir` keeps the cache files, the hub index with its capture refs, and the capture docs: a restart starts no kernel. It did not recompute staleness (defect D1, fixed). Without `--data-dir`, the tempdir is gone. | S1 B7, D1 |
| U7 | Knitr-only, Python plus knitr, Julia and marimo (the extension) all behave identically with the gate on and off. The EXR-15 mixed-document concern did not reproduce. | S1 B10 |
| U8 | Fail-soft everywhere on the preview side (unwritable cache, corrupt record, future version, deleted legacy file). A failed `record: true` write aborts the render with no HTML (RIU-1 decision). | S1 B11 |
| U9 | Works with a real local hub, `provide-hub` through a pty with consent, and a hub client built from this commit. The record doc is stored; its MIME is the constant in `quarto-hub/src/resource.rs`, not observed on the wire. | S1 B12, B13 |
| U10 | Yes: a record-only entry is valid end to end (first open, restart, cache hit with no ref, re-execute). The change is five functions and one test assertion; no test beyond that one broke. | S2(a) |
| U11 | The `CaptureRef` alone suffices while the cache lives under `data_dir`. A path-keyed pointer (about 85 lines) is needed only if the cache is moved to outlive the index. | S2(b) |
| U12 | Strandable: `provide-hub` to a hub with an older client, `q2 hub` writes likewise, and a persisted `--data-dir` across a downgrade (observed: a full-page Render Error). Not strandable: the default local preview and a `--share` session. | S3 |
| U13 | Yes. Consent is consulted once, at `execute.rs:313` in `run_and_store`; an import method that does not call `run_and_store` never reaches it. | S3 |
| U14 | The design works: engineless record, empty exec hashes, spliced in the real preview with no kernel, on a real Positron cache. | S4 |
| U15 | At conversion, with a caller-supplied owner function; the occurrence is counted per hash before the gate. | S4 |
| U16 | Yes, WASM-clean through the npm build. | S4 |
| U17 | Yes. One method on `QuartoOutputContribution` (`spikePushCells`) plus a parse helper on the cache service, matched by content hash; no reload path and no change to `_cachedOutputsLoaded`. Seen live, persisted, and unharmed by a dirty document. | S5 |
| U18 | Dev build: `~/Library/Application Support/code-oss-dev/User/globalStorage/quarto-inline-outputs/`. Installed release: `~/Library/Application Support/Positron/User/globalStorage/quarto-inline-outputs/`. Observed on disk, not read from code. | S5 |
| U19 | An extension command runs `q2` with `child_process.execFile` in the extension host and hands the result to a workbench command through an internal `_` command (same pattern as `_executeQuartoCellFormattingProvider`). `q2` is **not** found by name when Positron starts without the shell PATH; an absolute path in a setting works. The `q2` on PATH can be an old nightly. | S6 |
| U20 | Positron shows a q2 record of Jupyter Python cells correctly: stream, PNG figure, HTML table, error with traceback and colors, labels, and a duplicate-hash pair. A raw record loads with no transform for these; the exporter only matters for outputs that are arrays or objects (untested). Inline spans, and any non-Jupyter cell, are skipped. `quarto_source` is ignored on load. | S7 |

## What S1 to S4 settled, and what S5 needed at the keyboard

### What changes in RIU-1 to RIU-3

- **RIU-1.** The local preview needs no hold-back; `provide-hub` does (S3). The gate does not cover
  the local cache's record file (S1 B1), and the `record: true` write failure aborts the render. The
  runbook now says what a stranded record looks like. D1 is already fixed on this branch.
- **RIU-2.** The size is smaller than the survey feared: five functions, one test assertion. The
  pointer is optional and depends on whether the cache moves. The cache must treat an unreadable
  record as a miss only when there is no legacy file to fall back to (that stops being true once the
  legacy file is gone).
- **RIU-3.** `record_from_positron` is proven, with its signature, where the gate runs, and the
  three sites that assume engines. Open for RIU-3: the owner function from the real registry, the
  "outputs from Positron" viewer banner, and whether R through Ark routes to Jupyter.
- **RIU-4.** The unguarded re-execute POST under `off`.
- **RIU-5.** S7: a Jupyter Python record is already close to Positron cache format; the export
  step is a filter (inline units, non-Jupyter units, duplicate groups) plus stringifying
  non-string data values. Untested: array and JSON outputs.
- **RIU-6.** S5: pushing into an open document is one method on the output contribution and a
  parse helper. S6: an extension runs `q2` with `execFile` and hands the result to the workbench
  through an internal command; `q2` is not found by name without the shell PATH, so a path
  setting is required, and `q2 --version` must be checked because the `q2` on PATH can be an old
  nightly. The cache path cannot be computed from the document path, so the cache service must
  supply it.

### What Gordon needed to do for S5 (the Positron window; done, see S5)

When this was written S5 to S7 had not started; S5 has since run. They need the Positron dev build from
`/Users/gordon/src/positron-spike-s3` (`scripts/code.sh`), and an agent cannot drive that Electron
window from here. At the keyboard:

1. Launch the dev build from a new worktree cut from `spike/q2-s3-inline-exec` (not
   `/Users/gordon/src/positron`), with a Quarto document open that has already been run.
2. Watch the editor while the S5 command (to be added in that worktree) pushes cached outputs into
   the open document: do the outputs appear without closing it, do they persist across a close and
   reopen, and does a dirty in-memory document overwrite them?
3. Report where the dev build keeps its cache. The installed Positron's is
   `~/Library/Application Support/Positron/User/globalStorage/quarto-inline-outputs/` (two files
   there today, the one used in S4 among them); the dev build's is probably
   `code-oss-dev/User/globalStorage/...` (unverified).
4. For S7, the real record to export is any record from the S1 fixtures; the S4 document `ks3.qmd` is the
   one with real Ark outputs and ANSI-colored errors.
