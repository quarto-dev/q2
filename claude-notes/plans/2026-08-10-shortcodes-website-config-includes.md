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

## Open design questions for the user

1. **Metadata walk scope (Phase 1).** Resolve shortcodes in *all* metadata
   `PandocInlines` values, or only a blessed presentation set (title, subtitle,
   description, …)? All-values matches "body text behavior everywhere" but runs
   handlers on values that may never be displayed; blessed-set risks whack-a-mole. Do
   we know what Q1 actually does for arbitrary metadata keys? (I did not find a local
   quarto-cli checkout to verify against — `external-sources/` has only
   commonmark-spec.)
2. **Which config keys get markdown-parsed (Phase 2).** Wholesale switching
   `ProjectConfig` interpretation to markdown-parse strings is off the table (paths,
   hrefs, ids must stay literal). Proposed blessed list: `website.title`,
   `website.navbar.title`, `page-footer.left/center/right` text form, sidebar title
   (already `ConfigValue`-preserved but from an unparsed scalar — check). Anything
   else the Connect docs need (`navbar.subtitle`? item `text:` fields?)? Also: where
   should the parse happen — at project-config load (shape change visible to all
   consumers, needs an `as_str()` audit) or at the generate-transform consumption
   sites (localized, but the parse is repeated per consumer)?
3. **`<title>` element contents.** After resolution, `pagetitle` would contain
   `<small>…</small>` raw HTML, which browsers display literally inside `<title>`.
   Should plain-text flattening for `pagetitle` strip raw-HTML inlines (what does Q1
   emit for the Connect docs' `<title>` exactly)?
4. **Include-file handler scope (Phase 3).** Builtins only (`env`, `meta`, `var`) or
   Lua shortcodes too? Lua in include files means running the Lua engine inside a
   pipeline stage rather than the transform — heavier. Q1 supports its full shortcode
   set in includes; is builtins-only acceptable for a first cut?
5. **Unresolved-shortcode behavior in the new contexts.** Body text renders a visible
   `?env` marker + Q-16-5. Same treatment in navbar/footer/title/includes? A visible
   marker in `<title>`/navbar is prominent — is that desirable (matches body-text
   policy) or should these degrade to the literal source text + warning?
6. **Coordination with `bd-environment-files-372u9qbs`.** Both strands modify
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
