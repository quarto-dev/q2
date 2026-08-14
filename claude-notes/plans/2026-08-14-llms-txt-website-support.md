# website.llms-txt: llms.txt + per-page markdown companions (bd-llms-txt-unimplemented-oih6z6j7)

**Date:** 2026-08-14
**Braid:** bd-llms-txt-unimplemented-oih6z6j7
**Checkout:** main @ `3ac596e0` (investigation committed in place; implementation should get its own branch/worktree)
**Status:** Design aligned 2026-08-14 (all six questions resolved — see Resolved design decisions). Ready to implement on a dedicated branch/worktree.

## Triage verdict

**Ready to design.** The symptom reproduces at HEAD, all the architectural
slots the feature needs already exist (a website post-render hook surface,
`DocumentProfile`/`ProjectIndex` threading, and pampa's qmd writer), and the
user has explicitly asked to use this as an opportunity to *improve on* Q1's
output rather than port it byte-for-byte. What remains is a design
conversation about output shape (index organization, `llms-full.txt`,
naming/link conventions) — the questions are listed below.

## Issue context

`website: {llms-txt: true}` in `_quarto.yml` is accepted and silently
dropped by q2. Q1 emits `_site/llms.txt` (a markdown index) plus one
`<page>.llms.md` companion per page (main content scraped from the rendered
DOM and converted back to markdown via `pandoc -f html -t gfm-raw_html`
with an `llms.lua` filter).

- Type: feature, priority 3, label `website`. Filed 2026-08-14 (today) by
  Carlos — assumptions are fresh, nothing stale.
- Real-world hit: the Posit Connect docs port sets `llms-txt: true`; Q1
  ships `llms.txt` + 348 companions, the q2 port ships none, and the docs
  landing page links `llms.txt` → the ported site 404s on its own front
  page.
- Origin strand in the connect-docs porting skein:
  `br-llms-txt-unimplemented-qmgjbb46`.

**User direction for this session (from the /investigate-beads invocation):**

1. Leverage q2's qmd AST writer instead of Q1's HTML-scraping approach —
   Q1's output is "only adequate."
2. The perennial Q1 user request: the top-level `llms.txt` should be a
   **well-organized index** into the per-page files, not a bare list of
   links. Survey what other SSGs do.

## Dependency graph

**Empty.** `braid dep tree` / `dep list` show no edges in the q2 skein. The
pressure comes from outside the graph: the connect-docs porting effort
(origin strand above) and the strand description itself.

## What the code looks like today

Confirmed at HEAD (3ac596e0), end-to-end:

```
$ cargo run --bin q2 -- render claude-notes/plans/llms-txt-website-support-investigation/repro
Rendered 2 of 2 files to .../repro/_site
$ find _site -name llms.txt -o -name '*.llms.md'
(empty)
```

Repro copied to `claude-notes/plans/llms-txt-website-support-investigation/repro/`
(from the connect-docs skein's repro; `_site/` not committed).

### Existing q2 anchor points (all verified in-tree)

- **Only the input side of the convention exists.** `crates/quarto-core/src/project/discovery.rs`
  excludes `*.llms.md` as agent-instruction files (so a rendered site's
  companions are never treated as project inputs — convenient, already
  done). `crates/quarto-core/src/transforms/draft_alert.rs:92` mentions
  llms.txt as "on q2's roadmap" (drafts must be excluded from it).
- **Post-render hook surface:** `WebsiteProjectType::post_render`
  (`crates/quarto-core/src/project/orchestrator.rs:370`) already runs
  `copy_favicon` / `write_sitemap` / `write_robots_txt` /
  `write_alias_redirects` (all in
  `crates/quarto-core/src/project/website_post_render.rs`), native-only,
  each short-circuiting when its config key is absent. `llms.txt` assembly
  is a sibling of `write_sitemap`.
- **`post_render` receives `&ProjectIndex`**, which is a wrapper around
  `Vec<DocumentProfile>` (`crates/quarto-core/src/project/index.rs`).
  `DocumentProfile` already carries `title`, `subtitle`, `description`,
  `categories`, `keywords`, `draft`, `output_href` — everything an
  organized index needs, **including on incremental renders** (profiles are
  cached). No new plumbing needed for the index side.
- **Per-page markdown needs a pipeline capture.** `RenderOutput`
  (`crates/quarto-core/src/pipeline.rs:162`) carries only
  `html + diagnostics + source_context`; the final AST is not retained on
  the HTML path. pampa has a full qmd writer
  (`crates/pampa/src/writers/qmd.rs`, `write<T: Write>` +
  `write_metadata`).
- **The body AST stays clean of nav chrome.** Navigation-phase transforms
  populate `ast.meta.navigation.*` / `rendered.*` for template insertion —
  they do *not* inject sidebar/navbar blocks into the body. So an
  AST-serialization approach doesn't need Q1's DOM-stripping step at all.
  (This is the architectural payoff of "no DOM postprocessor".)
- **Capture-point subtlety:** `CrossrefRenderTransform` runs in the
  **Finalization** phase (after Navigation), so resolved crossref numbers
  only become writer-visible inlines late. Also `SectionizeTransform`
  (Normalization) wraps headers in section Divs for HTML semantics, which
  the qmd writer would emit as `:::` fenced divs. A capture in Finalization
  after `CrossrefRenderTransform` + a small llms-specific cleanup pass
  (unwrap section divs, drop HTML-only raw blocks) looks like the right
  shape. See design question 5.

### Q1 reference behavior (external-sources/quarto-cli/src/project/types/website/website-llms.ts)

- Per-page: HTML finalizer clones the rendered DOM, strips nav/sidebar/
  scripts, restores annotated-code original text, then shells out to
  `pandoc -f html -t gfm-raw_html --lua-filter llms.lua --wrap=none` to
  write `<output>.llms.md`. Skips drafts (unless draft mode makes them
  visible).
- Site-level `updateLlmsTxt`: `# {site title}`, `> {site description}`,
  `## Pages`, then a **bare** `- [title](path)` list of every `.llms.md`
  (404 page and drafts excluded; absolute URLs when `site-url` is set,
  relative otherwise). Incremental render + existing file ⇒ skip
  regeneration (sitemap-like discipline).
- Extra Q1 machinery we should scope explicitly: `llms-only` /
  `llms-hidden` conditional-content divs, and code-annotation
  preservation via `data-llms-code-original`.

## The landscape: llms.txt in other SSGs

The [llms.txt spec](https://llmstxt.org) (Jeremy Howard / Answer.AI, 2024)
prescribes exactly the structure Q1 half-implements:

```markdown
# Site title
> One-paragraph summary
Optional freeform context paragraphs
## Section name
- [Page title](url): one-line description
## Optional
- [Deep reference](url): skippable when context is tight
```

Two spec details matter for us: **each link may carry a `: description`
annotation** (Q1 omits these — biggest cheap win), and the literal
`## Optional` section has defined semantics (safe to skip for shorter
context).

What the ecosystem converged on (state of my pre-training knowledge,
worth a quick re-verification during design):

- **Mintlify**: auto-generates `llms.txt` organized by the nav hierarchy
  with per-page descriptions, plus **`llms-full.txt`** (entire docs
  concatenated into one file), plus a raw-markdown variant of every page
  reachable by URL convention.
- **docusaurus-plugin-llms** and **vitepress-plugin-llms**: `llms.txt`
  sectioned by the sidebar structure with descriptions from front matter,
  plus `llms-full.txt`, plus per-page `.md` files.
- **Astro Starlight (`starlight-llms-txt`)**: `llms.txt` +
  `llms-full.txt` + `llms-small.txt` (aggressively stripped variant).
- **Fumadocs**: `llms.txt` + `llms-full.txt` route handlers.

So the de facto standard bundle is: **organized, description-annotated
`llms.txt`** (sections mirroring site navigation) + **`llms-full.txt`**
+ per-page markdown. Q1 ships only the third piece well.

## Phases and work items

Design is settled (see Resolved design decisions). TDD throughout: each
phase's tests are written and observed failing before implementation.

### Phase 0 — Test plan (failing tests first)

- [x] E2E project-render test: website with `llms-txt: true` produces
      `_site/llms.txt`, per-page `<page>.md` companions, and
      `_site/llms-full.txt`; content assertions on index structure
      (H2 sections, `- [title](href): description` entries)
      — `crates/quarto-core/tests/integration/llms_txt.rs`, 15 tests
      written 2026-08-14, observed failing as expected (13 red: missing
      artifacts / collision render succeeded; 2 absent-by-default guards
      trivially green). Collision code reserved: **Q-5-28**.
- [x] Snapshot tests for qmd serialization of representative pages:
      crossrefs (resolved numbers), callouts, footnotes, code cells,
      section-div unwrapping — `llms_companion_rich_content_snapshot`
      (insta snapshot reviewed + accepted 2026-08-14). Discovered
      bd-4vbd3b7g while reviewing: `prefix_caption` misses
      Plain-block captions, so table-caption floats lose their
      "Table N:" prefix in HTML *and* markdown (pre-existing, filed
      discovered-from).
- [x] Draft-exclusion test (draft page: no companion, absent from index)
- [x] 404-page exclusion test
- [x] Collision test: resource-copied `<page>.md` at a companion path
      fails the render with Q-5-28
- [x] User-provided `llms.txt` resource collision test
- [x] Warn-on-inert test: `llms-txt: true` on a non-website project warns
- [x] Incremental-render test: `llms.txt` regenerated from cached
      profiles; skipped pages' companions persist and llms-full.txt
      covers them via on-disk read-back
      (`llms_incremental_render_covers_skipped_pages`)
- [x] Multi-sidebar + straggler test: pages in no sidebar land in
      "Other"; home page pinned first when uncovered; flat site (no nav)
      uses a single `## Pages` section instead of "Other"
- [x] Conditional-content test: `when-format="llms"` /
      `unless-format="llms"` honored in companions and HTML

### Phase 1 — Config plumbing + inert-key warning

- [x] Read `website.llms-txt` boolean (`website_llms_txt_enabled`,
      `website_description` added alongside; unit-tested)
- [x] Warn when set on non-website project types (default
      `ProjectType::post_render`, next to `warn_aliases_ignored`)

### Phase 2 — Per-page markdown capture

- [x] Finalization-phase capture after `CrossrefRenderTransform`:
      `LlmsCaptureTransform` (`transforms/llms.rs`), registered at the
      tail of Finalization. Cleanup unwraps section divs, float/figure
      chrome (keeping one `::: {#id}` anchor wrapper per crossref
      float), code-copy scaffolds, anonymous wrappers; reconstructs
      callouts back to `::: {.callout-note}` form; drops raw HTML;
      strips `quarto-*`/`data-*`/`aria-*` presentation attrs;
      simplifies footnote plumbing; synthesizes the `# title` header
      (the HTML h1 lives in the template, not the AST)
- [x] Conditional content: four-quadrant evaluation *inside*
      `ConditionalContentTransform` (not a re-run) — with the llms view
      active it evaluates both views and tags one-view-only content
      with `.quarto-llms-{omit,keep}` markers; `LlmsCaptureTransform`
      is the sole marker consumer and resolves them for both views.
      llms-view format check = literal `llms` token OR anything the
      html target matches (the companion mirrors the html page, so
      `when-format="html"` content stays in it). Known caveat
      (shared with Q1): headings inside llms-only divs can surface in
      the html TOC; floats inside are unnumbered — documented in the
      module header. (bd-stbdlesy)
- [x] Rewrite same-site internal links to `.md` siblings (decision 4),
      draft/404/external/non-page targets untouched, fragments kept;
      unit-tested (`retarget_rewrites_eligible_links_only`)
- [x] Thread the string out — **better than planned**: path-less
      Project-scoped artifacts (`llms-md/<href>` keys) ride the
      existing artifact channel to post-render; `RenderOutput` is
      untouched (no wasm API ripple), and path-less artifacts are
      skipped by every flusher by contract
- [x] Output ledger: **new `ProjectType::post_resources` hook** runs
      after the orchestrator's resource-copy pass (post_render runs
      *before* resource copies — writing there would race them), so
      companion writes are the last producer and an existence check
      against `.quarto/llms-manifest.json` (paths we generated last
      run) is sound. Collision ⇒ Q-5-28 (catalog entry +
      `docs/errors/project/Q-5-28.qmd` added; `cargo xtask lint`
      green); all collisions reported together, whole llms write
      abandoned

### Phase 3 — `llms.txt` assembly

- [x] `write_llms_artifacts` in new `project/llms_post_render.rs`
      (kept out of `website_post_render.rs` for size), called from
      `WebsiteProjectType::post_resources`; set-subtraction assembler
      per decision 1
- [x] Entry format `- [title](href): description` from
      `DocumentProfile`; absolute URLs when `site-url` set
- [x] "Other" catch-all; home page pinned when uncovered; flat sites
      (no sidebar) use a single `## Pages` section; navbar stage only
      refines sites that declare sidebars
- [x] Incremental discipline: llms.txt + llms-full.txt regenerate from
      the full (cached) profile index; skipped pages' companion content
      read back from disk when the manifest vouches for it

### Phase 4 — `llms-full.txt`

- [x] Concatenate per-page markdown in **llms.txt reading order** with
      `---\ntitle: …\nurl: …\n---` separators

### Phase 5 — E2E verification + docs

- [ ] `cargo run --bin q2 -- render` on the investigation repro; inspect
      actual `llms.txt` / companions / `llms-full.txt` (record snippet in
      plan or transcript per end-to-end policy)
- [ ] Render the connect-docs port; compare coverage vs Q1's 348
      companions
- [ ] User-facing docs page under `docs/` (rendered with q2, not Q1)
- [x] Child strands filed: bd-stbdlesy (conditional content, in PR
      scope — see Phase 2), bd-to3vh0od (code-annotation preservation,
      deferred until q2 has code annotations)

## Resolved design decisions

1. **Index organization (resolved 2026-08-14).** Set-subtraction
   algorithm, per user discussion:
   - Manifest = every `DocumentProfile` with a companion (drafts + 404
     excluded).
   - For each **declared** sidebar in config order (via the
     `sidebar_membership.rs` / resolved-entry machinery, so `auto`
     expansion is honored), walk the entry tree in author order; each
     entry resolving to a manifest page emits
     `- [title](href): description` and **removes** the page from the
     manifest (first occurrence wins across sidebars). External links and
     non-manifest targets are skipped.
   - Then navbar direct links, same emit-and-remove.
   - Remainder → a final **"Other"** section (not the spec's
     `## Optional`, whose "skippable" semantics stragglers don't
     deserve; explicit routing into `## Optional` can be a future config
     option).
   - Headings: single-sidebar site → one H2 per top-level sidebar
     *section*; multi-sidebar site → one H2 per sidebar (title/id),
     internal sections flattened. Deeper nesting always flattens (the
     spec only defines flat lists under H2s).
   - **Home page pinned:** if `index.html` is uncovered by sidebars/
     navbar, emit it first rather than letting it land in "Other".
   - Shape constraint: assembler is a pure function
     `(sidebars, navbar, manifest) → document` so a future explicit
     `website.llms-txt: {sections: …}` config can substitute its own
     structure.

2. **Companion naming (resolved 2026-08-14, with a gate).** Emit
   **`<page>.md`** (the ecosystem convention; users find `.llms.md`
   surprising) — *gated on a collision-safe write mechanism*:
   - Collision surface: rendered outputs don't collide (pages render to
     `.html`), but **resource-copied `.md` files do** (a verbatim-copied
     `about.md` in `_site/` would be overwritten by the companion for
     `about.html`), and a user-provided `llms.txt` resource collides with
     the index itself.
   - Mechanism: the llms subsystem never writes to the filesystem
     directly. It resolves every desired path against a **project output
     ledger** (rendered `output_paths` ∪ resource-copy set ∪ sibling
     post-render artifacts) through a claim-style helper that refuses
     already-claimed paths. Collision ⇒ render **error** with a new `Q-*`
     code naming both producers (docs page in the same commit, per the
     error-docs lint rule). Precedent: `write_alias_redirects` errors on
     alias collisions (Q-5-23…Q-5-26) for the same "silently wrong file
     is worse than failing" rationale. Fallback policy if the error
     proves too harsh in practice: warn + omit the page from the index.
   - `discovery.rs`'s `*.llms.md` source-side exclusion stays untouched;
     the output dir is already excluded from discovery.

3. **`llms-full.txt` (resolved 2026-08-14): in scope.** Concatenate the
   per-page markdown in index order (same order as `llms.txt`'s
   sections), with per-page separators carrying title + canonical URL.

4. **Internal links inside companions (resolved 2026-08-14): rewrite to
   the `.md` siblings.** In-body links to same-site pages point at each
   page's markdown companion, keeping an LLM inside the markdown mirror.
   Links to pages with no companion (drafts, non-manifest targets) and
   external links stay as-is.

5. **Fidelity bar (resolved 2026-08-14): readable, semantically complete
   markdown; no Q1 byte-parity.** Capture after
   `CrossrefRenderTransform`, llms-cleanup pass, snapshot-test on
   representative fixtures. Gross discrepancies handled in follow-ups as
   they appear.

6. **Q1 extras (resolved 2026-08-14): wanted in the eventual PR**, filed
   as child strands to organize the work (split across sessions if the
   work runs long):
   - *Conditional content for llms* (**bd-stbdlesy**, in scope for this
     PR per user) — q2 already has the full
     `.content-visible`/`.content-hidden` `when-format`/`unless-format`
     machinery as an AST transform
     (`crates/quarto-core/src/transforms/conditional_content.rs`), so
     Q1's `.llms-conditional-content` marker dance collapses into
     running that transform with an `llms` format target on the cloned
     AST before serialization. (Check the format-alias table accepts
     `llms`.)
   - *Code-annotation preservation* (**bd-to3vh0od**, p4) — **deferred;
     moot until q2 implements code annotations at all** (no
     code-annotation machinery exists in crates/ today; user confirmed
     deferral 2026-08-14). The child strand records the requirement; it
     activates when code annotations land.

## Open design questions for the user

None — all six resolved above. Next step is implementation, on its own
branch/worktree.

## Risks / tradeoffs (draft)

- **`RenderOutput` is a wasm-visible type.** Adding a field ripples into
  `wasm-quarto-hub-client`; plain `cargo build --workspace` will not catch
  breakage — full `cargo xtask verify` required (CLAUDE.md already warns).
- **qmd-writer fidelity is the unknown.** Post-transform ASTs may contain
  nodes the writer renders poorly (sectionize divs, HTML raw blocks,
  resolved-crossref structures). Budget for an iteration loop on snapshot
  fixtures; worst case the cleanup pass grows.
- **Incremental renders.** `llms.txt` can regenerate fully from cached
  profiles (unlike Q1, which skips regeneration entirely); per-page
  companions for skipped pages persist on disk from the prior render.
  Needs a test.
- **Pre-flight verify wrinkle (pre-existing, unrelated).** At HEAD,
  `cargo xtask verify --skip-hub-build` still runs hub-client `test:ci`,
  and one WASM smoke-all fixture fails (`markdown/heading-auto-id.qmd`
  expects `<section id="using-a-volume">` per commit `6af97135`) — the
  embedded WASM is stale because the build leg was skipped. All 11,924
  Rust tests pass. Worth confirming a full `verify` is green before
  implementation starts; possibly related territory to bd-nhn7snpg's
  smoke-all work.
