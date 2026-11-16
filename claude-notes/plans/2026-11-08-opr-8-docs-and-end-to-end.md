---
title: 'OPR-8: user docs, embedded-docs refresh and end-to-end checks'
date: 2026-11-08
description: 'The last plan of the one-presenter epic. Writes the user-facing changes inside the existing docs pages (no page about differences), refreshes the embedded docs once, and runs the end-to-end checks for the whole epic, which merges as a single pull request.'
status: complete  # 2026-11-16 on branch epic-closeout; not pushed
depends-on: [opr-1b, opr-2, opr-5, opr-6, opr-9g, opr-4d, rfu-8]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/8-docs-and-end-to-end` · **Lane D** (`workspace-6`) · **Blocks:** nothing (it is last).
**Needs:** R 4.3 with knitr, Julia 1.12, Node 24; a full build of the preview chain.

Dependencies count as resolved when `complete` **or** `cancelled`; OPR-4d may have been cancelled with the Julia work.
RFU-8 (the RIU follow-up docs) must land first: it edits `execution-records.qmd` too, and this plan does the one
embedded-docs refresh for the whole integration branch.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-6 && pwd
git switch -c opr/8-docs-and-end-to-end feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/8-docs-and-end-to-end
```

## Goal (decision D11)

The epic merges as one pull request, so every docs change is written here, once, against the code as it landed.

**Style and structure (Gordon, 2026-11-08).** The docs site is a user-facing Quarto 2 site: document usage, not
internals. Differences between engines are listed **inside the pages about the functionality**, not on a page about
differences (see `docs/guides/authoring/computations.qmd`: "Empty values differ by engine", "Other engines",
"Inline expressions with Jupyter", and its "Migrating from Quarto 1" subsections). There is no page about cell
options; do not create one. **Write the prose with the `/reader-expectations-prose` skill** (Gopen and Swan); invoke
it for each page you edit.

## Pages and the sentences to change (found 2026-11-08; re-read each page first)

- `docs/guides/authoring/computations.qmd`: the per-engine behavior of `echo`, `warning`, `message`, `prompt` and
  numeric `echo` after OPR-1a to OPR-4d (what works for which engine; `prompt` and numeric `echo` need expression
  starts, which knitr and Julia supply and Jupyter does not, so Jupyter cells treat code as one expression; who
  hides warnings follows each engine); the sentence at about line 31, "Each cell is shown as code with no output, and
  no engine is started", is false for `--no-execute` with `execute: record-sources: q2`; keep the page\'s existing structure
  and add to its "differ by engine" sections.
- `docs/guides/authoring/execution-records.qmd`: it says a plain `q2 render` writes nothing under `.quarto/` (about
  line 20), that `q2 render` does not read Positron\'s cache (about lines 80-82), and that `q2 preview` and
  `q2 provide-hub` take `--records` (about lines 89-114). The option is gone (decision R5, Gordon, 2026-11-09): document
  the metadata key `execute: record-sources` instead, a comma-separated scalar (`positron,engine`) set in a document, a
  directory\'s `_metadata.yml`, `_quarto.yml`, or for one run with `-M 'execute:{record-sources: positron,engine}'` (the
  braces are needed: `-M` does not expand dotted keys). Cover: the entries and what each command allows and defaults to
  (preview `cache,q2,engine`, render `engine`, convert `q2,engine`, provide-hub `engine`); that a valid entry a command
  cannot use is ignored with a note while an unknown one is an error; that the list is resolved per document; that a
  directory entry must be **inside the project** (relative to the file that wrote it, a leading `/` is the project root;
  an absolute path or one outside the project is an error, and the pitfall is that a Positron cache outside the project
  cannot be named, so use the `positron` entry or link the cache in); that `-M` overrides the document; `--no-execute`
  and `execute: enabled: false` veto `engine`; `--replay` wins; the committed-file behavior (an entry with nothing to
  find falls through to the next, so the output can depend on whose machine runs it); that `provide-hub` and a `--share`
  preview take the host list as a ceiling (a shared document may only reorder or narrow it, and the host sees a warning
  for the rest); and the new `q2 render` behavior (read only, default `engine`, stale or format-mismatched records count
  as not supplied, a document with no source warns and stays inert, `--strict`), the fact that
  a plain render of Julia and knitr now presents from an in-memory record and writes nothing, and the `cache:
  true` chunk behavior (see the caching item below).
- `docs/guides/projects/preview.qmd`: the rename (about lines 147, 153, 162, 247), and the difference that preview
  shows a stale record while render treats it as not supplied; `preview.engine` is preview-only.
- The render options page (`docs/guides/projects/render-options.qmd`): there is no new flag; point to `execute: record-sources` and the `-M` spelling.
- Search for any other page that mentions `--records` or `--execution-records`: `git grep -n -e '--records' -e '--execution-records' docs/`; the result must be empty.
- If OPR-7c/7d changed Julia figure file names or the figure div classes, say so in the Julia engine page (user links to
  `<stem>_files/figure-html/cell-N-output-M.png` change); and that `fig-format` does not change what Julia writes
  (OPR-0c, ran, same on Quarto 1). Do not document the unresolved subfigure-group references as intended behavior; link
  `bd-gbaykhth` in the notes if still open.
- A knitr page or note about caching, only if a page describes it today (`git grep -n -i 'cache' docs/guides`);
  say that chunks with `cache: true` keep knitr\'s cache. If [OPR-7e](2026-11-09-opr-7e-cache-hits-as-records.md) is
  `complete`, say that a cache hit presents like any other chunk and that a stale cached output is knitr\'s usual cache
  caveat (adding code above a cached chunk re-runs it, because the chunk\'s identity includes its position); mention the
  legacy-entry case from 7e\'s notes. If 7e is blocked or not done, say that such chunks are not part of the execution
  record (OPR-7b).

## Checklist (every box is `[unattended]`)

- [x] Read each page, the landed plans\' "Notes from the run", and OPR-0c, OPR-0d and OPR-4a notes. Draft the changes
      as sentences first and list them in "Notes from the run".
- [x] Edit the pages with the `/reader-expectations-prose` skill. Render: `target/debug/q2 render docs/` and fix
      every warning your edits caused.
- [x] `git grep -n -e '--records' -e '--execution-records'` outside `claude-notes/` prints nothing.
- [x] **Embedded docs, once:** `cargo xtask build-agents-docs` then `cargo build --bin q2`; confirm with
      `target/debug/q2 docs llms --embed-info`. (It renders the docs with the `q2` you just built, so run it after a
      Rust build.)
- [x] **End-to-end for the whole epic**, through the real binary, recording each invocation and what you saw (say
      which were seen by a person and which were automated): `q2 render` plain on Jupyter, Julia, knitr and a LaTeX
      target; `q2 render -M 'execute:{record-sources: q2}'` with a prior record; `-M 'execute:{record-sources: q2,engine}'` with a
      stale record; `-M 'execute:{record-sources: q2}' --no-execute`; the key in front matter instead of `-M`; `q2 preview`
      of the same documents; `q2 render` of a project of several documents with a different key in each.
- [x] The preview chain: `cd hub-client && npm run build:wasm`, `cargo xtask build-q2-preview-spa`,
      `cargo build --bin q2`, then confirm the preview.
- [x] `cargo xtask lint`; `cargo clippy` on the crates the epic touched (`quarto`, `quarto-core`, `quarto-preview`,
      `quarto-hub-provider`); `target/debug/q2 render --strict claude-notes/plans/2026-11-07-one-presenter-path-epic.md`
      and each OPR plan and note.
- [x] **Do not run the workspace nextest or `cargo xtask verify`.** Write the pre-push recommendation in
      "Notes from the run": run `cargo xtask verify` (full, because `quarto-core` and the WASM leg changed) and say
      which dependents of `quarto-core` to run (`cargo tree -i quarto-core`).
- [x] If anything under `hub-client/` changed in the epic, the two-commit changelog rule in AGENTS.md applies.
- [x] Reconcile the epic: re-read it, verify each plan\'s status and exit criteria against the integration branch, and
      list the **companions** still owed to Gordon (pushes to the Julia engine and runner forks; the Positron
      fork\'s work (RFU-7); any push of this epic\'s branch and the rebase onto `main`).

## Exit criteria

- The pages are true for the code as landed; the docs render clean; the embedded docs are fresh.
- The end-to-end record covers each invocation above.
- The pre-push recommendation and the companion list are written for Gordon.

## Notes from the run

Run 2026-11-16, one agent, branch `epic-closeout` (cut from `main` at `a3cb3e2de`). The plan\'s "Branch and land"
steps (`opr/8-docs-and-end-to-end`, `workspace-6`, `feature/one-presenter`) were obsolete: the integration branch is
already an ancestor of `main`, so the work sits on `epic-closeout` and is not pushed. I wrote the prose with the
`/reader-expectations-prose` skill (Gopen and Swan: topic and stress position, old before new) and the Google developer
documentation style guide (GDDSG) together. The skill governed structure and GDDSG governed wording and mechanics.

**Pages changed (5 files, no snapshot files).**

- `docs/guides/authoring/execution-records.qmd`: the opt-in callout (a plain render of Julia and knitr builds the record
  in memory and writes nothing); the Positron paragraph; the whole `--records` section replaced by "Choosing where records
  come from" (the key, the `-M` spelling with braces and the quoting rule, entries, a per-command accepts-and-defaults table,
  in-project directories, per-document resolution, fall-through, what `q2 render` does, the preview-versus-render stale
  difference, the `--share` and `provide-hub` ceiling); one Limits bullet (single-file books).
- `docs/guides/authoring/computations.qmd`: the `--no-execute` paragraph (a saved record can supply outputs with
  `record-sources: q2`), and a new section "Showing and hiding code and output" with the per-engine table.
- `docs/guides/projects/preview.qmd`: every `--records` mention renamed; the stale-versus-render difference; `preview.engine`
  is preview-only.
- `docs/guides/projects/render-options.qmd`: a section that points at `execute: record-sources` and the `-M` spelling.
- `docs/guides/authoring/knitr.qmd`: caching (a cached chunk is recorded on a miss and a hit; the two quiet exclusions).

**Findings that changed the wording.**

1. `-M 'execute:{record-sources: q2,engine}'` (the spelling in the plan) is **not safe**: YAML ends the value at the comma,
   `engine` becomes a null sibling key under `execute`, and the record then reads stale (observed: "the record is stale").
   The docs say to quote: `-M 'execute:{record-sources: "q2,engine"}'`. A fix in the reader is a better answer than a docs
   rule (OPR-5 finding 1); not done here.
2. `execute: enabled: false` is part of the record hash, so setting it makes a record stale instead of vetoing the engine. I
   documented `--no-execute` only. The plan listed both.
3. A render with `record: true` and `engine` in the list rewrites the record, so a probe that changes `execute:` rewrites the
   record it then tries to read (this explained one stale result while checking).
4. Julia figure file names did not change on the recorded route (OPR-7f keeps the plain route\'s rule), so the docs need no
   sentence about them. `fig-format` is unchanged (OPR-0c).
5. Not documented, per the plan: unresolved subfigure-group references (`bd-gbaykhth`). `figures.qmd` "Subcaptions" still
   shows static Quarto 1 images and says computed subcaptions are cross-referenceable; OPR-7j recorded that q2 numbers later
   figures differently. That needs a separate docs pass (the page is not in this plan\'s list).
6. Julia `warning: false` hides only warnings (an `@info` and a raw `println(stderr, ...)` stayed); Jupyter hides all
   `stderr`; knitr hides warnings and messages. The table states this.

**What I ran** (`target/debug/q2`, scratch directory in the session scratchpad, R 4.x with knitr, Julia 1.12; HTML read with
`grep` for the sentinel outputs `KNITR-OUT` and `JULIA-OUT`; **automated, not seen by a person**):

| Invocation | Observed |
|---|---|
| `q2 render k.qmd` and `q2 render j.qmd` (knitr, Julia) | Output present (source and output lines); `.quarto/` holds only `render-manifest.json` |
| `q2 render rk.qmd` and `rj.qmd` with `execute: {record: true}` | `.quarto/execution-records/rk.ipynb` and `rj.ipynb` written |
| `PATH=/usr/bin:/bin q2 render rk.qmd -M 'execute:{record-sources: q2}'` (and `rj`) | `KNITR-OUT` and `JULIA-OUT` presented with no engine on `PATH` |
| the same with the value unquoted, `q2,engine` | `Warning: No execution record ... (q2: the record is stale ...)` |
| the same with the value quoted, `"q2,engine"` | no warning, output present |
| edited cell, `-M 'execute:{record-sources: q2}'` | warning "record is stale"; code without output |
| edited cell, `-M 'execute:{record-sources: "q2,engine"}'` | the engine ran; output present |
| `-M 'execute:{record-sources: q2}' --no-execute` with a fresh record | output present, engine hidden |
| `q2 render rk.qmd --no-execute` | code only |
| key in front matter (`fj.qmd`), no engine on `PATH` | `JULIA-OUT` presented |
| `-M 'execute:{record-sources: positron}'`, no record | warning names `positron: no record`; with `--strict` it is an error |
| `-M 'execute:{record-sources: bogus}'` | error listing `cache`, `q2`, `positron`, `engine` and the directory rule |
| `-M 'execute:{record-sources: "cache,q2"}'` | `Info: ... cache is ignored; q2 render cannot use it` |
| directory `../../outside` in a project | error: "outside the project" |
| `q2 render` of a project of three documents (a different key in each directory) | two per-document warnings, three files rendered |
| `q2 render k.qmd --to pptx` and `--to typst` | rendered with knitr\'s own lines only |
| Julia cell with `prompt: true`, `echo: [2]`, `warning: false` | prompts per expression; only `c = b + 1` shown; `@warn` hidden, `@info` and raw stderr kept |
| `q2 preview rk.qmd --static -M 'execute:{record-sources: q2}'`, `curl` of `rk.html` | recorded output served, no engine |
| `q2 preview rk.qmd --no-browser` (live viewer) | HTTP 200 on `/`; the viewer page was **not looked at in a browser** |

**Not run.** Jupyter (no `jupyter_client` here: `Engine 'jupyter' is registered but its runtime is not available`), so the
Jupyter columns of the new table rest on the presenter unit tests (`prompt_without_starts_treats_the_code_as_one_expression`)
and the OPR-2 audit, not a live run. A LaTeX target (`pdf` is not supported by the binary; `pptx` and `typst` stand in).
`--replay` with a key (the sentence "a `--replay` render ignores the key" rests on decision D6 and the OPR-6 unit test).
The live viewer in a browser, `--share` against a live tunnel, and the Windows run.

**Gates.** `cargo xtask build-agents-docs` then `cargo build --bin q2`: `q2 docs llms --embed-info` prints `source: real`,
`pages: 430`, commit `9ff0122b1 (dirty)`; `q2 docs llms guides/authoring/execution-records.md` contains `record-sources`
(7 hits). `q2 render docs/`: 430 files, 33 warnings, none from the five edited pages (the existing Q-5-6, Q-13-4, Q-2-50 and
Q-16-5 ones). `git grep -n -e '--records' -e '--execution-records' -- docs` prints nothing; outside `claude-notes/` the option
still appears only in two negative tests (`crates/quarto/src/main.rs`, `convert_positron_cli.rs`), which must spell it.
Preview chain: `npm run build:wasm` succeeded; `cargo xtask build-q2-preview-spa` first failed on `@viz-js/viz` (a dependency
added to `main` after this worktree\'s `node_modules` was installed) and succeeded after `npm install` from the repo root (I
restored `package-lock.json`, which `npm install` had rewritten); then `cargo build --bin q2`. `cargo xtask lint`: all checks
passed (1688 files). `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets -- -D
warnings`: clean. No Rust, hub-client or snapshot file changed in this branch, so nextest and the changelog two-commit rule do
not apply.

**Pre-push recommendation (for Gordon).** This branch changes docs and plans only. The epic code is already on `main`, so the
useful wider check is the one the epic never had as a whole: `cargo xtask verify` (full, because `quarto-core` and the WASM leg
changed across the epic), plus `cargo nextest run` on the direct dependents of `quarto-core` (`cargo tree -i quarto-core`).

**Companions still owed to Gordon.** The Julia engine fork (`quarto-julia-engine`, `q2/node-port`) has no unpushed commits (in
step with `origin`). The runner fork has `q2/opr-expression-facts` on `origin`; whether upstream QuartoNotebookRunner took the
`emitUnits` change was not checked. The Positron fork work (RFU-7) sits on the local branch `q2/rfu-7-demo-followups` in
`external-sources/positron` and needs Gordon to push it. Rebasing onto `main` is moot: the integration branch is already
merged. The Windows run (RFU-6) is still Gordon\'s.
