# Measured fallout of direction (B), 2026-08-18

Direction (B) = make `sectionize_blocks` recurse into non-section Divs and apply
pandoc's absorb rule, then restrict `collect_toc_entries` to the section tree
(and drop its `BlockQuote` arm).

Spike implemented, measured, and **reverted**. The exact diff measured is
`spike-B.patch` (33 added / 5 removed lines across two files). It is a
throwaway — the absorb rule in it is deliberately conservative and is *not* a
proposed implementation.

## Method

Two clean renders per corpus (`rm -rf _site .quarto`, render twice to reach
steady state, then copy), so the diff reflects the code change and nothing
else.

> **A first pass got this wrong and it mattered.** The baseline was an
> incremental render while the spike side was clean, which inflated the diff to
> 131 files with link-rewriting and attribute-ordering noise
> (`../../licensing/index.html#…` vs `/admin/licensing/index.md#…`). Redone
> clean, the true number is 36. Any future re-measurement must double-render
> both sides.

## Corpus 1 — workspace test suite

`cargo nextest run --workspace` → **12306 passed, 0 failed**, no snapshot
changes. Same result for the recursion-only spike.

That is not the reassurance it looks like: it means **the suite has no coverage
of a heading nested inside a Div**. Phase 0's tests are new coverage, not
regression guards.

## Corpus 2 — `docs/` (q2's own site, 247 source files → 258 pages)

**0 files changed.** The docs site has no headings nested in Divs. Useless as a
canary for this change.

## Corpus 3 — Connect docs port (352 source files → 451 pages)

**36 files changed: 35 HTML + `sitemap.xml`** (a single `<lastmod>` timestamp).

| measure | result |
| --- | --- |
| HTML pages changed | 35 (27 TOC + DOM, 8 DOM-only) |
| TOC entries **removed** | 46 |
| TOC entries **added** | 0 |
| vs Q1: closer / unchanged / further | 25 / 8 / **2** |
| **whole corpus, exact TOC match with Q1** | **421 → 444 of 451** |

The 46 removals line up with the strand's independently-measured 44 leaked
entries (the strand counted structurally; callout-body leaks account for the
rest).

### What a DOM-only change looks like

The absorb rule firing — pandoc's exact behavior, class merged into the section:

```diff
-<div class="content-hidden">
-<h4 id="username-limitations-without-service-credentials">Username limitations…</h4>
+<section id="username-limitations-without-service-credentials" class="section level4 content-hidden">
+<h4>Username limitations…</h4>
 …
-</div>
+</section>
```

TOC unchanged on that page: the entry was reached via the bare-`Header` path
before and via the section path after.

## The 2 regressions — root cause is NOT (B)

`admin/authentication/ldap-based/{active-directory,ldap}-double-bind/index.html`
lose `#using-group-memberships`, which Q1 keeps. Source
(`admin/authentication/ldap-based/include/_users.qmd:184`):

```markdown
::: {.content-visible when-meta="authentication.double-bind"}
Posit Connect offers ways to map their user information…      ← Para first
#### Using group memberships
```

**Q1 deletes the `content-visible` Div entirely** — its conditional-content
filter splices the contents into the parent section, so the heading is an
ordinary sibling by TOC time:

```html
<section id="user-role-mapping" class="level3">
<h3>Automatic user role mapping</h3>
<p>Posit Connect offers ways to map…</p>
<section id="using-group-memberships" class="level4">
```

**q2 keeps the Div** (`<div class="content-visible">`), so the restricted walk
stops at it. Exposure: **10 q2 pages retain a `content-visible`/`content-hidden`
Div; Q1 has 0.**

So (B) carries a dependency that was not visible before this measurement:
**conditional-content Divs must be unwrapped the way Q1 unwraps them**, or those
pages lose entries. Under today's over-permissive collector the un-unwrapped Div
is a harmless DOM difference; (B) converts it into a missing TOC entry.

### Correction to `div-recursion-probe/README.md` finding 5

That note attributed `.content-visible` headings surviving Q1's TOC to pandoc's
**absorb** rule. Wrong: Q1 **unwraps** those Divs in its conditional-content
filter. The probe could not distinguish the two (both yield a lone section with
no wrapping Div); the Connect-docs case has a `Para` before the heading, which
absorb cannot explain and unwrapping can. Absorb is real and does apply to
arbitrary Divs — `.my-wrapper` → `class="level4 my-wrapper"`, and the
`content-hidden` diff above — but it is not what saves `.content-visible`.

## Latent trap, not a live one

`SectionizeTransform` is pushed only in the non-reveal branch
(`pipeline.rs:1332`), while `TocGenerateTransform` is ungated (`:1387`). Under
(B) a revealjs TOC would find no section tree and come up empty. Verified that
**q2 emits no `nav#TOC` for revealjs today**, so nothing breaks now — but
whoever implements a reveal TOC (or bd-y5j0m376's reveal tabsets) inherits the
trap unless sectionize runs there too.

## Bottom line

- Cost: 35 pages on the only corpus that exercises the feature; 0 test failures;
  0 spurious TOC entries.
- Benefit: **+23 pages to exact Q1 TOC parity** (421 → 444 of 451).
- Blocker: unwrap conditional-content Divs first, or accept 2 known regressions.
- Remaining 7 non-matching pages are pre-existing differences unrelated to this
  change.
