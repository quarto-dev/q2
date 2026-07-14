# Issue #233 — New file error: 'Sibling page failed to parse'

- **GitHub**: https://github.com/quarto-dev/q2/issues/233
- **Reporter**: @shikokuchuo (Charlie Gao), 2026-05-22
- **Triage date**: 2026-07-14
- **Worktree**: `.worktrees/issue-233` (branch `issue-233`, based on `main` @ `cd89283b`)
- **Braid strand**: bd-3a3ymh26 (presentation fixes; see Outcome)
- **Scope**: the single reported behavior — "Sibling page '/project/index.qmd' failed to
  parse" appearing when a new `.qmd` file is created, while existing files reportedly do
  not show it. Covers both hub-client and `q2 preview` (shared render pipeline).

## Summary

The reporter has a project where `index.qmd` contains a parse error. Creating a new
`.qmd` file immediately surfaces "Sibling page '/project/index.qmd' failed to parse",
while (per the report) existing files do not show this error even when modified.

**The reported asymmetry does not reproduce at HEAD.** The sibling-failure banner shows
*consistently on every page* of the project — existing pages (on file switch and on
edit) and freshly-created files alike, in both hub-client and `q2 preview`. This is the
deliberate bd-rqba design (PR #149): Pass-1 of the project orchestrator extracts a
profile from **every** project file on every active-page render (needed for sidebar
titles, navbar, cross-doc links), and a sibling that fails to parse is surfaced as a
banner so the user sees the real parse error instead of only the misleading
"Sidebar/Navbar references missing document" warning. Answering the comment on the
issue: there is no inadvertent "required rendering of index.qmd" — it is Pass-1 profile
extraction (parse + metadata only, no Pass-2 render), and it is intentional and
active-page-independent.

Code reading found no mechanism that could have produced the asymmetry at issue time
either: the banner logic in `Preview.tsx` and the `pass1_failures` plumbing in the WASM
entry point were byte-equivalent (in behavior) at `02e6deea` (main on 2026-05-22) to
today's, Pass-1 failures are never cached (the IndexedDB profile cache stores only
successful profiles, keyed by content hash), and `fail_fast` is off in WASM. Most
likely the reporter's existing pages showed the same overlay in its **collapsed** state
(a small "⚠ Error" pill, easy to miss — collapsedness is a persisted preference) while
the new file presented it expanded, or the deployed hub was mid-window between builds.
Not conclusively determinable from outside the deployed environment.

**What *is* wrong** (filed as bd-3a3ymh26):

1. **Severity mismatch**: hub-client shows the sibling failure as a red **"Render
   Error"** even though the active page rendered fine. The q2-preview SPA shows the same
   state as an amber **"Render Warning"** (`PreviewDiagnosticsOverlay` has a
   `--warning` variant; the shared `PreviewErrorOverlay` used by hub-client is
   error-only). A brand-new empty file greeting its author with a red render *error*
   about a different file is exactly what alarmed the reporter.
2. **Path leak**: the banner prints the VFS-internal absolute path
   (`/project/index.qmd`) instead of the project-relative `index.qmd`
   (`pass1_failures_to_json` uses `failure.input.to_string_lossy()`).

## Reproduction

Fixture: `claude-notes/issue-reports/233/repro/` — a two-page website project where
`index.qmd` contains the known Q-2-10 apostrophe parse error (same content as the
bd-rqba regression tests) and `about.qmd` is fine.

### 1. Native render (confirms the Pass-1 failure)

```
cargo run --bin q2 -- render claude-notes/issue-reports/233/repro
```

Observed: `warning: profile-pass skipped .../index.qmd: Error: [Q-2-10] Closed Quote
Without Matching Open Quote ... index.qmd:5:36`, plus `Warning [Q-13-1]: Sidebar
references missing document`, and "Rendered 1 of 2 files … 1 error, 1 warning".

### 2. `q2 preview` (browser, existing page)

```
cargo run --bin q2 -- preview claude-notes/issue-reports/233/repro
# open http://127.0.0.1:<port>/?page=about.qmd
```

Observed: `about.qmd` renders; a collapsed "⚠ Warning" pill sits bottom-right, which
expands to the Q-13-1 warning **plus** "⚠ /project/index.qmd failed to parse …
[Q-2-10] …". Same overlay after creating `new.qmd` on disk. Symmetric; presented as a
*warning* here.

### 3. hub-client (browser, the reporter's exact scenario)

```
target/debug/hub --data-dir <scratch> -P 3000 -H 127.0.0.1 --allow-insecure-auth
cd hub-client && VITE_DEFAULT_SYNC_SERVER=/ws npm run dev
# open http://localhost:5173/, create a Website project "issue-233-repro"
```

Steps and observations (all output inspected in the live browser):

1. Created `about.qmd` with valid content → no overlay (clean baseline).
2. Appended the apostrophe line to `index.qmd` (active) → "⚠ Error" overlay pill.
3. **Switched to existing `about.qmd`** → overlay present; expanded it:
   `Sibling page '/project/index.qmd' failed to parse` + `⚠ /project/index.qmd failed
   to parse` + `Line 9: Closed Quote Without Matching Open Quote - …` — the exact
   reported message, **on an existing page**, contradicting the reported asymmetry.
4. **Edited `about.qmd`** → overlay persists across re-renders.
5. **Created `newpage.qmd` via + New** → same overlay immediately, styled as a red
   "Render Error", plus a `[Q-13-2] Navbar references missing document` banner over the
   editor. (Screenshot state recorded in session transcript.)

## Localization

- Banner construction: `hub-client/src/components/render/Preview.tsx:89-94`
  (`pass1FailuresBannerMessage`) and `:310-317` (`setCurrentError` on *successful*
  renders when `pass1Failures` is non-empty).
- Error-only shared overlay: `ts-packages/preview-renderer/src/overlays/PreviewErrorOverlay.tsx`
  (hard-codes "Render Error"); warning-capable sibling:
  `q2-preview-spa/src/components/PreviewDiagnosticsOverlay.tsx:127` (`--warning`).
- `/project/` path leak: `crates/wasm-quarto-hub-client/src/lib.rs:558`
  (`pass1_failures_to_json`); the project-relativization model to copy is
  `pass1_project_relative_source_path` in
  `crates/quarto-core/src/project/orchestrator.rs:1548`.
- Sibling failures are produced per-file and unconditionally on every project-mode
  render: `pass1_profile_with_cache` / `pass_one_dispatch_async` in
  `crates/quarto-core/src/project/orchestrator.rs` (failures are never cached; the
  profile cache stores successes only, keyed by source bytes).
- Existing regression tests already pin the symmetric behavior:
  `crates/quarto-core/tests/integration/render_page_in_project.rs:389`
  (`pass1_parse_error_in_sibling_surfaces_alongside_active_render`).

## Open questions — resolved during triage

- **Q: Does anything make Pass-1 skip or cache the broken sibling depending on which
  page is active?** Experiment: code audit of `pass1_profile_with_cache`,
  `profile_cache::load/save`, `cache_key::Pass1KeyInputs`, plus live renders. A: No.
  The cache key includes the file's own bytes; failures produce no cache entry; every
  project-mode render re-parses the broken sibling and reports it.
- **Q: Was the front-end asymmetric at issue time (2026-05-22)?** Experiment: diffed
  `Preview.tsx`, `PreviewErrorOverlay.tsx`, and the WASM `pass1_failures` plumbing at
  `02e6deea` against HEAD. A: Behaviorally identical — banner set on every successful
  render with failures; only collapse (not dismissal) exists.
- **Q: Does the hub project scaffold include `_quarto.yml`?** A: Yes for both project
  types; renders take the project-aware path (`render_page_in_project` →
  `ProjectPipeline` with `RenderMode::ActivePage`). Without a `_quarto.yml` the
  single-doc path never reports `pass1_failures` at all — it cannot produce the
  reported banner, so the reporter's project had one.

## Outcome / recommended next step

- **The reported inconsistency is not reproducible at HEAD**; the banner is symmetric
  and by design (bd-rqba). Recommend responding on GH with the explanation (broken
  sibling pages are parsed on every render to build navigation; the banner is
  informational and points at the file that actually needs fixing).
- **Filed bd-3a3ymh26** for the two real presentation bugs that made the banner read as
  a scary error about the new file: (1) show sibling Pass-1 failures with *warning*
  severity in hub-client (as q2-preview already does), (2) print project-relative paths
  instead of `/project/…`.
- Suggested GH reply draft (for the maintainer to post):

  > The message comes from the live-preview building navigation (sidebar titles,
  > cross-links) for the whole project: every page is parsed — parse-only, no full
  > render — on each preview render, and one of your files (`index.qmd`) has a parse
  > error at the indicated line, so the preview reports it no matter which file is
  > open. As of current main this banner shows consistently on every page (we could not
  > reproduce it appearing only for new files), and fixing the parse error in
  > `index.qmd` makes it go away. That said, the presentation is misleading — it's
  > styled as a red "Render Error" on a page that rendered fine, and it prints an
  > internal `/project/` path — both are being fixed (bd-3a3ymh26).

## Verification commands used

```
gh issue view 233 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build                  # green at cd89283b before starting
cargo xtask create-worktree --issue 233
cargo run --bin q2 -- render claude-notes/issue-reports/233/repro
cargo run --bin q2 -- preview claude-notes/issue-reports/233/repro   # + Chrome DevTools MCP
target/debug/hub --data-dir <scratch> -P 3000 -H 127.0.0.1 --allow-insecure-auth
cd hub-client && VITE_DEFAULT_SYNC_SERVER=/ws npm run dev            # + Chrome DevTools MCP
git rev-list -1 --before=2026-05-23 main             # 02e6deea; diffed overlay + WASM plumbing
```

## Cross-references

- bd-rqba — sibling Pass-1 failures surfaced alongside successful active-page renders
  (design this behavior comes from; see `Preview.tsx` comments and
  `render_page_in_project.rs` tests).
- bd-mwtf — active-page Pass-1 failure diagnostics.
- bd-8d6rk — structured Q-13-1/Q-13-2 "references missing document" diagnostics.
- bd-3a3ymh26 — the follow-up filed by this triage.
- `claude-notes/issue-reports/233/repro/` — fixture used throughout.
