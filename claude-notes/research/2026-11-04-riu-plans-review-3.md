---
title: 'RIU plans review 3: implementer and integration views'
date: 2026-11-04
description: 'Third review of RIU-1 to RIU-6, after the RIU-0c spikes. Two read-only reviewers (the implementer view and the integration view) on branch feature/records-in-use at 7bd4115aa; findings, verdicts on six unapproved choices, and decisions for Gordon.'
---

**Reviewed:** `feature/records-in-use` at `7bd4115aa`: the epic, RIU-1 to RIU-6, the 0b, 0c and
review notes, the code the plans touch, and the spike branches (read with `git show`, never checked
out).
**Method:** a2 (this note) ran two read-only sonnet reviewers: C (can each plan start test-first)
and D (do the plans compose, can they be verified). Nothing was built or run by anyone; every
"verified" below means read in the tree, and the claims I rely on I re-read myself. Reviewer
statements I did not re-read are marked "inferred". **No settled decision (1 to 15) is
contradicted.** Findings 1, 3 and 5 strain the epic Goal rather than a numbered decision.

## Blocking (a test or a surface cannot be built as the plans read)

1. **`provide-hub --records` has no composition path, and `q2` is a dead source there.**
   Verified. RIU-2 B2 puts the resolver in `quarto-preview`; `quarto-hub-provider` depends on
   `quarto-core` and `quarto-hub` only (`crates/quarto-hub-provider/Cargo.toml`), so it cannot call
   it. RIU-3 Phase D says "a separate method that does not call `run_and_store`" but names no entry
   point, no `ExecOutcome` variant, and no rule for the consent-gated exec loop when `engine` is not
   in the list. RIU-4\'s never-execute guard is in `claim_and_spawn` (preview only), so a provider
   started with `--records positron` still runs the engine on an editor request. Also the provider
   materializes the project from the hub index, so a `.quarto` copy exists there only if someone
   uploaded it; `q2` is effectively empty and decision 4\'s own example `q2,engine` is `engine`
   only. Needs a decision before RIU-2: put the resolver behind a trait in `quarto-core` that
   preview and provider each implement, or give the provider its own entry point.
2. **The HTTP re-execute handler cannot inject a registry.** Verified:
   `re_execute.rs:110` calls `re_execute_with_registry_and_cache(ctx, body, None, &cache_dir)` and
   `ReExecuteConfig` carries no registry, so RIU-4\'s test "fake engine that fails if run" through
   the handler cannot be written. Either add a registry field to the config or say the handler test
   asserts only the 403 and an unchanged sidecar, with the no-engine proof at `claim_and_spawn`.
3. **The stamp-on versus stamp-off switch is undefined.** Verified: spike 2 used a
   production-code `std::env::var_os("RIU_NO_STAMP")` read (spike/riu-s11-stamp-shapes,
   `record_splice.rs:346`) flipped by `unsafe set_var` in tests; RIU-2 Phase A says the plans avoid
   `set_var` (edition 2024). RIU-3 Phase E\'s shape tests and render-invariance test compare "with
   the stamp on and off" without naming the mechanism (a parameter on `splice_record`, a
   `StageContext` field, or a test-only function).

## Important

4. **RIU-2 versus RIU-4 on the re-execute transport (C I1).** Verified: the handler reads
   `cache_dir` from a process-wide `static CACHE_DIR` (`re_execute.rs:53`). RIU-2 makes
   `perform_re_execute` publish through the gate, but RIU-4 owns the `axum::Extension` that removes
   the static. Between them the handler path has no way to get `record_gate` or `sources`.
   `claim_and_spawn` and `trigger_auto_re_execute` also need the config. Not a cycle; pull the
   Extension into RIU-2 or say RIU-2 uses a second static.
5. **RIU-3 Phase G and its exit criterion need `--records`, which is RIU-4.** Verified from the
   plans: RIU-4 depends on RIU-3. Nothing sets `PreviewConfig.record_sources` from the CLI before
   RIU-4. Split the clap wiring forward, or move the browser check into RIU-4.
6. **The resolver trait has no signature** (C I2), and its argument decides whether the pre-engine
   pipeline runs once or twice. Verified for the code: `record_one` calls
   `record_capture_run_cached`, which calls `compute_input_qmd`, a wrapper over
   `compute_source_inputs`; the import step would call it again. Smallest change: the resolver runs
   `compute_source_inputs` once, derives the key from `inputs.input_qmd`, and passes `&SourceInputs`
   to every source. RIU-3 says "one pipeline run yields key, units, hashes, owner" but RIU-2 B2
   does not carry the `SourceInputs`. (The engine path already runs the pipeline twice on a miss;
   that is existing behavior.) RIU-3 also does not say which plan implements the trait for
   `Positron` and `Dir`.
7. **The default list serves an unchecked `.quarto` record and marks it fresh.** Verified:
   `record_one` publishes `staleness: Some(false)` unconditionally (`capture_driver.rs:200-206`) and
   RIU-2 Phase E says the `q2` copy is not checked. With `cache,q2,engine`, an old `record: true`
   copy on a cold cache is shown and never re-run; edited cells lose output silently. Today that
   document gets a fresh engine run. Neither the epic nor RIU-2 flags it.
8. **No UI path to Re-execute an imported page.** The RIU-3 guard returns not-stale forever, the
   overlay shows only on `staleness === true` (inferred from the reviewer\'s read of
   `StaleCaptureOverlay.tsx:11-12`), so decision 5\'s `positron,engine` re-execute is reachable only
   by a hand-made POST. Plans silent.
9. **"The page says so" has no implementation.** The Goal and RIU-4 Phase E promise it; no checklist
   item produces a message, and `record_one` returns `Ok(false)` with no ref.
10. **A persisted `--data-dir` makes the Positron source sticky.** Verified: `record_eager_captures`
    skips any path with a `CaptureRef`. After an engine re-execute the ref points at the engine
    record and the Positron one cannot return; with a persisted dir a restart never re-reads the
    list either. RIU-4\'s "not seen until restart" is wrong for that case. Intended in effect (the
    same mechanism gives prose-edit reuse), undocumented. The "at least one unit matches" rule
    applies only to the Positron and directory sources; the cache and `q2` sources do not match by
    unit, which the decision-4 wording already says ("for a source that matches by unit").
11. **Whether an imported record is cached is unspecified**, and with it whether Re-execute
    bypasses `lookup_cached_run`. Verified: `perform_re_execute` goes through
    `record_capture_run_cached`, which looks up the key first. If imports are cached, a Re-execute
    on an unchanged document can return the Positron record with no engine run, which breaks the
    RIU-2 Phase A assertion "fresh engine record in the ref".
12. **The refusal at the publish seam: plans are silent on every caller.** From the code: in
    `record_one` an `Err` skips `set_capture`, and `record_eager_captures` logs "Engine capture
    failed" (Q-PREVIEW-CAP-1), a misleading title for a refused import; first-wins means it does not
    fall through to `engine`, so with `QUARTO_ENGINE_RECORD_WRITE=0` and the default list a stale
    `.quarto` copy now gives an error and no output where today it runs the engine. In
    `perform_re_execute` it becomes `state: Error` with Q-PREVIEW-RE-1, practically unreachable. In
    `run_and_store` the error is a `String`, so RIU-2\'s mapping to `ProviderError::Protocol` loses
    its type; watch mode only logs `warn!`, one-shot exits 1. Exit code in preview is unaffected.
13. **Provider construction.** Verified: `Provider::new` and `new_with_overlay`
    (`execute.rs:101,113`), one production caller and four tests
    (`tests/integration/execute.rs`); the gate is read at `:357-362`. "Stores the gate at
    construction" needs a gate parameter or a third constructor; none is named. The `cache`
    start-up error has no named owner: the natural one is `provide_hub::run` next to
    `validate_mode`, so it is not a parser test as RIU-4 lists it. A directory-source check at start
    likewise needs a named validator called from both commands. `--records` is also missing from
    the `conflicts_with_all` lists of `--join` and `--static` in `main.rs`.
14. **Walker rebuild is not equivalent.** Verified in spike P (reviewer C): the copy of `remap.rs`
    empties the attr and target source visits and uses a `u8` class; production `remap_file_ids`
    rewrites ids in attr, target, caption, custom and meta sources. "Rebuild on one traversal"
    must say how those are visited, and needs a `Header` tag the spike lacks.
15. **Phase E porting sources are on three spike branches** and only s9 is named in RIU-3; the
    shape harness (s11) and the viewer test (s10) are not, and the spike 2 helpers are private to
    `record_preview_splice.rs`, so shape tests belong in that file or the helpers need
    `pub(super)`.
16. **RIU-1 and RIU-2 collide** on `record_writing_enabled` call sites (`capture_driver.rs:405`,
    `execute.rs:361`) though the epic says they can overlap. Order them.
17. **Shared recording engine.** `CountingRecordingEngine` is private in the lib test module
    (`cache.rs`), invisible to `tests/integration/`; RIU-2 says share it without saying how.
18. **Spike 2\'s mis-paired Div is worse than "unstamped".** Reviewer D read `Div.tsx:62-71`,
    `resolveSource` and `sourceIndex.ts`: that Div carries the author\'s `Orig(f0)` entry, so with
    `--allow-edit` it gets an edit affordance whose edit targets the author\'s `.note` Div while the
    user is looking at the cell: a wrong-target write (inferred; no browser run). This strengthens
    the open decision on mis-paired cells; the plan\'s "known limit" wording understates it.
    Related: the editor-to-preview scroll lookup (`scrollSyncDom.ts:52-96`) does not filter by file
    id, so unstamped `Orig(f1)` scaffold nodes with reparse-slot `data-loc` can capture an author
    line (pre-existing; inferred).
19. **Sizing.** The compile-error counts hold, but the split does not: only `lib.rs:449` is a
    production caller of `record_eager_captures` (verified; I corrected RIU-4). RIU-2 is nearer 3 to
    4 sessions and RIU-3 4 to 5 (or split 3a import, locator, provider, guard; 3b stamp,
    TypeScript). RIU-4 is light at 1 if it owns the provider plumbing. Epic total about 12 to 16
    sessions remaining (inferred estimate).
20. **Release runbook section "Engine-record writer gate"** (`release-runbook.md:362-378`) still
    says "Do not flip it in the same release that first ships the reader" and that a preview newer
    than its hub must keep the gate off. After decisions 2 and 9 it must say: per surface (local
    preview flips with the reader, `provide-hub` waits on the hosted `q2_ref`); the gate decides
    only the hub payload form, not the cache or `.quarto`; a record with no legacy form is refused
    with the gate off, naming the variable; the preview rule is moot (it publishes only to its own
    store). RIU-1 Phase B owns it.

## Minor

- `provide-hub` default and `engine` under `preview.engine: off`: RIU-4\'s "ignored with a warning"
  fires on every default `off` run (noise).
- `overlay_free_run` (`execute.rs`, around 496-518) could call `rehash_record` on an imported record
  under `-M`; RIU-3 Phase D should say the import path skips it (inferred).
- RIU-3 Phase B says `import_record_source` sits "beside the other record sources, not in the
  `record` module, which the WASM compiles", but there are no other sources, `record_from_positron`
  itself is in the WASM-compiled module, and RIU-2 puts `record/sources.rs` in `record/`. The reason
  does not hold; the location is unspecified.
- `dirs` is not a `quarto-core` dependency (verified: it is not in `Cargo.toml`; `sha1` is); it must
  go in the target-specific table at `Cargo.toml:128`. RIU-3 says the Linux rule "uses
  `dirs::config_dir()`" and also that the locator takes the environment as a parameter; to test on
  one host the Linux rule should read `XDG_CONFIG_HOME` and home from its arguments.
- The preview-renderer ATOMIC_KINDS change triggers CI paths the plans do not mention
  (`deploy-q2-sandbox.yml`, `q2-preview-spa-e2e.yml` path filters; the ts-test-suite integration
  tier needs the WASM build), and the stamp changes `data-loc` for spliced output in hub-client
  without a hub-client file edit; the hub-client e2e specs that read `data-loc` run only with
  `--e2e`. The `ATOMIC_KINDS` comment points at a crate that is now external.
- Planned diagnostics are uncoded; a `Q-` code would trip the error-docs lint rules. Docs edits
  make the embedded `q2 docs` stale (`cargo xtask build-agents-docs`); preview.qmd probably also
  needs the `--records` text.
- Windows cross-clippy lines: RIU-2 omits `-p quarto` although `commands/preview.rs:354` is a
  `PreviewConfig` literal it must change; RIU-4\'s native line omits `quarto-core`; RIU-5 may need
  `quarto-preview` if the export reuses the resolver (inferred).
- RIU-5 exit codes 10 to 13 need an explicit `std::process::exit`; `main` returns `Result`.
- Open-items drift: the epic lists five open decisions but 0c listed four of its own, two of which
  (table-cell `Plain`, scaffold nodes) reach only RIU-3 as "known behavior". RIU-1 still hedges
  decision 10. The epic\'s session total omits the four 0c spikes.

## Fixed (verified factual errors only)

- RIU-4: "`re_execute.rs:391` runs the driver after a re-execute" was wrong; it is the test helper
  `seed_capture`, so `lib.rs:449` is the only production call.
- RIU-3 Phase B: `run_has_record` renamed `run_has_content`, the name RIU-2 defines.
- Epic, "Providers": "encoding, the payload document and `set_capture` are shared" contradicted
  RIU-2 and 0c spike 4; `set_capture` is not part of the seam.

## Rejected or discounted

- Reviewer C\'s "RIU-3 Phase E first test order reads backward" (I5): the viewer test failing until
  `ATOMIC_KINDS` changes is test-first as intended. Dropped.
- Reviewer D\'s statement that RIU-3 Phase B and RIU-2 name the same predicate differently was right
  and fixed above; its "six places" content mismatch is cosmetic (counts agree). Dropped.
- Reviewer C\'s count of 17 `PreviewConfig` literals against the plan\'s 18: the difference is a
  grep scope question, not a plan error. Dropped.

## Verdicts on the six unapproved choices

1. **`cache` in a provide-hub list is a start-up error: sound, incomplete.** Correct (the provider
   has no cache directory). The owning function is unnamed; say `provide_hub::run`.
2. **Publish split (`publish_payload_doc` in `quarto-hub`, `encode_for_publish` and `PublishError`
   in `quarto-core`, `set_capture` outside): sound.** The duplicated part is small; the refusal\'s
   per-caller outcome and the `String`-typed provider error need stating (finding 12).
3. **RIU-3 Phase E shape tests, render-invariance test and scroll-sync check: needs one addition**
   (the on/off switch, finding 3) and the porting locations (finding 15).
4. **`ReExecuteConfig` carrying gate, policy, list, cache dir: sound, incomplete.** Counts verified
   by reviewer C and me for the two call-site groups; no caller missed in other crates. It needs
   the handler transport (finding 4) and, for the RIU-4 test, a registry (finding 2). No cycle and
   every RIU-2 field is used without RIU-4.
5. **`async-trait` in `quarto-preview`: acceptable**, but moot if the resolver moves to
   `quarto-core` (finding 1); in `quarto-core` it must also compile for wasm32.
6. **Source-list type in `quarto-core` `record/sources.rs`, resolution in `quarto-preview`: the
   type is fine, the resolution location is wrong for `provide-hub`** (finding 1).

## Decisions for Gordon

- Where the resolver lives and how `provide-hub` composes the list (finding 1).
- Whether a `.quarto` copy is staleness-checked on the default path (finding 7), and what a refusal
  does when the list has a later source (finding 12): first-wins-final, or fall through.
- Whether imports are cached and whether Re-execute bypasses the cache (finding 11).
- Whether an imported page needs a UI path to re-execute or a "no record" message (findings 8, 9).
- Mis-paired expanded cells: the evidence now includes a wrong-target edit under `--allow-edit`
  (finding 18).
- Whether to split RIU-3 and move the clap wiring forward (findings 5, 19).
- Still open from before: hosted hub `q2_ref`, R through Ark, fail-soft for the optional copy,
  the `Header` exemption.

## What was run

Only greps, file reads and `git show`; no builds, tests, browser or WASM chain. `npm run build:wasm`
has still not been run on this branch.
