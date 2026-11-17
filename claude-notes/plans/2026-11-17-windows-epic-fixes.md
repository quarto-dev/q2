---
title: 'Windows fixes for the one-presenter epics (EXR, KNR, RIU, RFU, OPR)'
date: 2026-11-17
description: 'RFU-6 ran Windows against a stale base and found path-form defects. This plan reruns on the finished epics and fixes only the Windows failures the epics caused. The older Windows failures are out of scope.'
status: 'rerun done (2026-11-17); fixes not started'
depends-on: [rfu-6]
lane: none
---

**Evidence:** [RFU-6 notes](2026-11-08-rfu-6-windows-run.md). **Scope:** Windows failures that originate in the
EXR, KNR, RIU, RFU and OPR epics. **Not in scope:** the 82 older Windows failures (book, glob, `metadata_layers`,
`preview_out`, `route_tests`, `lmd_snapshots`, `pandoc_request_*`, and similar), which predate the epics and fail on
Windows regardless. Windows is not in CI, so file one braid strand for them if Gordon wants them tracked.

## Why a rerun comes first

RFU-6 ran on a branch cut 252 commits before `epic-closeout`. Four of its failures are already fixed there, so the
list below is the candidates, not the confirmed set. The first task produces the real set.

## Checklist

**Phase 0: the real failure list (Gordon, on Windows)**

- [x] Check out `rfu/6-windows-tooling` (cut from `epic-closeout`), install pandoc and typst, run
      `scripts/rfu6-windows/run.ps1`, then `extract_failures.py`, and push the results. Compare with the candidates
      below and strike any that now pass.
  (Run `20261117-021842-GWWIN`: 91 failures; the confirmed set is in [RFU-6 run 3](2026-11-08-rfu-6-windows-run.md).)

**Phase 1: the RFU-6 findings (each test first; a Windows failure cannot be watched on macOS, so Gordon confirms red then green)**

- [x] Box 1 (not a defect on this code; the probe test passes, so port nothing): `quarto-hub-provider/src/execute.rs` lines 349, 549, 644 call `canonicalize()`, which yields a `\\?\` path.
      Failing test: `a_figure_reported_under_the_materialized_root_is_captured` (probe test, kept on the tooling
      branch; port it here). Fix with `quarto_system_runtime::canonical::canonicalize`.
- [ ] Box 4: `convert_positron_cli.rs:61` canonicalizes in the test helper. Fix the test, since the product output
      (`C:/...`) is right.
- [x] Box 6 (passes on Windows now, 56 of 56): diagnose `capture_driver::tests::a_record_only_cache_entry_is_published` (`Cannot start a runtime from
      within a runtime`), then fix it. Cause unknown; look at what RIU-2 made the test call from async code.

**Phase 2: other tests the epics added (candidates; confirm on the rerun)**

- [ ] OPR-5: `metadata_layers::tests::layered_value_returns_the_highest_layer_that_sets_the_path` and the two
      `record::source_key::tests` (`a_directory_resolves_against_the_declaring_layer`,
      `directories_outside_the_project_or_missing_are_errors`): `\\?\` against plain paths.
- [ ] RIU-1: `engine_record_stage::an_unwritable_record_location_warns_and_the_render_succeeds`.
- [ ] EXR: `front_matter_provenance::merged_front_matter_root_file_ids_identify_the_documents_own_keys`
      (a file id not registered) and, if they still fail, the Jupyter record goldens.
- [x] The three `execute::` tests and the julia handshake test pass on Windows now.
- [ ] The `jupyter_markdown_goldens` tests (two) still fail on Windows (`twenty-cells` golden differs); diagnose with
      the failure details, since `6b84fd6aa` fixed only the macOS side.
- [ ] Look at `live_reexecute::the_cache_does_not_answer_a_re_execute_under_engine_only_sources` (20 s timeout, new
      since run 2) in isolation; decide whether it is the missing julia or R, or a real defect.

**Phase 3: close the loop**

- [ ] For every fix, apply the path rule in `.claude/rules/cross-platform.md`: a path that leaves the process in a
      wire format uses `/` (fix the product); a native path in a local diagnostic keeps `\` (fix the test).
- [ ] `cargo clippy -p <crate> --all-targets --target x86_64-pc-windows-msvc -- -D warnings` for each crate touched.
- [ ] A final Windows rerun: every Phase 1 and 2 test passes, and the failures that remain are the documented older ones.
- [ ] Also confirm CRLF (RFU-6 box 7) in a clone with `core.autocrlf=true`, then close RFU-6 from this rerun.

## Exit criteria

- None of the epic-originated Windows failures remains, each fixed with a test that failed first.
- RFU-6's boxes carry real results from the current code, and its status is complete.
- The tooling branch is deleted; nothing under `scripts/rfu6-windows/` reaches main.
