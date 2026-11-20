---
title: 'RIU plans review 5: compiler view and cold read of the whole set'
date: 2026-11-05
description: 'Fifth review of RIU-1 to RIU-6, after the review-4 decisions were folded into RIU-2, RIU-3, RIU-4, RIU-6 and the epic (5b18516e1, 1b4816cff). Two read-only reviewers on branch feature/records-in-use; findings, fixes, and decisions for Gordon.'
---

**Reviewed:** `feature/records-in-use` after `1b4816cff`: the epic, RIU-1 to RIU-6, review 4, and the code
RIU-2 and RIU-3 touch.
**Method:** a4 (this note) ran two read-only sonnet reviewers: G (does RIU-2 plus RIU-3a compile, and does each
test fail first for the stated reason) and H (the whole set read cold: consistency, ownership, exit
criteria, product versus demo, docs). Nothing was built or run. "Verified" means I read it in the tree or the
plan myself; "inferred" means a reviewer said it and I did not re-read it. **No settled decision is
contradicted.** Findings B1 and B2 touch the mechanism behind decisions 5 and 1 and go to Gordon.

## Blocking

1. **The "cache older than the document is flagged at boot" rule cannot work with the specified
   conversion.** Verified three ways (the spike's `record_from_positron`, `positron_import.rs`, and
   `matching.rs`). `record_from_positron` loops over the document's current units and asks the import for
   each unit's outputs; a cached cell that no current unit looks up is never copied into the record. So a
   record converted at boot has no cell for an edited or deleted unit, and `Matching::orphaned` is empty.
   Orphans appear only on later recomputes, against a record published earlier. RIU-3 Phase A's tests
   ("deleting a cell flags the page", "a cache older than the document is flagged at boot") would fail for
   a reason the plan does not give, and `record_one` would publish every imported record as fresh. Related
   and verified in `matching.rs:61-95`: `UnitStaleness::Stale` means a **document unit** no cell matched,
   the opposite of the rule (an uncovered unit must stay quiet), so the new function must read only
   `Matching::orphaned` and ignore `staleness()`. **A choice for Gordon** (decision 1 below); I changed
   nothing.
2. **No plan makes the provider's request path call the walk, and the trait does not fit the provider.**
   Verified in `execute.rs`: `execute_document` calls `run_and_store` directly, and `run_and_store`
   runs consent, the engine, writes the capture doc and the sidecar itself, returns `ExecOutcome` and errors
   on `captures.is_empty()` (`:338`). RIU-2's `run_engine(&SourceInputs) -> Result<CaptureRun, _>` returns a
   run for the caller to publish. RIU-2 says only `record_one` calls the walk; RIU-3 Phase D says the
   provider "needs no import-specific path"; RIU-4 implements the trait but never says the provider's
   request handling is rewired to call the walk. As written the trait implementation is dead code, RIU-3a's
   provider publish method is reachable only from a test, and RIU-4's exit criterion "a `provide-hub` with
   `--records positron` cannot run the engine" is not delivered by any item. **A choice for Gordon**
   (decision 2 below).

## Important

3. **No single callable refusal predicate exists, and the two error types never meet.** Verified: the only
   gate logic is the inline filter at `capture_files.rs:303`. RIU-2 says the walk and `encode_for_publish`
   "call the same refusal predicate" without saying which is extracted, so the two can drift (reviewer G4).
   `RecordError` is `quarto-preview`'s and `PublishError` is `quarto-core`'s; the walk reports an outcome,
   so nothing says what turns "refused, last source" into `RecordError::Refused` (preview) or
   `ProviderError::Protocol` (provider). Choice: extract a small pure function (gate, captures, record) or
   have the walk call `encode_for_publish`; and name the mapping.
4. **The walk and trait signatures are incomplete.** `LookupError`, `EngineError`, `RecordSource`, the
   outcome type and the walk itself are never defined; the walk also needs the gate and the policy as
   parameters (the refusal check and the `unavailable` outcome read them). Inferred from G1, consistent with
   the plan text. The plan says the names are illustrative, so an implementer invents these.
5. **`async-trait` is missing from `quarto-preview`, `quarto-hub-provider` and `quarto-hub`.** Verified by
   grep: only `quarto-core` has it. **Fixed** in RIU-2 Phase B2.
6. **The guard in `claim_and_spawn` must sit before the in-flight insert.** Verified (`re_execute.rs:211`
   region): a refusal placed after the insert leaks the slot and later POSTs for that path answer 409.
   **Fixed** in RIU-2 Phase B3, along with the four in-module seam tests (`:412`, `:437`, `:545`, `:607`)
   that must take the config.
7. **RIU-2 and RIU-3 verify lists omit the `quarto` crate.** Verified: the production `PreviewConfig` literal
   is in `crates/quarto/src/commands/preview.rs:354` and RIU-3a's flag test lives there, but both plans
   gave clippy and nextest lines without `-p quarto`. **Fixed.**
8. **RIU-2 and RIU-3 Phase A still said RIU-4 adds the `--records` parser.** Verified at four places in
   RIU-2 (`:16`, `:146`, `:185`, `:251`); the 2026-11-05 split gave the minimal parse to RIU-3a. **Fixed.**
9. **The provider fall-through stderr line has no owner.** Verified: epic decisions 3 and 6 promise it;
   RIU-2 covers the preview page warning and RIU-4 the "no source supplies" stderr text, not the refusal
   line. Moot until finding 2 is decided, since the provider has no walk to fall through in.
10. **The stamp "on and off" comparison has no switch.** Verified: RIU-3 Phase E (`:406`, `:412`) requires
    renders with the stamp on and off, and no plan defines how it is turned off (a parameter, a test seam).
    Carried from review 4 and still open. Choice.
11. **`compute_source_inputs` would resolve engines for every caller.** Inferred (G3.5): adding
    `resolve_engines_for_document` makes the cache-key path and every file-change recompute pay for it,
    though only the boot import needs `ownership`. Choice: make it lazy or accept the cost. The registry
    override's route to the resolution call is also unstated.
12. **A Julia cache fixture has no source.** Verified: RIU-3a's exit criterion says Julia gets outputs
    "tested with the real registry", Phase A lists only R and Python fixtures, and Phase G's browser checks
    name only those. Julia has no Positron-produced cache (the J7 push route only).
13. **RIU-3b's exit criterion depends on RIU-4.** Verified: Phase F says "document the option after RIU-4
    names it", but the 3b exit lists "the documentation of Phase F is written". Three plans edit
    `execution-records.qmd` (RIU-1, RIU-3, RIU-4). The runbook has a single owner (RIU-1), but the
    `provide-hub` flip text for it appears only in the epic.
14. **The embed refresh appears once.** Verified: `cargo xtask build-agents-docs` then `cargo build --bin
    q2` is named only at RIU-4 Phase D, as "before relying on `q2 docs`". RIU-1 and RIU-3 change docs and
    do not mention it, and no plan says when the embed must be fresh (before shipping the flip).
15. **RIU-5's `q2` source is a third list consumer with no stated rule.** Verified at `riu-05:150-155`: it
    takes `--records`, reads the `.quarto` copy without a staleness check and calls `record_capture_run`
    directly; epic decision 2 (staleness check when chosen) is scoped to RIU-4 and `record_one`, so this
    may be intended, but the plan does not say so, and what `positron`, `cache` and directory entries mean
    in an export is unstated.
16. **RIU-6 Phase 0 is runnable today but underspecified.** Verified in part: it does not name a fixture
    (a document with code cells under the default list runs an engine, so it needs a venv or a prose-only
    document) or say to confirm that `target/debug/q2` embeds a current SPA. Inferred: it omits WASM
    compilation under the pane's content-security policy, webview storage and postMessage behavior, and
    `localhost` versus `127.0.0.1`.
17. **Guard tests cannot fail on behavior first.** Inferred (G2.3): `ReExecuteConfig` and the `Extension` do
    not exist, so every listed guard test fails to compile before it can fail on behavior. Only "a request
    with no config is refused" fails today on behavior (the handler falls back to a temp directory,
    `re_execute.rs:106`). A stub config type landing first would fix the order; choice.
18. **Session estimates.** Inferred, from the checklists and not measured: G puts RIU-2 at 4 to 5 and 3a at
    3 to 4. RIU-3's header says "4 to 5" while its halves (2 to 3 and 1 to 2) add to 3 to 5; the epic table
    sums to 12 to 15.5 against "12 to 16", so "19 to 24" and the table disagree by about a session.

## Minor

- RIU-6's dependency line lists RIU-4; its status line and the epic table say command 3 needs only RIU-3a,
  RIU-2 and RIU-5 (the epic ordering still puts RIU-4 first).
- RIU-6 command 3 does not say which Positron cache directory it reads; a dev build writes under
  `code-oss-dev`, a release under `Positron`, and the locator tries the release path first.
- In the demo a stale imported page shows a Re-execute overlay that is then refused (403) when `engine` is
  absent; RIU-6 does not say what the demo shows or does about it.
- RIU-3 Phase G 3b names `--share` and a guest, but the interim `.cell` rule check needs `--allow-edit`;
  its "a guest cannot start a kernel" line duplicates RIU-4's share test.
- RIU-4 Phase E's `positron` check does not say where the cache comes from; a hand-built fixture needs the
  directory override or a bare-path source, because the locator hashes the document URI.
- RIU-6 Phase E rewrites plan files but only the findings note is told to run `q2 render --strict`.
- A stale doc comment at `diagnostics.rs:121` names `CACHE_DIR`; it needs updating when the static goes.
- R1 versus R2 order on `record_writing_enabled` (surface-aware in RIU-1, read once in `lib.rs` by
  RIU-2) is still not stated (carried from review 4, inferred).
- The walker rebuild has no named regression test for `remap_file_ids` parity; the full suites are the net.
- Windows clippy lines are present in RIU-1 to RIU-5.

## Fixed

First pass (verified factual errors only), then a second pass after Gordon agreed to every finding and the
four recommendations (plan text rewritten in place, not appended):

- **Finding 1 (orphans at boot):** RIU-3 and the epic (decision 5 of 2026-11-05, and the S4 signature note)
  now say the conversion stores the count of cache cells no unit paired with in the record `extra`
  (`quarto_unclaimed_cache_cells`), counting a cell a unit paired with as covered even when the language
  gate refuses it. One function returns stale when that count is above zero or `match_units` leaves a record
  cell orphaned, reads only `orphaned`, and reuses the splice's engineless key construction (extracted, not
  copied). Phase A gained conversion tests and boot tests. I chose to store the count in the record, not
  recompute it, because the list is read once at boot and publish and recompute must agree; this is a detail
  the answer to decision 1 did not spell out.
- **Finding 2 (provider and the walk):** RIU-2 gives `run_engine` an `EngineResult` (`Run` or `Published`)
  and the walk a `Chosen` with `EnginePublished`; RIU-4 makes `execute_document` call the walk when the list
  names a source other than `engine` and `run_and_store` as today otherwise; RIU-3 Phase D is only the
  publish method; the epic and RIU-4 tests and exit criteria follow.
- **Findings 3 and 4:** RIU-2 names one pure refusal predicate that `encode_for_publish`, `encode_capture_payload`
  and the walk all call, the mapping to `RecordError::Refused` and `ProviderError::Protocol`, the walk
  signature and its context type.
- **Findings 5 to 8:** `async-trait` named for `quarto-preview` and `quarto-hub-provider`; the guard before
  the in-flight insert; `-p quarto` in the verify lists of RIU-2 and RIU-3; RIU-2 no longer says RIU-4 parses
  the flag.
- **Finding 9:** RIU-4 owns the `provide-hub` stderr fall-through line.
- **Finding 10:** RIU-3 Phase E defines the stamp switch as an explicit boolean on the post-walk, a test seam
  only.
- **Finding 11:** `ownership` is computed only when asked (RIU-2 decisions, RIU-3 Phase B), and the
  registry-override seam is named as a commit-time statement.
- **Finding 12:** RIU-3 adds a hand-built Julia cache fixture, marked hand-built, and Julia in the Phase G
  browser check and the 3a exit criteria.
- **Finding 13:** documentation ownership is stated in RIU-4 Phase D (RIU-1 the `record: true` paragraph,
  RIU-3 first text for the positron source, RIU-4 the final text); RIU-3b documents the flag as 3a leaves it;
  RIU-1 Phase E gained the runbook step at the flip.
- **Finding 14:** the embed refresh (`build-agents-docs`, `cargo build --bin q2`, `q2 docs llms --embed-info`,
  `q2 render docs/`) is a step in RIU-1, RIU-3 Phase F and RIU-4 Phase D.
- **Finding 15:** RIU-5 accepts only `q2` and `engine`, rejects the other names with exit code 2, does not use
  the walk, and says the `q2` copy is deliberately not staleness-checked there.
- **Finding 16:** RIU-6 Phase 0 names a setup (prose-only or Python plus venv, the SPA and WASM chain) and
  lists the WASM and CSP, postMessage, iframe sandbox, storage, `localhost` versus `127.0.0.1`, remote
  workspace and stop checks.
- **Finding 17:** RIU-2 Phase B3 lands the config type and `Extension` as a compile stub first, so the guard
  tests fail on behavior; the stale doc comment at `diagnostics.rs:121` is an item.
- **Finding 18:** sessions are RIU-2 4 to 5, RIU-3 4 to 6 (3a 3 to 4, 3b 1 to 2), RIU-4 1.5 to 2; the epic
  total is about 13.5 to 18 for the six plans and 21 to 26 with the spikes.
- **Minor:** RIU-6 dependency line and diagram (RIU-3a and RIU-2, not RIU-4); the demo passes the running
  build's Positron cache directory and states the refused Re-execute click; RIU-3 Phase G adds
  `--allow-edit` and drops the duplicated guest line; RIU-4 Phase E says where the fixture cache comes from
  and how the provider path is checked; RIU-6 renders every plan file Phase E edits; RIU-1 states the order of
  its gate function and RIU-2's `record_gate`; the `execute.rs` citation became `272-286`.
- **A question I raised and then withdrew:** I first recorded as open that a non-engine source in
  `provide-hub` would run the pre-engine pipeline before consent, believing that happened only after consent
  today. Wrong: `write_review_file` calls `compute_input_qmd` (the same pipeline) before the consent gate
  (`execute.rs:448`). RIU-4 and the epic now say it is settled, with no new pre-consent work.

## Rejected or discounted

- G2.2 said the two "test callers of the no-argument form" (`render_download.rs:264`, `route_tests.rs:396`)
  are misdescribed. They call `extend_with_preview(Router::new())`, which is the no-argument form the plan
  means; the cites are correct. Dropped.
- G1.2's wasm32 concern: the reviewer found no non-wasm type in `CaptureRun` or `SourceInputs` and the
  modules are not cfg-gated, so the plan's claim holds. The `npm run build:wasm` check stays the proof.
- H1's "stamp by unit span" and "no code runs" sweeps found no stale wording; the demo-route and K-S2 sweeps
  were clean. I did not re-run those greps beyond spot checks.
- H's note that the `provide-hub` stderr line is unowned is kept (finding 9) but depends on finding 2.
- Not verified by me: the Phase 0 omissions in finding 16 (inferred by H), G's estimates, the Linux and
  Windows Positron cache paths, VS Code URI encoding, the two-binary `--share` case, and whether the viewer
  SPA works inside Positron's preview pane (RIU-6 Phase 0 will tell, with Gordon).

## Decisions (answered by Gordon, 2026-11-05)

Gordon agreed with every finding and with all four recommendations:

1. **Orphans at boot:** the conversion reports the cache cells no unit claimed (option a); a gated-out unit
   counts as covered. Written into RIU-3 and the epic.
2. **The provider and the walk:** keep `run_and_store`, call the walk only when the list names a non-engine
   source, and call `run_and_store` when the walk reaches `engine` (option b). Written into RIU-2, RIU-3 Phase
   D, RIU-4 and the epic.
3. **The refusal predicate and the stamp switch:** one pure predicate; a boolean on the post-walk as a test
   seam. Written into RIU-2 and RIU-3.
4. **`ownership` is lazy:** only the boot import asks for it. Written into RIU-2 and RIU-3.

**Nothing is open.** The question I raised about pre-consent work in `provide-hub` was based on a misreading and
is withdrawn (see Fixed).

## What was run

Reads and greps, plus `git show` of the spike branch, and two read-only reviewers. The `q2 render --strict` of
the changed files is the only command that ran code. This worktree's `target/` directory had been deleted by
the time of the second pass, so I rendered with the main checkout's debug build,
`/Users/gordon/src/q2/target/debug/q2` (0.39.0, built 2026-11-03), not the `q2` on PATH and not a fresh build
(no builds are allowed in this review). It showed only the 36 known Q-12-13 listing errors.
