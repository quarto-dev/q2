# `source: file` brand fonts: never copied, `@font-face` URLs resolve against the theme CSS's directory (bd-ve916wr8)

**Date:** 2026-09-08
**Braid:** bd-ve916wr8
**Checkout:** main @ `b7e7c96a` (investigation ran in the main checkout; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** Every claim in the strand reproduces at HEAD with a
two-page fixture, and the investigation found two more defects in the same
mechanism (single-doc renders are equally broken; the SASS cache key omits
the font prefix, so pages alias onto whichever compiled first). The fix is
well-bounded — three consumers, one emitter — but there is one real design
fork (where the font bytes go and therefore what the URL is) that the user
should pick.

Evidence: `claude-notes/plans/brand-file-fonts-website-investigation/NOTES.md`.

## Issue context

Filed 2026-09-08 by Carlos, type `bug`, priority 2, labels `css`,
`theming`, `websites`. Found while moving cscheid-net-2026 from Google
Fonts to local woff2. Summary of the report:

1. `source: file` font files are never copied into the output.
2. The emitted `src: url("assets/x.woff2")` is relative to the *document*,
   but the compiled theme CSS is written to `site_libs/quarto/`, so the
   browser resolves it against the wrong directory.
3. The prefix is computed per document (`pathdiff(document_dir,
   brand_dir)`), so a site with pages at different depths ships N theme
   bundles.
4. Minor: no `format("woff2")` hint, no `font-display` for file fonts;
   `format:` on a files entry is silently ignored.

Workaround in use: a site-root-absolute `/assets/…` path plus an explicit
`project.resources` glob — correct only when served from the domain root.

## Dependency graph

No `discovered-from`, no incoming `blocks`. Two `related` edges:

- **bd-5fseopxy** (in_progress) — weight ranges `400..700` collapse to
  400 for google *and* file sources. Touches the same
  `file_font_face_block` (`font-weight: N M` for variable files). The two
  strands will conflict textually in `brand_layer.rs:553-584`; whichever
  lands second rebases. No semantic coupling — the URL/copy work here does
  not care about the weight value.
- **bd-qnylgu69** (open) — audit `docs/guides/authoring/brand.qmd` against
  actual Q2 support. This strand's docs phase should hand it the exact
  wording for `source: file` (paths are relative to the brand file; files
  are copied automatically once this lands; until then the `/assets` +
  `resources:` workaround).

Also relevant, not linked: **bd-r1y48cx0** (open) — `css:` files never
copied for websites. Same bug class ("theme/CSS side-input is read but
never published"); the copy mechanism chosen here should be the one that
strand reuses. And the path-resolution contract
(`claude-notes/designs/path-resolution-model.md`) lists `brand:` as
"project-root by construction" — font paths *inside* the brand file are a
consumer that inventory does not yet mention; add a row.

## What the code looks like today

All file paths in the strand still exist with the described shape
(spot-checked; line numbers in NOTES.md). Reproduced at HEAD:

| Case | Theme CSS location | Emitted `src` | File copied? |
|---|---|---|---|
| website, `index.qmd` | `_site/site_libs/quarto/quarto-theme-<fp>.css` | `assets/…` | no |
| website, `posts/one.qmd` (cold cache) | second bundle, different `<fp>` | `../assets/…` | no |
| website, warm cache | one bundle, prefix of whichever page compiled *first* (persistent across sessions) | order-dependent | no |
| single doc `doc.qmd` | `doc_files/styles.css` | `assets/…` → resolves to `doc_files/assets/…` | no |

Two corrections to the strand text:

- **Single-doc is broken too.** `styles.css` is not "next to the document";
  it is in `{stem}_files/` (`resource_resolver.rs:113-124`), so the
  document-relative prefix is wrong there as well.
- **The N-bundle split only happens on a cold cache.** With the persistent
  SASS cache warm, `cache_key()` (`compile_theme_css.rs:197`) — which
  hashes the brand YAML but not `document_dir` — makes every page reuse the
  first page's bundle, wrong prefix included. Either way the output is
  wrong; the cache-key omission is a separate correctness bug worth its own
  regression test.

Q1 parity: Q1 uses the identical document-relative prefix and does **not**
copy font files for HTML (only for typst). It works in Q1 only because Q1's
theme CSS sits beside the page. The `site_libs`/`{stem}_files` relocation
is the Q2 change that exposed both gaps.

## Proposed phases (draft)

Skeleton only — contents depend on the design answers below.

- **Phase 0 — Test plan (TDD).**
  - `quarto-sass` unit: `file_font_face_block` emits the chosen URL shape
    (and `format("woff2")` if in scope); `/`-rooted path handling per Q3.
  - `quarto-core` integration (drive `render_document_to_file` /
    project render, not `brand_to_layers` directly): (a) website with pages
    at two depths → exactly one theme bundle, both pages link it, the
    `@font-face` URL resolves from the bundle's directory to a file that
    exists in the output; (b) single-doc → same property against
    `{stem}_files/styles.css`; (c) cache-aliasing regression — render
    nested page first, then root page, assert the root page's URL is
    correct; (d) missing font file → warning naming the brand file key.
  - Fixture: promote `brand-file-fonts-website-investigation/repro` into
    the test tree.
- **Phase 1 — Font URL base.** Compute the `@font-face` URL relative to
  the theme artifact's location (or make it constant — see Q1). Remove
  `document_dir` from the prefix computation so the fingerprint is
  site-wide; add whatever *does* feed the URL to `cache_key()`.
- **Phase 2 — Publish the font bytes.** Per Q1: either emit fonts as
  project-scope artifacts next to the theme CSS, or register them as
  implicit resources / a post-render copy hook. Must cover single-doc and
  website; note WASM preview implications.
- **Phase 3 — Small emitter fixes (if in scope, Q4).** `format()` from the
  extension; reject unknown keys on file entries (`format:`/`display:`)
  instead of dropping them; optionally accept a display hint.
- **Phase 4 — Docs + contract.** `docs/guides/authoring/brand.qmd`
  (`source: file` semantics, coordinate with bd-qnylgu69); add the
  brand-font row to `path-resolution-model.md`'s inventory; end-to-end
  verification via `cargo run --bin q2 -- render` on the fixture, output
  inspected.

## Open design questions for the user

1. **Where do the font bytes live, and hence what is the URL?** Two shapes:
   - **(a) Fonts become project-scope artifacts beside the theme CSS**
     (`site_libs/quarto/fonts/<name>` / `doc_files/fonts/<name>`), and the
     `@font-face` URL is the constant `fonts/<name>`. Pros: one mechanism
     for website, single-doc, *and* the WASM preview (artifacts flush
     through both transports; the native-only `copy_*` hooks do not);
     the prefix is constant so the cache-key and per-depth problems vanish
     structurally; no dependence on how the site is hosted. Cons: font
     bytes pass through the in-memory artifact store; `_site/assets/`
     no longer mirrors the source tree for fonts (a user's `/assets/…`
     workaround keeps working but becomes redundant).
   - **(b) Copy fonts to their source-mirrored output path** (like logos:
     `_site/assets/x.woff2`) and emit a URL relative to the theme CSS
     (`../../assets/x.woff2` from `site_libs/quarto/`; `../assets/…` from
     `doc_files/`). Keeps the output tree familiar; needs the copy to run in
     both single-doc and project paths and stays native-only.
   My recommendation is (a). Do you agree, or do you want the output tree
   to mirror the source layout?
2. **Copy channel (only if 1b).** Post-render hook like `copy_navbar_logo`
   (needs a single-doc twin), or the resource channel
   (`ResolvedResource` with a brand-file origin, so the render manifest
   lists the fonts for `quarto publish`)? I'd take the resource channel,
   and bd-r1y48cx0 (`css:` copy) would reuse it.
3. **Leading `/` in a font path.** Today `/assets/x` accidentally survives
   as a site-root URL (`PathBuf::push` drops the prefix). The path contract
   says a leading `/` means *project root* for resolution. Should the fix
   (i) resolve `/assets/x` against the project root for the *copy* and
   emit the normal relative/constant URL like any other path, or (ii)
   keep `/…` as an explicit "emit verbatim, you host it" escape hatch?
   (i) is contract-conformant and what I'd do; it changes the workaround's
   emitted URL from `/assets/…` to the new form, which is still correct.
4. **Scope of the emitter niceties.** In scope here or a follow-up strand:
   `format("woff2")` from the extension (cheap, clearly good);
   `deny_unknown_fields` on the explicit file entry so `format:`/`display:`
   error instead of vanishing (the brand.yml spec has no `display` for
   file fonts — accepting one would be a Q2 extension); a `display:` /
   `font-display` hint for file fonts (spec extension — I'd file it
   separately).
5. **Interaction with bd-5fseopxy.** Both edit `file_font_face_block`.
   Sequence this after it lands, or land this first and let the weight
   work rebase? (Pure ordering question; either is fine.)

## Risks / tradeoffs (draft)

- **Artifact-store size (1a).** Variable fonts are ~100–500 KB each; a
  brand with several files puts a few MB through the artifact map per
  render. Should be fine (images already go this route) but worth one
  measurement on a real brand.
- **Fingerprint stability.** Whatever feeds the URL must feed both
  `theme_fingerprint` (already content-based, fine) and `cache_key()`
  (currently incomplete). If (1a), the URL is constant and the cache key
  is correct by construction; if (1b), the key must include the
  artifact-relative prefix.
- **Dark brand with different fonts** — the dark bundle's `@font-face`
  blocks reference the dark brand's files; under (1a) both variants' fonts
  land in the same `fonts/` dir, so filename collisions across brands
  need a content-hash or brand-scoped name.
- **Hub-client preview** — under (1b) fonts would not appear in the WASM
  preview at all (the copy hooks are native-only); under (1a) they should,
  but the preview transport's handling of binary project-scope artifacts
  needs a check before claiming it.
- **Test environment** — pre-flight's hub-client vitest leg is red on a
  clean `main` (`localStorage` undefined, Node v26.8.1); Rust legs green.
  Unrelated but will show up in any full `cargo xtask verify`.
