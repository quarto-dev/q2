# Shortcodes not evaluated in website.title, page-footer, or HTML include files (bd-shortcodes-in-metadata-bp06aub8)

**Date:** 2026-08-10
**Braid:** bd-shortcodes-in-metadata-bp06aub8
**Checkout:** `main` @ `0c5d0abe` (investigation committed in place; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Integration tests driving the real render path
  (project-render fixture like the repro; assert substituted `<title>`, navbar brand
  (with un-escaped markup), footer, include file, and doc `subtitle`; assert Q-16-5
  diagnostics for unresolvable shortcodes in each context). Verify each fails at HEAD.
- **Phase 1 — Metadata shortcode resolution (mechanism B).** Extend
  `ShortcodeResolveTransform` to walk `ast.meta`'s `PandocInlines`/`PandocBlocks`
  values (it already runs before `MetadataNormalizeTransform`, so `pagetitle`
  derivation then sees resolved text). Likely requires exposing the private
  `resolve_inlines` walker over `ConfigValue` trees. Fixes subtitle + doc-title.
- **Phase 2 — Website presentation strings (mechanism A).** Markdown-parse a blessed
  set of website presentation config values (`website.title`, `navbar.title`,
  `page-footer` text regions — exact list is design question 2) into `PandocInlines`
  so Phase 1 resolves them; fix `brand_title_fallback` to pass the `ConfigValue`
  through to `render_text` (sidebar precedent — also fixes the `<small>` escaping);
  give `push_inline` a `Shortcode` arm (visible marker + diagnostic instead of silent
  flatten).
- **Phase 3 — Include files (mechanism C).** Text-level shortcode expansion of include
  file contents in `IncludeResolveStage` (Q1 substitutes textually; include files are
  HTML, not qmd, so markdown-parsing them is wrong). Needs a small text-level
  shortcode scanner/parser + handler dispatch; emits Q-16-5 on unresolved. Also fix the
  silent `Shortcode` drop in `inlines_to_html_literal`.
- **Phase 4 — End-to-end verification + docs.** Re-render the repro and the Connect
  docs; record invocation + output snippets; docs/ note if user-facing behavior needs
  documenting.

## Design questions answered by the Q1 study (2026-08-10)

1. **Metadata walk scope: ALL metadata values.** Q1's shortcode filter traverses the
   whole meta tree (jog walks `element.meta`), no blessed list. Also matches the
   user's stated direction ("processing the contents of markdown entries in metadata
   is something we want to do not only for shortcodes"). → Phase 1 walks every
   `PandocInlines`/`PandocBlocks` value in `ast.meta`.
2. **`<title>` element: substitute, then strip markup.** Q1 emits
   `<title>Home – My Site Version 2026.08.0</title>` (innerText of the rendered
   title). So q2's plain-text flattening for `pagetitle` is the right shape — it must
   *stringify resolved shortcode output* instead of dropping the node, and continue
   dropping raw-HTML tags while keeping their text content (Q1-equivalent: `<small>`'s
   contents survive, the tags don't).

## Open design questions for the user

1. **Which config keys get markdown-parsed (Phase 2).** Wholesale switching
   `ProjectConfig` interpretation to markdown-parse strings is off the table (paths,
   hrefs, ids must stay literal). Q1's envelope set is: navbar title (fallback:
   website title), sidebar title + sidebar footer, `page-footer` regions, next/prev
   page text, announcements, margin header/footer, page title, og:/twitter: titles.
   Minimal for this strand: `website.title`, `website.navbar.title`, `page-footer`
   text regions, sidebar title. Bless just these now and file follow-ups for the
   rest? And where should the parse happen — at project-config load (shape change
   visible to all consumers; needs an `as_str()` audit of the blessed keys) or at the
   consumption sites (localized, repeated parse)?
2. **Include-file handler scope (Phase 3).** Q1's text-level path dispatches the FULL
   handler registry (env/meta/var + extension Lua shortcodes), stringifies results,
   and leaves unresolved names as literal text. Builtins-only covers the Connect
   docs; full parity means Lua availability at `IncludeResolveStage` (heavier). Is
   builtins-first acceptable, with a follow-up strand for Lua-in-text-contexts?
3. **Unresolved-shortcode behavior in the new contexts.** Q1 leaves unknown
   shortcodes silently literal in text contexts. q2's body policy is a visible `?env`
   marker + Q-16-5 warning. Proposal: marker + warning in metadata/navbar/footer
   (markdown contexts), literal passthrough + warning in include files (text context
   — injecting marker markup into arbitrary HTML is risky). q2 would be deliberately
   noisier than Q1 (warnings everywhere). OK?
4. **Coordination with `bd-environment-files-372u9qbs`.** Both strands modify
   `shortcode_resolve.rs`; that strand's env map lives on `StageContext`. Preferred
   order — land env-files first and build on its env plumbing, or proceed in parallel
   and let whoever lands second rebase?

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
