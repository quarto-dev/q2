# website.llms-txt: llms.txt + per-page markdown companions (bd-llms-txt-unimplemented-oih6z6j7)

**Date:** 2026-08-14
**Braid:** bd-llms-txt-unimplemented-oih6z6j7
**Checkout:** main @ `3ac596e0` (investigation committed in place; implementation should get its own branch/worktree)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Failing tests first: an e2e test through
  `render_document_to_file`/project render asserting `_site/llms.txt` +
  companions exist with expected content; snapshot tests for the qmd
  serialization of representative pages (crossrefs, code cells, callouts,
  footnotes); a draft-exclusion test; a warn-on-inert test for non-website
  project types.
- **Phase 1 — Config plumbing + inert-key warning.** Read
  `website.llms-txt` (boolean; mind `as_plain_text` vs `as_str` lint);
  warn when set on a non-website project (mirror the `aliases` precedent
  noted in `DocumentProfile`).
- **Phase 2 — Per-page markdown capture.** Transform/stage in Finalization
  (after `CrossrefRenderTransform`) that clones the AST, runs llms cleanup
  (unwrap section divs, drop format-only raw blocks, restore code-cell
  source), serializes via pampa's qmd writer, and hands the string out —
  likely `RenderOutput.llms_md: Option<String>` + a write in the
  render-to-file path. **Touches `RenderOutput` ⇒ full `cargo xtask
  verify` (wasm-quarto-hub-client depends on it); WASM path skips the
  write like the other native-only hooks.**
- **Phase 3 — `llms.txt` assembly.** New `write_llms_txt` in
  `website_post_render.rs`, sibling of `write_sitemap`: sections derived
  from the website sidebar/navbar structure, entries as
  `- [title](href): description` from `DocumentProfile`, drafts + 404
  excluded, absolute URLs when `site-url` set. Incremental discipline
  mirrors sitemap.
- **Phase 4 — `llms-full.txt`** (pending design question 2): concatenate
  the per-page markdown in index order with separators.
- **Phase 5 — E2E verification + docs.** Render the repro and the
  connect-docs port; inspect actual output; user-facing docs page under
  `docs/` (rendered with q2, not Q1).

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

## Open design questions for the user
2. **`llms-full.txt`.** Every major SSG now emits it alongside `llms.txt`.
   Include it in scope (cheap once per-page markdown exists), or file as a
   follow-up strand?
3. **Companion naming.** Q1 uses `<page>.llms.md` and the connect-docs
   landing page links assume it; the broader ecosystem trend is
   `<page>.md` / `<page>.html.md` next to the HTML. Keep `.llms.md` for Q1
   parity (my recommendation, given discovery.rs already excludes it), or
   adopt/also-emit the ecosystem convention?
4. **Internal links inside companions.** Should in-body links to other
   pages of the same site point at the `.html` outputs (Q1 behavior) or be
   rewritten to the `.llms.md` siblings (keeps an LLM inside the markdown
   mirror)? The index itself links `.llms.md` either way.
5. **Serialization fidelity bar.** The qmd writer was built for
   qmd-in/qmd-out round-tripping, not for post-transform ASTs full of
   `CustomNode`s and HTML raw blocks. I propose: capture after
   `CrossrefRenderTransform` (so figure/table numbers and `@ref` text are
   resolved), then an llms-cleanup pass, then snapshot-test the output on
   representative fixtures and iterate. Is "readable, semantically
   complete markdown that need not match Q1's output at all" the right
   acceptance bar?
6. **Q1 extras scope.** `llms-only`/`llms-hidden` conditional content and
   code-annotation preservation: defer to follow-up strands filed as
   `discovered-from` this one? (My recommendation — the MVP is the file
   set + organized index.)

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
