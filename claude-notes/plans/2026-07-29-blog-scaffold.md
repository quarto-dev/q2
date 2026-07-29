# `q2 create`: blog scaffold (`website:blog`)

**Strand:** bd-r1by4u2a (discovered-from bd-oa5kd2yr)
**Created:** 2026-07-29
**Parent plan:** `claude-notes/plans/2026-07-23-q2-create-command.md` (the `q2 create` foundation — artifact seam, JSON mode, writer — all landed; this strand only adds the blog choice)

## Overview

Implement the `blog` project choice (`website:blog`) in
`quarto-project-create` and flip it to `implemented`, so
`q2 create project blog <dir>` (CLI, JSON mode, interactive) and the
hub-client WASM path all pick it up. Port Quarto 1's blog scaffold
(`$CLI/src/project/types/website/website.ts:465`,
`$CLI/src/resources/projects/website/templates/blog/`), adapted to what
Q2 actually supports. This is the first scaffold with **binary files**
(post images) — it exercises the existing-but-unused
`ScaffoldContent::Binary` path end to end.

The strand was gated on listings maturity (epic bd-61cd). Verified
2026-07-29: L0–L9 closed; only docs (bd-hzsi) and close-out (bd-qb4o)
remain. Empirical check (Q1-shaped blog fixture rendered with
`cargo run --bin q2 -- render`): listings, categories sidebar/chips,
RSS feed, `_metadata.yml` layering, `title-block-banner`, and navbar
icon entries all work. Three listing-machinery gaps surfaced; they are
fixed in-strand (Phase A below) because they are exactly the "listings
maturity" this strand was waiting on:

1. **`sort: "date desc"` mis-parses** — front-matter strings arrive as
   `PandocInlines`; `parse_sort`'s scalar arm misses the
   `as_plain_text()` route and emits Q-12-3 with an empty sort. Already
   filed as **bd-2qjnd** (fix spelled out there; close it here).
2. **`contents: posts` (bare directory) matches nothing** — Q1 expands
   a directory entry to everything under it; Q2's `glob_match`
   requires segment-count agreement, so the pattern `posts` can never
   match `posts/welcome/index.qmd`. The canonical Q1 blog config —
   shown in all Q1 docs — silently produces an empty listing.
3. **Front-matter `image:` on a post is neither rebased nor copied** —
   the listing card on the root page emits `src="image.jpg"` verbatim
   (404) and nothing copies the file into `_site/` (a *body*-referenced
   image is copied by the post's own render and auto-fill stores a
   project-relative path — that's why the `thumbnail.jpg` flow works
   and the `image:` flow doesn't). No existing strand; file + fix here.

Known, deliberately out-of-scope caveat: **bd-57y4** (P2, open) — Q1's
`quarto-listing.scss` is not vendored, so listing cards render with
default browser styling. The blog is functional but visually plainer
than Q1 until that lands. Flag to Carlos at handoff; the choice flip is
one line to hold back if he wants to wait.

## Scaffold file set (decided)

Q1-familiar, adapted per the established precedent ("scaffolds should
feel familiar, not byte-for-byte; drop what Q2 doesn't support" — see
the brand decision in the parent plan). Files (paths relative to the
new project dir):

| File | Kind | Notes |
| --- | --- | --- |
| `_quarto.yml` | Template | Q1 shape: `website.description` "A blog built with Quarto", placeholder `site-url` (**required** — Q2's feed completion silently no-ops without it, `feed/complete.rs:87`), `website.title: "$title$"`, navbar right: `about.qmd` + github/bluesky icon items (icon-only navbar items verified supported), `format.html: {theme: cosmo, css: styles.css}`. Plus `project.resources: [styles.css]` (bd-b87tmmi4, same as website scaffold). **No** `brand` marker, **no** `editor:`. |
| `index.qmd` | Template | Q1's listing front matter verbatim: `contents: posts`, `feed: true`, `sort: "date desc"`, `type: default`, `categories: true`, `sort-ui: false`, `filter-ui: false` (both inert in Q2 but accurate — Q2 emits no sort/filter UI), `page-layout: full` (class emitted; styling is theme-owned), `title-block-banner: true`. Works warning-free once Phase A lands. |
| `posts/_metadata.yml` | StaticText | `title-block-banner: true` (verified working via directory layering). **Drops Q1's `freeze: true`** — no Q2 freeze implementation (bd-mx5x609r); shipping a knob that silently does nothing misleads. |
| `posts/welcome/index.qmd` | Template (date) | Q1 content: author Tristan O'Malley, `date:` = today − 3 days, `categories: [news]`, body embeds `![](thumbnail.jpg)` + the "first image is used in the listing" blurb. |
| `posts/welcome/thumbnail.jpg` | **Binary** | copied from Q1 (one-time copy per external-sources policy). |
| `posts/post-with-code/index.qmd` | Template (date) | Q1 content: author Harlow Malloc, `date:` = today, `categories: [news, code, analysis]`, explicit `image: "image.jpg"` (works once fix 3 lands). |
| `posts/post-with-code/image.jpg` | **Binary** | copied from Q1. |
| `about.qmd` | StaticText | Simplified like the website scaffold: title About, body "About this blog". **Drops Q1's `about: {template: jolla, links: …}` block and `profile.jpg`** — Q2 has no about-page feature (verified: zero implementation; silently ignored). File a feature strand and revisit when it lands. |
| `styles.css` | StaticText | `/* css styles */`. |

Deviations from Q1, summarized: no `brand`, no `editor:`, no `freeze:`,
no `about:`/`profile.jpg`, `project.resources` added. Everything else
byte-familiar.

### Post dates

Q1 stamps the two posts with real dates (today − 3d, today) so the
listing sorts sensibly on first render. Design: `quarto-project-create`
gains a `time = "0.3"` dep (pure date arithmetic + formatting is
wasm-safe; getting *now* on wasm32 uses time's `wasm-bindgen` feature,
target-gated). `CreateFromChoiceOptions` gains
`today: Option<time::Date>` (builder `with_today`) so tests are
deterministic; `None` → the crate computes today itself, so neither the
CLI nor the WASM export changes signature. The two post files become
`Template`s interpolating `$first-post-date$` / `$second-post-date$`
(and the scaffold sweep test's "every Template contains `$title$`"
invariant is relaxed to "every Template interpolates at least one
context variable it actually receives").

Fallback if time's wasm `now` proves broken in our WASM build: add an
optional date argument to the WASM `create_project` export and have the
hub-client pass `new Date()`. Decide empirically in Phase C.

## Work Items

### Phase A: listing fixes (TDD, one at a time; independent of scaffold)

- [x] A1 `parse_sort` PandocInlines (bd-2qjnd): failing test in
      `config.rs` tests (scalar `sort:` value shaped as PandocInlines →
      currently Q-12-3 + empty), then the one-line `as_plain_text()`
      fix. Close bd-2qjnd. Also remove the documented workaround in
      `docs/errors/index.qmd` if present (bd-2qjnd notes it).
- [x] A2 bare-directory `contents:`: failing tests at both agreeing
      call sites (listing_generate `matches_any_glob`, dependency_graph
      edges) for `contents: posts` matching `posts/welcome/index.qmd`;
      fix via a shared dir-aware match helper next to
      `glob_match_path` (pattern without glob metacharacters also
      tried as `pattern/**`). Deliberately scoped to listing matching —
      not general project-input discovery. File strand, link
      discovered-from bd-r1by4u2a, close when green.
- [x] A3 front-matter `image:` rebase + copy: failing integration test
      (post with front-matter `image:`, listed from root page →
      expect `src="posts/…/image.jpg"` in host page AND the file in
      `_site/`); fix = rebase relative front-matter image to
      project-relative at profile extraction/hydration (matching the
      auto-fill convention) + register a `ResourceCopyIntent` from the
      listing resolve path (precedent: `title_banner.rs:139-149`).
      File strand, link discovered-from, close when green.
- [x] A4 re-render the Q1-shaped fixture; confirm warning-free with
      Q1's exact `contents: posts` + `sort: "date desc"` + `image:`.

**Phase A completed 2026-07-29.** A2 = `glob_match_path_or_dir` in
`discovery.rs` (used by `matches_any_glob` + dep-graph edges); A3 =
document-relative rebase in `hydrate_item` (`rebase_image`),
host-relativization in `helpers::image_html` (`host_relative_url`),
copy intents in `ListingGenerateTransform`. Fix strands: bd-2qjnd,
bd-9arwdicv, bd-qv2lsab0. A4 e2e: Q1-shaped fixture renders
warning-free with `contents: posts`, `sort: "date desc"`, front-matter
`image:` rebased to `posts/post-with-code/image.jpg` and copied;
date-desc order verified. quarto-core suite: 2668/2668.

### Phase B: scaffold tests first (TDD)

- [ ] B1 crate render tests: blog file set (9 files in the order
      above), `_quarto.yml` parsed with serde_yaml (website.title,
      description, site-url present, navbar right shape, cosmo, no
      brand/freeze/about), post front matter carries the two dates
      (fixed via `with_today`), binary files present with
      `image/jpeg` MIME and non-empty bytes, no template residue.
- [ ] B2 scaffold registry tests: `get_scaffold(website:blog)` file
      list; sweep-test invariant update (see "Post dates").
- [ ] B3 CLI integration tests (`crates/quarto/tests/integration/create.rs`):
      `create project blog myblog` writes all files incl. binaries
      (byte-compare a jpg); `--list --json` now reports
      `blog: implemented=true`; repoint the two
      unimplemented-choice tests (`unimplemented_choice_says_so`,
      `colon_form_routes_through_template_parser`) at `manuscript` /
      `website:nonexistent`-style targets so they keep their meaning.
- [ ] B4 run all new tests; record expected failures.

### Phase C: implementation

- [ ] C1 copy `thumbnail.jpg` + `image.jpg` from
      `external-sources/quarto-cli/.../templates/blog/` into
      `crates/quarto-project-create/resources/templates/website/blog/`;
      author the blog templates/static files.
- [ ] C2 `templates.rs` blog module (`include_str!`/`include_bytes!`),
      `scaffold.rs` `Some("blog")` arm, date plumbing (`time` dep,
      `with_today`, context vars), flip the choice to implemented.
- [ ] C3 make Phase B tests green; full `-p quarto-project-create` and
      `-p quarto` suites.
- [ ] C4 hub-client: extend `projectCreate.wasm.test.ts` (blog choice
      listed implemented; `create_project('blog', …)` returns the file
      set with `content_type: 'binary'` + mime for the jpgs); rebuild
      WASM, run `npm run test:wasm`. Two-commit changelog rule applies.

### Phase D: verification + handoff

- [ ] D1 `cargo build --workspace` + `cargo nextest run --workspace`.
- [ ] D2 full `cargo xtask verify` (shared-crate + quarto-core changes
      → WASM leg affected).
- [ ] D3 **End-to-end (record here):** `cargo run --bin q2 -- create
      project blog myblog "My Blog"` → inspect every file on disk
      (binary jpgs byte-identical to Q1's); `cargo run --bin q2 --
      render myblog` → inspect `_site/index.html` (both posts listed,
      dates ordered desc, categories chips + sidebar, thumbnail +
      image srcs resolve, files copied), `_site/index.xml` feed,
      `about.html`. Also the `--json` directive path and `--dry-run`.
- [ ] D4 braid: close bd-2qjnd + the two new fix strands + this strand;
      file the about-pages feature strand; update this plan; report the
      bd-57y4 styling caveat to Carlos.
