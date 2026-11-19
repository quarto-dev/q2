---
title: 'GE-18: Docs and closeout'
date: 2026-11-18
description: Writes the user docs for git-aware live share and the VS Code extension, reconciles every plan checklist with what landed, runs the final verification and composition review, and prepares the pull request for Gordon without pushing.
status: approved
depends-on: [ge-17]
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-17](ge-17-windows-run-c.md) ·
**Unblocks:** the pull request for the whole epic ·
**Branch:** `ge/18-docs-and-closeout` off `feature/git-epochs`, merged by rebase and fast-forward. Nothing is pushed in
this plan; the pull request waits for Gordon.

## Goal

Finish the epic. Document what users can now do and what they cannot, make sure every plan file says what actually
landed, run the full verification, have the finished work reviewed once more as a whole, and hand Gordon a pull request
summary and a pre-push recommendation. Gordon decides when to push and which held items become strands.

## Decisions implemented

- **G33:** `q2 preview --git <track|ignore>` is documented: experimental, default `ignore`, a value is required, and it
  conflicts with `--join`, `--static`, and `--hub-url`; `track` means this preview owns the project\'s pointer.
- **G35:** the accepted gaps are stated in the docs, not hidden: the hub restart gap, a stale pointer while the owner is
  offline, extension builds older than this epic overwriting a working tree on a flip, two tips after `git add -p`,
  `git stash pop` and `git restore` still looking like edits, nested repos and submodules, two sync agents on one
  directory, unknown retention on the public server, file deletions on mirror folders of older builds, arrival under
  colocated jj being unsupported, and peers staying split after a `reset --hard` or `git stash` by one peer.
- **G13, G15, G19, G21, G22, G24:** the extension\'s user-facing behavior (modes, join, attach, share) is documented in
  its README and in a section of the live-share docs, marked experimental.
- **G4:** colocated jj is documented as gate-only (no arrival: files take the step 5 fallback) and non-colocated jj
  as off.
- **Epic closeout rules:** the old plan is already marked superseded; the held items are reviewed, not opened;
  nothing is pushed.

## Context

- **Where docs live today.** `docs/guides/projects/preview.qmd` covers the default preview, the editor UI, a
  `### Sharing` section on `--share`, the static preview, and a `### Flags` section that says "the live-session flags
  (`--share`, `--allow-edit`, `--ui`, `--join`)". The sidebar entry is in `docs/_quarto.yml`. The extension\'s only
  documentation is `vscode-sync-experiment/README.md`, rewritten here.
- **Docs are user-facing.** Document usage, not internals (`AGENTS.md`). The site is rendered with Quarto 2:
  `cargo run --bin q2 -- render docs/`, never `quarto`. Staging the embedded docs fails if the site loads a remote
  subresource.
- **Embedded docs trap.** `q2 docs llms`, `q2 agents-info`, and `q2 docs serve` read `agents-docs-dist/`, which is stale
  until `cargo xtask build-agents-docs` and then `cargo build --bin q2` run (in that order: staging renders with `q2`).
- **The hub-client changelog entry** is owned by GE-7 (two-commit workflow: the change, then the changelog with the
  hash). This plan verifies it exists.
- **Rules that apply at the end.** Reconcile each plan\'s checklist against what landed before handing off
  (Gordon\'s finishing rule). A workspace-wide test run is recommended to Gordon, not run unprompted; when it is run,
  report its delta against a live baseline measured at the time, not a figure from an older document. A braid strand
  is only for work outside every active plan.

## Test specifications

This plan adds no product tests. Its checks are the documentation render, the embed check, the lint, the full verify,
and the review. Each states what it proves:

| Check | A pass proves | It does not prove |
|---|---|---|
| `cargo run --bin q2 -- render docs/` | The new docs sections parse and render under Quarto 2 | That the prose is accurate; the review checks that |
| `q2 docs llms --embed-info` | The embedded docs carry the new pages and the right source commit | Anything about the rest of the binary |
| `cargo xtask lint` | The custom lint rules hold, including `ci-test-suite-unwired` for the new packages | Test coverage |
| `cargo xtask verify` and `cargo xtask check-noop-rebuild` | Rust and hub-client build, the workspace tests pass, and a second build compiles nothing | Windows behavior (GE-15 to GE-17) |
| The composition review | Independent readers found no unaddressed blocking or important defect in the finished work | That there are none |

## Checklist

- [ ] Confirm GE-17 is complete, including its final statement and any fix plans it inserted (each must be complete).
- [ ] **Docs, usage.** In `docs/guides/projects/preview.qmd`, add a section after `### Sharing`, "Git and live share":
  what `--git track` does (a checkout, pull, rebase, or reset in the shared directory is handled as a switch of
  versions, not as edits; commits disturb nobody), what collaborators see (the notice that the project owner\'s
  checked-out version changed, and that edits typed during the switch stay with the previous version), the
  "paused for git" indicator, colocated jj support and non-colocated jj off, and that `--git` conflicts with `--join`,
  `--static`, and `--hub-url`. Add `--git` to the `### Flags` text. Mark the feature experimental.
- [ ] **Docs, gaps.** In the same section, list the accepted gaps from G35 in plain language.
- [ ] **Docs, extension.** In the same page, add "The VS Code extension (experimental)": the three modes (mirror,
  editor only, clone) in a sentence each, the Join session and Share this folder flows, and a link to the README for
  detail.
- [ ] **Extension README.** Rewrite `vscode-sync-experiment/README.md`: every command (Connect to Document, Disconnect,
  Create Folder from Project, Join session, Leave session, Share this folder), the modes and how a folder gets its mode,
  attach and the clone check, rebinding, the `quartoHub.userName` and `quartoHub.q2Path` settings, requirements (git on
  `PATH`, `q2` for ticket joins), the known gaps, and "experimental". It must say that extension builds older than this
  epic can overwrite a working tree when a project owner switches versions, and that older web clients ignore a flip.
  The stale "workspace member" line is already fixed by GE-8; confirm.
- [ ] **Public server warning.** Put the warning (anyone with the link can read and edit; publishing cannot be undone)
  in the README, the docs section, and confirm the Share this folder dialog shows it.
- [ ] Render the docs: `cargo run --bin q2 -- render docs/`. Fix anything it reports.
- [ ] **Rebuild every embedded artifact** (the epic\'s rule; a plan that finished early may have left one stale):
  `cargo xtask build-hub-mcp-bundle`, `cd hub-client && npm run build:wasm`, `cargo xtask build-q2-preview-spa`,
  `cargo xtask build-hub-client-embed`, then `cargo build --bin q2`; check `q2 mcp --launcher-info`. The agents docs
  come last, because staging renders with `q2`.
- [ ] Rebuild the embed: `cargo xtask build-agents-docs`, then `cargo build --bin q2`; run `q2 docs llms --embed-info`
  and record the source commit, the dirty marker, and the page count. Staging fails on a remote subresource: fix the
  page, not the allowlist.
- [ ] Verify the hub-client changelog entry from GE-7 exists with real commit hashes; if hub-client changed, run
  `cd hub-client && npm run test:wasm` (the changelog render test). Do not add a new entry unless it is missing.
- [ ] **Reconcile the plans.** Re-read the epic and each of GE-1 to GE-17. For every `- [ ]` and `- [x]`, check the
  box against the code, the tests, and the notes; correct wrong marks; fix stale status lines; commit the updated
  plan files. Set the epic\'s `status:` to `done` with a one-line comment, and confirm the table "What became of D1 to
  D36" still matches what landed.
- [ ] Confirm the old plan `2026-11-17-git-aware-live-share-epochs.md` is marked superseded with the pointer to the epic.
- [ ] **Lint and render.** `cargo xtask lint`. Then `q2 render --strict` on the claude-notes files this epic touched
  (the epic, the changed plans, the design doc from GE-1, the pull request summary), not the whole directory unless
  Gordon asks. A single-file render prints about 56 lines of Q-12-13 listing noise about other plans; ignore those and
  fix anything that names your file.
- [ ] **Full verification, with Gordon\'s agreement first.** `cargo xtask verify` includes the workspace nextest run, which
  Gordon\'s rule says to recommend, not run unprompted. Ask. If he agrees, run `cargo xtask verify` (not
  `--skip-hub-build`: the WASM leg is affected), then `cargo xtask check-noop-rebuild`, and report the pass and skip
  counts against a baseline measured at that time on `main`, accounting for the difference.
- [ ] **Composition review.** Launch two independent review agents (model `sonnet`, no sub-agents of their own, read-only,
  nothing edited or committed), one on correctness and mechanics (git operations, the registry, arrival, the gate,
  the extension modes) and one on clients, UX, plan quality, and conventions, as the review that started this epic
  did. Ask each for severity (blocking, important, minor), evidence (file and line, or the exact command and output),
  a suggested change, and under 900 words. Verify every blocking or important claim yourself; drop or correct what
  you cannot confirm and say which. Fix confirmed defects with new tasks or plans; record the rest in the notes.
- [ ] **Held items.** Present the epic\'s held items 1 to 12 to Gordon with what the epic learned about each. Create no
  braid strand without his say-so, and use `braid`, never `br`. Record his decisions in the notes.
- [ ] **Windows tooling branch.** Confirm the results and the final statement are copied into the notes of GE-15 to
  GE-17, and that nothing from `scripts/git-epochs-windows/` is reachable from `feature/git-epochs`
  (`git log feature/git-epochs -- scripts/git-epochs-windows` prints nothing). Then, only after Gordon agrees, delete
  the branch locally (`git branch -D windows/git-epochs`) and on the remote
  (`git push origin --delete windows/git-epochs`), and check with `git ls-remote origin windows/git-epochs`.
- [ ] **Pull request summary.** Write a draft for Gordon (detail chosen by the plan author; confirm the path:
  `claude-notes/plans/git-epochs/pr-summary.md`, with front matter and a description): what changed per plan; every
  snapshot file added, modified, or removed, with counts and what changed, calling out anything surprising; the e2e
  transcripts from GE-4, GE-13, and GE-14; the Windows results and final statement; the known gaps; the demos and how to
  run them. List the affected snapshot files so Gordon can review the diffs.
- [ ] **Pre-push recommendation.** In the summary, recommend (do not run unprompted) `cargo nextest run --workspace` and,
  more cheaply, `cargo nextest run -p <crate>` for the direct dependents of each crate touched (`cargo tree -i
  quarto-hub`, `cargo tree -i quarto-preview`, `cargo tree -i quarto`, and the new `quarto-git-epochs` crate).
- [ ] Land this plan: rebase onto `feature/git-epochs`, fast-forward, set this plan\'s `status:` to `done`.
- [ ] Wait for Gordon. Only if he says to push: `git push origin feature/git-epochs`, then check that
  `git ls-remote origin feature/git-epochs` prints `git rev-parse HEAD`; never silence the push.

## Gates

- `cargo run --bin q2 -- render docs/`, `cargo xtask build-agents-docs`, `cargo build --bin q2`, and
  `q2 docs llms --embed-info` as above.
- `cargo xtask lint`; `q2 render --strict` on the affected claude-notes files.
- `cargo xtask verify` and `cargo xtask check-noop-rebuild`, after Gordon agrees.
- Cross-platform: the Windows evidence is GE-15 to GE-17; this plan adds no platform-specific code.

## End-to-end verification

The docs are verified by rendering them and by the review reading them against the finished behavior. The feature itself
was verified end to end in GE-4, GE-13, GE-14, and GE-17; this plan records pointers to those transcripts in the pull
request summary and does not redo them. The embedded-docs check is run through the real binary:
`q2 docs llms --embed-info`, and open the new section with `q2 docs serve` or `q2 agents-info`.

## Exit criteria

- The docs and the extension README are written, render, and are embedded.
- Every plan file\'s checklist matches what landed, and the epic is `done`.
- Lint, render, and (with Gordon\'s agreement) the full verification are green, or each failure is explained.
- The composition review ran and every confirmed blocking or important defect is fixed or recorded.
- The held items are reviewed with Gordon; the Windows tooling branch is handled as he decides.
- The pull request summary and the pre-push recommendation are with Gordon. Nothing is pushed.

## Out of scope

- Pushing, opening the pull request, or running the workspace tests unprompted.
- Creating braid strands for the held items without Gordon\'s say-so.
- New features; documentation of extension distribution or publishing (out of scope for the epic).
- Fixing Windows defects (fix plans from GE-15 to GE-17 are separate plans).

## Notes from the run

Not started.
