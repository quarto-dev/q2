---
title: "Review of the records-in-use plans (RIU-1 to RIU-6)"
---

## What was reviewed

The epic, RIU-1 to RIU-6 and the RIU-0 findings note, at commit `62e98cea4`. Two read-only
sub-agents reviewed them: A on design (do the plans follow from the evidence and agree with each
other), B on implementation (are the code references real, could the tasks run test-first). I
verified the findings marked **verified** below myself, by grep and by reading code and plans.
Findings marked **agent-reported** are the sub-agents\' claims that I did not re-check; treat them as
leads. Nothing was run or built.

## Fixed in the plans (verified factual errors)

- Stale line numbers: `capture_driver.rs:388` is `:405`; `:181` is `:198`; `:379-418` is
  `:396-441`; the stale comment cited at `:370` is `:387-395`; `cache.rs:109` is `:108`;
  `execution-records.qmd:15-23` is `:17-24`. The spike-era numbers predate commit `f607ad7d2`.
- RIU-1 `record: true` failure test: with `.quarto/execution-records` a regular file,
  `dir_create` fails first (`engine_execution.rs:1109`, message "creating ..."), so the plan\'s
  assertion on "writing the execution record" (`:1118`) cannot pass. The plan now says to assert
  the first message, or to make `<doc>.ipynb` a directory.

## Findings, ranked

### High

1. **RIU-4 and RIU-5 depend on each other** (verified). RIU-5 says "Depends on: RIU-4", and RIU-4
   says "Blocks: RIU-5" while its Phase C wraps RIU-5\'s function. Not fixed: it is an ordering
   choice (see Decisions).
2. **"Omit `engine` means never execute" is false for the re-execute POST** (verified in part).
   `re_execute_handler` (`re_execute.rs:97`) takes no policy; RIU-4 gates it only on
   `preview.engine: off`. RIU-6 says demo command 3 executes nothing because `engine` is omitted.
   Agent B also reports that the handler reads process-wide `OnceLock` state, so a test cannot set
   a per-test policy (agent-reported).
3. **Provenance banner has no plumbing** (agent-reported). The banner reads the sidecar
   `CaptureRef`, and no code reads the splice report aux payload; RIU-3 Phase E needs a concrete
   route, and a `hub-client` change brings the changelog and `npm run build:all` rules, which the
   plan states only conditionally.
4. **`provide-hub --records` default `cache,q2,engine`** contradicts the provider\'s "always a
   fresh run, uncached" decision (`execute.rs:332-335`, agent-reported).
5. **The `q2 hub` writes flip has no owner** (verified). `record_writing_enabled()` has two
   production callers, `capture_driver.rs:405` and `execute.rs:361`; RIU-1\'s surface table and
   the epic\'s "all three capture writers" wording have no third writer. How a preview publishing
   to a non-embedded store is classified is undefined.
6. **Rollback and `--data-dir`**: a persisted store is stranded on downgrade and the plan only
   documents it; a hosted-hub rollback strands documents regardless of the gate (verified that
   the plan text says "documented, not gated"; severity inferred).

### Medium

7. Julia contradiction (verified): RIU-6 Phase C and exit criteria promise a Julia document,
   RIU-5 skips cells not owned by Jupyter and names Julia among them.
8. RIU-2 cache work is under-listed (agent-reported): the `.ipynb` rename breaks
   `read_cached_record`, `write_gzipped_atomic` and three cache tests; the cache wrapper fuses
   lookup and run, so an ordered source list needs it split; the cache needs a gated test seam.
   The spike did not rename the file, so it is not "ported".
9. RIU-3 hash derivation (agent-reported): the real splice path hashes sugared author text; the
   spike used raw-scan text with a hard-coded owner. The registry-derived owner is untested.
10. `recompute_staleness` (`capture_driver.rs:262`) calls `record_is_stale` on any record doc;
    plans say never to call it on an imported record. Decide: guard on provenance or accept.
11. Positron cache directory (agent-reported, from `userDataPath.ts`): `dirs::data_dir()` is
    wrong on Linux (Positron uses the config directory); VS Code\'s `URI.toString()` encodes
    differently from `url`; `--user-data-dir` and portable mode are untested.
12. Windows cross-clippy is listed only in RIU-4\'s verify steps; RIU-1, 2, 3 and 5 touch paths
    and fixtures too.
13. Session estimate: the plans sum to 9 to 10 sessions beyond RIU-0, the epic says 14 to 18
    (verified arithmetic from the plan headers by agent A; the epic\'s figure includes RIU-0).
14. Claims stronger than their evidence (agent-reported, partly verified): "offers Re-execute" on
    the hub client (S1 B12 says only Clear results); RIU-5\'s statement that Positron loads a
    hand-placed file (S7 did not test the open path); `q2 render` not consulting the gate is true
    by grep but is cited to S1 B1.

### Low

15. RIU-5 `ExportError` and RIU-4\'s "stable exit code per refusal class" have no defined causes or
    classes (agent-reported).
16. Verification environment (agent-reported): `jupyter`/`ipykernel` are not importable here; the
    `q2` on PATH is 0.36.0-nightly and cannot read records, so steps must use `target/debug/q2`.
17. The `.quarto` copy is not staleness-checked before export; Positron skips mismatches by hash.

## Rejected or not acted on

- Agent A item 13 (consistency nits): consistent on inspection; nothing to change.
- KNR-5 `fallback: bool` (agent B): unverified either way; left alone.

## Decisions for Gordon

1. Order of RIU-4 and RIU-5: build the export function first (RIU-5) and the subcommand after,
   or split RIU-4 into 4a (`--records`, `off` fix) and 4b (export).
2. Does an absent `engine` in `--records` mean effective policy `off` for the re-execute POST?
3. `provide-hub --records` default: keep `cache,q2,engine`, or `q2,engine`?
4. Is "documented, not gated" enough for a persisted `--data-dir` after the flip, or do we stamp a
   store version?
5. How is a preview that publishes to a non-embedded hub classified for the per-surface flip?
6. Is the plain `<key>.ipynb` cache rename worth its migration and test cost, now that it has
   its own decision weight (epic decision 8)?
7. Julia: in scope for RIU-6 (then RIU-5\'s skip rule changes) or out?
8. Should `export-positron` default to running the kernel (`q2,engine`) without a consent check?
9. Provenance banner: which route, and does the hub client get one?

## Resolution (2026-11-03, with Gordon)

All nine decisions were settled and written into the epic and plans in the commit that follows
this note.

1. RIU-5 owns the function and the `q2 record export-positron` subcommand; RIU-4 keeps `--records`
   and the gating, and no longer waits on RIU-5.
2. An absent `engine` in `--records` means never execute: the re-execute POST is refused when the
   policy is `off` or `engine` is not in the list.
3. `provide-hub` defaults to `--records engine`; `q2 preview` keeps `cache,q2,engine`.
4. A persisted `--data-dir` is documented, not gated; RIU-1 runs the downgrade once and records
   the recovery before the docs state it. A persisted store holds `automerge/`, `captures/`,
   `hub.json`, `hub.lock` and `.gitignore` (read from `quarto-hub/src/storage.rs`, not run).
5. No third surface: a preview publishes only to its own embedded store (`peers: Vec::new()`,
   `quarto-preview/src/lib.rs:694`). The "`q2 hub` writes" wording was deleted.
6. The cache rename to plain `<key>.ipynb` was dropped.
7. Julia is in scope; the new RIU-0b spike J settles the specifics first.
8. `export-positron` defaults to `q2,engine`, with a stderr line before it runs code and
   `--records q2` to forbid execution; no consent prompt.
9. Provenance goes through the existing `SourceInfo` system (`By::` kind, node level), proven
   first by RIU-0b spike P. The splice reparses text, so a post-walk is expected.

Nothing in RIU-1 to RIU-6 was implemented, and neither spike has been run.

## Follow-up: the agent-reported items, verified (2026-11-03, after RIU-0b)

The items above marked **agent-reported** were each checked against the tree in the session that ran
RIU-0b, and the plans were rewritten from the result. Two were wrong and were dropped. Read and run
are distinguished: everything here was **read** (grep and file reads) unless it says run.

| # | Item | Outcome |
|---|---|---|
| 1 | RIU-2: the cache wrapper fuses lookup and run | **True** (`cache.rs:230`: key, lookup, engine run and store in one function). RIU-2 Phase B2 splits it. |
| 1 | RIU-2: `read_cached_record`, `write_gzipped_atomic` and the round-trip, corrupt-entry and concurrent-writer tests are affected | **Not affected**: the file form does not change (epic decision 8). Only the wrapper's hit and store conditions change. RIU-2 says so. |
| 1 | RIU-2: the cache wrapper reads the gate, so a gate-on test needs a `_gated` seam | **Dropped, false.** The gate is read only by the writers (`capture_driver.rs:405`, `execute.rs:361`) and handed to `encode_capture_payload` as the `record_gate` boolean (`capture_files.rs:298`); `cache.rs` never reads it, and a gate-on test passes `true`. |
| 2 | RIU-1: `run_engine_stage` returns only a Result, so the fail-soft warning is unobservable | **True** (`engine_stage_driver.rs:76`, it drops the stage context). |
| 2 | RIU-1: `render_document_to_file` has no engine-registry seam | **Dropped, false.** `RenderToFileOptions.engine_registry_override` (`render_to_file.rs:128`, applied at `:500-504`), and the result carries `render_output.diagnostics`. RIU-1 uses it for the fail-soft test. |
| 3 | RIU-3: the real splice hashes sugared author text, not a raw scan; the owner must come from the registry | **True**: `author_texts(&units, sugared)` at `record/staleness.rs:51`, `scan_and_fill_ids` at `engine_execution.rs:989`; the S4 spike used `AuthorTexts::from_scanned` and a hard-coded owner. The agent cited `:31-62`, which is the whole function. RIU-3 specifies both. |
| 3 | RIU-3: `recompute_staleness` calls `record_is_stale` on any record document | **True** (`capture_driver.rs:324`). Harmless today (no engines gives not-stale), but it breaks the plan's "never". RIU-3 Phase B adds a provenance guard as a task. |
| 4 | RIU-3: `dirs::data_dir()` is wrong on Linux; the profile layer; the URI-to-name rule | **True** (`userDataPath.ts:50-115`: Linux uses `$XDG_CONFIG_HOME` or `~/.config`; portable, `POSITRON_APPDATA` and `--user-data-dir` override; the cache uses the default profile's `globalStorageHome`, `quartoOutputCacheService.ts:133`). `URI.toString()` lowercases a drive letter and encodes `:` (`uri.ts:656-720`). None run on Linux or Windows. RIU-3 Phase C lists each rule with its status, names the two fallbacks, and puts the locator in `quarto-preview`, outside the WASM-compiled `record` module. |
| 5 | The banner text is `PreviewStatusBar.tsx` and reads the sidecar capture ref | **True** (`:204`, from the `capture` prop, a `CaptureRef` with `captureDocId`, `staleness`, `state`, `lastError`). The banner is not in RIU-3; it is a decision for Gordon, and the plans state the changelog and `npm run build:all` rule that follows if it is built. |
| 6 | RIU-4: `value_delimiter = ','` splits a `dir:` path with a comma | **True in kind** (clap behavior; `main.rs` uses it for other options). RIU-4 uses a custom parser. |
| 6 | RIU-4: what a file change does is unspecified | **True**: the watcher calls only `recompute_staleness` (`lib.rs:499`), the source list runs at boot (`:449`). RIU-4 states and tests it. |
| 6 | RIU-4: the `off` guard exists twice | **True** (`capture_driver.rs:67` and `:262`). RIU-2 and RIU-4 move both. |
| 7 | Windows cross-clippy in RIU-1, 2, 3, 5 | Added (RIU-3 also for `quarto-ast-reconcile`). |
| 8 | RIU-5 `ExportError` had no cause; refusal classes unnamed | **True** (the S7 function has no failing path). RIU-5 defines two causes and a table of six exit codes; `quarto_source` is passed through, not built, so its Windows and non-ASCII branches disappear. |
| 9 | Fixture paths | Checked: `basic`, `echo`, `julia`, `knitr`, `marimo`, `mixed` and `project/` exist. **`dup.qmd` does not**, no S7 record is checked in, and `ks3.qmd` is only in another session's scratchpad; the plans create durable fixtures. `~/docs/inline-output/python.qmd` and its dev-build cache exist. |
| 10 | The epic's Goal promised a record "always available" | Reworded; RIU-6 Phase E owns the closing reconciliation. |
| 11 | Untitled, split views, dirty documents, non-Python languages | RIU-6 refuses an untitled document, skips changed cells in a dirty one, and lists split views and remote workspaces as known limits. |
| 12 | Session estimates | Recomputed: the remaining six plans are 9 to 12; with the spikes 16 to 20. |

Also found while checking: the provenance contract's catalog is out of date and `quarto-source-map` is
an external crate (RIU-0b findings, "What the catalog now holds").

