# Navigation item `text:` is HTML-escaped; bare-string page-footer item becomes an empty link (bd-page-footer-items-f4th80mj)

**Date:** 2026-08-11
**Braid:** bd-page-footer-items-f4th80mj (bug, P1, labels `parity` / `websites`)
**Branch:** written on `main` @ `6dc835c2` in the main checkout (`/Users/cscheid/rooms/room-1/q2`). No worktree/branch was created — see "Where to do the work".
**Status:** Design settled 2026-08-11 (all six questions answered by Carlos — see "Settled decisions"). Implementation in progress on branch `braid/bd-page-footer-items-f4th80mj-nav-item-text`.

## Settled decisions (2026-08-11)

1. **Mechanism: `MARKDOWN_CONFIG_PATHS`.** Confirmed. `ANNOTATIONS` in pampa is not touched.
2. **Blast radius: option (b)** — the whole navigation item `text:` slice, including nested navbar `menu:` and all sidebar `contents` levels. *Not* in scope: `about.links[].text`, navbar tools, `website.sidebar.header/footer`, `margin-header/footer`, `body-header/footer`, `announcement`. Also deliberately out of scope: sidebar `section:` (a display-text sibling of `text:`, but it additionally feeds section identity, so it deserves its own check) — **follow-up strand filed**.
3. **Sidebar: option (a)** — unify on markdown everywhere. q2 deliberately diverges from Q1's entities-only sidebar; that inconsistency is a Q1 bug we are comfortable not reproducing.
4. **Inline parse** for item `text:`. q2's existing `center:` already omits Q1's `<p>`, so inline is the internally-consistent choice and avoids `<p>` inside `<a>`.
5. **Recursive descent: option (a)**, with guards. Add a `**` segment to the pattern language, but bound the descent at an obviously-artificial depth (32) and emit a diagnostic rather than risking stack overflow. **Q-1-27 "Config Nesting Too Deep" already exists in the catalog as an un-emitted stub with a docs page** — wire it up rather than minting a new code. Carlos flagged a concern that the pattern language is growing; **regroup if the blast radius turns out to be large.**
6. **Branch, not worktree** — `braid/bd-page-footer-items-f4th80mj-nav-item-text`. Needs a PR and review.

## Triage verdict

**Ready to design** — all five defects reproduce at HEAD, both root causes are confirmed empirically, and the fix mechanism is one the repo has *already designed and aligned on*; what remains is four scoping decisions, not open-ended research.

The single most consequential finding: **the handoff's suggested fix direction points at the wrong mechanism.** It proposes extending `ANNOTATIONS` in `crates/pampa/src/pandoc/meta_annotations.rs` (load-time, per-key-path interpretation). But bd-shortcodes-in-metadata-bp06aub8 already built a *second*, purpose-built mechanism for exactly this class of bug — `ConfigMarkdownTransform` / `MARKDOWN_CONFIG_PATHS` in `crates/quarto-core/src/transforms/config_markdown.rs` — and its plan (`claude-notes/plans/2026-08-10-shortcodes-website-config-includes.md`, Design decision #2) explicitly (a) rejects the load-time approach and (b) names **item `text:` fields** as the intended growth path, "one-line additions". This strand is that growth step. Details in "Two mechanisms" below.

## Q1's bare-scalar rule (measured 2026-08-11)

The strand and the original handoff both described defect 2 as "a bare
footer string should be display text". **Measurement refuted that**, and
the correction shaped the implementation, so it is recorded here.

Given a project containing `about.qmd` (title "About Us"):

| `_quarto.yml` footer item | Q1 output |
|---|---|
| `- about.qmd` | `<a class="nav-link" href="./about.html"><p>About Us</p></a>` |
| `- https://example.com` | plain `<li>` text |
| `- nonexistent.qmd` | plain `<li>` text |
| `- Copyright 2026 Example, Inc.` | plain `<li>` text |

So the rule is **resolution-dependent**: a bare scalar is a link *iff* it
names a project document, and display text otherwise — a bare external
URL included. That is exactly what q2's `enrich_navigation_items` already
computes, which is why a blanket "bare scalar is text" rule broke four
existing tests that encode the resolving case.

Implementation consequence: the parser cannot decide, because it has no
project index. `NavigationItem::bare_text` carries the original
`ConfigValue` as a fallback and `FooterGenerateTransform` demotes items
enrichment failed to resolve.

**One deliberate divergence.** Demotion runs only when a project index was
actually consulted. In a single-file render no index is attached, so
*nothing* would resolve and every bare footer item would demote to text on
no evidence at all. There, q2 keeps today's behavior (bare scalar stays an
href). Q1 parity in that corner is untested and unclaimed.

## Issue context

Filed 2026-08-11 by Carlos Scheidegger, one day old, no staleness concerns. Five defects in how `_quarto.yml` navigation item `text:` is interpreted and rendered, none of which emits a warning:

1. `text:` is HTML-escaped instead of rendered as markdown/HTML.
2. A bare-string page-footer item lands in `href=` with an **empty** link body.
3. Shortcodes (`{{< env … >}}`) stay literal in item strings.
4. Named entities (`&copy;`) stay literal there.
5. An item with `text:` but no `href:` is still wrapped in `<a href="#">`.

Real-world hit: the Posit Connect docs port, all 352 pages, on the most-seen chrome of the site.

**(5) is a sequencing constraint, not an independent nice-to-have.** The cookie-preferences pattern carries its own `<a>` inside `text:`. Once (1) is fixed, that `<a>` becomes real markup inside q2's unconditional wrapping anchor — **nested anchors, invalid HTML**. (5) must land in the same pass as (1), and the test suite should pin that specific combination.

## Dependency graph

`braid dep tree` / `dep list` return **nothing** — the strand is an isolated node. That changes the calculus in two ways: no incoming urgency from dependents, and no `discovered-from` parent carrying the "why was this filed" context. The context instead lives in the strand's own (unusually thorough) description and in the referenced repro.

Two strands are named in the description but *not* linked as edges; both are worth linking:

- **bd-v7ixzsp5** (closed 2026-08-10, "Listing contents globs") — introduced the `ANNOTATIONS` key-path table the handoff proposes to extend. Reading its plan is what surfaced that a *different* mechanism supersedes it for this case.
- **bd-shortcodes-in-metadata-bp06aub8** — introduced `ConfigMarkdownTransform`. Its plan is the design authority for this strand. Its status should be checked; if open, this strand plausibly belongs under it.

Also named in that plan as adjacent gaps, not in scope here: **bd-fz6gwfq0** (Q1's text-level shortcode contexts: code, attributes, image `src`, link targets) and **bd-1fue1ly5** (listing markdown).

## What the code looks like today

Every path in the description still exists with the described shape; nothing has been refactored out from under it.

### Reproduced at HEAD (not inferred)

`cargo build --bin q2` at `6dc835c2`, then `REPRO_YEAR=2026 q2 render` on a copy of the repro. All five defects present. Full markup in `2026-08-11-navigation-item-text-markdown-investigation/observed-output.txt`; the repro configs are committed alongside it.

q2 today:

```html
<div class="nav-footer-left">
  <ul class="nav footer-items">
    <li class="nav-item"><a href="https://example.com" class="nav-link">&lt;img src=&quot;logo.svg&quot; …&gt;</a></li>   <!-- (1) -->
    <li class="nav-item"><a href="Copyright &amp;copy; 2015-{{&lt; env REPRO_YEAR &gt;}} …" class="nav-link"></a></li>    <!-- (2),(3),(4) -->
  </ul>
</div>
<div class="nav-footer-center">Copyright © <b>2015</b>-2026 <em>Example</em>, Inc.</div>                                 <!-- control: correct -->
<div class="nav-footer-right">
  <ul class="nav footer-items">
    <li class="nav-item"><a href="https://example.com/support" class="nav-link">Support</a></li>
    <li class="nav-item"><a href="#" class="nav-link" aria-label="Cookie Prefs">&lt;a href=&quot;#&quot; …&gt;…&lt;/a&gt;</a></li>  <!-- (5) + nesting hazard -->
  </ul>
</div>
```

Navbar and sidebar are escaped identically — confirming the strand's "not page-footer-specific" claim:

```html
<li class="nav-item"><a href="https://example.com" class="nav-link">Navbar &amp;copy; &lt;b&gt;bold&lt;/b&gt; *emph* {{&lt; env REPRO_YEAR &gt;}}</a></li>
<span class="menu-text">Sidebar &amp;copy; *emph*</span>
```

### Quarto 1 reference, re-measured here (v99.9.9 dev checkout)

Two Q1 behaviors matter beyond what the strand records:

```html
<li class="nav-item">
  <a class="nav-link" href="https://example.com">
    <p><img src="logo.svg" alt="Logo" width="65px" class="footer-logo"></p>   <!-- NOTE: <p> — Q1 parses item text as BLOCKS -->
  </a>
</li>
<li class="nav-item">
  Copyright © 2015-2026 Example, Inc. All Rights Reserved.                     <!-- bare string: plain <li>, no anchor -->
</li>
<li class="nav-item">
  <a href="#" id="open_preferences_center">Cookie Preferences</a>              <!-- href-less: NO wrapping anchor -->
</li>
```

- **Q1 wraps footer item text in `<p>`** (block parse). q2's `center:` region emits no `<p>` today. So "match Q1" is ambiguous here — see design question 4.
- **Q1's sidebar really does resolve entities but not markdown**: `Sidebar © *emph*` — entity resolved, `*emph*` literal. Confirmed empirically, and it *contradicts* the Q1 source survey in the 2026-08-10 plan (which lists `website.sidebar.contents[].text` as going through the markdown envelope). Q1 is internally inconsistent between navbar (`Navbar © <b>bold</b> <em>emph</em> 2026`) and sidebar. Design question 3.

### Root cause A — interpretation, not rendering (defects 1, 3, 4)

`render_text` (`crates/quarto-navigation/src/render_html.rs:695`) is already correct: it walks `PandocInlines`/`PandocBlocks` and escapes only literal scalars. Item `text:` simply arrives as a literal scalar, because untagged strings in `InterpretationContext::ProjectConfig` stay literal (`crates/pampa/src/pandoc/meta.rs:177` `yaml_to_config_value_at`).

Verified decisively with the `!md` control project — same string, explicit tag:

```
<li class="nav-item"><a href="…/tagged" class="nav-link">Tagged © <b>2026</b> <em>emph</em> 2026</a></li>
```

Entity, raw HTML, emphasis, **and** the shortcode all resolve. **The renderer needs no change for 1/3/4** — and, importantly, shortcode resolution comes along for free once the value is inlines, because `ShortcodeResolveTransform` walks `ast.meta`.

### Two mechanisms — and why the registry is the right one

| | `ANNOTATIONS` (pampa) | `MARKDOWN_CONFIG_PATHS` (quarto-core) |
|---|---|---|
| File | `pampa/src/pandoc/meta_annotations.rs` | `quarto-core/src/transforms/config_markdown.rs` |
| When | YAML **load** time | `Normalization` transform over **merged** metadata |
| Strand | bd-v7ixzsp5 (globs) | bd-shortcodes-in-metadata-bp06aub8 |
| Purpose | keys whose value is *not markdown* (globs, paths) | website **presentation** strings that *are* markdown |
| Array wildcard | arrays transparent (elements share parent path) | explicit `*` segment per array level |
| Provenance | per-source-file | provenance-independent (profiles + frontmatter overrides already merged) |

The 2026-08-10 plan's Design decision #2 chose the registry deliberately: *"Parse happens at transform time over merged metadata …, not at project-config load: one site, provenance-independent …, no `InterpretationContext` change"* — and named the growth path: *"item `text:` fields, hrefs, margin/body header/footer, about links… the table supports array-wildcard paths from day one so those are one-line additions."*

The registry already contains `website.page-footer.left/center/right`, with the comment *"Item-list regions are arrays and therefore skipped"* — this bug is precisely that skipped case. `center:` works today for exactly this reason; the two regions of one feature disagree because one is blessed and the other isn't.

Ordering also favours the registry: `ConfigMarkdownTransform` runs in `Normalization` (`pipeline.rs:1219`), before `ShortcodeResolveTransform`, and well before the `Generate`-phase transforms that parse `NavigationItem`s — so items are constructed from already-parsed inlines. The registry is registered once in the shared `build_transform_pipeline`, so render and preview both get the fix.

**Recommendation: extend `MARKDOWN_CONFIG_PATHS`; do not touch `ANNOTATIONS`.**

### Root cause B — item model and footer renderer (defects 2, 5)

- `NavigationItem::from_config_value` (`crates/quarto-navigation/src/item.rs:76`) tries `cv.as_plain_text()` *first* and treats any bare scalar as a **file path** → `href`, `text: None`. Right for navbars/sidebars (`- about.qmd`), wrong for page-footer regions. `enrich_navigation_items` would normally backfill `text` from the project index, but a copyright sentence matches no document, so the anchor body stays empty.
  **Note:** blessing the array-element path in the registry does *not* fix this on its own — `as_plain_text()` returns `Some` for `PandocInlines` too, so a bare item would still be flattened into `href`. Defect 2 needs a footer-specific parse rule regardless.
- `render_footer_item` (`render_html.rs:678`) emits `<li class="nav-item"><a …>` unconditionally, defaulting `href` to `"#"`.
  **Precedent already in the tree:** the sidebar solved exactly this with `SidebarEntry::Heading` — rendered at `render_html.rs:478`, pinned by a test at `render_html.rs:1525` ("Heading must not be wrapped in an anchor"). The footer should mirror that shape rather than invent one.

## Pre-flight

`cargo xtask verify --skip-hub-build` at `6dc835c2`: Rust legs green; hub-client `test:wasm` reported 3 failures (`includes/in-code-fence`, `quarto-test/callout-title-attribute` ×2). **These were stale-WASM artifacts of `--skip-hub-build`, not real breakage** — after `cd hub-client && npm run build:wasm`, `npm run test:wasm` is 131/131 green. HEAD is clean. (Worth remembering: `--skip-hub-build` leaves `test:wasm` running against whatever WASM was last built.)

## Implementation record (2026-08-11)

- [x] **Phase 0 — Tests first.** Seven tests written and confirmed red at
      `dad4397b` (two regression guards green from the start). Registry
      tests in `config_markdown.rs`; renderer tests in `render_html.rs`;
      parse test in `footer.rs`.
- [x] **Phase 1 — Registry entries + `**` + depth bound.** 14 new entries
      in `MARKDOWN_CONFIG_PATHS`. `**` matches zero or more levels through
      arrays *and* maps; descent bounded at `MAX_CONFIG_DEPTH = 32`,
      reporting `Q-1-27` (a catalogued but never-emitted code) once per
      walk via a latch.
- [x] **Phase 2 — Bare footer scalars.** `BareScalar::TextIfUnresolved` +
      `NavigationItem::bare_text` + `demote_unresolved_bare_items`. See
      "Q1's bare-scalar rule" — this deviates from the drafted phase
      because measurement showed the drafted rule was wrong.
- [x] **Phase 3 — href-less footer item.** `render_footer_item` emits the
      label directly in the `<li>`.
- [x] **Phase 4 — Sidebar.** Covered by the `**` entry; markdown
      everywhere (decision 3a).
- [x] **Phase 5 — E2E + docs.** Verified through the binary (below);
      `docs/guides/authoring/shortcodes.qmd` updated to list item `text:`;
      `docs/errors/yaml/Q-1-27.qmd` rewritten to describe the walk that
      now emits it rather than a hypothetical loader bound.

**End-to-end verification.** `REPRO_YEAR=2026 q2 render` on the committed
repro, output inspected:

```html
<li class="nav-item"><a href="https://example.com" class="nav-link"><img src="logo.svg" alt="Logo" width="65px" class="footer-logo"></a></li>
<li class="nav-item">Copyright © 2015-2026 Example, Inc. All Rights Reserved.</li>
...
<li class="nav-item"><a href="#" id="open_preferences_center">Cookie Preferences</a></li>
```

plus navbar `Navbar © <b>bold</b> <em>emph</em> 2026` and sidebar
`Sidebar © <em>emph</em>`. All five defects fixed; the cookie-preferences
item emits exactly one anchor. Full workspace suite green (11659); **no
snapshot files changed**.

**Known consequence.** Item `text:` containing raw HTML now raises the
informational `Q-2-9` ("HTML element converted to raw HTML") — the repro
went from 4 to 7 such warnings. This is the same diagnostic `center:` and
`website.title` already raise for blessed keys, not a new noise source,
but sites with `<img>` in footer items will see it on every render.

## Proposed phases (draft)

Contents depend on the design answers below; ordering does not.

- **Phase 0 — Tests first.** Unit tests in `config_markdown.rs` for each newly blessed path (including negative cases: `href`/`icon` must stay scalar). Renderer tests in `quarto-navigation` for the href-less item and the bare-string footer item. One end-to-end fixture covering footer + navbar + sidebar in a single `_quarto.yml`, driven through the real binary. **Explicitly include the nested-anchor case** (`text:` containing `<a>`, no `href:`). Verify all fail first.
- **Phase 1 — Registry entries** for navigation item `text:` (defects 1/3/4). Scope per design questions 1–3.
- **Phase 2 — Footer bare-scalar → text** (defect 2), via a footer-specific constructor rather than changing the shared bare-scalar rule that navbars and sidebars depend on.
- **Phase 3 — href-less footer item renders without an anchor** (defect 5), mirroring `SidebarEntry::Heading`. Must land with Phase 1.
- **Phase 4 — Sidebar decision** applied (Q1-bug-compatible vs unified), whichever design question 3 settles on.
- **Phase 5 — E2E verification + docs.** Render the committed repro through `q2 render`, diff against the Q1 baseline captured in `observed-output.txt`, and record the invocation + observed markup per the end-to-end verification rule. Re-check the Connect-docs site if convenient.

## Open design questions for the user

1. **Mechanism — confirm the registry.** Do you agree this belongs in `MARKDOWN_CONFIG_PATHS` (per the 2026-08-10 Design decision #2) rather than in pampa's `ANNOTATIONS`, contrary to the handoff's suggestion? *Recommendation: yes, registry.*

2. **Blast radius of blessed paths.** The repro needs footer + navbar + sidebar item `text:`. The 2026-08-10 survey lists a wider navigation slice: navbar `menu:` (nested dropdowns), navbar tools, `about.links[].text`, `website.sidebar.header/footer`, `website.margin-header/footer`, `website.body-header/footer`, `website.announcement`. Do we (a) bless only what the repro covers, (b) take the whole *navigation item `text:`* slice including nested menus, or (c) take the whole survey? *Recommendation: (b) — nested `menu:` items are the same field and would otherwise be an obvious follow-up bug report.*

3. **Sidebar: copy Q1's inconsistency, or unify?** Q1 renders navbar item text as full markdown but sidebar item text as entities-only (measured, above). q2 has no "entities-only" interpretation; building one is a new mechanism for the sole purpose of reproducing what looks like a Q1 bug. Options: (a) unify — markdown everywhere, accept the divergence from Q1's sidebar; (b) match Q1 exactly, building entity-only decoding; (c) leave the sidebar untouched for now and file it separately. *Recommendation: (a). If you want Q1-exactness, it should be a separate strand with its own justification.*

4. **Inline or block parse for item `text:`?** Q1 emits `<p>` inside footer item anchors (block). q2's blessed `center:` currently emits no `<p>`. Should item text parse as inline (consistent with q2's existing footer regions, cleaner markup inside an `<a>`) or as blocks (byte-closer to Q1, but `<p>` inside `<a>` inside `<li>`)? *Recommendation: inline — and note that q2's existing `center:` already diverges from Q1 here, so "inline" is the internally-consistent choice.*

5. **Recursive nesting in the registry.** `website.sidebar.contents[].text` nests arbitrarily deep (`contents` → `contents` → …), but the registry's `*` matches exactly one array level, so recursion cannot be expressed as a finite list of patterns. Do we (a) add a recursive-descent segment (`**`) to the pattern language, (b) enumerate a bounded depth (3–4 levels, silently wrong deeper), or (c) special-case the sidebar walk? This question only bites if question 3 answers (a) or (b). *Recommendation: (a) if the sidebar is in scope — a bounded depth is the kind of silent cliff this codebase's lint rules exist to prevent.*

6. **Where should the work happen?** This plan is committed on `main` in the main checkout; no branch or worktree was created (per skill policy). Given the change touches `quarto-core` + `quarto-navigation` and will churn navigation snapshots, a worktree via `cargo xtask create-worktree` seems right — but that's your call to set up.

## Risks / tradeoffs (draft)

- **Behavior change for existing sites, silently.** Blessing more paths means any existing site whose item `text:` happens to contain `*`, `_`, `<`, or `&…;` changes rendering with no warning. Same tradeoff the 2026-08-10 strand already accepted for `website.title`, so it's precedent rather than a new risk — but the repro set should include a "text that looks like markdown but wasn't meant to be" case so the behavior is pinned, not incidental.
- **`!str` cannot opt out.** Per `config_markdown.rs`'s own module docs, a `!str`-tagged project-config string is indistinguishable from an untagged one after load. So there is *no* escape hatch for an author who wants literal `*text*` in an item label, beyond backslash-escaping. Worth deciding whether that's acceptable at this blast radius, or whether the registry needs a genuine opt-out first.
- **Sequencing is load-bearing.** Fixing (1) without (5) actively *creates* invalid HTML (nested anchors) on the Connect docs. Do not split these across PRs.
- **Snapshot churn** across `quarto-navigation` and any website-fixture snapshots. Per the repo's snapshot policy, count and summarize them in the commit message, and flag anything that changed for a reason other than "item text is now rendered".
- **Two mechanisms remain in the tree** after this. Both are documented as temporary pending schema-driven interpretation. Someone will propose extending the wrong one again; a cross-reference note in `meta_annotations.rs` pointing at `config_markdown.rs` (and vice versa) would be cheap insurance. Worth filing as a follow-up chore.
