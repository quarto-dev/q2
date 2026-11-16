---
title: 'OPR and RFU plans: what main changed since the branch was written (2026-11-08)'
date: 2026-11-08
description: 'The history cleanup of feature/one-presenter onto main 8f17e950d and a drift review of the OPR and RFU plans: facts that are now false, what main added that touches the plans, and the decisions that are Gordon\''s.'
---

**Epics:** [one presenter path](../plans/2026-11-07-one-presenter-path-epic.md) and
[RIU follow-up](../plans/2026-11-08-rfu-epic-riu-follow-up.md).
Marks: **read** (I read the code or file), **ran** (I ran it and read the output), **relied** (I did not check it
myself). The review ran before any OPR or RFU plan was claimed.

## What the cleanup did

- The branch was `f67b4f63b` (175 non-merge commits, 33 of them already on main as the rebased EXR and KNR copies).
  It is now rebased onto `8f17e950d`, linear, with no merge commits. The old tip is the local branch
  `backup/one-presenter-pre-revise`; the state just before the final squash is `history/feature/one-presenter`.
- Dropped because main has them in a changed form: the knitr and scanner commits `7d5b1b3cf`, `fccc30ff8`,
  `37d930635`, `ce05d95f4`, `54907b7dc`, `375ece023`, `7cdec8403` (main: `72c8cae43`, `538c2dd77`, `b09e68c6a`,
  `4c61652fd`, `5eaf65372`, `953db1126`, `f104f8fa9`), and `f66c454d2` (main's `Cargo.lock` already lists `sha1`).
  The flattened merge `ad954ee85` brought in `d5ac058b3`, which is already on main; the Julia subtree under
  `resources/extension-subtrees/julia-engine` matches main exactly. **read** (`git diff 8f17e950d HEAD` is empty there).
- RIU is 12 commits (110 originals squashed by chronological group). RFU, One-presenter and plan commits follow.
- **Q-18-14 collision.** RIU-1 used `Q-18-14` for the record-less-engine warning; main gave `Q-18-14` to knitr child
  documents. The RIU warning is now `Q-18-15` everywhere (catalog, docs page, sidebar, code, RIU-1 plan, runbook).
- **Tree check.** `git merge-tree` of main with the old tip conflicts in 99 files, because the old EXR and KNR
  commits differ from main\'s rebased copies, so that comparison cannot be made clean. Instead, the added changes of
  `main..HEAD` were compared per file with those of the old tip over its last on-main commit: they are identical
  except the Q-18-15 renumber and the dropped `Cargo.lock` line. **ran**
- Two RIU commits (RIU-6 closeout, RIU-3b) edited a file that the RFU commits create, so those two hunks were split
  into one small commit placed after the RFU plan commits (`RIU follow-up: carry the RIU-6 closeout ...`).

## Facts in the plans that are now false

| Plan | Claim | Now | Action |
|---|---|---|---|
| OPR-7b | the knitr cache message is `Q-18-11` | `Q-18-11` is the Jupyter image warning; the knitr cache message is `Q-18-12` (hook replaced is `Q-18-13`, child document `Q-18-14`) | **fixed** in the plan. **read** |
| OPR-1a, review findings | fix at `99e3b2bb8` | main\'s copy is `2649c02ea` | **fixed** |
| epic, review findings, rewrite log | line numbers at `4099022dc` | that commit was rewritten; the same text is at `94906556f` | **fixed** |
| RFU-1 | `q2 record` landed in `d6954ccdf` | squashed; it is in `10cfb0f93` | **fixed** |

Other cited hashes (the `62afa5177` spike on `spike/one-presenter-s1-s3`, and everything on main) still resolve.
**ran** (every cited hash was resolved and tested against `HEAD`).

## What main changed that matters

- **Presenter notes (affects OPR-1a and OPR-1b).** Main\'s `present_document` now returns
  `PresentedDocument.notes: Vec<PresentNote>` and `ranged_units`
  (`crates/quarto-core/src/presenter/document.rs`), with `PresentNote::ImageUnusable` (`Q-18-11`), and
  `engine::diagnostics::note_diagnostic` turns notes into warnings on the replay and `text_execute` paths. The splice
  stage (`record_splice.rs`: `present_cell`, `present_span`) does not use `PresentNote`. **read**
  The OPR-1a and OPR-1b texts do not mention it. Whether the one function must also warn about an unusable image on the
  splice path is a boundary question; see Decisions.
- **Image failure handling.** Main added unpadded base64 and ASCII-whitespace tolerance (`79ef7b274`) and
  `present_skeleton` returns image-failure notes with their ranged units (`5df80f760`). The three
  `presenter-goldens/notebooks/*-invalid-base64-no-fallback/expected.md` goldens changed. **read**
- **knitr.** `KNITR_HASH_VERSION` is `2` (`knitr/hash_rule.rs`); `capture_install` is R code in
  `knitr/resources/rmd/capture.R`, not Rust, so OPR-7b edits R. The sql-chunk `#|` options commit, the Positron
  import into knitr-owned R units, and the stale-on-deleted-header rule are in main. **read**
- **Julia.** `JULIA_HASH_DEFINITION` exists twice: `julia-engine.js` and `src/record-wire.ts`. OPR-3 and OPR-4 cite
  both; they still exist. The runner functions in OPR-4a (`run_post_eval_hooks`, `render_code.jl`) are in
  `QuartoNotebookRunner.jl` (`external-sources/`, not vendored), so they cannot be re-checked in the tree. **relied**
- **Test gates.** `.config/nextest.toml`: the serialized Julia group now also includes
  `engine_output_parity::parity_julia_cell_and_inline_span`, and its comment says isolated rows (j7, pc4a) must not
  assert on the shared file. OPR-0b, OPR-4 and OPR-7c, which run Julia tests, must keep that in mind. CI pins `uv`
  to `0.13.7`. **read**
- **Error codes.** Next free numbers: `Q-18-16` (engine), `Q-12-27`, `Q-2-61`, `Q-19-6`, `Q-13-20`. If OPR-6 adds a
  code, take the next free one in the subsystem it belongs to at that time. `Q-2-58` to `Q-2-60` are taken on main. **read** (catalog)
- **Nothing on main does part of an OPR plan.** In the cited paths only `pipeline.rs` changed (it registers
  `editorial-marks-typst`); `pass2_renderer.rs` and `render_to_file.rs` did not change. No lint rule changed in
  `crates/xtask/src/lint`. I did not read the `project/mod.rs` change. **read**
- **Symbols that still exist:** `present_document`, `present_cell`, `present_span`, `core_presents`,
  `plan_capture`, `record_is_stale`, the three `records:` clap fields (`preview.rs`, `provide_hub.rs`, `record.rs`).
  `present_units` does not exist; OPR-1b proposes it. **read**
- **RFU fork paths.** `/Users/gordon/src/positron-riu6-a` (branch `spike/riu-6-phase-a`) has
  `extensions/q2-records-demo/src/{q2Contract,exportCommand,previewCommand}.ts`, `buildExportArgs`,
  `buildPreviewArgs`, `warmJuliaServer` (line 17) and the e2e test; `q2-raw-record-spike.test.ts` is untracked there.
  Paths the plans give relative to `external-sources/positron` do not exist; the fork is the right base. **read**

## Plan edits made

- OPR-7b: `Q-18-11` to `Q-18-12`.
- Hash citations in OPR-1a, RFU-1, the epic, the rewrite log and the review findings (see the table).
- OPR protocol: one sentence, "Line numbers in a plan are approximate; locate code by symbol."
- No plan status was changed.

## Decisions that are Gordon\'s (plan text left as is)

1. **Notes on the splice path.** After OPR-1b, should the splice stage report `PresentNote`s (image unusable, markdown
   format mismatch, inline no usable value) as warnings the way the replay path does? Today only the replay and
   `text_execute` paths do. This changes the OPR-1b boundary.
2. **Known stale, left alone.** The historical RIU notes cite pre-squash hashes (`2026-11-07-one-presenter-spikes.md`
   and `2026-11-07-plan-rewrite-log.md` cite `8cfec0fc1`; the RIU-5 and RIU-6 plans cite `d6954ccdf`). The
   `hub-client/changelog.md` does not cite any commit from this branch; its older entries are main\'s own and were not
   touched.
