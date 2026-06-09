# Audit: metadata string reads using `as_str()` that should use `as_plain_text()`

**Strand:** bd-y89ihf0i (task, p2; labels: footnotes, tech-debt)
**Discovered from:** bd-9ez3ngt1 (PR #265, reference-location front matter)
**Date:** 2026-06-09

## Overview

In **document-metadata context**, a bare YAML string value is parsed as
markdown and stored as `ConfigValueKind::PandocInlines`, **not**
`Scalar(String)` (see `quarto-pandoc-types/src/config_value.rs:189-195`).
Consequences for the two string accessors:

- `ConfigValue::as_str()` (config_value.rs:641-649) returns `None` for
  `PandocInlines`.
- `ConfigValue::as_plain_text()` (config_value.rs:675-684) handles **both**
  `Scalar(String)` and `PandocInlines`.

So any **user-authored front-matter string option** read with `as_str()` is
**silently ignored** unless the user writes the undocumented `!str` escape tag.
This is the exact class of bug that broke `reference-location` entirely
(bd-9ez3ngt1 / PR #265).

This plan sweeps the remaining call sites, switches the genuine
user-front-matter reads to `as_plain_text()` (each with a failing
end-to-end regression test first, per TDD), and adds a `cargo xtask lint`
rule to prevent recurrence.

### Decision criterion (per call site)

- **SWITCH** `as_str()` → `as_plain_text()` when the value is a user-authored
  front-matter/metadata STRING that could reasonably be written as a bare
  scalar, and markdown-in-value is harmless because we only want its text.
- **LEAVE** `as_str()` when the code deliberately distinguishes a scalar
  string from inlines, when the value is internal (AST `plain_data`, attrs,
  computed config, `serde_json::Value`), or when it is a test assertion on
  transform-generated data.

## Branch base & sequencing

**Decision (user, 2026-06-09): stack on the #265 branch.**

Base this work on `bugfix/bd-9ez3ngt1-reference-location-front-matter`
(fetched; remote tip `a35df31a`). That branch already fixes
`footnotes.rs:78` and `appendix.rs:92` (reference-location) and adds the
`render_to_file` regression-test template we mirror below. By stacking, the
"already fixed" sites are present in-tree and both fixes ship together.

```bash
git fetch origin bugfix/bd-9ez3ngt1-reference-location-front-matter
git switch -c beads/bd-y89ihf0i-metadata-as-str-audit \
  origin/bugfix/bd-9ez3ngt1-reference-location-front-matter
```

**Do NOT redo** `footnotes.rs:78` / `appendix.rs:92` — they are fixed on the
base branch.

## Confirmed classification

Verified by reading each site (not a blanket replace). Line numbers are on
`main` / the #265 base; re-confirm after switching branches.

### SWITCH — real user-front-matter reads (~10 sites)

| File | Line | Option | Notes |
|------|------|--------|-------|
| `transforms/appendix.rs` | 80 | `appendix-style` | bare scalar |
| `transforms/appendix.rs` | 275 | `license` (bare-string form) | nested: also map `.text`/`.type` |
| `transforms/appendix.rs` | 282 | `license.text` / `license.type` | |
| `transforms/appendix.rs` | 324 | `copyright` (bare-string form) | nested: also map `.statement`/`.holder` |
| `transforms/appendix.rs` | 331 | `copyright.statement` / `.holder` | |
| `transforms/appendix.rs` | 374 | `citation.url` | bare URL scalar → PandocInlines |
| `transforms/toc_generate.rs` | 91 | `toc: auto` comparison | use `.as_plain_text().as_deref() == Some("auto")` |
| `transforms/toc_generate.rs` | 117 | `toc-title` | |
| `format.rs` | 311 | `theme` (`none`/`pandoc`) in `is_minimal_html` | **Not in seed — found by the lint.** Bare `theme: none`/`pandoc` silently failed to trigger minimal mode. List/map themes still yield `None` (correct). Unit-tested. |
| `transforms/code_block_generate.rs` | 118 | `code-copy` (string values) | `as_bool` fast-path stays; only `always` string affected. **No e2e observable** — Hover/Always emit identical markup and Q2 hardcodes the hover SCSS `$code-copy-selector`; tested via a `PandocInlines` unit test on `resolve_default_copy_mode`. Fix prevents a latent bug once the selector is wired. |

**Removed from SWITCH after per-site verification:**

- `transforms/shortcode_resolve.rs:169` (`config_value_to_inlines`, the
  `{{< meta key >}}` shortcode) → **LEAVE**. The `as_str()` is a fast-path;
  `PandocInlines` is explicitly handled at line 192 (`inlines.clone()`), so a
  bare front-matter string already resolves. Switching to `as_plain_text()`
  would be a **regression** — it would flatten inline markdown the meta
  shortcode should preserve (e.g. emphasis in a subtitle). Same fast-path +
  explicit-`PandocInlines`-handling shape as title_block/metadata_normalize.

### LEAVE — verified not a bug

| File | Line(s) | Reason |
|------|---------|--------|
| `transforms/title_block.rs` | 132 | `as_str()` is a fast-path; the next `match` explicitly handles `PandocInlines`/`PandocBlocks`. Already correct. |
| `transforms/metadata_normalize.rs` | 98 | Same fast-path + `match` pattern as title_block. Already correct. |
| `transforms/shortcode_resolve.rs` | 169 | Fast-path; `PandocInlines` handled at line 192 (`inlines.clone()`). Switching would flatten inline formatting the `meta` shortcode must preserve — a regression. |
| `transforms/metadata_normalize.rs` | 330, 371, 414 | `#[cfg(test)]` asserts on generated `pagetitle`. |
| `transforms/website_canonical_url.rs` | 153, 205, 230 | All under `#[cfg(test)]` (mod starts line 114). Production reads `website.site-url` via `website_config::website_site_url`, which **already** uses `as_plain_text()`. |
| `transforms/callout_resolve.rs` | 559 | Operates on `serde_json::Value`, not `ConfigValue`. |
| `transforms/crossref_render.rs` | 228, 234, 325, 331, 661, 667 | Internal CustomNode `plain_data` (ref_type/kind/identifier). |
| `transforms/crossref_index.rs` | 252, 321, 326 | Internal CustomNode `plain_data`. |

## Phases

### Phase 0 — Branch setup
- [x] Fetch + branch off the #265 head (commands above) — branch
      `beads/bd-y89ihf0i-metadata-as-str-audit` off `a35df31a`
- [x] `cargo build -p quarto-core` to confirm green base — clean.
      Confirmed #265 fixes present (footnotes.rs:82, appendix.rs reference-location).
      NOTE: line numbers shifted vs. the table above due to #265's added
      comments — re-grep current lines per site during Phase 2.

### Phase 1 — TDD: failing regression tests (write & confirm-fail FIRST)

Mirror the #265 template in `crates/quarto-core/src/render_to_file.rs`
(`mod tests`): write a bare front-matter string, render end-to-end via
`render_to_file`, assert on the produced HTML. Each test MUST fail on the
base branch before its accessor is switched.

All 7 e2e tests added to `crates/quarto-core/src/render_to_file.rs` `mod tests`;
code-copy is a unit test in `code_block_generate.rs`.

- [x] `appendix-style`: `appendix-style: plain` + `::: {.appendix}` div →
      container `id="quarto-appendix" class="plain"`
      (`test_render_to_file_honors_appendix_style_plain`)
- [x] `license`: bare `license: CC BY-SA 4.0` → `quarto-reuse` section + text
      (`test_render_to_file_honors_license_string`)
- [x] `license.text`: `license:\n  text: My Custom Reuse Terms` → text appears
      (`test_render_to_file_honors_license_nested_text`)
- [x] `copyright`: bare `copyright: Copyright 2026 ACME Corp` → `quarto-copyright` + text
      (`test_render_to_file_honors_copyright_string`)
- [x] `citation.url`: `citation:\n  url: https://example.com/cite` →
      "For attribution" + url in output (`test_render_to_file_honors_citation_url`)
- [x] `toc: auto` (bare) → output contains generated TOC (`nav-link`)
      (`test_render_to_file_honors_toc_auto_string`)
- [x] `toc-title: My Custom Contents` (bare) → custom title replaces default
      (`test_render_to_file_honors_toc_title`)
- [x] `code-copy: always` as `PandocInlines` → `CopyMode::Always`
      (`resolve_default_copy_always_inlines_is_always`; unit test — no e2e
      observable, see SWITCH-table note)
- [x] Ran the new tests pre-fix: all 7 e2e + the code-copy unit test FAILED for
      the expected reason (`as_str()` → `None` → default). TDD step 2 ✓.

### Phase 2 — Switch accessors (one site at a time)
- [x] appendix.rs: appendix-style (80), license (278/285), copyright (327/334),
      citation.url (377) — all → `as_plain_text`, with bd-y89ihf0i comments.
      Map-form license/copyright still fall through correctly (`as_plain_text`
      returns `None` for `Map`, same as `as_str` did).
- [x] toc_generate.rs: 91 (`.as_plain_text().as_deref() == Some("auto")`), 117
- [x] code_block_generate.rs: 118 (string branch → `as_plain_text`)
- [x] shortcode_resolve.rs: 169 — **left as_str** (verified correct; switching
      would regress, see classification)
- [x] Re-ran Phase 1 tests post-fix: all 35 in scope pass. TDD step 4 ✓.

### Phase 3 — Lint rule (recurrence prevention)

**Decision (user, 2026-06-09): include the lint in this strand.**

New rule `crates/xtask/src/lint/metadata_as_str.rs` (syn-AST based, mirrors
`external_sources.rs`):
- [x] `check(path, content)` parses with `syn` and visits method calls.
- [x] Flags `<meta>.get("<lit>")…as_str()` in three shapes: direct
      `.as_str()`, `.and_then(|v| v.as_str())`, `.map(|v| v.as_str())`.
- [x] **Solved the false-positive problem with a receiver heuristic** rather
      than a naive textual scan: only flags when the `.get(<string literal>)`
      receiver is a *metadata expression* — a path/field whose final identifier
      is `meta`/`metadata` (the codebase convention: `meta`, `ast.meta`,
      `doc.ast.meta`). Internal `node.plain_data.get(..)`, `serde_json` reads,
      and attr maps are NOT flagged because their receiver isn't `meta`.
- [x] Skips `#[cfg(test)]` modules and `#[test]`/`#[tokio::test]` fns (test
      asserts legitimately read generated scalars).
- [x] Allowlist marker `// lint:allow(metadata-as-str)` on the line or the
      line above suppresses a deliberate scalar-only read.
- [x] Wired into `lint/mod.rs::check_file()`; 12 unit tests for the rule.
- [x] Documented in CLAUDE.md "Current Lint Rules".
- [x] `cargo xtask lint` passes clean (815 files) — **zero allowlist
      annotations needed** after the fixes, because the receiver heuristic +
      test-skipping leaves no production false positives.
- [x] Verified the lint *catches* a regression: temporarily reverting the
      toc-title fix made `cargo xtask lint` fail pointing at the exact line.

**The lint paid off immediately:** it surfaced a real bug *not* in the seed
inventory — `crates/quarto-core/src/format.rs:311`, `is_minimal_html` reading
`meta.get("theme").as_str()`. A bare `theme: none` / `theme: pandoc` was
silently not triggering minimal mode. Fixed (→ `as_plain_text`) with a
`PandocInlines` unit test (`format::tests::test_is_minimal_html_theme_none_as_inlines`),
revert-verified to fail without the fix. NOTE: the e2e effect is partly masked
— `theme: none` already suppresses Bootstrap CSS/JS via the independent
`ThemeConfig::suppress_bootstrap` path; the isolated `is_minimal_html` effect
is minimal-template selection. Unit test is the regression of record.

### Phase 4 — Full verification
- [x] `cargo build --workspace` — clean
- [x] `cargo nextest run --workspace` — 9580 passed, 196 skipped, 0 failed
- [x] `cargo xtask lint` — clean (815 files)
- [x] `cargo xtask verify` (full) — all 12 steps passed, including the WASM
      hub-client build + tests
- [x] End-to-end through the real `q2` binary (see record below)

## Notes / open items

- **shortcode_resolve.rs:169** is the least-certain SWITCH — confirm the value
  read really is user front-matter (vs. an internal inlines container) before
  changing it. If internal, move to LEAVE and note here.
- **`!str` escape tag** is a working-but-undocumented escape hatch. Out of
  scope here; the proper fix is the accessor switch. (A separate docs/behavior
  question — whether bare strings *should* parse as inlines in metadata — is
  not in this strand.)
- The `extract_plain_text` duplication noted in `metadata_normalize.rs:103`
  (bd-zzke consolidation candidate) is out of scope.

## End-to-end verification record

**Invocation** (2026-06-09): a single fixture exercising six fixed options, all
written as bare front-matter strings, rendered with the real binary:

```bash
cargo run --bin q2 -- render .tmp-e2e/audit.qmd
```

Fixture front matter:
```yaml
toc: auto
toc-title: On This Page
license: CC BY-SA 4.0
copyright: Copyright 2026 ACME Corp
appendix-style: plain
citation:
  url: https://example.com/cite
```

**Observed in the generated HTML (inspected via grep):**

| Option | Evidence in output |
|--------|--------------------|
| `toc-title` | `<h2 ... id="toc-title">On This Page</h2>` — and **no** "Table of Contents" |
| `toc: auto` | TOC rendered (`nav-link` anchors present) |
| `license` | `quarto-reuse` section containing `CC BY-SA 4.0` |
| `copyright` | `quarto-copyright` section containing `Copyright 2026 ACME Corp` |
| `appendix-style: plain` | `<div id="quarto-appendix" class="plain">` |
| `citation.url` | "For attribution, please cite this work as:" + link to `example.com/cite` |

All six render correctly with bare strings (they were silently dropped/defaulted
before the fix). Output was inspected directly; the temp fixture was removed.

`theme: none` (the lint-surfaced 7th fix) is covered by a unit test; its e2e
effect is masked by `ThemeConfig::suppress_bootstrap` (see Phase 3 note).

## Status

All phases complete. Net change:
- **7 accessor fixes** (`as_str` → `as_plain_text`): appendix-style, license
  (+nested text/type), copyright (+nested statement/holder), citation.url,
  toc:auto, toc-title, code-copy, theme.
- **9 regression tests** (7 e2e in `render_to_file.rs`, 1 unit in
  `code_block_generate.rs`, 1 unit in `format.rs`), each revert-verified to
  fail without its fix.
- **New `metadata-as-str` lint** (12 unit tests) + CLAUDE.md doc.
- **3 sites verified as correct-and-left**: `title_block.rs:132`,
  `metadata_normalize.rs:98`, `shortcode_resolve.rs:169`.

Remaining before merge: stage/commit, then request push approval (the branch
stacks on the open #265).
