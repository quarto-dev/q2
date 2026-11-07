---
title: 'Review of the RIU follow-up plan'
date: 2026-11-07
description: 'Design and implementation review of the plan that follows the Records in use epic. Two read-only reviewers on branch feature/riu-follow-up-review (cut from feature/records-in-use at 3f0c55a6d), merged, with the claims I re-checked, what I did not verify, and how the plan was amended.'
---

**Reviewed:** [the follow-up plan](../plans/2026-11-07-riu-follow-up-did-we-build-it.md), the epic, RIU-3, RIU-5, RIU-6
and the RIU-6 commands findings, against the tree at `3f0c55a6d`.
**Method:** two read-only Sonnet reviewers. A asked "is this the right plan?"; B asked "will this hold up in the
tree?". Neither built or ran anything. "Verified" means I re-read it in the tree with `git grep` or ran it myself.
Anything else is "reported" by a reviewer. Tags: **blocking**, **should-fix**, **note**.

## Blocking

1. **Item 1's keep rule cannot simply add `knitr`.** (Verified.) `EXPORT_OWNERS = ["jupyter", "julia"]`
   (`crates/quarto-core/src/record/positron_export.rs:101`) gates every cell, and the importer admits knitr only
   for the exact language token `r` (`positron_import.rs`, test near `:419`, reported). Putting `knitr` in the
   list unconditionally would export a knitr-owned `python` unit. The mapping needs a language check, and a test
   that such a unit stays skipped. The plan's Phase C said only "replace `EngineNotExportable`".
2. **The plan says the knitr cells' kind is unknown, and the verdict hangs on it. I ran it.** (Verified, CLI,
   `target/debug/q2`, scratch directory.) A three-chunk document with `execute: record: true` writes
   `.quarto/execution-records/d.ipynb` with three *structured* code cells, all `quarto_engine: knitr`:
   a `stream` output; a `display_data` with `image/png`; two `stream` outputs (the message and the data frame
   print as text, not HTML). Each carries `quarto_hash_rule` and `quarto_header_keys`. So none is a fallback
   cell, and the export's only reason to skip them is the owner rule. This does not settle the hash question
   (finding 3) or whether Ark's cache shapes match; it removes one unknown. The export itself, run as
   `q2 record export-positron d.qmd --source-uri file:///x/d.qmd --out out.ipynb`, printed
   `{"cells":0, ... 3 x "engine is not jupyter or julia"}`, then `error: the record has no cells Positron can
   display`. The plan said exit 11; I did not capture the exit code (my pipeline reported the exit of `tail`, not of q2), so I leave that
   figure alone.
3. **A hash mismatch is the likeliest "needs more than a mapping" outcome, and the plan did not say what that
   means.** (Reported, from `engine/knitr/hash_rule.rs` and the plan's own line about `#|` lines.) A knitr
   record's `quarto_content_hash` uses the `structured` hash profile, and Positron hashes the cell text including
   `#|` lines. If they differ, either q2 recomputes the hash at export (a q2-side mapping) or Positron changes.
   Gordon should hear which before Phase B. The Julia `#|` question is the same one; fold it into Phase A.
4. **No gate between the Phase A verdict and Phase B.** (Reported.) Phase B's first decision point comes after a
   verdict that may already be "needs a Positron-side change". Add an explicit "ask Gordon" step after Phase A.

## Should-fix

5. **Item 5's text is stale after RIU-3b** (verified: `record_splice.rs:357-368` stamps, `sourceInfo.ts` lists
   `jupyter-output`, `record-output-stamp.integration.test.tsx` and `record_splice/tests/stamp.rs` exist). The
   whole "never started" paragraph, "14 open boxes" and the status line are wrong. The epic is internally
   inconsistent too: its "Closing reconciliation" still says "RIU-3b never started" (`records-in-use-epic.md:755`)
   while its Exit criteria mark 3b met. Fixed in the plan; the epic line is a box.
6. **The Item 2 note "fork change uncommitted" is wrong** (verified: `git -C /Users/gordon/src/positron-riu6-a log`
   shows `53bf9ca105` "warm the Julia server, longer Julia timeout...", tree clean, branch
   `spike/riu-6-phase-a`, tip `99aa72fcb4`). The reviewers disagreed here; A assumed uncommitted from the plan's
   text, B looked. B is right. The "three runs" box is still unreported.
7. **Order rationale is wrong.** (Reported; reasoning checked.) "Item 2 decides whether the e2e tests can be
   trusted for Item 1" holds only for Julia. R through knitr starts no Julia server. What Item 1 Phase D
   needs is an R session in the dev build (`POSITRON_R_VER_SEL`) and the same e2e file in the fork. So Item 2 is
   independent of Item 1 Phases A to C and only contends with Phase D for the e2e file.
8. **Item 4 is called done in the Order section but its second box is open.** The miniature pass is in
   "Intent not covered" below; that box now points at it.
9. **The EXR-16 sweep is exact now.** (Verified count: 21 `CASES` in
   `crates/quarto-core/tests/integration/positron_conformance.rs:25-66`; fixtures in
   `tests/fixtures/positron-conformance/`.) That file tests the generator, the loader checks and
   `positron-expected.json` only. `positron_source.rs` covers three durable fixtures (python, hand-built julia,
   r) through import and splice (`:142`, `:172`, `:182`, `:197`, `:206`, reported). No test runs each of the 21
   cases through import, convert and splice. Indirect unit coverage exists for `blockquote`, `crlf`,
   `duplicate_pair` and the marimo cases (hash, placement, matching, export). The labeled `fig-cap` case is
   covered for hash and record shape (`knitr_positron_import.rs:179`, `:261`, reported) but not for
   "output shows on the page after splice". The Re-execute test exists
   (`capture_driver.rs:3194`, reported), so only its sentence in the plan is stale.
10. **Item 3 ranking** (design judgment; the plan ranked only the first two):
    - *Blocks command 2 and 3:* cache location from an API (a non-default profile fails); the Viewer close button
      leaving q2 running (command 3 only).
    - *Missing from Item 3, blocks command 2:* a progress and cancel affordance (the engine may run the whole
      document), and the dirty-document wording. Epic decision 7 says a dirty document exports from disk and the
      push skips changed cells with a report; the commands findings note says "exported as saved, with a note".
      These can be reconciled by a sentence, so the box is to check the actual behavior.
    - *Should-fix before shipping:* stale Julia servers fail later runs (`startpath must be non-empty`, reported
      from the findings note) and a missing Plots project exports an `ArgumentError` as an error output that looks
      like a working run; the "this page reconnects automatically" text in the preview banner is wrong for a
      one-shot server (and epic decision 15 says no banner, so it needs reconciling); whether a push over existing
      outputs asks first; the three unrun window checks.
    - *Polish or documentation:* syntax highlighting after an edit; command 2 not changing kernel state (write it
      in the user text); command 1 on Julia and R.
11. **Inconsistency in the plan's "not run" list.** The first of the unrun window checks (wrong `q2.path`) is
    recorded in the commands findings as run under Playwright and failing correctly (reported by A; I did not
    re-read the results table). The box should say "run in the harness, seen to fail correctly", and only
    untitled document, "q2 too old" and non-default profile are unrun.

## Intent the epic states that no checklist covers (Item 4, in miniature)

(Reported by A, spot-checked against the epic's Exit criteria.)

- **Hub verification.** Exit criterion 1 says preview *and hub* work from records alone. No follow-up item says
  where the hub was verified.
- **The embedded-docs refresh** (the epic's "last step") is not a checkbox anywhere. The 3b docs edits make the
  embed stale.
- **The Windows run** (unchecked in the epic, the one item every plan defers) is in no follow-up item.
- **RIU-3 browser items never seen:** `--records positron,engine` running the engine, a cache older than the
  document in a browser, `--share --allow-edit` with a guest, "Clear results" clicked, real click or scroll sync.
- **Stamped cells carry no `data-loc`**, so a click inside a stamped cell maps to no source line. That follows from
  decision 14 (stamped means not editable) but no one has said it is acceptable for the editor.
- Decision 12 (the failed optional copy is soft) says "not re-verified against the code".

## Item 2 (Julia slowness and EPIPE): what the code supports

(Reported by B; the `serve(; timeout = 300)` location is verified-by-grep per B at
`resources/extension-subtrees/julia-engine/_extensions/julia-engine/quartonotebookrunner.jl:48`.)

- The julia engine is a TS engine extension: a Node child of q2 holds the client socket to the detached Julia
  server. q2 talks to Node over a pipe. So an EPIPE on the server's write means the **Node child or its parent
  was killed**, not that q2 closed a socket.
- Candidate closers: the extension's `execFile` timeout (default 300 s, SIGTERM, setting
  `q2.exportTimeoutSeconds`); Playwright test timeout or worker teardown (the findings note already records a
  "worker teardown timeout" on every run); q2's own cancel or kill path in `ts_process.rs`.
- The 120 s timeout with 0 outputs does not fit a cold start alone (cold is about 20 s measured); a first-ever
  precompile could be much longer. So "cold server" is plausible, not proven, as the plan said.
- A kill mid-run during a *passing* test implies another q2 run (the warm helper, or an orphan from an earlier
  timed-out test) was still going at teardown. Check that.
- **Cheapest experiment (no Positron):** in a scratch directory run `q2 record export-positron` on the Julia
  fixture, SIGKILL q2 and its Node child a few seconds in, and read the server log for EPIPE. Then compare the
  EPIPE timestamp in an e2e server log with the per-test end times. Inside a test means timeout; at the end means
  teardown. Not run by me.

## Windows: targeted list

(Reported by B, from reading only; nothing was cross-compiled and `aws-lc-sys` blocks the target from macOS.)
Highest risk first. I verified only the file:line anchors marked (v).

1. `crates/quarto-hub-provider/src/execute.rs:331` and `:527` (v): `canonicalize()` returns a `\\?\` verbatim path
   on Windows, and the later "outside the document directory" comparison may compare it with a non-verbatim path.
2. `crates/quarto-core/src/positron_cache.rs:150` (v) `vscode_file_uri`, and the `%APPDATA%` rule near `:130`:
   Windows branches are tested only with hand-written strings; the module's own comment says drive letter, `%3A`
   and non-ASCII are unverified.
3. `crates/quarto/src/commands/record.rs:139` (v): without `--source-uri` the `quarto_source` value is
   `doc.to_string_lossy()`, a backslash path on Windows. A wire-format path should use `/`
   (`quarto_util::to_forward_slashes`). Product question, not just a test.
4. `crates/quarto/tests/integration/record_export_positron_cli.rs` (reported): `:250` shells out to `python3`;
   `:113` expects exit 2 for the literal `/some/dir`, which may be a valid path form on Windows.
5. `crates/quarto-core/tests/integration/julia_engine_e2e.rs:1075-1098` (v: `kill` and `pgrep`; the file's header
   says the tests early-return without julia and node, but I found no `cfg(unix)` gate): confirm they cannot run
   on Windows with julia installed.
6. `quarto-core/src/record/sources.rs:49,101-107,179-199` (reported): `RecordSource::Dir` display uses native
   separators; check it never reaches a wire format.
7. `quarto-preview/src/capture_driver.rs:2932-2934` (reported): a test builds a URI from `abs.to_string_lossy()`
   with `HostOs::current()`, self-consistent but unproven against VS Code.
8. CRLF: the fixtures `positron-conformance/crlf.qmd` and `positron-cache/*.ipynb` under `autocrlf`.

Suggested run (on Windows): the epic's own command, which already names `quarto`, `quarto-core`, `quarto-preview`,
`quarto-hub`, `quarto-hub-provider`, `quarto-ast-reconcile`.

## What Playwright can do and what needs Gordon

(Reported by B; the premise that Playwright reaches only `q2 preview`, not the Positron workbench, is correct.)

- **Playwright against `q2 preview`:** the "edited cell loses syntax highlighting" observation; a re-confirm of the
  Re-execute refusal UI (the Rust test exists); the click-to-source and `--share` guest items from RIU-3;
  "Clear results"; the cache-older-than-document banner.
- **Positron's own e2e harness (Playwright Electron, in the fork), no human needed:** wrong `q2.path`; "q2 too
  old"; untitled document; an R session starting and the R version of `quarto-render-and-push.test.ts`.
- **Needs Gordon:** the cache-location API (a Positron design choice), the Viewer close-button fix, the decisions
  below, and watching one end-to-end run per language ("seen by a person" cannot be delegated).

## Where the reviewers disagreed or were wrong

- A said the fork change was uncommitted; B and I found it committed (finding 6). A also gave the keep-rule line
  as `:98`; the tree has `:101`. Minor.
- A put `record_splice.rs` under `record/`; it is `crates/quarto-core/src/stage/stages/record_splice.rs`. B had it
  right.
- A marked the wrong-`q2.path` check as "listed not run, but actually run"; I did not re-read the results table.
- Otherwise they agree: Item 5 stale, Order rationale needs fixing, Item 4 not done.

## What I did not verify

- Positron's hash for an R cell, Ark's cache shapes, and whether `POSITRON_R_VER_SEL=4.3.2` is set in the fork's
  e2e config: the fork is outside this worktree and I ran no Positron.
- Any Julia server log; who killed q2 in the failing runs.
- The Windows items other than the `file:line` anchors marked (v).
- The 3b plan's own box states after `3f0c55a6d`.
- Playwright against `q2 preview`: not run (no item needed it for the review).
- The knitr exit code (see finding 2) and the knitr record's `quarto_hash_rule` value.
- Nothing was pushed, and I ran no test suite.

## What changed in the plan

Nothing was deleted without a note here. Item 1: new Phase A boxes (dump the record, hash rule and the exit-time
Gordon gate, language check, Julia `#|`); Phase C box rewritten for the language check. Item 2: the "uncommitted"
statement corrected, the EPIPE box split into the experiment. Item 3: boxes ranked and four added. Item 4: the
second box points here and lists the uncovered intent as new boxes. Item 5: rewritten as "landed" with two exact
open boxes (the 21-case sweep; the labeled `fig-cap` splice case). New Item 6 for the Windows run. Order section
rewritten with dependencies and who runs what. Status line and Overview corrected.
