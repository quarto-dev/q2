# reveal.js: linked shared assets instead of inlined output

**Strand:** bd-jij5gge2
**Related:** bd-bea550b0 (reveal Phase 2), bd-kjrpya2d (embed-in-preview — blocked by this)
**Date:** 2026-06-10
**Status:** DRAFT — iterating on the plan. **Do not implement until the user gives the go-ahead.**

> This is a first draft meant for discussion. Open questions are called out
> inline as **[Q-n]**; design decisions we still need to make are in
> §"Open questions / decisions". Expect this document to change before any code.

## Problem

`q2` renders a reveal.js deck by **inlining every asset** into the output
`slides.html`:

- `crates/quarto-core/src/revealjs/assemble.rs::render_revealjs_document`
  builds the whole document as one string with `reset.css`, `reveal.css`, the
  theme CSS, `quarto-reveal.css`, and the full `reveal.js` pasted into
  `<style>` / `<script>` blocks. Each asset is pulled in at **compile time**
  via `include_str!` of the vendored `resources/revealjs/` copy.
- This is wired in as a **special branch** in
  `crates/quarto-core/src/stage/stages/apply_template.rs:285`
  (`None if ctx.format.identifier == Revealjs`) that returns
  `(html, Vec::new())` — i.e. it **bypasses** the `HtmlDependency` / `site_libs`
  machinery that every other HTML output uses, and contributes **no**
  `css_paths` / `script_paths`.

Result: a ~700 KB `slides.html`, and on a website with *N* presentations the
same ~700 KB of reveal core is **duplicated N times** with no sharing.

### Why this is wrong (and why the original rationale doesn't hold)

The assemble.rs module doc justifies inlining with: *"so the output is a single
self-contained file **and** `q2` stays a single binary."* Those are **two
orthogonal concerns**:

- **"`q2` stays a single binary"** is about **compile-time embedding**
  (`include_str!` / `include_dir!` bakes the bytes into the executable so it
  needs no runtime `resources/` dir). `format: html` does the *same* thing
  (`resources.rs:27` — *"Resources embedded at compile time using `include_dir!`
  that are extracted…"*).
- **"single self-contained file"** is about **render-time output emission**
  (inline vs. link). This does **not** follow from the binary point.

`format: html` is the living proof they're independent: it embeds its assets in
the binary **and** emits them as **links** — it *extracts* the embedded bytes to
a shared lib dir **once** and references them via `<link>` / `<script src>`.
From `dependency.rs`: project-scope deps *"land at
`_site/site_libs/libs/{name}/{filename}` **once, deduplicated across all pages**
that reference the same extension."* Reveal should behave the same way.

### Provenance (not introduced by the embed branch)

The inlining is **pre-existing** from the reveal epic — PR #266 (`ade34bed`) on
`main`. `git diff main...HEAD -- crates/quarto-core/src/revealjs/` on the current
embed branch is **empty**. The reveal authors explicitly deferred the fix:
assemble.rs reads *"Tier-1 scope: linked (non-inlined) assets … are later phases
of the revealjs epic."* This strand is that later phase.

### Why it's also a blocker for embed-in-preview (bd-kjrpya2d)

The preview-embed feature tried to inline the deck into the preview iframe (blob
URL / `srcdoc`). That **only works because reveal currently self-contains**.
Once reveal links to `site_libs/revealjs/…`, an inlined iframe can't resolve
those — the deck must be **served** (a VFS-backed virtual fetch / service
worker), not inlined. So fixing this strand is a prerequisite to doing
bd-kjrpya2d correctly, and it changes that approach from "inline" to "serve".
(The interim inline TS in `preview-renderer` — `assetWalker`/`RawBlock`/
`iframePostProcessor` — likely gets reverted or reworked; see **[Q-7]**.)

## How `format: html` does it today (the model to mirror)

1. **Embed in binary** — `include_dir!`/`include_str!` of `resources/…`.
2. **Register as artifacts** — `store_html_dependencies(deps, artifacts, runtime)`
   reads each stylesheet/script and stores an `Artifact` under key
   `css:{name}:{file}` / `js:{name}:{file}` with relative path
   `libs/{name}/{file}`.
3. **Emit links** — `apply_template.rs` calls
   `collect_artifact_urls(ctx, "css:"/"js:", resolver)` →
   `css_paths`/`script_paths`, which the template renders as
   `<link>` / `<script src>`. The **resolver** computes the right URL per
   context (single-doc relative, website `…/site_libs/…`, preview vfs-root).
4. **Flush bytes** —
   - website render: `flush_site_libs(store, resolver, runtime)` writes
     Project-scope artifacts to `site_libs/…` (deduped once).
   - single-doc: artifacts land under `<doc>_files/…`.
   - preview (WASM): artifacts flush to `/.quarto/project-artifacts/…` in the
     VFS (`wasm-quarto-hub-client/src/lib.rs` artifact flush).

## Target design for reveal

Make reveal participate in steps 2–4 instead of inlining:

1. **Keep** `include_str!`-embedding the reveal assets in the binary (no change
   to how they're vendored).
2. **Register them as artifacts** under a `revealjs` lib name — e.g.
   `css:revealjs:reveal.css`, `css:revealjs:reset.css`,
   `css:revealjs:theme-<name>.css`, `css:revealjs:quarto-reveal.css`,
   `js:revealjs:reveal.js`, path `libs/revealjs/<file>`. Because the bytes are
   **in-binary, not on disk**, we can't use `store_html_dependencies`
   (it `runtime.file_read`s a path); we add a small helper that registers an
   `Artifact::from_bytes(...)` directly from the `include_str!` constants.
   **[Q-1]** scope: Project (shared `site_libs`, deduped) for website renders;
   Page for single-doc — mirror how html deps pick scope.
3. **Emit `<link>` / `<script src>`** in the reveal scaffold using the
   resolver-computed URLs (the `css_paths`/`script_paths` the rest of
   apply_template already builds), instead of inline `<style>`/`<script>`.
   The `Reveal.initialize({…})` inline `<script>` stays inline (it's
   per-document config, not a shared asset) — **[Q-2]**.
4. **Flush** rides the existing paths automatically once the artifacts are
   registered with the right scope (website `flush_site_libs`, single-doc
   `<doc>_files`, preview VFS flush).

### Three contexts must all work

| Context              | Lib location                                   | URL form                         |
| -------------------- | ---------------------------------------------- | -------------------------------- |
| single-doc render    | `<doc>_files/libs/revealjs/…`                  | page-relative                    |
| website render       | `site_libs/libs/revealjs/…` (once, shared)     | page-relative to `site_libs`     |
| preview (WASM)       | `/.quarto/project-artifacts/…/libs/revealjs/…` | vfs-root; served to the iframe   |

The resolver already distinguishes these (`single_doc` / `website` / `vfs_root`
constructors in `resource_resolver.rs`); the work is making reveal *use* it.

## Decisions (resolved with user 2026-06-10)

- **[Q-1] Artifact scope (Project vs Page).** OPEN (implementation detail, not a
  product decision). Mirror the html rule — Project scope under a website
  project (shared/deduped `site_libs`), Page scope for a lone `q2 render
  deck.qmd` (`<doc>_files`). Confirm exactly how html deps pick scope in
  `store_html_dependencies` callers during Phase 1.
- **[Q-2] What stays inline.** ✅ DECIDED: `Reveal.initialize(config)` stays
  inline (per-doc config); everything vendored/shared is linked.
- **[Q-3] Theme handling.** ✅ DECIDED: single theme (`white`) is fine for now —
  full reveal theming is explicitly future work. So: link the one theme CSS as
  an artifact (`theme-white.css` or similar); the per-theme/multi-theme dedup
  story comes with the future theming phase, not here.
- **[Q-4] `embed-resources: true` opt-in.** ✅ DECIDED: **do NOT offer
  `embed-resources` at all yet.** Linked is the **only** mode. Self-contained
  single-file output is a big cross-cutting feature (Quarto 2 emits complex
  HTML/JS that doesn't embed easily) to be handled **outside** revealjs later —
  out of scope here. (Removes the old Phase 4.)
- **[Q-5] reveal.js monolith vs plugins.** ✅ DECIDED: keep one `reveal.js`
  artifact; plugins are a later phase, out of scope.
- **[Q-6] WASM/preview byte source.** ✅ CONFIRMED: the WASM render must register
  + flush these artifacts to the VFS the same way html deps do, so a deck
  previewed *directly* (not embedded) loads its libs. The embedded-iframe case
  is bd-kjrpya2d's separate served-fetch work.
- **[Q-7] Revert the inline-embedding behavior.** ✅ DECIDED: **revert any
  changed behavior relative to `main` that embeds resources into an HTML file.**
  See §"Revert scope" below — this is its own phase (Phase R), and it can/should
  happen up front since it's independent of the reveal link-emission work.
- **[Q-8] Scope boundary of this strand.** ✅ DECIDED: this strand is
  **render-side only** (link emission + flush, single-doc + website + direct
  preview), verifiable without a browser. The preview *served-fetch* iframe
  rework + embed UX stays under bd-kjrpya2d.

## Revert scope (Phase R) — the inline-embedding behavior to undo

"Embedding resources into an HTML file" relative to `main`, introduced by the
embed-in-preview work on this branch:

- **Definitely revert (embeds deck bytes into preview HTML):**
  - `ts-packages/preview-renderer/src/utils/iframePostProcessor.ts` — the
    `<iframe>` `srcdoc`-inlining (added in part 1 `bfff2d3e`, generalized in
    part 2 `f5e76537`). Restore to `main` (no iframe handling).
  - `ts-packages/preview-renderer/src/q2-preview/assetWalker.ts` — the embed
    deck → blob-URL handling (part 2). Restore to Image-only.
  - `ts-packages/preview-renderer/src/q2-preview/blocks/RawBlock.tsx` — the
    manifest src-rewrite (part 2). Restore to plain `dangerouslySetInnerHTML`.
  - `ts-packages/preview-renderer/src/q2-preview/embedIframe.ts` + its test —
    new files; delete. Plus the embed tests added to `assetWalker.test.ts`,
    `iframePostProcessor.embed.test.ts`, and `RawBlock.test.tsx`.
- **[Q-R1] Decide: Rust VFS-sync of resources-scoped `.html`** (discovery
  `resource_files` category, `HubConfig.resource_files`,
  `config::resolve_project_resource_html`, `PreviewConfig.resource_html_files`,
  `preview.rs` wiring). This does **not** embed into HTML — it makes resource
  `.html` available in the preview VFS — and the *served* approach (bd-kjrpya2d)
  will likely need exactly this. Recommendation: **keep** it (neutral, reusable
  infra), but it's the one ambiguous item; the user may prefer a clean-slate
  revert and re-add under bd-kjrpya2d. **← confirm before Phase R.**

Mechanism: targeted `git revert`/manual restore of the above files to their
`main` state (the relevant commits are `bfff2d3e` part 1 and `f5e76537` part 2);
keep the `.embed-example-iframe` *transform* (bd-z1smhvuo) and crossref Demo
blocks (bd-t3cert81) — those emit a plain `<iframe src>` reference and are the
docs feature, not resource-embedding.

## Phasing (TDD-first) — DRAFT

- [ ] **R — Revert inline-embedding behavior** (§"Revert scope"). Independent of
  the rest; do up front so the branch is back to a clean state. Restore the TS
  files to `main`; resolve [Q-R1] (Rust VFS-sync keep/revert) first. Verify the
  preview-renderer + hub-client builds/tests are green after restore.
- [ ] **0 — Tests first.** Snapshot/integration tests asserting a rendered deck
  contains `<link href="…/libs/revealjs/reveal.css">` + `<script src="…/libs/
  revealjs/reveal.js">` and **no** inlined reveal core; a website render with
  two decks writes `site_libs/libs/revealjs/reveal.js` **once**; single-doc
  writes `<doc>_files/libs/revealjs/…`.
- [ ] **1 — Register reveal assets as artifacts** (in-binary bytes →
  `Artifact::from_bytes`, correct keys/scope per [Q-1]). Helper alongside
  `store_html_dependencies`.
- [ ] **2 — Link emission.** Rework `render_revealjs_document` (or replace the
  apply_template reveal branch) to emit `<link>`/`<script src>` from the
  resolver URLs; keep `Reveal.initialize` inline. No `embed-resources` path —
  linked is the only mode.
- [ ] **3 — Flush wiring.** Confirm website `flush_site_libs` + single-doc +
  preview VFS flush all carry the new artifacts; add tests per context.
- [ ] **4 — Re-stage examples + verify** `q2 render docs/`: decks link shared
  `site_libs/revealjs/…`, one copy; deck still renders standalone. Update
  `cargo xtask stage-doc-examples` expectations (decks now ship `_files/libs`).
- [ ] **5 — Hand back to bd-kjrpya2d** for the served-iframe preview rework
  (now that decks reference `site_libs/revealjs/…`, the embed must *serve*).

## Out of scope (tracked elsewhere)

- The embed-in-preview *served* fetch / service-worker work — **bd-kjrpya2d**.
- reveal plugins, additional themes, transitions — later reveal-epic phases.
- Any change to `format: html`'s existing dep machinery (we *consume* it).
