---
title: 'OPR-8: user docs, embedded-docs refresh and end-to-end checks'
date: 2026-11-08
description: 'The last plan of the one-presenter epic. Writes the user-facing changes inside the existing docs pages (no page about differences), refreshes the embedded docs once, and runs the end-to-end checks for the whole epic, which merges as a single pull request.'
status: ready
depends-on: [opr-1b, opr-2, opr-5, opr-6, opr-7d, opr-4d, rfu-8]
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

- [ ] Read each page, the landed plans\' "Notes from the run", and OPR-0c, OPR-0d and OPR-4a notes. Draft the changes
      as sentences first and list them in "Notes from the run".
- [ ] Edit the pages with the `/reader-expectations-prose` skill. Render: `target/debug/q2 render docs/` and fix
      every warning your edits caused.
- [ ] `git grep -n -e '--records' -e '--execution-records'` outside `claude-notes/` prints nothing.
- [ ] **Embedded docs, once:** `cargo xtask build-agents-docs` then `cargo build --bin q2`; confirm with
      `target/debug/q2 docs llms --embed-info`. (It renders the docs with the `q2` you just built, so run it after a
      Rust build.)
- [ ] **End-to-end for the whole epic**, through the real binary, recording each invocation and what you saw (say
      which were seen by a person and which were automated): `q2 render` plain on Jupyter, Julia, knitr and a LaTeX
      target; `q2 render -M 'execute:{record-sources: q2}'` with a prior record; `-M 'execute:{record-sources: q2,engine}'` with a
      stale record; `-M 'execute:{record-sources: q2}' --no-execute`; the key in front matter instead of `-M`; `q2 preview`
      of the same documents; `q2 render` of a project of several documents with a different key in each.
- [ ] The preview chain: `cd hub-client && npm run build:wasm`, `cargo xtask build-q2-preview-spa`,
      `cargo build --bin q2`, then confirm the preview.
- [ ] `cargo xtask lint`; `cargo clippy` on the crates the epic touched (`quarto`, `quarto-core`, `quarto-preview`,
      `quarto-hub-provider`); `target/debug/q2 render --strict claude-notes/plans/2026-11-07-one-presenter-path-epic.md`
      and each OPR plan and note.
- [ ] **Do not run the workspace nextest or `cargo xtask verify`.** Write the pre-push recommendation in
      "Notes from the run": run `cargo xtask verify` (full, because `quarto-core` and the WASM leg changed) and say
      which dependents of `quarto-core` to run (`cargo tree -i quarto-core`).
- [ ] If anything under `hub-client/` changed in the epic, the two-commit changelog rule in AGENTS.md applies.
- [ ] Reconcile the epic: re-read it, verify each plan\'s status and exit criteria against the integration branch, and
      list the **companions** still owed to Gordon (pushes to the Julia engine and runner forks; the Positron
      fork\'s work (RFU-7); any push of this epic\'s branch and the rebase onto `main`).

## Exit criteria

- The pages are true for the code as landed; the docs render clean; the embedded docs are fresh.
- The end-to-end record covers each invocation above.
- The pre-push recommendation and the companion list are written for Gordon.

## Notes from the run

(Fill in at landing.)
