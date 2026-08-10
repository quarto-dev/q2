# Shortcodes not evaluated in website.title, page-footer, or HTML include files (bd-shortcodes-in-metadata-bp06aub8)

**Date:** 2026-08-10
**Braid:** bd-shortcodes-in-metadata-bp06aub8
**Checkout:** `main` @ `0c5d0abe` (investigation committed in place; no worktree created)
**Status:** Design aligned (all questions resolved 2026-08-10). **Awaiting explicit user go-ahead to begin implementation.**

## Triage verdict

**Ready to design.** The symptom reproduces at HEAD, the root causes are located and
understood (three distinct mechanisms, detailed below), and the codebase has clear
precedents for the shape of the fix. One material correction to the strand: its premise
that document-metadata shortcodes already work is **wrong at HEAD** — `subtitle:` with a
shortcode renders an unresolved `?env` marker — so the scope is slightly wider than
filed.

## Issue context

Filed 2026-08-10 (same day), priority 1, type bug, labels `parity`/`websites`. q2 passes
shortcodes through literally in project-level contexts that Quarto 1 evaluates:

1. `website.title` → every page's `<title>` and the navbar brand (navbar additionally
   HTML-escapes markup like `<small>`, showing it as text);
2. `website.page-footer` text regions;
3. `include-in-header` / `include-before-body` / `include-after-body` files (contents
   injected verbatim).

No warning in any of the three. Real-world hit: Posit Connect docs use
`{{< env CONNECT_VERSION >}}` in all three contexts — all 352 pages affected.

## Dependency graph

- **related**: `bd-environment-files-372u9qbs` (in_progress) — `_environment` file
  loading for the `env` shortcode. Its design (recorded in
  `claude-notes/plans/2026-08-09-environment-files-loading.md`, commit `315271cf`, not
  yet on this machine's `main`) puts a project env map on `StageContext` with no
  process-env mutation, and `EnvShortcodeHandler` will consult it. **Interaction:** any
  new expansion call sites this strand adds (metadata walk, include-stage expansion)
  must consult the same env source, and both strands touch
  `shortcode_resolve.rs` — coordinate merge order.
- No `discovered-from`, no incoming `blocks`. Origin is the connect-docs porting skein
  (`br-shortcodes-in-metadata-due080a1`), whose repro README documents Q1's behavior
  (Q1 substitutes in all three contexts, proven by its rendered output).

## What the code looks like today

All file paths in the strand are current. Reproduced at HEAD (`0c5d0abe`); local repro at
`claude-notes/plans/shortcodes-website-config-includes-investigation/repro/` (extends the
external fixture with a `website.navbar` so the navbar-brand path actually fires);
observed output in `../observations.md`. Summary:

```
<title>Home – My Site <small>Version {{< env REPRO_VERSION >}}</small></title>
<a class="navbar-brand" href="./">My Site &lt;small&gt;Version {{&lt; env REPRO_VERSION &gt;}}&lt;/small&gt;</a>
You are viewing version <strong>{{< env REPRO_VERSION >}}</strong>.        <!-- include -->
<p>Body-text shortcode (works in q2): version is 2026.08.0.</p>            <!-- body: OK -->
<div class="nav-footer-center">My Product {{&lt; env REPRO_VERSION &gt;}}  <!-- footer -->
<p class="subtitle lead">Subtitle version <span class="quarto-unresolved-shortcode">?env</span></p>
```

The last line is the scope correction: doc-frontmatter `subtitle:` **is** markdown-parsed
(so a `Inline::Shortcode` node exists) but is never resolved.

### Root causes — three distinct mechanisms

**(A) Project config strings are never markdown-parsed.** `_quarto.yml` is loaded with
`InterpretationContext::ProjectConfig` (`crates/quarto-core/src/project/mod.rs:191`,
`:1317`); in that context untagged strings stay literal
`ConfigValueKind::Scalar` (`crates/pampa/src/pandoc/meta.rs:288-305`). Document
frontmatter uses `InterpretationContext::DocumentMetadata`, where strings *are*
markdown-parsed (→ `parse_yaml_string_as_markdown_to_config`, `meta.rs:26-100`), and
shortcode syntax survives qmd parsing as `Inline::Shortcode`
(`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:1222-1225`). So
`website.title` / `page-footer.center` from `_quarto.yml` contain no shortcode node at
all — there is nothing for any resolver to find. This same fact explains the escaped
`<small>`: scalars are emitted through `escape_html`, whereas `PandocInlines` would carry
the markup as `RawInline`. (An author can work around both today with `!md`-tagged
strings — except the shortcode inside would still hit mechanism B.)

**(B) `ShortcodeResolveTransform` never walks metadata.** The transform
(`crates/quarto-core/src/transforms/shortcode_resolve.rs`, phase `Normalization`,
registered at `pipeline.rs:1198`) calls `resolve_blocks(&mut ast.blocks, …)` only
(`shortcode_resolve.rs:1217-1224`); `ast.meta` is read-only input. Hence the `?env`
subtitle. Downstream, `MetadataNormalizeTransform::inlines_to_plain_text` **silently
drops** `Inline::Shortcode` when deriving `pagetitle`
(`metadata_normalize.rs:184`), and `quarto-navigation`'s `push_inline`
(`crates/quarto-navigation/src/render_html.rs:730-818`) has no `Shortcode` arm (falls
into escaped-plain-text catch-all).

**(C) Include files are opaque text.** `IncludeResolveStage`
(`crates/quarto-core/src/stage/stages/include_resolve.rs:88-168`, registered
`pipeline.rs:303`) reads include files via `read_include_file` (`:473-512`) and pushes
the raw string through to `rendered.includes.*`. No parsing, no expansion. (Its
`inlines_to_html_literal` helper also silently drops `Shortcode` inlines from
`{text: …}` smart-includes, `:386`.) This stage runs before all AST transforms, but
after `MetadataMergeStage`, so merged metadata *is* available in-stage.

### Secondary defects found along the way

- Navbar brand fallback flattens and escapes: `brand_title_fallback`
  (`navbar_render.rs:173-181`) does `as_plain_text()` and `render_html.rs:330`
  unconditionally `escape_html`s it — the odd one out vs. the sidebar, which passes the
  `website.title` `ConfigValue` verbatim so `render_text` walks inlines
  (`sidebar_generate.rs:104-108`, the stated precedent).
- `ListingRenderTransform` re-parses markdown at `Navigation` phase
  (`listing_render.rs:209`) — anything it produces is past `Normalization`, so
  shortcodes there are also never resolved. Same family, out of this strand's scope;
  filed as a discovered strand.

## Quarto 1 ground truth (verified 2026-08-10)

Studied `external-sources/quarto-cli` and rendered the same fixture with the system Q1
dev binary (`quarto 99.9.9`, fixture + outputs recorded in
`shortcodes-website-config-includes-investigation/observations.md`). Q1 output:

```
<title>Home – My Site Version 2026.08.0</title>                        <!-- tags STRIPPED -->
<span class="navbar-title">My Site <small>Version 2026.08.0</small></span>  <!-- raw HTML kept -->
You are viewing version <strong>2026.08.0</strong>. **md-test** `code-test` <!-- substituted, NOT markdown-parsed -->
<p class="subtitle lead">Subtitle version 2026.08.0</p>
<p>My Product 2026.08.0</p>                                             <!-- markdown-parsed -->
```

Q1's five mechanisms:

1. **Reader-level text preprocessing.** `readqmd.lua` runs the lpeg shortcode parser
   over the *entire* source text — frontmatter included — turning `{{< … >}}` into
   `quarto-shortcode__` spans, which become custom `Shortcode` AST nodes wherever
   markdown is parsed (including metadata values).
2. **The shortcode filter walks metadata.** `pre-shortcodes-filter`
   (`customnodes/shortcodes.lua`) traverses with `jog`, whose `Pandoc` case does
   `element.meta = jogger(element.meta)` (`modules/jog.lua:173`) — so `Shortcode`
   nodes in **any** metadata value are resolved, no blessed list. This is why doc
   `subtitle:` works in Q1.
3. **Text-level substitution in non-markdown contexts.** The same filter applies
   `apply_code_shortcode` (an lpeg text scanner) to `Code`/`CodeBlock`/`RawBlock`/
   `RawInline`/`Math` text, element attributes, image `src`, and link targets. It
   dispatches through the same `handlerForShortcode` registry (env/meta/var **and**
   extension Lua shortcodes), stringifies results, and leaves unresolved names as
   literal text, silently.
4. **Include files become metadata raw blocks, not pandoc `--include-*` args.**
   `quarto-init/includes.lua` (`read_includes`) reads each include file into meta
   (`header-includes` / `include-before` / `include-after`) as raw content; the
   template emits them. Because of (2)+(3), the shortcode filter's meta walk performs
   *text-level* substitution inside those raw blocks — substituted but never
   markdown-parsed (verified: `**md-test**` stays literal).
5. **Website config strings use the "markdown pipeline" envelope**
   (`core/markdown-pipeline.ts`): navbar title (fallback: website title), sidebar
   title/footer, next/prev text, announcements, margin header/footer
   (`website-navigation-md.ts`), plus the computed page title and og:/twitter: titles
   (`website-meta.ts`) are injected after the body as hidden spans/divs
   (`kMarkdownAfterBody`), processed by the full filter chain (so shortcodes resolve
   and markdown renders), then extracted from the rendered DOM by postprocessors and
   grafted into their slots. `<title>` specifically takes the rendered element's
   `innerText` — which is why `<small>` is stripped there but kept in the navbar.

**Mapping to q2** (per user direction, confirmed by the study): q2 does not need the
envelope — it exists because Q1 can only run filters over the document body. q2 has
native `Inline::Shortcode` nodes in parsed metadata, so walking `ast.meta` in
`ShortcodeResolveTransform` replicates mechanisms 1+2 directly, and q2's no-DOM-
postprocessor rule stays intact. Website config strings need to become `PandocInlines`
(mechanism 5's q2 equivalent is "parse the presentation strings as markdown and let
the meta walk + shape-preserving renderers do the rest"). Includes need a text-level
expander (mechanism 3/4's equivalent) at `IncludeResolveStage`. Q1's text-level
contexts beyond includes (code, attributes, image src, link targets) are a separate
gap q2 also has (verified with a probe fixture), filed as **bd-fz6gwfq0**.

## Q1 markdown-processed config entries (survey, 2026-08-10)

Full enumeration of what Q1 renders through the envelope (from
`website-navigation-md.ts`, `website-meta.ts`, `website-about.ts`,
`website-listing.ts`; books fold `book.*` into `website.*` in `book-config.ts`, so
they inherit all of it — no separate book pipeline):

**Navigation** (`quarto-navigation-envelope`): `website.sidebar.title` and
`website.navbar.title` (both falling back to `website.title`, then page `title`) —
inline, markup kept; `website.sidebar.contents[].text` (all levels; also feeds
next/prev page text and breadcrumbs); `website.navbar.left/right[].text` incl. nested
menus; navbar/tools/about `href`s (rendered then **innerText**-extracted so
`{{< var >}}` works in hrefs); `website.page-footer` regions — bare-string and
per-region strings as **blocks**, nav-item `text` too; `website.sidebar.header/footer`,
`website.margin-header/footer` (merged with page frontmatter), `website.body-header/footer`
— blocks, entries may be *file paths* (`.md`/`.html` read from disk);
`website.announcement`(.content) — block.

**Meta** (`quarto-meta-markdown`): computed page title → `head > title`
(**innerText** — markup deliberately dropped); og:/twitter: titles and descriptions
(with fallback chains through page `description`/`abstract`/`subtitle` and
`website.description`) → meta tag `content` (**innerText**); `website.title` →
`og:site_name` + written back into metadata for feeds.

**About** (`quarto-about-pipeline`): `about.links[].text` (inline) and `.href`
(innerText).

**Listing** (`quarto-listing-pipeline`): the entire EJS-rendered listing markdown
(block) — relevant to bd-1fue1ly5, not this strand.

Two Q1 quirks we do *not* need to copy: the envelope's brittle re-matching of targets
by rendered-text comparison (flagged by a TODO in Q1 itself), and the dummy
`$e=mC^2$` math-dependency forcing. q2's shape-preserving `ConfigValue` approach
avoids both.

## Design decisions (2026-08-10, aligned with user)

1. **Metadata walk: all metadata values** (Q1 parity; also groundwork for processing
   markdown entries in metadata beyond shortcodes).
2. **Config keys to markdown-parse: registry-driven.** A single declarative table in
   `quarto-core` mapping config key paths → parse flavor (inline vs block), e.g.
   `website.title` → inline, `website.page-footer.center` → block. Initial entries
   (this strand): `website.title`, `website.navbar.title`, `website.sidebar.title`,
   `website.page-footer` (bare-string + `left/center/right` string forms),
   `website.announcement`(.content). The survey above is the growth path (item
   `text:` fields, hrefs, margin/body header/footer, about links…) — the table
   supports array-wildcard paths from day one so those are one-line additions;
   eventual user-extensibility (e.g. via schema annotation) is a design note, not in
   scope. Parse happens **at transform time over merged metadata** (a small
   `Normalization` transform ordered before `ShortcodeResolveTransform`), not at
   project-config load: one site, provenance-independent (project config, profiles,
   frontmatter overrides all pass through), no `InterpretationContext` change, and
   `ConfigValue`'s `SourceInfo` gives the re-parse correct spans (precedent:
   `listing_render.rs`). Downstream consumers keep using `as_plain_text()` /
   `render_text`, which already handle both shapes.
3. **`<title>`: substitute, then flatten to plain text.** Verified against the real
   Connect docs Q1 render (`docs-quarto-1/_site/index.html:10`):
   `<title>Posit Connect Documentation – Connect Documentation Version development</title>`
   — shortcode substituted, `<small>` tags stripped, inner text kept. q2's
   `inlines_to_plain_text` already drops `RawInline` and keeps `Str`/`Space`, so
   after shortcode resolution the flattening is Q1-equivalent; the only change is
   stringifying resolved shortcode output instead of dropping the node.
4. **Include files: full handler set with lazy Lua.** Pre-scan the include text with
   the text-level shortcode parser; resolve builtins (`env`/`meta`/`var`) directly;
   instantiate the Lua engine **only if** a non-builtin shortcode name is present
   *and* Lua shortcode paths are configured. No engine cost on the common path.
5. **Unresolved shortcodes: body-text policy everywhere.** Visible marker + Q-16-5
   diagnostic in metadata, navbar, footer, `<title>` flattening, and include files
   alike. (Deliberately noisier than Q1, which is silent in text contexts.)
6. **Env-files strand (`bd-environment-files-372u9qbs`): parallel, expect rebase.**
   Tests here must be independent of environment-file changes: use `{{< meta >}}`
   (fully self-contained) as the primary shortcode in tests; the few env-specific
   tests set process env explicitly (process env wins over `_environment` files in
   both designs, so they stay valid after the other strand lands).

## Phases

### Phase 0 — Test plan (TDD; write first, verify each fails at HEAD)

- [x] Project-render integration test (real render path, repro-shaped fixture with
      navbar): asserts substituted `<title>` (plain text, tags stripped), navbar
      brand (markup un-escaped, shortcode substituted), page-footer region, sidebar
      title, include file content, and doc `subtitle`/`title` — primary shortcode
      `{{< meta >}}`, one env case with explicitly set process env.
      → `crates/quarto-core/tests/integration/shortcode_config_pipeline.rs`;
      12/13 fail at branch point (verified 2026-08-10), the 13th is the
      plain-strings no-regression guard which passes by design.
- [x] Unresolved-shortcode tests: visible marker in website.title contexts and in
      include files (Q-16-5 diagnostic assertions live at unit level in the
      transform's test module, added with each phase's implementation).
- [ ] Include lazy-Lua test: include with extension shortcode resolves via Lua when
      configured (added in Phase 3 with the text expander; with the revised
      architecture — expansion inside `ShortcodeResolveTransform` — laziness is
      inherited from the transform's existing engine gating).
- [x] Regression: `{text: …}` smart-includes with shortcodes (currently silently
      dropped), escaped shortcodes in include files, plain scalar config strings
      unaffected.

**Architecture revision discovered during Phase 0 scouting:** include text is already
in metadata (`rendered.includes.*`, written by `IncludeResolveStage` which runs
*before* the transform pipeline) — so Phase 3's text-level expansion belongs in
`ShortcodeResolveTransform` alongside the meta walk, reusing its handler registry,
its (already conditionally-created) Lua engine, and its diagnostics channel. No
second engine site, no stage-level Lua. Engine-contributed includes appended later
by `ApplyTemplateStage` are engine output and deliberately not expanded (Q1's
`cell-code` opt-out analog).

### Phase 1 — Metadata shortcode resolution (mechanism B) ✅

- [x] `resolve_config_value` walker over `ConfigValue` trees (PandocInlines →
      `resolve_inlines`, PandocBlocks → `resolve_blocks`, recursing maps/arrays;
      scalars untouched).
- [x] `ShortcodeResolveTransform::transform` walks `ast.meta` (all values) using a
      pre-walk snapshot as handler context — meta walk runs BEFORE the blocks walk
      so body-level `{{< meta k >}}` sees resolved values. Runs before
      `MetadataNormalizeTransform`, so `pagetitle` derivation sees resolved text.
- [x] Silent-drop concern in `inlines_to_plain_text` resolved without changing the
      helper: after the meta walk, unresolved shortcodes are already replaced by
      `?key` marker Str nodes (`make_error_inline`) + Q-16-5 diagnostics, so the
      flattener never sees a `Shortcode` node from walked metadata.
- Result: `doc_subtitle` and `doc_title` (h1 + pagetitle) tests pass; full
  workspace run 11218 passed / 10 failed — the 10 are exactly the still-open
  Phase 2/3 tests. No regressions from walking all metadata.

### Phase 2 — Website presentation strings (mechanism A) ✅

- [x] `ConfigMarkdownTransform` (`transforms/config_markdown.rs`), `Normalization`,
      registered immediately before `ShortcodeResolveTransform`; applies the
      `MARKDOWN_CONFIG_PATHS` registry (path patterns with `*` array wildcard) to
      merged metadata via a new public pampa entry point
      (`pampa::pandoc::meta::parse_config_string_as_markdown` — untagged-value
      semantics, Q-1-20 warning on parse failure). Only `Scalar(String)` values are
      re-parsed; the parse auto-detects inline (single paragraph) vs block, which
      preserves q2's current footer DOM shape — the plan's per-entry "flavor" field
      proved unnecessary. Documented limitation: `!str` in project config is
      indistinguishable post-load, so it can't opt a blessed key out.
- [x] Seed registry: `website.title`, `navbar.title` + `sidebar.title` +
      `page-footer` (bare + left/center/right string form), each in both top-level
      and `website.`-scoped forms where applicable.
- [x] `brand_title_fallback` passes the `ConfigValue` through to `render_text`
      (with a `is_renderable_title` gate preserving the old `title: false`
      behavior); `navbar_to_html` fallback param is now `Option<&ConfigValue>`.
- [x] `push_inline`: `Shortcode` arm renders the `<strong>?name</strong>` marker
      instead of silently dropping (defense-in-depth — the meta walk normally
      replaces unresolved shortcodes before rendering).
- [x] Consumer audit: all blessed-key readers use `as_plain_text()` (handles
      PandocInlines, drops RawInline — which is exactly Q1's `<title>` innerText
      semantics) or preserve the ConfigValue; no `as_str()` hazards found.
- Result: all five config-string tests green (title, navbar brand, sidebar,
  footer, unresolved marker). Workspace: 11228 passed / 5 failed — exactly the
  Phase-3 include tests. No regressions.

### Phase 3 — Include files (mechanism C)

- [ ] Text-level shortcode scanner/expander (parse `{{< … >}}` spans in arbitrary
      text, dispatch handlers, stringify results) — shared building block, also the
      natural basis for bd-fz6gwfq0 later.
- [ ] Apply it to include-file contents in `IncludeResolveStage` (merged metadata is
      available there), with lazy Lua per decision 4 and Q-16-5 + marker per
      decision 5.
- [ ] Fix silent `Shortcode` drop in `inlines_to_html_literal` (`{text: …}`
      smart-includes).

### Phase 4 — End-to-end verification + docs

- [ ] `cargo run --bin q2 -- render` on the investigation repro: inspect all five
      contexts in the output; record invocation + snippets here.
- [ ] Full workspace verification (`cargo build`, `cargo nextest run --workspace`,
      `cargo xtask verify`).
- [ ] docs/ update if user-facing behavior warrants it (shortcodes-in-config
      documentation).
- [ ] Braid: close strand; re-check discovered strands (bd-1fue1ly5, bd-fz6gwfq0)
      against the shared expander.

## Design questions — all resolved

All six original design questions are settled; the outcomes are consolidated in
**Design decisions** above. Resolution history: metadata walk scope and `<title>`
semantics were answered by the Q1 ground-truth study; the registry approach, lazy-Lua
compromise, body-text unresolved policy, and parallel-with-rebase coordination were
decided by the user on 2026-08-10 (session discussion). Implementation awaits
explicit user go-ahead.

## Risks / tradeoffs (draft)

- **Config shape change fallout (Phase 2).** Turning blessed keys into
  `PandocInlines` breaks any consumer doing `as_str()` on them — exactly the failure
  class the `metadata-as-str` lint exists for. Needs a consumer audit of the blessed
  keys before flipping.
- **Merge conflict surface** with the in-flight env-files strand (design question 6).
- **Performance:** metadata walking is cheap; include-file scanning is per-file
  per-render — negligible. Lua engine instantiation in `IncludeResolveStage` (if
  question 4 says Lua-yes) is the only heavy option.
- **Silent-drop sites** (`inlines_to_plain_text`, `push_inline`,
  `inlines_to_html_literal`) are fixed incidentally by the phases above, but each is a
  behavior change that needs its own test.
- The `Navigation`-phase markdown-reparse gap (listings) is deliberately out of scope
  here — tracked separately (discovered strand, see below).

## Discovered work filed

- Listing/`Navigation`-phase markdown re-parse gap: strings parsed into AST after
  `Normalization` never get shortcode resolution (`listing_render.rs:209`) — filed as
  **bd-1fue1ly5** (discovered-from this strand).
- Text-context shortcode substitution gap (code blocks, element attributes, image
  src, link targets — Q1 substitutes all of these at text level; q2 leaves them
  literal, verified by probe) — filed as **bd-fz6gwfq0** (discovered-from this
  strand).
