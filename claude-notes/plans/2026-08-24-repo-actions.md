# Website `repo-actions` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Strand:** `bd-repo-actions-missing-99ezd2fe` (`discovered-from` the websites MVP epic `bd-0tr6`)

**Branch:** `braid/bd-repo-actions-missing-99ezd2fe-repo-actions`, branched from `main` at `596ceb572`

**Goal:** Render Quarto 1's `repo-actions` links ("Edit this page", "View source", "Report an issue") on website pages, in both of Q1's placements — the foot of the TOC and the page footer — so a site configuring `repo-actions` gets the links instead of silence.

**Architecture:** A pure model + HTML emitter in `quarto-navigation` (URL construction, no project context), consumed by one `Navigation`-phase transform in `quarto-core` that writes two rendered HTML strings into `rendered.navigation.*`. The TOC copy is picked up by the existing `toc-block` template partial (and its hand-maintained Rust twin); the footer copy is appended inside `.nav-footer-center` by `FooterRenderTransform`, which also gains the ability to synthesize a bare footer when none is configured. Same shape as the breadcrumbs port (`bd-breadcrumbs-missing-1vpuqh34`), which is the closest precedent in the tree.

**Tech Stack:** Rust; crates `quarto-navigation` (model + HTML), `quarto-core` (transform, template, pipeline), `quarto-error-catalog` (diagnostic codes); `docs/` Quarto site for the error pages.

**Spec:** No separate spec document — the design was settled in conversation and is recorded inline in **§ Design** and **§ Decisions** below. Executors should read both before starting Task 1.

---

## Global Constraints

- **Q1 reference implementation:** `external-sources/quarto-cli/src/project/types/website/website-navigation.ts` — `handleRepoLinks` (line 647) and `repoActionLinks` (line 830); config helpers in `website-config.ts` (`websiteRepoInfo` 227, `websiteRepoBranch` 255, `repoUrlIcon` 263, `websiteConfigActions` 271). Read these before changing URL construction.
- **Testing:** `cargo nextest run`, never `cargo test`. Never pipe nextest through `tail` — it hangs.
- **Per-task gate:** `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>`.
- **Per-phase gate:** `cargo nextest run --workspace` (~3 min) at each phase boundary and before any push. Report its pass/skip delta against the live baseline captured in Task 0.
- **Integration tests** go in `crates/<crate>/tests/integration/<name>.rs` and are registered in `tests/integration/main.rs` as `pub mod <name>;`, alphabetized. **Never** add a new top-level `tests/<name>.rs` — see `.claude/rules/integration-tests.md`.
- **`as_plain_text()`, never `as_str()`** when reading document metadata. A bare YAML string in front-matter context is `ConfigValueKind::PandocInlines`, for which `as_str()` returns `None`. The `metadata-as-str` lint rule enforces this.
- **Every new error code needs its docs page and sidebar entry in the same commit** — the `error-docs-page-missing` and `error-docs-sidebar-unlisted` lint rules both fail otherwise. Sidebar entries within a section must ascend by code number.
- **Async traits use `#[async_trait(?Send)]`** — see `.claude/rules/wasm.md`.
- **Cross-platform:** no `/` path literals; the source path is normalized to forward slashes by `page_relative_source`.
- **Commit at each clean phase boundary** without stopping to ask (approved-plan execution); **never push** without explicit permission.

---

## Design

### What Q1 does

`handleRepoLinks` is a DOM postprocessor. It builds **one** link list and appends the same `<div class="toc-actions">` in up to two places:

| target | Q1 selector | extra classes |
| --- | --- | --- |
| TOC | `nav[role="doc-toc"]` | — |
| footer | `.nav-footer .nav-footer-center` | `d-sm-block d-md-none`, **only when the TOC copy also landed** |

When `.nav-footer-center` does not exist, Q1 **synthesizes** `<footer class="footer"><div class="nav-footer"><div class="nav-footer-center">` purely to host the links. Verified in the repro: `_site-q1/index.html` has no `page-footer:` configured and still gets a footer.

URL construction:

| action | URL |
| --- | --- |
| `edit` | `{base}edit/{branch}/{subdir}{source}` |
| `edit` (`.ipynb` on github.com) | `{base with github.com→github.dev}blob/{branch}/{subdir}{source}` |
| `edit` (`.ipynb` elsewhere) | **dropped, silently** |
| `source` | `{base}blob/{branch}/{subdir}{source}` |
| `issue` | `issue-url` if set, else `{base}issues/new` |

where `base` = `repo-url` with a trailing slash, `subdir` = `repo-subdir` with a trailing slash (or empty), `branch` = `repo-branch` or `"main"`, and `source` = the input path relative to the project root.

Other rules: only the **first** link gets an icon (`bi-github` when the base contains `github.com`, else `bi-git`); every other link gets `<i class="bi empty">`. `issue-url` set but `issue` absent from the list → `issue` is appended anyway. `repo-link-target` / `repo-link-rel` become `target=` / `rel=` on every anchor. Link text comes from `repo-action-links-{edit,source,issue}` in the language files.

### What q2 already has

- `.toc-actions` / `.toc-action` SCSS: `resources/scss/bootstrap/_bootstrap-rules.scss:1816-1948`
- Bootstrap Icons shipped unconditionally for websites: `crates/quarto-core/src/transforms/website_bootstrap_icons.rs`
- `repo-action-links-*` in every `resources/language/_language*.yml`, reachable via `LanguageTerms::from_meta(&ast.meta)` then `.get(key)`
- `page_relative_source(ctx)` in `crates/quarto-core/src/transforms/navigation_active.rs:36` returns exactly the `source` Q1 needs, forward-slashed
- `quarto_config::resolve_website_value(meta, key) -> Option<ConfigValue>` (`crates/quarto-config/src/website.rs:39`) implements the top-level-overrides-`website.`-scope precedence, preserving `source_info` for diagnostics

### The two emission sites in q2

**TOC.** `<nav id="TOC" role="doc-toc">` is built in four places: `$toc-block()$` appears three times in `FULL_HTML_TEMPLATE` (`template.rs:293`, `304`, `326`) and `toc_block_html()` in `sidebar_render.rs:195` is a hand-maintained Rust twin for the website-left placement. **Exactly one fires per page** — verified against `TocLocationTransform` (`toc_location.rs:201-230`): `Right` sets no flag (template 304), `Left`+website sets `toc-relocated`+`toc-in-sidebar` (twin only), `Left`+standalone sets `toc-relocated`+`toc-left` (template 293), `Body` sets `toc-relocated`+`toc-body` (template 326). So adding the variable to `TOC_BLOCK_PARTIAL` and to the twin yields exactly one actions div, in every placement.

**The twin imposes a pipeline-ordering constraint that is easy to miss.** The three template sites read the variable at template time, long after every transform has run — but the Rust twin does not. `toc_block_html` is called by `SidebarRenderTransform` (`sidebar_render.rs:94-103`, its only caller), which is pushed at `pipeline.rs:1366`. So `RepoActionsRenderTransform` must run **before** `SidebarRenderTransform`, or the website-left placement silently gets no actions while the other three work — a failure invisible to any test that uses the default `toc-location: right`. Task 9 registers accordingly and Task 10 covers the left placement explicitly.

**Preview.** `build_q2_preview_transform_pipeline` (`pipeline.rs:1580`) derives from the full pipeline by *exclusion* (`Q2_PREVIEW_TRANSFORM_EXCLUDED`, `:1523`), so `repo-actions-render` is included in `q2 preview` automatically — no separate registration, and the footer copy appears there because `footer-render` is in the preview's chrome set. The **TOC** copy reaches the DOM only through `TOC_BLOCK_PARTIAL` / `toc_block_html`; whether hub-client's TOC rendering consumes `rendered.navigation.toc-actions` is **not established**. This plan does not attempt preview parity for the TOC copy; if the preview turns out to drop it, that is follow-up work, not a defect in this port. Do not add a preview-specific code path here.

**Footer.** `FooterRenderTransform` currently returns early when `navigation.footer` is absent, so a repo-actions-only site has no footer to append to. `FooterRegion` is `Empty | Text(ConfigValue) | Items(Vec<NavigationItem>)` with no raw-HTML variant, so the append needs a new `PageFooter` field.

### Path-resolution contract

Per `claude-notes/designs/path-resolution-model.md`, repo-action URLs consume the contract's **pivot form** — "project-root-relative canonical form with forward slashes", which is what `page_relative_source` returns — and then exit through *neither* documented exit: not a filesystem read, and not `page_url_for` (they are absolute external URLs, which rule 2's carve-out classifies as external before any path handling). That is a third exit with no row in the consumption-site inventory at line 148; Task 14 adds one. `repo-subdir` is **outside** the contract entirely: it is a path in the *repository's* namespace, never resolved against the project or the site. This does **not** need a `bd-oejuizi9` strand — the seam warning covers decrees, fixes, and audits scoped to one space, and this is a new consumer.

---

## Decisions

Each of these was settled deliberately. Do not silently revisit them; if one looks wrong during execution, stop and raise it.

- **D-1 — Scope: full parity minus DOM-only bits.** In scope: `edit` / `source` / `issue`; `repo-url`, `repo-branch`, `repo-subdir`, `issue-url`, `repo-link-target`, `repo-link-rel`; the `github.dev` `.ipynb` case; first-link-only icons; page-level `repo-actions: false`. Out of scope: `data-quarto-source-url="repo"` rewriting (`website-navigation.ts:814`) — it rewrites an attribute on markup Q1's DOM postprocessor emits for embedded notebooks; q2 emits no such attribute, so nothing is dropped and no diagnostic is warranted. Note it in the module doc; file nothing.

- **D-2 — Footer synthesis: yes.** When `navigation.footer` is absent but repo-actions produced markup, `FooterRenderTransform` builds a `PageFooter` with only `center_append` set. This is what makes the repro match Q1 byte-for-byte.

- **D-3 — `page-footer: false` suppresses the footer copy, at *either* scope (deliberate divergence).** Q1's `handleRepoLinks` never checks `page-footer`, so with `page-footer: false` it synthesizes a footer anyway — arguably a Q1 bug. q2 honours the disable. Signed off.

  **This does not fall out for free, and the obvious implementation gets it wrong.** `FooterRenderTransform`'s existing gate is `is_feature_disabled(&ast.meta, "page-footer")`, which reads the **top level only** (`transforms/config.rs:23`). Website-scoped `page-footer: false` never reaches it — it is handled further down by `resolve_page_footer`, which goes through `resolve_website_value` and returns `None` (`quarto-navigation/src/footer.rs:246-250`), leaving `navigation.footer` **absent**. Since D-2's synthesis branch fires precisely on "absent", a naive implementation would synthesize a footer for exactly the config that asked for none. Task 8 therefore gates synthesis on the **website-aware** form. Both scopes must be tested; Task 8 and Task 10 each cover both.

  The TOC copy is unaffected at either scope.

- **D-4 — Page-level `repo-actions: true` is not supported; `false` is.** In Q1, `forceRepoActions` cannot enable the feature — the action list always comes from `website.repo-actions` — its only effect is `if (repoTargets.length === 0 && forceRepoActions)`, falling back to `#quarto-margin-sidebar` on a page with no TOC. Supporting it would require duplicating the `#quarto-margin-sidebar` div across an if/else in `FULL_HTML_TEMPLATE`'s hottest conditional **and** suppressing the `fullcontent` body class (`template.rs:903-915`), whose margin segments sum to ~0.28 × margin-width (~70px) and would squash the links. Disproportionate for a placement preference. Degradation is graceful: the page still shows all three links in the footer at every width. Announced via `Q-13-13` (severity `info`).

- **D-5 — No diagnostic for the non-GitHub `.ipynb` edit drop; match Q1's silence.** Git archaeology shows the suppression is *deliberate*, not an oversight: `5c2186680` ("dont show edit source for ipynb", JJ Allaire, 2022-04-13 09:50) suppresses `edit` for all notebooks, and `967197b12` ("generate notebook edit urls for github.com", same author, 10:54 the same morning) carves out GitHub because `github.dev` can edit a notebook where GitHub's plain `/edit/` cannot. The rule is "don't offer an edit link that won't work"; what is stale is its allowlist. q2 also has no warn-once machinery, so a diagnostic would fire once per notebook page. The real gap is multi-host support — Task 15 files a strand referencing Q1 issue [#5301](https://github.com/quarto-dev/quarto-cli/issues/5301) (open; dups [#7155](https://github.com/quarto-dev/quarto-cli/issues/7155), [#12138](https://github.com/quarto-dev/quarto-cli/issues/12138)).

- **D-6 — Action-list scope is `website.` only; string keys use the merged helper.** `repo-actions` is read from `website.repo-actions` **only**, matching Q1's `websiteConfigActions(key, kWebsite, config)`. This is required, not stylistic: the top-level slot is where page-level `repo-actions: true/false` lands, so merging the two scopes would collide a bool with an array. Every other key (`repo-url`, `repo-branch`, `repo-subdir`, `issue-url`, `repo-link-target`, `repo-link-rel`) goes through `resolve_website_value`, which allows a front-matter override. Q1 permits a per-page override only for `repo-url`; widening it to the sibling keys is a deliberate, documented convenience.

- **D-7 — `none` anywhere in the list clears it (deliberate divergence).** Q1 only special-cases `none` in the *string* form; `repo-actions: [none]` reaches Q1's `default:` branch and warns "Unknown repo action 'none'". But `[none]` is schema-legal (`definitions.yml:705` declares `maybeArrayOf: enum [none, edit, source, issue]`), and warning on schema-legal input is bad behavior. q2 treats `none` as clearing the list wherever it appears. Task 2 tests this.

  **`none` clears the list outright — it also suppresses `issue-url`'s forced
  link (revised 2026-08-25, deliberate divergence).** Q1 pushes `issue` onto the
  action list unconditionally whenever `issue-url` is set, immediately after
  `websiteConfigActions` has returned `[]` for `none`
  (`website-navigation.ts:661-670`), so Q1 renders one issue link for
  `none` + `issue-url`. That is the same statement-ordering accident as D-8, not
  a design choice. Replicating it would contradict this very decision in the
  same breath: D-7 already holds that `none` states the author's intent rather
  than being something to warn about. `none` says "no repo action links";
  `issue-url` says *where* the tracker is, not *whether* to render a link to it.
  So `none` wins.

  The `issue-url` convenience still applies where it was meant to: `issue-url`
  set with **no** action list of its own still yields one issue link. That case
  is unambiguous — `issue-url` has exactly one consumer in the tree, so setting
  it and nothing else does mean "give me the issue link". Both halves are tested
  (`none_clears_the_list_even_when_issue_url_is_set`,
  `issue_url_alone_still_forces_an_issue_link`).

- **D-8 — Icons go to the first *surviving* link (revised 2026-08-25, deliberate divergence).** Q1 does `actions.map((action, i) => …icon: i === 0 ? firstIcon : undefined).filter(non-null)`, so if `actions[0]` is `edit` and it gets dropped (notebook on a non-GitHub host), *no* surviving link carries an icon — every one gets the empty spacer. That is an ordering accident between `map` and `filter`, not a design choice: there is no reader who benefits from an icon-less action list, and the case is invisible in every other configuration. q2 keys the icon to the first link that actually survives. Task 2 tests it (`dropped_first_action_still_leaves_an_icon_on_the_survivor`).

  *Originally* this plan replicated the Q1 behaviour on parity grounds. Revised after review: the branch already diverges deliberately where Q1's behaviour is a bug (D-3, D-7), and this is also a bug, just a cosmetic one. Byte-identity with Q1 on the reference repro is unaffected — that project has no notebook pages.

- **D-9 — Link text is not HTML-escaped.** Q1 assigns `a.innerHTML = link.text`. q2's own `toc_block_html` interpolates the `toc-title` term unescaped too (`sidebar_render.rs:207`). Match both. URLs and class attributes **are** escaped via `escape_attr`.

- **D-10 — The `issue-url`-without-`repo-url` link carries `bi-chat-right`.** Q1 short-circuits `repoActionLinks` entirely in this case (`website-navigation.ts:758-771`): with no `repoInfo` it emits a single hand-built issue link with `icon: "chat-right"`, not the `github`/`git` icon the normal path would pick. Parity, and the icon is not DOM-only, so replicate. Task 2 tests it.

- **D-11 — `Q-13-13` fires only on a page with no TOC.** In Q1, `forceRepoActions` is consulted at exactly one place — `if (repoTargets.length === 0 && forceRepoActions)` (`website-navigation.ts:685-691`) — which can only be true when there is no TOC. On a page *with* a TOC, `repo-actions: true` is a no-op in Q1 as well, so q2 and Q1 already agree and there is nothing to report. Firing the diagnostic there would tell the reader something false about their page (the docs page says "a page with no table of contents…"). Gate the diagnostic on `!has_toc`, which means computing `has_toc` before the page-flag branch.

  One further consequence, deliberate: the `actions.is_empty()` early return sits *above* this gate, so a page writing `repo-actions: true` on a site with **no** `website.repo-actions` gets no diagnostic. That matches Q1 (`repoActions` empty → `handleRepoLinks` does nothing at all) and is the right message discipline — the author's problem there is the missing site config, not the ignored `true`, and saying "your `true` was ignored" would point at the wrong line. Do not "fix" this by hoisting the diagnostic above the early return.

  Note the early return's condition is `actions.is_empty() && issue_url.is_none()`,
  not `actions.is_empty()` alone (see Task 6). A site configuring only `issue-url`
  therefore *does* reach this gate, and `repo-actions: true` on such a page with no
  TOC *does* report `Q-13-13` — correctly, because Q1's `repoActions` is non-empty
  there too (the `issue` push ran) and so Q1 does consult `forceRepoActions`.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `crates/quarto-navigation/src/repo_actions.rs` *(create)* | `RepoActionsConfig`, `RepoActionLink`, `RepoActionLabels`, `RepoActionWarning`, `repo_action_links()`. Pure: no project context, no I/O. |
| `crates/quarto-navigation/src/render_html.rs` *(modify)* | `repo_actions_to_html()`; `render_footer_region()` restructured to accept an append. |
| `crates/quarto-navigation/src/footer.rs` *(modify)* | `PageFooter::center_append` field. |
| `crates/quarto-navigation/src/lib.rs` *(modify)* | Re-export the new public items. |
| `crates/quarto-core/src/transforms/repo_actions_render.rs` *(create)* | `RepoActionsRenderTransform`: config resolution, language terms, diagnostics, writes the two `rendered.navigation.*` slots. |
| `crates/quarto-core/src/transforms/mod.rs` *(modify)* | Register the module and re-export the transform. |
| `crates/quarto-core/src/transforms/footer_render.rs` *(modify)* | Consume `footer-actions`; synthesize a footer when none is configured. |
| `crates/quarto-core/src/transforms/sidebar_render.rs` *(modify)* | `toc_block_html()` twin emits the TOC copy. |
| `crates/quarto-core/src/template.rs` *(modify)* | `TOC_BLOCK_PARTIAL` emits the TOC copy; doc-comment the new variables. |
| `crates/quarto-core/src/pipeline.rs` *(modify)* | Register the transform between `TocLocationTransform` and `NavbarRenderTransform` — **before** `SidebarRenderTransform`; see Task 9 for why that bound is load-bearing. |
| `crates/quarto-core/tests/integration/repo_actions_pipeline.rs` *(create)* | End-to-end-through-`ProjectPipeline` tests. |
| `crates/quarto-core/tests/integration/main.rs` *(modify)* | `pub mod repo_actions_pipeline;` |
| `crates/quarto/tests/smoke-all/repo-actions/` *(create)* | Smoke-all fixtures driving the real binary's document path — a happy-path page plus one per diagnostic, each carrying its own assertions. Two diagnostics need their own nested project dir; see Task 11. |
| `crates/quarto-error-catalog/error_catalog.json` *(modify)* | `Q-13-11`, `Q-13-12`, `Q-13-13`. |
| `docs/errors/navigation/Q-13-{11,12,13}.qmd` *(create)* | One reference page per code. |
| `docs/_quarto.yml` *(modify)* | Three sidebar entries after `Q-13-10`. |
| `docs/guides/projects/repo-actions.qmd` *(create)* | User-facing documentation. Sibling precedent: `docs/guides/projects/breadcrumbs.qmd`, from the breadcrumbs port. |
| `docs/_quarto.yml` *(modify, second site)* | The hand-maintained `guides/projects/*` sidebar list (lines 32-42) — **no lint rule covers this section**, so an unlisted page is simply unreachable. |
| `claude-notes/designs/path-resolution-model.md` *(modify)* | One inventory row for the external-URL exit. |

---

## Phase 1 — Model and HTML emitter (`quarto-navigation`)

### Task 0: Baseline

**Files:** none (measurement only)

- [x] **Step 1: Capture the live workspace-test baseline**

Run: `cargo nextest run --workspace`
Record the pass/skip counts here so later phase gates can be compared against a real number, not one copied from an older document:

```
Baseline (2026-08-24, worktree workspace-2 @ 8f84c4f5e): 13130 passed, 199 skipped
```

- [x] **Step 2: Confirm the defect reproduces**

Build the binary first and address it absolutely — `cargo run` searches upward from the current directory for a `Cargo.toml`, so it cannot work from a scratch directory outside the repo:

```bash
cargo build --bin q2                      # from the worktree
Q2=$(git rev-parse --show-toplevel)/target/debug/q2

mkdir -p /tmp/q2-repro-repo-actions && cd /tmp/q2-repro-repo-actions
cat > _quarto.yml <<'YAML'
project:
  type: website
website:
  title: "Repo actions"
  repo-url: https://github.com/example/example-docs
  repo-branch: main
  repo-actions: [edit, source, issue]
format:
  html:
    toc: true
YAML
printf -- '---\ntitle: Home\n---\n\n## First section\n\nText.\n\n## Second section\n\nText.\n' > index.qmd
"$Q2" render
grep -c "Edit this page\|View source\|Report an issue" _site/index.html
```

Expected: exit 0, and the grep prints `0`. This is the silence the strand reports. Keep this directory and the `$Q2` path — **Task 12** reuses both.

### Task 1: `RepoActionsConfig` / `RepoActionLink` types

**Files:**
- Create: `crates/quarto-navigation/src/repo_actions.rs`
- Modify: `crates/quarto-navigation/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `RepoActionsConfig`, `RepoActionLink`, `RepoActionLabels`, `RepoActionWarning`, all `pub` from `quarto_navigation::repo_actions` and re-exported at the crate root.

- [x] **Step 1: Write the module with types only**

```rust
/*
 * repo_actions.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Repository action links — "Edit this page", "View source",
//! "Report an issue" — for website pages
//! (bd-repo-actions-missing-99ezd2fe).
//!
//! This module owns the *model* and *URL construction* only: it takes
//! resolved configuration plus the page's project-root-relative source
//! path and returns the links to render. It has no project context and
//! does no I/O, so it unit-tests standalone. Config resolution,
//! localization, and diagnostics live in `quarto-core`'s
//! `RepoActionsRenderTransform`; HTML emission lives in
//! [`crate::render_html::repo_actions_to_html`].
//!
//! Q1 parity: `website-navigation.ts::repoActionLinks` (line 830) and
//! `website-config.ts::{websiteRepoInfo, websiteRepoBranch,
//! repoUrlIcon}`.
//!
//! Deliberately **not** ported: the `data-quarto-source-url="repo"`
//! attribute rewrite (`website-navigation.ts:814`). It rewrites an
//! attribute on markup Q1's DOM postprocessor emits for embedded
//! notebooks; q2 emits no such attribute, so nothing is dropped.

/// Repository coordinates and the action list, already resolved from
/// configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoActionsConfig {
    /// `website.repo-url`. Without it only an `issue-url`-backed
    /// issue link can be built.
    pub repo_url: Option<String>,
    /// `website.repo-branch`, defaulted to `"main"` by the caller.
    pub branch: String,
    /// `website.repo-subdir` — the project's directory *within the
    /// repository*. Not a project path; see the path-resolution note
    /// in the plan.
    pub subdir: Option<String>,
    /// `website.issue-url`, overriding `{base}issues/new`.
    pub issue_url: Option<String>,
    /// Action names in author order, `none` already applied.
    pub actions: Vec<String>,
    /// `website.repo-link-target` → `target=` on every anchor.
    pub link_target: Option<String>,
    /// `website.repo-link-rel` → `rel=` on every anchor.
    pub link_rel: Option<String>,
}

/// One rendered action link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoActionLink {
    /// Display text, from the language terms. Emitted unescaped
    /// (Q1 assigns `a.innerHTML`); see decision D-9.
    pub text: String,
    pub url: String,
    /// Bootstrap icon suffix without the `bi-` prefix. `None` renders
    /// `<i class="bi empty">`.
    pub icon: Option<String>,
}

/// Localized link labels (`repo-action-links-*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoActionLabels {
    pub edit: String,
    pub source: String,
    pub issue: String,
}

impl Default for RepoActionLabels {
    /// The English defaults from `resources/language/_language.yml`,
    /// used when no language terms are attached (standalone renders).
    fn default() -> Self {
        Self {
            edit: "Edit this page".to_string(),
            source: "View source".to_string(),
            issue: "Report an issue".to_string(),
        }
    }
}

/// Something the caller should tell the author about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoActionWarning {
    /// An action name outside `{none, edit, source, issue}`.
    UnknownAction(String),
    /// Actions were requested but no `repo-url` or `issue-url` exists,
    /// so no link can be built.
    NoRepoUrl,
}
```

- [x] **Step 2: Declare the module and export the types**

In `crates/quarto-navigation/src/lib.rs`, add `pub mod repo_actions;` alphabetically among the `pub mod` lines (after `pub mod page_nav;`), and add to the re-exports:

```rust
pub use repo_actions::{RepoActionLabels, RepoActionLink, RepoActionWarning, RepoActionsConfig};
```

**Export only the types here.** `repo_action_links` is re-exported in Task 2, alongside the function it names — naming it now would make this commit fail to build, breaking `git bisect` and per-commit CI for no benefit.

- [x] **Step 3: Verify the tree still builds, then commit**

Run: `cargo build -p quarto-navigation`
Expected: clean. Every commit in this plan compiles.

```bash
git add crates/quarto-navigation/src/repo_actions.rs crates/quarto-navigation/src/lib.rs
git commit -m "Add repo-action model types (bd-repo-actions-missing-99ezd2fe)"
```

### Task 2: `repo_action_links()` URL construction

**Files:**
- Modify: `crates/quarto-navigation/src/repo_actions.rs`

**Interfaces:**
- Consumes: the Task 1 types.
- Produces: `pub fn repo_action_links(cfg: &RepoActionsConfig, source: &str, labels: &RepoActionLabels) -> (Vec<RepoActionLink>, Vec<RepoActionWarning>)`.

- [x] **Step 1: Write the failing tests**

Append to `crates/quarto-navigation/src/repo_actions.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(actions: &[&str]) -> RepoActionsConfig {
        RepoActionsConfig {
            repo_url: Some("https://github.com/example/docs".to_string()),
            branch: "main".to_string(),
            actions: actions.iter().map(|a| (*a).to_string()).collect(),
            ..RepoActionsConfig::default()
        }
    }

    fn urls(links: &[RepoActionLink]) -> Vec<&str> {
        links.iter().map(|l| l.url.as_str()).collect()
    }

    #[test]
    fn builds_all_three_actions() {
        let (links, warns) = repo_action_links(
            &cfg(&["edit", "source", "issue"]),
            "index.qmd",
            &RepoActionLabels::default(),
        );
        assert!(warns.is_empty());
        assert_eq!(
            urls(&links),
            vec![
                "https://github.com/example/docs/edit/main/index.qmd",
                "https://github.com/example/docs/blob/main/index.qmd",
                "https://github.com/example/docs/issues/new",
            ]
        );
    }

    #[test]
    fn trailing_slash_on_repo_url_does_not_double() {
        let mut c = cfg(&["source"]);
        c.repo_url = Some("https://github.com/example/docs/".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(urls(&links), vec!["https://github.com/example/docs/blob/main/a.qmd"]);
    }

    #[test]
    fn subdir_is_prepended_to_the_source_path() {
        let mut c = cfg(&["edit"]);
        c.subdir = Some("website".to_string());
        let (links, _) = repo_action_links(&c, "guide/intro.qmd", &RepoActionLabels::default());
        assert_eq!(
            urls(&links),
            vec!["https://github.com/example/docs/edit/main/website/guide/intro.qmd"]
        );
    }

    #[test]
    fn branch_is_used_verbatim() {
        let mut c = cfg(&["source"]);
        c.branch = "gh-pages".to_string();
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(urls(&links), vec!["https://github.com/example/docs/blob/gh-pages/a.qmd"]);
    }

    #[test]
    fn issue_url_overrides_the_default_issue_target() {
        let mut c = cfg(&["issue"]);
        c.issue_url = Some("https://github.com/example/product/issues/".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(urls(&links), vec!["https://github.com/example/product/issues/"]);
    }

    /// Q1 `handleRepoLinks`: an `issue-url` forces an issue link even
    /// when `issue` is absent from `repo-actions`.
    #[test]
    fn issue_url_appends_issue_when_not_requested() {
        let mut c = cfg(&["edit"]);
        c.issue_url = Some("https://example.com/bugs".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(links.len(), 2);
        assert_eq!(links[1].url, "https://example.com/bugs");
    }

    /// …and does not duplicate it when `issue` *is* requested.
    #[test]
    fn issue_url_does_not_duplicate_a_requested_issue() {
        let mut c = cfg(&["issue"]);
        c.issue_url = Some("https://example.com/bugs".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn notebook_edit_uses_github_dev() {
        let (links, _) = repo_action_links(
            &cfg(&["edit"]),
            "notebooks/demo.ipynb",
            &RepoActionLabels::default(),
        );
        assert_eq!(
            urls(&links),
            vec!["https://github.dev/example/docs/blob/main/notebooks/demo.ipynb"]
        );
    }

    /// Deliberate Q1 parity (decision D-5): a notebook on a non-GitHub
    /// host drops the edit action with no warning.
    #[test]
    fn notebook_edit_is_dropped_on_non_github_hosts() {
        let mut c = cfg(&["edit", "source"]);
        c.repo_url = Some("https://gitlab.com/example/docs".to_string());
        let (links, warns) = repo_action_links(&c, "demo.ipynb", &RepoActionLabels::default());
        assert_eq!(urls(&links), vec!["https://gitlab.com/example/docs/blob/main/demo.ipynb"]);
        assert!(warns.is_empty(), "the drop is silent by design");
    }

    #[test]
    fn only_the_first_link_gets_an_icon() {
        let (links, _) = repo_action_links(
            &cfg(&["edit", "source", "issue"]),
            "a.qmd",
            &RepoActionLabels::default(),
        );
        assert_eq!(links[0].icon.as_deref(), Some("github"));
        assert_eq!(links[1].icon, None);
        assert_eq!(links[2].icon, None);
    }

    #[test]
    fn non_github_host_gets_the_generic_git_icon() {
        let mut c = cfg(&["source"]);
        c.repo_url = Some("https://gitlab.com/example/docs".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(links[0].icon.as_deref(), Some("git"));
    }

    /// Decision D-8 — the icon goes to the first *surviving* link, not
    /// to the pre-filter index as Q1 does.
    #[test]
    fn dropped_first_action_still_leaves_an_icon_on_the_survivor() {
        let mut c = cfg(&["edit", "source"]);
        c.repo_url = Some("https://gitlab.com/example/docs".to_string());
        let (links, _) = repo_action_links(&c, "demo.ipynb", &RepoActionLabels::default());
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].icon.as_deref(), Some("git"));
    }

    /// Decision D-7 — divergence from Q1, which warns on `[none]`.
    #[test]
    fn none_in_the_list_clears_it() {
        let (links, warns) = repo_action_links(
            &cfg(&["edit", "none", "source"]),
            "a.qmd",
            &RepoActionLabels::default(),
        );
        assert!(links.is_empty());
        assert!(warns.is_empty());
    }

    #[test]
    fn unknown_action_warns_and_is_skipped() {
        let (links, warns) = repo_action_links(
            &cfg(&["edit", "publish"]),
            "a.qmd",
            &RepoActionLabels::default(),
        );
        assert_eq!(links.len(), 1);
        assert_eq!(warns, vec![RepoActionWarning::UnknownAction("publish".to_string())]);
    }

    #[test]
    fn missing_repo_url_warns_and_yields_nothing() {
        let mut c = cfg(&["edit", "source"]);
        c.repo_url = None;
        let (links, warns) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert!(links.is_empty());
        assert_eq!(warns, vec![RepoActionWarning::NoRepoUrl]);
    }

    /// An `issue-url` alone is enough — no `repo-url` needed. Decision
    /// D-10: Q1 short-circuits and hand-builds this link with the
    /// `chat-right` icon rather than the usual github/git one.
    #[test]
    fn issue_url_alone_builds_a_chat_right_issue_link_without_repo_url() {
        let mut c = cfg(&[]);
        c.repo_url = None;
        c.issue_url = Some("https://example.com/bugs".to_string());
        let (links, warns) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(urls(&links), vec!["https://example.com/bugs"]);
        assert_eq!(links[0].icon.as_deref(), Some("chat-right"));
        assert!(warns.is_empty());
    }

    /// …but with a `repo-url` present the normal path runs, so the
    /// first link gets the host icon, not `chat-right`.
    #[test]
    fn issue_link_uses_the_host_icon_when_repo_url_is_present() {
        let mut c = cfg(&["issue"]);
        c.issue_url = Some("https://example.com/bugs".to_string());
        let (links, _) = repo_action_links(&c, "a.qmd", &RepoActionLabels::default());
        assert_eq!(links[0].icon.as_deref(), Some("github"));
    }

    #[test]
    fn empty_action_list_yields_nothing_and_no_warning() {
        let (links, warns) = repo_action_links(&cfg(&[]), "a.qmd", &RepoActionLabels::default());
        assert!(links.is_empty());
        assert!(warns.is_empty());
    }

    #[test]
    fn labels_come_from_the_supplied_terms() {
        let labels = RepoActionLabels {
            edit: "Modifier".to_string(),
            source: "Source".to_string(),
            issue: "Signaler".to_string(),
        };
        let (links, _) = repo_action_links(&cfg(&["edit"]), "a.qmd", &labels);
        assert_eq!(links[0].text, "Modifier");
    }
}
```

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p quarto-navigation repo_actions`
Expected: FAIL to compile — `repo_action_links` is not defined.

- [x] **Step 3: Implement**

Insert above the `#[cfg(test)]` block in `crates/quarto-navigation/src/repo_actions.rs`:

```rust
/// Append a trailing slash if one is missing. Q1's `ensureTrailingSlash`.
fn ensure_trailing_slash(s: &str) -> String {
    if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{s}/")
    }
}

/// Q1 `repoUrlIcon`: GitHub gets its own mark, everything else the
/// generic git one.
fn repo_url_icon(base: &str) -> &'static str {
    if base.contains("github.com") { "github" } else { "git" }
}

/// Build the repository action links for one page.
///
/// `source` is the page's project-root-relative path with forward
/// slashes — exactly what `page_relative_source` returns in
/// `quarto-core`.
///
/// Returns the links in author order plus any warnings the caller
/// should surface. An empty `Vec` of links with an empty `Vec` of
/// warnings means "nothing configured", not "something failed".
pub fn repo_action_links(
    cfg: &RepoActionsConfig,
    source: &str,
    labels: &RepoActionLabels,
) -> (Vec<RepoActionLink>, Vec<RepoActionWarning>) {
    let mut warnings = Vec::new();

    // Decision D-7: `none` anywhere clears the list. Q1 only handles
    // the scalar form and warns on `[none]`, which is schema-legal.
    let mut actions: Vec<String> = if cfg.actions.iter().any(|a| a == "none") {
        Vec::new()
    } else {
        cfg.actions.clone()
    };

    // Q1 `handleRepoLinks`: an explicit `issue-url` forces an issue
    // link even when the author did not list `issue`. Kept for the case
    // it was meant for, but NOT over an explicit `none` — see D-7.
    let cleared_by_none = cfg.actions.iter().any(|a| a == "none");
    if !cleared_by_none && cfg.issue_url.is_some() && !actions.iter().any(|a| a == "issue") {
        actions.push("issue".to_string());
    }

    if actions.is_empty() {
        return (Vec::new(), warnings);
    }

    // `let … else` rather than an `is_none()` check plus an `expect()`
    // further down: it binds the unwrapped `String` once and leaves no
    // panic path behind.
    let Some(base) = cfg.repo_url.as_deref().map(ensure_trailing_slash) else {
        let Some(issue_url) = cfg.issue_url.as_deref() else {
            // Q1 `warnOnce("Repository links require that you specify
            // a repo-url")` — nothing can be built.
            warnings.push(RepoActionWarning::NoRepoUrl);
            return (Vec::new(), warnings);
        };
        // Decision D-10: with no repo info Q1 bypasses this function
        // entirely (`website-navigation.ts:758-771`) and hand-builds a
        // single issue link with the `chat-right` icon. Same result,
        // expressed as an early return so the caller stays simple.
        return (
            vec![RepoActionLink {
                text: labels.issue.clone(),
                url: issue_url.to_string(),
                icon: Some("chat-right".to_string()),
            }],
            warnings,
        );
    };
    let base = base.as_str();

    let path = cfg
        .subdir
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(ensure_trailing_slash)
        .unwrap_or_default();
    let first_icon = repo_url_icon(base);
    let is_notebook = source.ends_with(".ipynb");
    let branch = &cfg.branch;

    let mut links = Vec::new();
    for action in actions.iter() {
        // Decision D-8: the icon goes to the first *surviving* link.
        // Q1 keys it to the index in the unfiltered list, so a dropped
        // first action leaves every survivor icon-less — a map/filter
        // ordering accident we deliberately do not replicate.
        let icon = if links.is_empty() { Some(first_icon.to_string()) } else { None };

        let link = match action.as_str() {
            "edit" => {
                if !is_notebook {
                    Some(RepoActionLink {
                        text: labels.edit.clone(),
                        url: format!("{base}edit/{branch}/{path}{source}"),
                        icon,
                    })
                } else if base.contains("github.com") {
                    // github.dev can edit a notebook; github.com's
                    // plain `/edit/` web editor shows raw JSON.
                    Some(RepoActionLink {
                        text: labels.edit.clone(),
                        url: format!(
                            "{}blob/{branch}/{path}{source}",
                            base.replace("github.com", "github.dev")
                        ),
                        icon,
                    })
                } else {
                    // Decision D-5: deliberate, silent. Q1 commit
                    // 5c2186680 suppresses notebook edit links;
                    // 967197b12 carves out GitHub only.
                    None
                }
            }
            "source" => Some(RepoActionLink {
                text: labels.source.clone(),
                url: format!("{base}blob/{branch}/{path}{source}"),
                icon,
            }),
            "issue" => Some(RepoActionLink {
                text: labels.issue.clone(),
                url: cfg
                    .issue_url
                    .clone()
                    .unwrap_or_else(|| format!("{base}issues/new")),
                icon,
            }),
            other => {
                warnings.push(RepoActionWarning::UnknownAction(other.to_string()));
                None
            }
        };

        if let Some(link) = link {
            links.push(link);
        }
    }

    (links, warnings)
}
```

- [x] **Step 4: Re-export the function**

Now that it exists, extend the `pub use` in `crates/quarto-navigation/src/lib.rs` (from Task 1 Step 2) to name it:

```rust
pub use repo_actions::{
    RepoActionLabels, RepoActionLink, RepoActionWarning, RepoActionsConfig, repo_action_links,
};
```

- [x] **Step 5: Run tests to verify they pass**

Run: `cargo nextest run -p quarto-navigation repo_actions`
Expected: PASS, 19 tests.

- [x] **Step 6: Gate and commit**

```bash
cargo clippy -p quarto-navigation --all-targets -- -D warnings
git add crates/quarto-navigation/src/repo_actions.rs crates/quarto-navigation/src/lib.rs
git commit -m "Build repo-action URLs with Q1's rules (bd-repo-actions-missing-99ezd2fe)"
```

### Task 3: `repo_actions_to_html()`

**Files:**
- Modify: `crates/quarto-navigation/src/render_html.rs`

**Interfaces:**
- Consumes: `RepoActionLink` from Task 1.
- Produces: `pub fn repo_actions_to_html(links: &[RepoActionLink], extra_classes: &[&str], link_target: Option<&str>, link_rel: Option<&str>) -> String`.

- [x] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` in `crates/quarto-navigation/src/render_html.rs`:

```rust
fn action(text: &str, url: &str, icon: Option<&str>) -> crate::repo_actions::RepoActionLink {
    crate::repo_actions::RepoActionLink {
        text: text.to_string(),
        url: url.to_string(),
        icon: icon.map(str::to_string),
    }
}

#[test]
fn repo_actions_html_matches_q1_shape() {
    let links = vec![
        action("Edit this page", "https://github.com/e/d/edit/main/index.qmd", Some("github")),
        action("View source", "https://github.com/e/d/blob/main/index.qmd", None),
    ];
    let html = repo_actions_to_html(&links, &[], None, None);
    assert_eq!(
        html,
        "<div class=\"toc-actions\"><ul>\
         <li><a href=\"https://github.com/e/d/edit/main/index.qmd\" class=\"toc-action\">\
         <i class=\"bi bi-github\"></i>Edit this page</a></li>\
         <li><a href=\"https://github.com/e/d/blob/main/index.qmd\" class=\"toc-action\">\
         <i class=\"bi empty\"></i>View source</a></li>\
         </ul></div>"
    );
}

#[test]
fn repo_actions_html_applies_extra_classes() {
    let links = vec![action("Edit this page", "https://x/e", Some("github"))];
    let html = repo_actions_to_html(&links, &["d-sm-block", "d-md-none"], None, None);
    assert!(html.starts_with("<div class=\"toc-actions d-sm-block d-md-none\">"));
}

#[test]
fn repo_actions_html_emits_target_and_rel() {
    let links = vec![action("Edit this page", "https://x/e", None)];
    let html = repo_actions_to_html(&links, &[], Some("_blank"), Some("noopener"));
    assert!(html.contains("href=\"https://x/e\" target=\"_blank\" rel=\"noopener\" class=\"toc-action\""));
}

#[test]
fn repo_actions_html_escapes_the_url() {
    let links = vec![action("Edit", "https://x/a\"b", None)];
    let html = repo_actions_to_html(&links, &[], None, None);
    assert!(!html.contains("a\"b"), "quote must be escaped in the href");
    // The negative alone is vacuously satisfied by an empty string (and by a
    // mutant that strips quotes instead of escaping them), so pin the
    // positive form too.
    assert!(
        html.contains("href=\"https://x/a&quot;b\""),
        "the escaped form must actually appear in the href"
    );
}

#[test]
fn repo_actions_html_is_empty_for_no_links() {
    assert_eq!(repo_actions_to_html(&[], &["d-md-none"], None, None), "");
}
```

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p quarto-navigation repo_actions_html`
Expected: FAIL to compile — `repo_actions_to_html` is not defined.

- [x] **Step 3: Implement**

Add to `crates/quarto-navigation/src/render_html.rs`, next to `breadcrumbs_to_html`:

```rust
/// Render the repository-action links as Q1's `.toc-actions` block.
///
/// `extra_classes` are appended to the wrapper's class list — the
/// footer copy carries `d-sm-block d-md-none` so it is the
/// small-screen fallback for the TOC copy.
///
/// Link text is emitted **unescaped**: it comes from the language
/// terms, and Q1 assigns it via `a.innerHTML`. q2's own
/// `toc_block_html` interpolates the `toc-title` term the same way.
/// URLs and classes are escaped.
///
/// Returns an empty string for an empty link list, so callers can
/// store the result unconditionally and gate on emptiness.
pub fn repo_actions_to_html(
    links: &[crate::repo_actions::RepoActionLink],
    extra_classes: &[&str],
    link_target: Option<&str>,
    link_rel: Option<&str>,
) -> String {
    if links.is_empty() {
        return String::new();
    }

    let mut class = String::from("toc-actions");
    for extra in extra_classes {
        class.push(' ');
        class.push_str(extra);
    }

    let mut html = format!("<div class=\"{}\"><ul>", escape_attr(&class));
    for link in links {
        html.push_str("<li><a href=\"");
        html.push_str(&escape_attr(&link.url));
        html.push('"');
        if let Some(target) = link_target {
            html.push_str(&format!(" target=\"{}\"", escape_attr(target)));
        }
        if let Some(rel) = link_rel {
            html.push_str(&format!(" rel=\"{}\"", escape_attr(rel)));
        }
        html.push_str(" class=\"toc-action\"><i class=\"bi ");
        match link.icon.as_deref() {
            Some(icon) => html.push_str(&format!("bi-{icon}")),
            None => html.push_str("empty"),
        }
        html.push_str("\"></i>");
        html.push_str(&link.text);
        html.push_str("</a></li>");
    }
    html.push_str("</ul></div>");
    html
}
```

- [x] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p quarto-navigation repo_actions`
Expected: PASS.

- [x] **Step 5: Gate and commit**

```bash
cargo clippy -p quarto-navigation --all-targets -- -D warnings
git add crates/quarto-navigation/src/render_html.rs
git commit -m "Render repo actions as Q1's toc-actions block (bd-repo-actions-missing-99ezd2fe)"
```

### Task 4: `PageFooter::center_append`

**Files:**
- Modify: `crates/quarto-navigation/src/footer.rs`
- Modify: `crates/quarto-navigation/src/render_html.rs:863-891` (`render_footer_region`), `:159-196` (`page_footer_to_html`)

**Interfaces:**
- Produces: `PageFooter { …, pub center_append: Option<String> }`, defaulting to `None` from `PageFooter::from_config_value`.

- [x] **Step 1: Write the failing tests**

Add to `#[cfg(test)] mod tests` in `crates/quarto-navigation/src/render_html.rs`:

```rust
#[test]
fn center_append_lands_inside_the_center_region() {
    let footer = PageFooter {
        center_append: Some("<div class=\"toc-actions\">X</div>".to_string()),
        ..PageFooter::default()
    };
    let html = page_footer_to_html(&footer);
    assert!(html.contains(
        "<div class=\"nav-footer-center\"><div class=\"toc-actions\">X</div></div>"
    ));
}

#[test]
fn center_append_follows_existing_center_text() {
    let footer = PageFooter {
        center: FooterRegion::Text(ConfigValue::new_string(
            "Positron 1.0",
            quarto_source_map::SourceInfo::for_test(),
        )),
        center_append: Some("<div class=\"toc-actions\">X</div>".to_string()),
        ..PageFooter::default()
    };
    let html = page_footer_to_html(&footer);
    let center = html
        .split("<div class=\"nav-footer-center\">")
        .nth(1)
        .expect("center region");
    let text_at = center.find("Positron 1.0").expect("existing text");
    let actions_at = center.find("toc-actions").expect("appended actions");
    assert!(text_at < actions_at, "actions must follow the configured text");
}

#[test]
fn center_append_absent_leaves_output_unchanged() {
    let footer = PageFooter::default();
    let html = page_footer_to_html(&footer);
    assert!(html.contains("<div class=\"nav-footer-center\"></div>"));
}
```

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p quarto-navigation center_append`
Expected: FAIL to compile — no field `center_append` on `PageFooter`.

- [x] **Step 3: Add the field**

In `crates/quarto-navigation/src/footer.rs`, add to `pub struct PageFooter` (after `right`):

```rust
    /// Raw HTML appended inside `.nav-footer-center`, after that
    /// region's own content.
    ///
    /// Not parsed from YAML — `from_config_value` always leaves this
    /// `None`. Populated only by `quarto-core`'s
    /// `FooterRenderTransform` from the repo-actions transform's
    /// output, mirroring Q1's DOM append into `.nav-footer-center`
    /// (`website-navigation.ts:698`).
    pub center_append: Option<String>,
```

`PageFooter` already derives `Default` (`footer.rs:144`), and all 12 `PageFooter { … }` literals in the tree (`footer.rs:168,174,443`; `render_html.rs:1796,1834,1858,1872,1896,1963,1982,2000`) end in `..PageFooter::default()` / `..Default::default()`, so **none needs `center_append: None`**. Two tests **do** compare a whole `PageFooter` with `assert_eq!` — `footer.rs:479` and `:487`. They still pass: both sides of each comparison are built through paths that leave `center_append` at its `None` default, so the new field compares equal. (`PageFooter` derives `PartialEq`, not `Eq`.)

**Do not add `center_append` to `PageFooter::to_config_value`.** That method serializes the footer back to `navigation.footer` for `FooterGenerateTransform`; `center_append` is set later, by `FooterRenderTransform`, and never round-trips. Serializing it would leak rendered HTML into the config tree. The omission is deliberate — say so in the field's doc comment so nobody "fixes" it.

- [x] **Step 4: Thread it through the renderer**

In `crates/quarto-navigation/src/render_html.rs`, replace `render_footer_region` with a version that takes an append. The three arms must produce **byte-identical** output when `append` is `None` — existing footer tests and snapshots depend on it:

```rust
fn render_footer_region(
    html: &mut String,
    class: &str,
    region: &FooterRegion,
    append: Option<&str>,
) {
    let inner = match region {
        FooterRegion::Empty => String::new(),
        FooterRegion::Text(cv) => render_text(cv),
        FooterRegion::Items(items) => {
            // `.footer-items` is the class Quarto 1's SCSS targets for
            // inline-flex alignment of links within a region.
            let mut s = String::from("\n      <ul class=\"nav footer-items\">\n");
            for item in items {
                s.push_str(&render_footer_item(item, 8));
            }
            s.push_str("      </ul>\n    ");
            s
        }
    };
    html.push_str(&format!(
        "    <div class=\"{}\">{}{}</div>\n",
        class,
        inner,
        append.unwrap_or("")
    ));
}
```

And update the three call sites in `page_footer_to_html`:

```rust
    render_footer_region(&mut html, "nav-footer-left", &footer.left, None);
    render_footer_region(
        &mut html,
        "nav-footer-center",
        &footer.center,
        footer.center_append.as_deref(),
    );
    render_footer_region(&mut html, "nav-footer-right", &footer.right, None);
```

- [x] **Step 5: Run tests to verify they pass**

Run: `cargo nextest run -p quarto-navigation`
Expected: PASS, including every pre-existing footer test. If a pre-existing test now fails on whitespace, the `Items` arm's reconstruction is wrong — fix the arm, not the test.

- [x] **Step 6: Gate and commit**

```bash
cargo clippy -p quarto-navigation --all-targets -- -D warnings
git add crates/quarto-navigation/src/footer.rs crates/quarto-navigation/src/render_html.rs
git commit -m "Let the footer's center region carry appended HTML (bd-repo-actions-missing-99ezd2fe)"
```

### Phase 1 gate

- [x] Run `cargo nextest run --workspace`. Compare pass/skip against the Task 0 baseline and account for the delta (expect **+27 passed** — 19 from Task 2, 5 from Task 3, 3 from Task 4 — and no change in skipped). Investigate any other movement before continuing.

  **Result: 13158 passed, 199 skipped** (118.2s) against the Task 0 baseline of 13130/199 — **+28 passed, skipped unchanged.**
  The delta is +28 rather than the +27 predicted above because a Task 2 review fix added one further test
  (`none_still_leaves_the_issue_url_link`) after this figure was written: 20 from Task 2 (19 + 1), 5 from
  Task 3, 3 from Task 4. `quarto-navigation` went 196 → 224, which is the same +28. Fully accounted for;
  no unexplained movement.

---

## Phase 2 — Diagnostics catalog and docs

Done before the transform so the codes exist when the transform references them, and so the two `error-docs-*` lint rules stay green at every commit.

### Task 5: Three `Q-13-*` codes with pages and sidebar entries

**Files:**
- Modify: `crates/quarto-error-catalog/error_catalog.json`
- Create: `docs/errors/navigation/Q-13-11.qmd`, `Q-13-12.qmd`, `Q-13-13.qmd`
- Modify: `docs/_quarto.yml:245` (after the `Q-13-10` entry)

- [x] **Step 1: Add the catalog entries**

Insert into `crates/quarto-error-catalog/error_catalog.json` after `"Q-13-10"`.

**Mind the commas.** `"Q-13-10"` is the **last** entry in the file (its closing `}` is immediately followed by the file's closing `}`), so it currently has no trailing comma. Add one after `Q-13-10`'s closing brace, and **delete** the trailing comma after `Q-13-13`'s — pasting the block below verbatim otherwise leaves a trailing comma before `}` and the JSON will not parse.

```json
  "Q-13-11": {
    "subsystem": "navigation",
    "title": "Repository actions require a repo-url",
    "message_template": "`repo-actions` lists actions to render, but neither `website.repo-url` nor `website.issue-url` is set, so no link can be built and no actions render. Set `website.repo-url` to the repository's web URL (for example `https://github.com/owner/repo`).",
    "docs_url": "https://quarto.org/docs/errors/navigation/Q-13-11",
    "since_version": "99.9.9"
  },
  "Q-13-12": {
    "subsystem": "navigation",
    "title": "Unknown repository action",
    "message_template": "A `repo-actions` entry names an action Quarto does not recognize. The supported actions are `edit`, `source`, and `issue`; `none` clears the list. The unrecognized entry is skipped and the remaining actions still render.",
    "docs_url": "https://quarto.org/docs/errors/navigation/Q-13-12",
    "since_version": "99.9.9"
  },
  "Q-13-13": {
    "subsystem": "navigation",
    "title": "Page-level `repo-actions: true` ignored",
    "message_template": "A page's front matter sets `repo-actions: true`. This key does not enable repository actions — the action list always comes from `website.repo-actions` — it asks only that a page with no table of contents show the actions in the margin rather than the page footer. Quarto 2 does not implement that placement, so the value is ignored and the actions render in the page footer at every width. Page-level `repo-actions: false`, which suppresses the actions for a single page, is supported.",
    "docs_url": "https://quarto.org/docs/errors/navigation/Q-13-13",
    "since_version": "99.9.9"
  },
```

- [x] **Step 2: Write the three docs pages**

Follow the structure of `docs/errors/navigation/Q-13-10.qmd` — front matter, blockquote restating the message, then `## What this means`, `## Why this happens`, `## How to fix`, `## Related`. Each page must set `code:` and `subsystem: navigation`, and its `docs_url` in the catalog must be `https://quarto.org/docs/errors/navigation/<code>`.

`docs/errors/navigation/Q-13-11.qmd`:

```markdown
---
title: "Repository actions require a repo-url"
description: "repo-actions lists actions to render, but no repo-url or issue-url is configured, so no links can be built."
code: Q-13-11
subsystem: navigation
status: complete
since: "99.9.9"
categories:
  - navigation
  - websites
---

# `Q-13-11` — Repository actions require a `repo-url`

> `repo-actions` lists actions to render, but neither
> `website.repo-url` nor `website.issue-url` is set, so no link can
> be built and no actions render.

## What this means

`repo-actions` names *which* links to show. The links themselves are
built from the repository's web URL, so Quarto needs to know where
the repository lives:

``` yaml
website:
  repo-url: https://github.com/owner/repo
  repo-actions: [edit, source, issue]
```

Without `repo-url` there is no base to build `edit` and `source`
URLs from, and no default target for `issue`. The render still
succeeds; the page simply has no action links.

## Why this happens

Common causes:

- **`repo-actions` added without `repo-url`.** The two keys are
  usually written together, and it is easy to add the action list
  while intending to fill in the URL later.
- **`repo-url` in the wrong scope.** It belongs under `website:`,
  not at the top level of `_quarto.yml` or inside `format:`.
- **`repo-url` set only in a profile file.** If it lives in
  `_quarto-<profile>.yml`, renders without that profile active will
  not see it.

## How to fix

Set `website.repo-url` to the repository's web URL — the address you
would visit in a browser, without a trailing `.git`:

``` yaml
website:
  repo-url: https://github.com/owner/repo
  repo-branch: main          # optional, defaults to main
  repo-actions: [edit, source, issue]
```

If you only want a "Report an issue" link and have no source
repository to link to, set `issue-url` instead — it works on its
own:

``` yaml
website:
  issue-url: https://example.com/support
  repo-actions: [issue]
```

## Related

- `Q-13-12` — an unrecognized `repo-actions` entry.
- `Q-13-13` — page-level `repo-actions: true` is ignored.
```

`docs/errors/navigation/Q-13-12.qmd`:

```markdown
---
title: "Unknown repository action"
description: "A repo-actions entry names an action Quarto does not recognize; it is skipped."
code: Q-13-12
subsystem: navigation
status: complete
since: "99.9.9"
categories:
  - navigation
  - websites
---

# `Q-13-12` — Unknown repository action

> A `repo-actions` entry names an action Quarto does not recognize.
> The supported actions are `edit`, `source`, and `issue`; `none`
> clears the list. The unrecognized entry is skipped and the
> remaining actions still render.

## What this means

`repo-actions` accepts a fixed set of names:

| action | link |
|---|---|
| `edit` | "Edit this page" — opens the source file in the host's web editor |
| `source` | "View source" — opens the source file for reading |
| `issue` | "Report an issue" — opens the issue tracker |

`none` is also accepted and clears the list. Anything else is
skipped with this message; the entries Quarto does recognize still
render.

## Why this happens

Common causes:

- **A typo** — `sources`, `edits`, `issues`.
- **A guessed action name** for something Quarto does not offer,
  such as `star`, `fork`, or `history`.

## How to fix

Use only the supported names:

``` yaml
website:
  repo-url: https://github.com/owner/repo
  repo-actions: [edit, source, issue]
```

To turn the links off for the whole site, use `none` or remove the
key:

``` yaml
website:
  repo-actions: none
```

## Related

- `Q-13-11` — `repo-actions` set with no `repo-url`.
- `Q-13-13` — page-level `repo-actions: true` is ignored.
```

`docs/errors/navigation/Q-13-13.qmd`:

```markdown
---
title: "Page-level repo-actions: true ignored"
description: "A page sets repo-actions: true, a placement request Quarto 2 does not implement; the actions still render in the footer."
code: Q-13-13
subsystem: navigation
status: complete
since: "99.9.9"
categories:
  - navigation
  - websites
---

# `Q-13-13` — Page-level `repo-actions: true` ignored

> A page's front matter sets `repo-actions: true`. This key does not
> enable repository actions — the action list always comes from
> `website.repo-actions` — it asks only that a page with no table of
> contents show the actions in the margin rather than the page
> footer. Quarto 2 does not implement that placement, so the value is
> ignored and the actions render in the page footer at every width.

## What this means

`repo-actions: true` in a page's front matter is easy to read as
"turn the actions on for this page". It is not: the list of actions
always comes from the site configuration, and a page-level `true`
cannot add actions that the site has not configured.

What it *does* request is a placement change. Normally the actions
render twice — once at the foot of the page's table of contents, and
once in the page footer as the small-screen fallback. On a page with
no table of contents there is nowhere to put the first copy, and
`repo-actions: true` asks for it to go in the right margin instead.

Quarto 2 does not implement that margin placement. The value is
ignored, and the page's actions render in the footer, visible at
every width. Nothing is lost but the margin position.

## Why this happens

Common causes:

- **Reading `true` as an enable switch.** The `false` value *does*
  work as a switch — it suppresses the actions for one page — so
  `true` reasonably looks like its opposite.
- **A page ported from Quarto 1** that used the margin placement.

## How to fix

Remove the key. The actions already render:

``` yaml
---
title: My page
---
```

To suppress the actions on a single page, `false` is supported:

``` yaml
---
title: My page
repo-actions: false
---
```

To change which actions appear site-wide, edit `_quarto.yml`:

``` yaml
website:
  repo-url: https://github.com/owner/repo
  repo-actions: [edit, issue]
```

## Related

- `Q-13-11` — `repo-actions` set with no `repo-url`.
- `Q-13-12` — an unrecognized `repo-actions` entry.
```

- [x] **Step 3: Add the sidebar entries**

In `docs/_quarto.yml`, after line 245 (`- errors/navigation/Q-13-10.qmd`), add:

```yaml
            - errors/navigation/Q-13-11.qmd
            - errors/navigation/Q-13-12.qmd
            - errors/navigation/Q-13-13.qmd
```

Match the surrounding indentation exactly. Entries within a section must ascend by code number; 11, 12, 13 after 10 satisfies this.

- [x] **Step 4: Verify the lint rules pass**

Run: `cargo xtask lint`
Expected: no `error-docs-page-missing` or `error-docs-sidebar-unlisted` violations.

- [x] **Step 5: Commit**

```bash
git add crates/quarto-error-catalog/error_catalog.json docs/errors/navigation docs/_quarto.yml
git commit -m "Add Q-13-11/12/13 for repo-action misconfiguration (bd-repo-actions-missing-99ezd2fe)"
```

### Phase 2 gate

- [x] Run `cargo nextest run --workspace`. This phase adds **no tests and no behaviour** — it is catalog data and documentation — so the delta against the Task 0 baseline must be **zero**. A non-zero delta here means the catalog JSON broke something that parses it.

- [x] Run `cargo xtask lint` once more from a clean tree. `cargo xtask verify` does **not** run these repo-level rules, and no CI check in this repo is required to pass before merge, so this command is the only thing standing between a missing sidebar entry and a shipped 404.

  **Result: both green.** `cargo nextest run --workspace` → **13158 passed, 199 skipped** — identical to the
  Phase 1 gate, i.e. the required **zero delta** for a catalog-and-docs phase. `cargo xtask lint` → `All checks
  passed! (1043 files checked)`, which covers `error-docs-page-missing` and `error-docs-sidebar-unlisted`
  (both are called unconditionally from `run_check` in `crates/xtask/src/lint/mod.rs:97,102`; `--verbose` only
  adds announcement lines).

  One nextest `LEAK` note appeared on `quarto-core stage::stages::compile_theme_css::tests::
  fingerprint_stable_for_identical_inputs` — the test **passed**; `LEAK` means it left a handle or subprocess
  behind. Unrelated to this phase (nothing here touches theme compilation) and not present in the Phase 1 run,
  so it reads as a timing artifact of the sass subprocess rather than a regression. Noted, not chased.

  The three new pages were additionally confirmed to **render**: `docs/_site/errors/navigation/Q-13-11.html`,
  `-12.html` and `-13.html` were produced with the expected `<title>`s ("Repository actions require a repo-url",
  "Unknown repository action", "Page-level repo-actions: true ignored"), real body content, and working
  cross-links to their siblings. (A bare `q2 render docs/` fails in a fresh worktree on the gitignored,
  generated `docs/examples` resource. **`cargo xtask stage-doc-examples` does not fix this in a worktree** —
  it resolves its output root through `create_worktree::repo_root` (`stage_doc_examples.rs:33`), which uses
  `git rev-parse --path-format=absolute --git-common-dir`; in a worktree that always points at the *main*
  repo, so the command reports success while staging into `../../docs/examples/` and never populating the
  worktree's own. Copy the main repo's already-staged `docs/examples/` across to render docs here. Filed as
  a follow-up strand.)

---

## Phase 3 — The transform (`quarto-core`)

### Task 6: `RepoActionsRenderTransform`

**Files:**
- Create: `crates/quarto-core/src/transforms/repo_actions_render.rs`
- Modify: `crates/quarto-core/src/transforms/mod.rs`

**Interfaces:**
- Consumes: `quarto_navigation::{RepoActionLabels, RepoActionWarning, RepoActionsConfig, repo_action_links}`, `quarto_navigation::render_html::repo_actions_to_html`, `quarto_config::resolve_website_value`, `crate::transforms::navigation_active::page_relative_source`, `crate::language::LanguageTerms`.
- Produces: `pub struct RepoActionsRenderTransform` with `RepoActionsRenderTransform::new()`, writing `rendered.navigation.toc-actions` and `rendered.navigation.footer-actions`.

- [x] **Step 1: Write the failing tests**

Create the module with only the test block, and **declare it in `mod.rs` in this same step** — add `mod repo_actions_render;` to `crates/quarto-core/src/transforms/mod.rs` now, not in Step 4. An undeclared file is never compiled, so without this Step 2 runs zero tests and exits 0 instead of failing. (The `pub use` for the transform still waits until Step 4, when the type exists.)

**Copy the harness; you cannot import it.** `footer_render.rs`'s helpers are private to its own `#[cfg(test)] mod tests`. Copy `make_test_project`, `config_map`, `s`, and `b` verbatim from `crates/quarto-core/src/transforms/footer_render.rs:184-228`, and `arr` from `crates/quarto-core/src/transforms/footer_generate.rs:225` (it is not among footer_render's helpers):

```rust
fn arr(items: Vec<ConfigValue>) -> ConfigValue {
    ConfigValue::new_array(items, SourceInfo::for_test())
}
```

The copied helpers need the same imports `footer_render.rs`'s test module carries (`:184-195`) — `use super::*;` alone will not do:

```rust
use super::*;
use crate::format::Format;
use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
use crate::render::BinaryDependencies;
use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::SourceInfo;
use std::path::PathBuf;
```

Then write a `run` that takes the page's source path, because `page_relative_source(ctx)` derives it from `ctx.document.input` stripped of `ctx.project.dir` — `make_test_project` uses `/project` as the dir, so the document must live under it:

```rust
async fn run(meta: ConfigValue, source: &str) -> (ConfigValue, Vec<DiagnosticMessage>) {
    let mut ast = Pandoc { meta, blocks: vec![] };
    let project = make_test_project();
    let doc = DocumentInfo::from_path(format!("/project/{source}"));
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    RepoActionsRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("transform");
    (ast.meta, ctx.diagnostics)
}
```

This mirrors `footer_render.rs`'s `run_with` (`:243-263`), with the document path parameterised so `page_relative_source` yields the `source` each test asks for. The tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // Mirror footer_render.rs's harness: build `ast.meta`, run the
    // transform, return `(meta, diagnostics)`.

    fn website(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        config_map(vec![("website", config_map(entries))])
    }

    fn toc_present(meta: &mut ConfigValue) {
        meta.insert_path(
            &["rendered", "navigation", "toc"],
            s("<ul><li>x</li></ul>"),
        );
    }

    #[tokio::test]
    async fn emits_both_copies_when_a_toc_exists() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit"), s("source"), s("issue")])),
        ]);
        toc_present(&mut meta);
        let (meta, diags) = run(meta, "index.qmd").await;
        let toc = meta.get_path(&["rendered", "navigation", "toc-actions"])
            .and_then(|v| v.as_plain_text()).unwrap();
        let footer = meta.get_path(&["rendered", "navigation", "footer-actions"])
            .and_then(|v| v.as_plain_text()).unwrap();
        assert!(toc.starts_with("<div class=\"toc-actions\">"));
        assert!(footer.starts_with("<div class=\"toc-actions d-sm-block d-md-none\">"));
        assert!(diags.is_empty());
    }

    /// Q1: the responsive classes exist only to hide the footer copy
    /// where the TOC copy is visible. With no TOC, the footer copy is
    /// the only one and stays visible at every width.
    #[tokio::test]
    async fn footer_copy_has_no_responsive_classes_without_a_toc() {
        let meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit")])),
        ]);
        let (meta, _) = run(meta, "index.qmd").await;
        assert!(meta.get_path(&["rendered", "navigation", "toc-actions"]).is_none());
        let footer = meta.get_path(&["rendered", "navigation", "footer-actions"])
            .and_then(|v| v.as_plain_text()).unwrap();
        assert_eq!(footer.find("d-sm-block"), None);
    }

    #[tokio::test]
    async fn skips_entirely_when_no_actions_configured() {
        let meta = website(vec![("repo-url", s("https://github.com/e/d"))]);
        let (meta, diags) = run(meta, "index.qmd").await;
        assert!(meta.get_path(&["rendered", "navigation", "footer-actions"]).is_none());
        assert!(diags.is_empty());
    }

    /// Q1 parity: `issue-url` alone is enough. `handleRepoLinks` pushes
    /// `issue` onto the action list before gating on its length
    /// (`website-navigation.ts:661-670`), so a site that configures only
    /// `issue-url` still gets one link. The empty-actions early return
    /// must not swallow this.
    #[tokio::test]
    async fn issue_url_alone_still_renders_without_any_repo_actions() {
        let meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("issue-url", s("https://example.com/file-a-bug")),
        ]);
        let (meta, diags) = run(meta, "index.qmd").await;
        let footer = meta
            .get_path(&["rendered", "navigation", "footer-actions"])
            .and_then(|v| v.as_plain_text())
            .expect("issue-url alone still produces a footer copy");
        assert!(footer.contains("https://example.com/file-a-bug"));
        assert!(diags.is_empty());
    }

    #[tokio::test]
    async fn page_level_false_suppresses_both_copies() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit")])),
        ]);
        meta.insert_path(&["repo-actions"], b(false));
        let (meta, diags) = run(meta, "index.qmd").await;
        assert!(meta.get_path(&["rendered", "navigation", "footer-actions"]).is_none());
        assert!(diags.is_empty(), "an affirmative disable is not worth a message");
    }

    /// Decision D-11: the placement `true` asks for only exists on a
    /// page with no TOC, so that is the only page worth telling.
    #[tokio::test]
    async fn page_level_true_reports_q_13_13_when_there_is_no_toc() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit")])),
        ]);
        meta.insert_path(&["repo-actions"], b(true));
        let (meta, diags) = run(meta, "index.qmd").await;
        assert!(meta.get_path(&["rendered", "navigation", "footer-actions"]).is_some());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-13-13"));
    }

    /// …and stays quiet with a TOC, where Q1 ignores `true` too.
    #[tokio::test]
    async fn page_level_true_is_silent_when_the_page_has_a_toc() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit")])),
        ]);
        meta.insert_path(&["repo-actions"], b(true));
        toc_present(&mut meta);
        let (meta, diags) = run(meta, "index.qmd").await;
        assert!(meta.get_path(&["rendered", "navigation", "toc-actions"]).is_some());
        assert!(diags.is_empty());
    }

    #[tokio::test]
    async fn missing_repo_url_reports_q_13_11() {
        let meta = website(vec![("repo-actions", arr(vec![s("edit")]))]);
        let (meta, diags) = run(meta, "index.qmd").await;
        assert!(meta.get_path(&["rendered", "navigation", "footer-actions"]).is_none());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-13-11"));
    }

    #[tokio::test]
    async fn unknown_action_reports_q_13_12() {
        let meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit"), s("publish")])),
        ]);
        let (_, diags) = run(meta, "index.qmd").await;
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-13-12"));
    }

    /// Decision D-6: the action list is read from `website.` only, so
    /// a page-level bool cannot be confused for a list.
    #[tokio::test]
    async fn top_level_repo_actions_list_is_not_the_action_source() {
        let mut meta = website(vec![("repo-url", s("https://github.com/e/d"))]);
        meta.insert_path(&["repo-actions"], arr(vec![s("edit")]));
        let (meta, _) = run(meta, "index.qmd").await;
        assert!(meta.get_path(&["rendered", "navigation", "footer-actions"]).is_none());
    }

    /// Decision D-6: string keys *do* accept a front-matter override.
    #[tokio::test]
    async fn page_level_repo_url_overrides_the_site_value() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/site/wide")),
            ("repo-actions", arr(vec![s("source")])),
        ]);
        meta.insert_path(&["repo-url"], s("https://github.com/page/local"));
        let (meta, _) = run(meta, "index.qmd").await;
        let footer = meta.get_path(&["rendered", "navigation", "footer-actions"])
            .and_then(|v| v.as_plain_text()).unwrap();
        assert!(footer.contains("https://github.com/page/local/blob/main/index.qmd"));
    }

    #[tokio::test]
    async fn existing_slot_is_not_overwritten() {
        let mut meta = website(vec![
            ("repo-url", s("https://github.com/e/d")),
            ("repo-actions", arr(vec![s("edit")])),
        ]);
        meta.insert_path(&["rendered", "navigation", "footer-actions"], s("<div>mine</div>"));
        let (meta, _) = run(meta, "index.qmd").await;
        assert_eq!(
            meta.get_path(&["rendered", "navigation", "footer-actions"])
                .and_then(|v| v.as_plain_text()).as_deref(),
            Some("<div>mine</div>")
        );
    }
}
```

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p quarto-core repo_actions_render`
Expected: **FAIL to compile** — `RepoActionsRenderTransform` is not defined. If instead you see "0 tests run" and a green exit, the `mod repo_actions_render;` line from Step 1 is missing and nothing was compiled.

- [x] **Step 3: Implement the transform**

```rust
/*
 * repo_actions_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for website repository actions
//! (bd-repo-actions-missing-99ezd2fe).
//!
//! Resolves `website.repo-*` configuration, builds the link list via
//! [`quarto_navigation::repo_action_links`], and writes two rendered
//! HTML strings for downstream consumers:
//!
//! - `rendered.navigation.toc-actions` — the copy that lands inside
//!   `nav#TOC`, emitted by the `toc-block` template partial and its
//!   Rust twin `toc_block_html`. Written only when
//!   `rendered.navigation.toc` is non-empty.
//! - `rendered.navigation.footer-actions` — the copy that lands
//!   inside `.nav-footer-center`, consumed by
//!   [`FooterRenderTransform`](super::FooterRenderTransform). Carries
//!   `d-sm-block d-md-none` only when the TOC copy also exists, so it
//!   is the small-screen fallback for it — Q1's exact conditional
//!   (`website-navigation.ts:698`).
//!
//! Q1 parity: `website-navigation.ts::handleRepoLinks` (line 647).
//!
//! ## Config scopes (decision D-6)
//!
//! The **action list** is read from `website.repo-actions` only,
//! matching Q1's `websiteConfigActions(key, kWebsite, config)`. This
//! is required, not stylistic: the top-level slot is where a page's
//! `repo-actions: true`/`false` lands, so merging the two scopes
//! would collide a bool with an array.
//!
//! Every **string** key goes through
//! [`resolve_website_value`], which lets front matter override the
//! site value. Q1 permits that only for `repo-url`; widening it to
//! the sibling keys is a deliberate convenience.
//!
//! ## Skip conditions
//!
//! - Page-level `repo-actions: false`.
//! - No actions resolved (key absent, or `none`).
//! - `rendered.navigation.footer-actions` already populated (user
//!   override).

use quarto_config::resolve_website_value;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_navigation::render_html::repo_actions_to_html;
use quarto_navigation::{
    RepoActionLabels, RepoActionWarning, RepoActionsConfig, repo_action_links,
};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::language::LanguageTerms;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::navigation_active::page_relative_source;

pub struct RepoActionsRenderTransform;

impl RepoActionsRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RepoActionsRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for RepoActionsRenderTransform {
    fn name(&self) -> &str {
        "repo-actions-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if ast
            .meta
            .contains_path(&["rendered", "navigation", "footer-actions"])
        {
            return Ok(());
        }

        // A page-level bool lands at the top level. `false` suppresses;
        // `true` is a placement request q2 does not honour (D-4).
        let page_flag = ast.meta.get("repo-actions").and_then(|v| v.as_bool());
        if page_flag == Some(false) {
            return Ok(());
        }

        let actions = resolve_actions(ast.meta.get_path(&["website", "repo-actions"]));
        // Q1 parity: `handleRepoLinks` pushes `issue` onto the action
        // list whenever `issue-url` is configured, and does so *before*
        // it gates on the list being non-empty
        // (`website-navigation.ts:661-670`). So `issue-url` alone — no
        // `website.repo-actions` key at all, or a scalar `none` — still
        // renders one "Report an issue" link. Resolving `issue-url`
        // here rather than at the `RepoActionsConfig` literal below is
        // what keeps that case alive; `repo_action_links` performs the
        // append itself.
        let issue_url = website_string(&ast.meta, "issue-url");
        if actions.is_empty() && issue_url.is_none() {
            return Ok(());
        }

        // The TOC copy exists only where there is a TOC to hang it on.
        // Exactly one `nav[role=doc-toc]` is emitted per page — see the
        // TocLocationTransform analysis in the plan — so this yields
        // exactly one TOC copy in every placement. Computed here rather
        // than at the point of use because the Q-13-13 gate needs it.
        let has_toc = ast
            .meta
            .get_path(&["rendered", "navigation", "toc"])
            .and_then(|v| v.as_plain_text())
            .is_some_and(|s| !s.is_empty());

        // Decision D-11: `repo-actions: true` only ever asked for the
        // margin placement, which only ever applied to a page with no
        // TOC. With a TOC, Q1 ignores it too — nothing to report.
        if page_flag == Some(true) && !has_toc {
            let location = ast
                .meta
                .get("repo-actions")
                .map_or_else(
                    || SourceInfo::generated(By::programmatic_config()),
                    |v| v.source_info.clone(),
                );
            ctx.diagnostics.push(page_level_true_info(location));
        }

        let cfg = RepoActionsConfig {
            repo_url: website_string(&ast.meta, "repo-url"),
            branch: website_string(&ast.meta, "repo-branch")
                .unwrap_or_else(|| "main".to_string()),
            subdir: website_string(&ast.meta, "repo-subdir"),
            issue_url,
            actions,
            link_target: website_string(&ast.meta, "repo-link-target"),
            link_rel: website_string(&ast.meta, "repo-link-rel"),
        };

        let terms = LanguageTerms::from_meta(&ast.meta);
        let labels = labels_from_terms(terms.as_ref());
        let source = page_relative_source(ctx);
        let (links, warnings) = repo_action_links(&cfg, &source, &labels);

        for warning in warnings {
            let location = ast
                .meta
                .get_path(&["website", "repo-actions"])
                .map_or_else(
                    || SourceInfo::generated(By::programmatic_config()),
                    |v| v.source_info.clone(),
                );
            ctx.diagnostics.push(match warning {
                RepoActionWarning::NoRepoUrl => no_repo_url_warning(location),
                RepoActionWarning::UnknownAction(name) => unknown_action_warning(&name, location),
            });
        }

        if links.is_empty() {
            return Ok(());
        }

        let target = cfg.link_target.as_deref();
        let rel = cfg.link_rel.as_deref();

        if has_toc {
            let html = repo_actions_to_html(&links, &[], target, rel);
            ast.meta.insert_path(
                &["rendered", "navigation", "toc-actions"],
                ConfigValue::new_string(&html, SourceInfo::generated(By::programmatic_config())),
            );
        }

        // Q1 gives the footer copy the responsive classes only when a
        // TOC copy exists to cover wide viewports.
        let footer_classes: &[&str] = if has_toc {
            &["d-sm-block", "d-md-none"]
        } else {
            &[]
        };
        let footer_html = repo_actions_to_html(&links, footer_classes, target, rel);
        ast.meta.insert_path(
            &["rendered", "navigation", "footer-actions"],
            ConfigValue::new_string(&footer_html, SourceInfo::generated(By::programmatic_config())),
        );

        Ok(())
    }
}

/// Read a website-scoped string, allowing a front-matter override.
///
/// `as_plain_text` (not `as_str`) because a bare string authored in
/// front matter is stored as `PandocInlines`.
fn website_string(meta: &ConfigValue, key: &str) -> Option<String> {
    resolve_website_value(meta, key)
        .and_then(|v| v.as_plain_text())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Q1 `websiteConfigActions`: a scalar `none` clears, any other scalar
/// is a one-element list, an array maps to strings. Decision D-7
/// extends the `none` handling to array elements.
fn resolve_actions(cv: Option<&ConfigValue>) -> Vec<String> {
    let Some(cv) = cv else {
        return Vec::new();
    };
    if let Some(items) = cv.as_array() {
        return items.iter().filter_map(|i| i.as_plain_text()).collect();
    }
    match cv.as_plain_text() {
        Some(s) if s == "none" => Vec::new(),
        Some(s) => vec![s],
        None => Vec::new(),
    }
}

fn labels_from_terms(terms: Option<&LanguageTerms>) -> RepoActionLabels {
    let defaults = RepoActionLabels::default();
    let get = |key: &str, fallback: String| {
        terms
            .and_then(|t| t.get(key))
            .map_or(fallback, str::to_string)
    };
    RepoActionLabels {
        edit: get("repo-action-links-edit", defaults.edit),
        source: get("repo-action-links-source", defaults.source),
        issue: get("repo-action-links-issue", defaults.issue),
    }
}

/// Q-13-11: actions requested but nothing to build a URL from.
fn no_repo_url_warning(location: SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Repository actions require a `repo-url`")
        .with_code("Q-13-11")
        .problem(
            "`repo-actions` lists actions to render, but neither `website.repo-url` \
             nor `website.issue-url` is set, so no links can be built.",
        )
        .add_hint(
            "Set `website.repo-url` to the repository's web URL, for example \
             `https://github.com/owner/repo`.",
        )
        .with_location(location)
        .build()
}

/// Q-13-12: an action name outside the supported set.
fn unknown_action_warning(name: &str, location: SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Unknown repository action")
        .with_code("Q-13-12")
        .problem(format!(
            "`{name}` is not a repository action Quarto recognizes; it is skipped."
        ))
        .add_hint("The supported actions are `edit`, `source`, and `issue`; `none` clears the list.")
        .with_location(location)
        .build()
}

/// Q-13-13: page-level `repo-actions: true` (decision D-4). `info`,
/// not `warning` — nothing visible is lost.
fn page_level_true_info(location: SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::info("Page-level `repo-actions: true` ignored")
        .with_code("Q-13-13")
        .problem(
            "`repo-actions: true` does not enable repository actions — the action list \
             always comes from `website.repo-actions`. It asks only that a page with no \
             table of contents show them in the margin rather than the footer.",
        )
        .add_hint(
            "The actions still render in this page's footer, at every width — \
             only the margin placement is unavailable.",
        )
        .add_hint(
            "Page-level `repo-actions: false`, which suppresses the actions for a \
             single page, is supported.",
        )
        .with_location(location)
        .build()
}
```

- [x] **Step 4: Export the transform**

In `crates/quarto-core/src/transforms/mod.rs`, add `pub use repo_actions_render::RepoActionsRenderTransform;` in the existing alphabetical position. The `mod repo_actions_render;` line already went in at Step 1.

- [x] **Step 5: Run tests to verify they pass**

Run: `cargo nextest run -p quarto-core repo_actions_render`
Expected: PASS, 12 tests. (The count was 11 before `issue_url_alone_still_renders_without_any_repo_actions`
was added to Step 1 — see the `issue-url` correction in the run body and in D-7.)

- [x] **Step 6: Gate and commit**

```bash
cargo clippy -p quarto-core --all-targets -- -D warnings
git add crates/quarto-core/src/transforms/repo_actions_render.rs crates/quarto-core/src/transforms/mod.rs
git commit -m "Resolve repo-action config into rendered markup (bd-repo-actions-missing-99ezd2fe)"
```

### Phase 3 gate

- [x] Run `cargo nextest run --workspace`. The transform is not yet in the pipeline, so **no rendered output changes** — the delta must be exactly the 12 new unit tests from Task 6 and nothing else. Any movement in an existing test at this phase means the module was wired up early.

  **Result: 13170 passed, 199 skipped** against the Phase 2 gate's 13158/199 — **+12 passed, skipped unchanged,**
  and zero failures. The delta is exactly Task 6's test module. (The "11" above was the pre-correction count;
  `issue_url_alone_still_renders_without_any_repo_actions` makes 12 — same stale figure as Task 6 Step 5.)
  No existing test moved, so nothing was wired into the pipeline early — that is Task 9's job.

---

## Phase 4 — Wiring

### Task 7: TOC copy through the template partial and its twin

**Files:**
- Modify: `crates/quarto-core/src/template.rs:608-616` (`TOC_BLOCK_PARTIAL`), and the variable doc-comment block near line 222
- Modify: `crates/quarto-core/src/transforms/sidebar_render.rs:195-212` (`toc_block_html`)

- [x] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `crates/quarto-core/src/template.rs`, alongside the existing `#quarto-margin-sidebar` tests. The helpers are `render_full(body, meta)` (`template.rs:3031`), `meta_with_navigation(toc, margin_categories)` (`:3049`), and `dummy_source_info()` — there is **no** `render_full_template` and no `s()` in this module:

```rust
#[test]
fn toc_block_emits_repo_actions_inside_the_nav() {
    let mut meta = meta_with_navigation(Some("<ul><li>x</li></ul>"), None);
    meta.insert_path(
        &["rendered", "navigation", "toc-actions"],
        ConfigValue::new_string(
            "<div class=\"toc-actions\">ACTIONS</div>",
            dummy_source_info(),
        ),
    );
    let html = render_full("<p>body</p>", &meta);
    let nav_open = html.find("<nav id=\"TOC\"").expect("nav");
    let nav_close = html[nav_open..].find("</nav>").expect("nav close") + nav_open;
    let actions = html.find("ACTIONS").expect("actions");
    assert!(
        nav_open < actions && actions < nav_close,
        "the actions block must sit inside nav#TOC"
    );
}

/// The conditional must not leave a stray block when unset.
#[test]
fn toc_block_omits_repo_actions_when_unset() {
    let meta = meta_with_navigation(Some("<ul><li>x</li></ul>"), None);
    let html = render_full("<p>body</p>", &meta);
    assert!(html.contains("<nav id=\"TOC\""));
    assert!(!html.contains("toc-actions"));
}
```

Add to `#[cfg(test)] mod tests` in `crates/quarto-core/src/transforms/sidebar_render.rs`:

```rust
#[test]
fn toc_block_html_twin_includes_repo_actions() {
    let mut meta = config_map(vec![]);
    meta.insert_path(&["rendered", "navigation", "toc"], s("<ul><li>x</li></ul>"));
    meta.insert_path(
        &["rendered", "navigation", "toc-actions"],
        s("<div class=\"toc-actions\">ACTIONS</div>"),
    );
    let html = toc_block_html(&meta).expect("toc block");
    assert!(html.contains("ACTIONS"));
    assert!(
        html.find("ACTIONS").unwrap() < html.find("</nav>").unwrap(),
        "actions must precede the closing nav tag"
    );
}
```

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p quarto-core toc_block`
Expected: FAIL — but only two of the three. `toc_block_emits_repo_actions_inside_the_nav` and `toc_block_html_twin_includes_repo_actions` fail because the actions text is absent. `toc_block_omits_repo_actions_when_unset` **passes already** (nothing in the tree emits `toc-actions`, and the SCSS is linked rather than inlined); it is a guard against the new conditional leaving a stray block behind, not a red-green test.

- [x] **Step 3: Add the variable to the partial**

In `crates/quarto-core/src/template.rs`, change `TOC_BLOCK_PARTIAL` to:

```rust
pub const TOC_BLOCK_PARTIAL: &str = r#"<nav id="TOC" role="doc-toc" class="toc-active">
$if(rendered.navigation.toc-title)$
<h2 id="toc-title">$rendered.navigation.toc-title$</h2>
$endif$
$rendered.navigation.toc$
$if(rendered.navigation.toc-actions)$
$rendered.navigation.toc-actions$
$endif$
</nav>
"#;
```

Extend the doc comment above it to mention that `rendered.navigation.toc-actions` is written by `RepoActionsRenderTransform` and must be kept in sync with the Rust twin. Add `rendered.navigation.toc-actions` and `rendered.navigation.footer-actions` to the template-variable list near line 222, following the format of the `rendered.navigation.breadcrumbs` entry.

- [x] **Step 4: Add it to the Rust twin**

In `crates/quarto-core/src/transforms/sidebar_render.rs`, in `toc_block_html`, insert before the closing `</nav>`:

```rust
    if let Some(actions) = meta
        .get_path(&["rendered", "navigation", "toc-actions"])
        .and_then(|v| v.as_plain_text())
        .filter(|s| !s.is_empty())
    {
        html.push_str(&actions);
        html.push('\n');
    }
    html.push_str("</nav>\n");
```

- [x] **Step 5: Run tests to verify they pass**

Run: `cargo nextest run -p quarto-core toc_block`
Expected: PASS, 3 tests.

- [x] **Step 6: Gate and commit**

```bash
cargo clippy -p quarto-core --all-targets -- -D warnings
git add crates/quarto-core/src/template.rs crates/quarto-core/src/transforms/sidebar_render.rs
git commit -m "Emit the TOC repo-actions copy from the toc-block partial and twin (bd-repo-actions-missing-99ezd2fe)"
```

### Task 8: Footer copy and footer synthesis

**Files:**
- Modify: `crates/quarto-core/src/transforms/footer_render.rs:65-116`

- [x] **Step 1: Write the failing tests**

Add to `#[cfg(test)] mod tests` in `crates/quarto-core/src/transforms/footer_render.rs`, using the file's existing `run` helper:

```rust
#[tokio::test]
async fn footer_actions_are_appended_to_a_configured_footer() {
    // Same starting point as the existing `renders_footer_html`
    // test (`footer_render.rs:386-392`): a configured footer whose
    // center region already holds text.
    let footer = PageFooter {
        center: FooterRegion::Text(s("Copyright 2026")),
        ..PageFooter::default()
    };
    let mut meta = ConfigValue::default();
    meta.insert_path(&["navigation", "footer"], footer.to_config_value());
    meta.insert_path(
        &["rendered", "navigation", "footer-actions"],
        s("<div class=\"toc-actions d-sm-block d-md-none\">ACTIONS</div>"),
    );
    let (meta, _) = run(meta).await;
    let html = meta.get_path(&["rendered", "navigation", "footer"])
        .and_then(|v| v.as_plain_text()).unwrap();
    let center = html.split("<div class=\"nav-footer-center\">").nth(1).unwrap();
    assert!(center.contains("ACTIONS"));
}

/// Q1 synthesizes the whole footer chain purely to host the
/// small-screen copy (decision D-2).
#[tokio::test]
async fn footer_is_synthesized_when_only_actions_exist() {
    let mut meta = config_map(vec![]);
    meta.insert_path(
        &["rendered", "navigation", "footer-actions"],
        s("<div class=\"toc-actions\">ACTIONS</div>"),
    );
    let (meta, _) = run(meta).await;
    let html = meta.get_path(&["rendered", "navigation", "footer"])
        .and_then(|v| v.as_plain_text()).unwrap();
    assert!(html.contains("<footer class=\"footer\""));
    assert!(html.contains("<div class=\"nav-footer-center\"><div class=\"toc-actions\">ACTIONS</div></div>"));
}

/// Decision D-3: deliberate divergence — Q1 synthesizes a footer even
/// with `page-footer: false`. Top-level scope.
#[tokio::test]
async fn page_footer_false_suppresses_the_actions_copy() {
    let mut meta = config_map(vec![("page-footer", b(false))]);
    meta.insert_path(
        &["rendered", "navigation", "footer-actions"],
        s("<div class=\"toc-actions\">ACTIONS</div>"),
    );
    let (meta, _) = run(meta).await;
    assert!(meta.get_path(&["rendered", "navigation", "footer"]).is_none());
}

/// The scope that the obvious implementation gets wrong (D-3).
/// `is_feature_disabled` reads the top level only, and website-scoped
/// `page-footer: false` reaches this transform as an *absent*
/// `navigation.footer` — indistinguishable from "no footer configured"
/// unless the synthesis branch checks the website scope itself.
#[tokio::test]
async fn website_scoped_page_footer_false_also_suppresses_the_copy() {
    let mut meta = config_map(vec![(
        "website",
        config_map(vec![("page-footer", b(false))]),
    )]);
    meta.insert_path(
        &["rendered", "navigation", "footer-actions"],
        s("<div class=\"toc-actions\">ACTIONS</div>"),
    );
    let (meta, _) = run(meta).await;
    assert!(
        meta.get_path(&["rendered", "navigation", "footer"]).is_none(),
        "website.page-footer: false must not synthesize a footer"
    );
}

#[tokio::test]
async fn no_footer_and_no_actions_still_skips() {
    let (meta, _) = run(config_map(vec![])).await;
    assert!(meta.get_path(&["rendered", "navigation", "footer"]).is_none());
}
```

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p quarto-core footer_render`
Expected: FAIL — exactly two of the five fail: `footer_actions_are_appended_to_a_configured_footer` and `footer_is_synthesized_when_only_actions_exist`. The other three pass against the *unmodified* transform, and it is worth knowing why before you start:

- `page_footer_false_suppresses_the_actions_copy` and `no_footer_and_no_actions_still_skips` pass because the current code returns early in both situations.
- `website_scoped_page_footer_false_also_suppresses_the_copy` passes **trivially** today — `navigation.footer` is absent, so the unmodified transform returns early for the right result by accident. It is a **regression guard, not a red-green test**: it goes red only if you add the synthesis branch *without* the website-aware re-check, which is precisely the mistake D-3 exists to prevent. If you want to see it fail, add the synthesis branch first and run it before adding the guard.

- [x] **Step 3: Implement**

In `crates/quarto-core/src/transforms/footer_render.rs`, keep the two existing early returns at the top of `transform` **unchanged and in place** — `is_feature_disabled(&ast.meta, "page-footer")` (`:66`) and the `rendered.navigation.footer` override check (`:70`). Then replace the block that reads `navigation.footer` — both the `let Some(footer_cv) = … else { return Ok(()) };` and the `let mut footer = PageFooter::from_config_value(footer_cv);` that follows it (`:76-80`) — with:

```rust
        // The repo-actions copy that belongs inside
        // `.nav-footer-center` (bd-repo-actions-missing-99ezd2fe).
        let actions_html = ast
            .meta
            .get_path(&["rendered", "navigation", "footer-actions"])
            .and_then(|v| v.as_plain_text())
            .filter(|s| !s.is_empty());

        let mut footer = match ast.meta.get_path(&["navigation", "footer"]) {
            Some(footer_cv) => PageFooter::from_config_value(footer_cv),
            None => {
                // Q1 `handleRepoLinks` builds the whole
                // `<footer><div.nav-footer><div.nav-footer-center>`
                // chain when none exists, purely to host the
                // small-screen actions copy. With nothing to host,
                // there is still no footer.
                if actions_html.is_none() {
                    return Ok(());
                }
                // Decision D-3. An absent `navigation.footer` has two
                // very different causes: nothing was configured, or
                // `page-footer: false` was — and `resolve_page_footer`
                // erases the difference by returning `None` for both
                // (`quarto-navigation/src/footer.rs:246-250`). The
                // top-level gate above cannot see the website-scoped
                // spelling, so re-check it here through the
                // website-aware helper. Without this, the one config
                // that explicitly asked for no footer gets one.
                if resolve_website_value(&ast.meta, "page-footer")
                    .and_then(|v| v.as_bool())
                    == Some(false)
                {
                    return Ok(());
                }
                PageFooter::default()
            }
        };
        footer.center_append = actions_html;
```

Add `use quarto_config::resolve_website_value;` to the imports.

Update the module doc comment's "Skip conditions" list: `navigation.footer` absent **and** either no `footer-actions` or `page-footer: false` at either scope.

- [x] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p quarto-core footer_render`
Expected: PASS — the 5 new tests plus every pre-existing footer test.

- [x] **Step 5: Gate and commit**

```bash
cargo clippy -p quarto-core --all-targets -- -D warnings
git add crates/quarto-core/src/transforms/footer_render.rs
git commit -m "Append repo actions to the footer, synthesizing one if needed (bd-repo-actions-missing-99ezd2fe)"
```

### Task 9: Pipeline registration

**Files:**
- Modify: `crates/quarto-core/src/pipeline.rs:1364-1365` (**between `TocLocationTransform` and `NavbarRenderTransform`** — see Step 3; registering later than `SidebarRenderTransform` at :1366 silently breaks one of the four TOC placements), plus the numbered stage list in the doc comment at 1059-1071

- [x] **Step 1: Write the failing test**

The pipeline has an ordering test, `test_build_transform_pipeline_phase_ordering`. Add a companion in the same test module:

Model the call on the existing `test_build_transform_pipeline_phase_ordering` (`pipeline.rs:3683`), which uses the module's `make_test_runtime()` helper (`:1622`):

```rust
#[test]
fn repo_actions_render_sits_between_its_producers_and_consumers() {
    let pipeline = build_transform_pipeline(
        vec![],
        vec![],
        make_test_runtime(),
        "html".to_string(),
        None,
        Default::default(),
        None,
    );
    let names: Vec<&str> = pipeline.iter().map(|t| t.name()).collect();
    let pos = |want: &str| {
        names
            .iter()
            .position(|n| *n == want)
            .unwrap_or_else(|| panic!("`{want}` must be in the html pipeline; got {names:?}"))
    };
    let actions = pos("repo-actions-render");

    assert!(
        pos("toc-render") < actions,
        "repo actions read rendered.navigation.toc to decide placement"
    );
    // The one that is easy to get wrong: `toc_block_html` runs inside
    // SidebarRenderTransform, so the website-left placement reads
    // `toc-actions` during that transform, not at template time.
    assert!(
        actions < pos("sidebar-render"),
        "sidebar-render builds the website-left TOC nav in Rust and must see toc-actions"
    );
    assert!(
        actions < pos("footer-render"),
        "footer-render consumes rendered.navigation.footer-actions"
    );
}
```

Confirm `SidebarRenderTransform::name()` returns `"sidebar-render"` before relying on the literal.

- [x] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p quarto-core repo_actions_render_sits_between`
Expected: FAIL with "`repo-actions-render` must be in the html pipeline".

- [x] **Step 3: Register the transform**

In `crates/quarto-core/src/pipeline.rs`, immediately **after** `pipeline.push(Box::new(TocLocationTransform::new()));` (`:1364`) and **before** `pipeline.push(Box::new(NavbarRenderTransform::new()));` (`:1365`):

```rust
    // Repository action links (bd-repo-actions-missing-99ezd2fe).
    // Ordering is load-bearing in both directions.
    //
    // AFTER `TocRenderTransform` (:1358): the TOC copy is emitted only
    // when `rendered.navigation.toc` is non-empty, and Q-13-13 is
    // gated on the same flag.
    //
    // BEFORE `SidebarRenderTransform` (:1366): for the website-left
    // placement the TOC's `<nav>` is built in Rust, by `toc_block_html`
    // — called from `SidebarRenderTransform`, its only caller. Running
    // after it would leave that one placement with no actions while the
    // three template-emitted placements worked, a failure no test using
    // the default `toc-location: right` can see.
    //
    // BEFORE `footer_render_stage` (:1396): `FooterRenderTransform`
    // consumes `rendered.navigation.footer-actions`.
    //
    // Not gated on format: revealjs's template includes neither the
    // `toc-block` partial nor the html footer stage, so both slots are
    // inert there.
    pipeline.push(Box::new(RepoActionsRenderTransform::new()));
```

Add `RepoActionsRenderTransform` to the `use crate::transforms::{…}` list at the top of the file, and add a numbered entry to the stage list in the doc comment — **between `13. TocRenderTransform` and `14. NavbarRenderTransform`** (`pipeline.rs:1063-1064`), matching the new position. There is no `PageNavRenderTransform` entry in that list to anchor against.

- [x] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p quarto-core pipeline`
Expected: PASS, including `test_build_transform_pipeline_phase_ordering` — `RepoActionsRenderTransform` declares `TransformPhase::Navigation`, the same rank as its neighbours.

- [x] **Step 5: Gate and commit**

```bash
cargo clippy -p quarto-core --all-targets -- -D warnings
git add crates/quarto-core/src/pipeline.rs
git commit -m "Register the repo-actions transform in the html pipeline (bd-repo-actions-missing-99ezd2fe)"
```

### Phase 4 gate


- [x] Run `cargo nextest run --workspace`. Expect **+9 passed** (3 from Task 7, 5 from Task 8, 1 from Task 9). This is the first phase where rendered output changes.

  **Result: 13179 passed, 199 skipped** against Phase 3's 13170/199 — **exactly +9, skipped unchanged, zero
  failures.** `git status --short` was empty afterwards, so no `.snap` was regenerated and the
  `phase5-single-doc-baseline` sha256 gate did not move. Zero movement in existing fixtures, as predicted.

  **The feature was also confirmed live end-to-end at this boundary** (ahead of Task 12's formal record), on the
  same `/tmp/q2-repro-repo-actions` project Task 0 used:

  ```
  $ q2 render && grep -o "Edit this page\\|View source\\|Report an issue" _site/index.html | sort | uniq -c
     2 Edit this page
     2 Report an issue
     2 View source
  $ grep -o 'class="toc-actions[^"]*"' _site/index.html
  class="toc-actions"
  class="toc-actions d-sm-block d-md-none"
  ```

  Six links where Task 0 recorded zero, in both placements, with the responsive classes on the footer copy only —
  exactly Q1's shape. Output inspected by the orchestrator.

  **Expect zero movement in existing fixtures, and treat any as a defect rather than a snapshot to refresh.** Two facts make that a hard assertion rather than a hope: no test, fixture, or snapshot anywhere in the tree configures `repo-actions` or `repo-url` (so nothing existing can gain links), and Task 4's `render_footer_region` rewrite is byte-identical when `center_append` is `None` (so no existing footer can shift by a single character).

  The artifact to watch is `crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt` — a sha256 byte-identity gate on `doc.html`, driven from `tests/integration/artifact_scoping_pipeline.rs`. Its `doc.qmd` has no `toc:` and no footer, so neither change should reach it. **If that hash moves, the refactor is wrong — do not regenerate the baseline.**

  Likewise for `.snap` files: none currently reference `nav-footer-center` or `doc-toc`, so a changed snapshot means something rendered that should not have.

---

## Phase 5 — Integration, smoke-all, and verification

### Task 10: Pipeline integration tests

**Files:**
- Create: `crates/quarto-core/tests/integration/repo_actions_pipeline.rs`
- Modify: `crates/quarto-core/tests/integration/main.rs`

- [x] **Step 1: Write the tests**

Create `crates/quarto-core/tests/integration/repo_actions_pipeline.rs`, copying the harness from `breadcrumbs_pipeline.rs:1-105` verbatim (`canonical`, `write`, `read`, `render_project`, `find_html`) — that file drives the real `ProjectPipeline` end to end. Then:

**Do not use `\` line continuations in these format strings.** Rust's continuation strips the newline *and all leading whitespace on the next line*, which silently moves `repo-url` to column 0 — out of the `website:` block, where per D-6 it is not the action source at all — and then produces a hard YAML indentation error on the following line. Keep each YAML line as its own `\n`-terminated segment:

```rust
/// The standard fixture: a website with repo-actions and a TOC.
/// `website_extra` is spliced inside the `website:` block and must
/// therefore arrive already indented by two spaces.
fn fixture(project_dir: &std::path::Path, website_extra: &str, front_matter: &str) {
    write(
        &project_dir.join("_quarto.yml"),
        &format!(
            concat!(
                "project:\n",
                "  type: website\n",
                "website:\n",
                "  title: \"Site\"\n",
                "  repo-url: https://github.com/example/docs\n",
                "  repo-branch: main\n",
                "{}",
                "format:\n",
                "  html:\n",
                "    toc: true\n",
            ),
            website_extra
        ),
    );
    write(
        &project_dir.join("index.qmd"),
        &format!("---\ntitle: Home\n{front_matter}---\n\n## One\n\nText.\n"),
    );
}

#[test]
fn repo_actions_render_in_both_placements() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [edit, source, issue]\n", "");
    });
    let html = find_html(&outputs, "index.html");

    assert_eq!(html.matches("class=\"toc-actions").count(), 2, "one TOC copy, one footer copy");
    assert_eq!(html.matches("Edit this page").count(), 2);
    assert_eq!(html.matches("View source").count(), 2);
    assert_eq!(html.matches("Report an issue").count(), 2);

    assert!(html.contains("https://github.com/example/docs/edit/main/index.qmd"));
    assert!(html.contains("https://github.com/example/docs/blob/main/index.qmd"));
    assert!(html.contains("https://github.com/example/docs/issues/new"));

    // The TOC copy sits inside nav#TOC; the footer copy carries the
    // responsive classes and sits in .nav-footer-center.
    let nav = html.split("<nav id=\"TOC\"").nth(1).expect("nav#TOC");
    let nav = &nav[..nav.find("</nav>").expect("nav close")];
    assert!(nav.contains("class=\"toc-actions\"><ul>"), "plain classes in the TOC copy");

    let center = html.split("nav-footer-center").nth(1).expect("footer center");
    assert!(center.contains("toc-actions d-sm-block d-md-none"));
}

#[test]
fn footer_is_synthesized_when_no_page_footer_is_configured() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [edit]\n", "");
    });
    let html = find_html(&outputs, "index.html");
    assert!(html.contains("<footer class=\"footer\""));
    assert!(html.contains("nav-footer-center"));
}

#[test]
fn actions_follow_existing_footer_content() {
    let outputs = render_project(|dir| {
        fixture(
            dir,
            "  repo-actions: [edit]\n  page-footer:\n    center: \"Version 1.0\"\n",
            "",
        );
    });
    let html = find_html(&outputs, "index.html");
    let center = html.split("<div class=\"nav-footer-center\">").nth(1).expect("center");
    let center = &center[..center.find("</div>\n").unwrap_or(center.len())];
    assert!(
        center.find("Version 1.0").unwrap() < center.find("toc-actions").unwrap(),
        "configured text must precede the appended actions"
    );
}

#[test]
fn page_level_false_suppresses_actions_on_that_page_only() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [edit]\n", "repo-actions: false\n");
        write(
            &dir.join("other.qmd"),
            "---\ntitle: Other\n---\n\n## One\n\nText.\n",
        );
    });
    assert_eq!(find_html(&outputs, "index.html").matches("toc-actions").count(), 0);
    assert!(find_html(&outputs, "other.html").matches("toc-actions").count() > 0);
}

/// D-3, at the website scope — the one the transform's top-level gate
/// cannot see. If the synthesis branch forgets its website-aware
/// re-check, this test is what catches it.
#[test]
fn page_footer_false_suppresses_only_the_footer_copy() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [edit]\n  page-footer: false\n", "");
    });
    let html = find_html(&outputs, "index.html");
    assert!(!html.contains("nav-footer-center"), "no footer at all");
    let nav = html.split("<nav id=\"TOC\"").nth(1).expect("nav#TOC");
    assert!(nav.contains("toc-actions"), "the TOC copy is unaffected");
}

#[test]
fn no_toc_yields_a_single_always_visible_footer_copy() {
    let outputs = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            concat!(
                "project:\n",
                "  type: website\n",
                "website:\n",
                "  title: \"Site\"\n",
                "  repo-url: https://github.com/example/docs\n",
                "  repo-actions: [edit]\n",
                "format:\n",
                "  html:\n",
                "    toc: false\n",
            ),
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nText.\n");
    });
    let html = find_html(&outputs, "index.html");
    assert_eq!(html.matches("class=\"toc-actions").count(), 1);
    assert!(!html.contains("d-sm-block"), "no TOC copy to fall back from");
}

/// The website-left placement builds its `<nav>` in Rust
/// (`toc_block_html`, called from `SidebarRenderTransform`) rather than
/// through the `toc-block` template partial. It is the one placement a
/// wrong pipeline slot would silently break, and the only one this
/// suite covers besides the default `right`.
#[test]
fn website_left_toc_placement_gets_the_actions_copy() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [edit, source]\n", "");
        // Re-write _quarto.yml with toc-location: left; the sidebar
        // gives SidebarRenderTransform something to merge the TOC into.
        write(
            &dir.join("_quarto.yml"),
            concat!(
                "project:\n",
                "  type: website\n",
                "website:\n",
                "  title: \"Site\"\n",
                "  repo-url: https://github.com/example/docs\n",
                "  repo-branch: main\n",
                "  repo-actions: [edit, source]\n",
                "  sidebar:\n",
                "    contents:\n",
                "      - index.qmd\n",
                "format:\n",
                "  html:\n",
                "    toc: true\n",
                "    toc-location: left\n",
            ),
        );
    });
    let html = find_html(&outputs, "index.html");
    let nav = html.split("<nav id=\"TOC\"").nth(1).expect("nav#TOC");
    let nav = &nav[..nav.find("</nav>").expect("nav close")];
    assert!(
        nav.contains("toc-actions"),
        "the Rust-built TOC nav must carry the actions; if this fails, \
         repo-actions-render is running after sidebar-render"
    );
}

#[test]
fn nested_pages_get_paths_relative_to_the_project_root() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-actions: [source]\n", "");
        write(
            &dir.join("guide/intro.qmd"),
            "---\ntitle: Intro\n---\n\n## One\n\nText.\n",
        );
    });
    let html = find_html(&outputs, "guide/intro.html");
    assert!(html.contains("https://github.com/example/docs/blob/main/guide/intro.qmd"));
}

#[test]
fn repo_subdir_is_prepended() {
    let outputs = render_project(|dir| {
        fixture(dir, "  repo-subdir: website\n  repo-actions: [source]\n", "");
    });
    let html = find_html(&outputs, "index.html");
    assert!(html.contains("https://github.com/example/docs/blob/main/website/index.qmd"));
}

#[test]
fn issue_url_overrides_and_forces_an_issue_link() {
    let outputs = render_project(|dir| {
        fixture(
            dir,
            "  issue-url: https://example.com/bugs\n  repo-actions: [edit]\n",
            "",
        );
    });
    let html = find_html(&outputs, "index.html");
    assert!(html.contains("https://example.com/bugs"));
    assert!(html.contains("Report an issue"));
}

#[test]
fn link_target_and_rel_are_applied() {
    let outputs = render_project(|dir| {
        fixture(
            dir,
            "  repo-link-target: _blank\n  repo-link-rel: noopener\n  repo-actions: [edit]\n",
            "",
        );
    });
    let html = find_html(&outputs, "index.html");
    assert!(html.contains("target=\"_blank\" rel=\"noopener\" class=\"toc-action\""));
}

#[test]
fn no_repo_actions_configured_changes_nothing() {
    let outputs = render_project(|dir| {
        fixture(dir, "", "");
    });
    let html = find_html(&outputs, "index.html");
    assert!(!html.contains("toc-actions"));
    assert!(!html.contains("<footer class=\"footer\""));
}
```

- [x] **Step 2: Register the file**

In `crates/quarto-core/tests/integration/main.rs`, add `pub mod repo_actions_pipeline;` in alphabetical position.

- [x] **Step 3: Run the tests**

Run: `cargo nextest run -p quarto-core --test integration repo_actions_pipeline`
Expected: PASS, 12 tests. Any failure here is a real wiring bug — fix the implementation, not the assertion.

- [x] **Step 4: Gate and commit**

```bash
cargo clippy -p quarto-core --all-targets -- -D warnings
git add crates/quarto-core/tests/integration/repo_actions_pipeline.rs crates/quarto-core/tests/integration/main.rs
git commit -m "Cover repo actions end to end through ProjectPipeline (bd-repo-actions-missing-99ezd2fe)"
```

### Task 11: Smoke-all fixture

Task 10 drives `ProjectPipeline` in-process. The smoke-all suite drives the **document render path the binary uses** (`render_to_file`), which is a different entry point — exactly the divergence CLAUDE.md's end-to-end rule exists for. It also gives the three diagnostics a real assertion instead of a hand-read transcript.

**Files:**
- Create: `crates/quarto/tests/smoke-all/repo-actions/` — `_quarto.yml`, `actions.qmd`, `page-level-true.qmd`
- Create: `crates/quarto/tests/smoke-all/repo-actions/no-repo-url/` — `_quarto.yml`, `page.qmd`
- Create: `crates/quarto/tests/smoke-all/repo-actions/unknown-action/` — `_quarto.yml`, `page.qmd`

**Verified before writing this task** (do not re-litigate): a `project: type: website` fixture renders with full website chrome under this harness — `nav#TOC` and `.nav-footer-center` both present — and `render_to_file` returns the `_site/` path, which the runner follows via `result.output_path`. A deliberately impossible selector was confirmed to **fail** the run, so the assertions genuinely execute. This would be the **first** `type: website` fixture in the suite; every existing one is `type: default` or bare.

- [x] **Step 1: Write the project config**

`crates/quarto/tests/smoke-all/repo-actions/_quarto.yml`:

```yaml
project:
  type: website
website:
  title: Repo action tests
  repo-url: https://github.com/example/docs
  repo-branch: main
  repo-actions: [edit, source, issue]
```

Site-level config applies to every `.qmd` in this directory **and any subdirectory without its own `_quarto.yml`**. `actions.qmd` and `page-level-true.qmd` use it; the two fixtures that need a *different* site config get their own subdirectory in Step 3.

- [x] **Step 2: Write the happy-path fixture**

`actions.qmd`:

```markdown
---
title: Repo actions
format: html
toc: true
_quarto:
  tests:
    html:
      noErrors: true
      ensureHtmlElements:
        - [
            "nav#TOC div.toc-actions a.toc-action",
            "nav#TOC div.toc-actions i.bi.bi-github",
            "div.nav-footer-center div.toc-actions.d-sm-block.d-md-none",
          ]
      ensureFileRegexMatches:
        - [
            # Both placements, three actions each.
            'href="https://github\.com/example/docs/edit/main/actions\.qmd"',
            'href="https://github\.com/example/docs/blob/main/actions\.qmd"',
            'href="https://github\.com/example/docs/issues/new"',
            # Only the first link carries an icon (decision D-8).
            '<i class="bi bi-github"></i>Edit this page',
            '<i class="bi empty"></i>View source',
          ]
---

## One

Text.
```

- [x] **Step 3: Write the three diagnostic fixtures**

**Two of these need their own project, not a front-matter override.** The obvious shortcut — overriding `repo-url` with `""` or re-declaring `website.repo-actions` in front matter — makes the fixture's outcome depend on `MergedConfig` scalar-over-scalar and array-merge semantics that nothing in this plan has verified. If merging resolves differently than assumed, the fixture passes for the wrong reason or fails confusingly. Give each its own directory with its own `_quarto.yml` instead; nested fixture directories are an established pattern in this suite (`highlighting/04-filter/`, `metadata/project-inherits/`, `themes/theme-array/`).

`repo-actions/no-repo-url/_quarto.yml` — `repo-actions` with no `repo-url` anywhere, which is exactly what `Q-13-11` reports:

```yaml
project:
  type: website
website:
  title: No repo url
  repo-actions: [edit, source]
```

`repo-actions/no-repo-url/page.qmd`:

```markdown
---
title: No repo url
format: html
_quarto:
  tests:
    html:
      noErrors: true
      printsMessage:
        level: WARN
        regex: "Repository actions require a"
      ensureHtmlElements:
        - []
        - ["div.toc-actions"]
---

Text.
```

`repo-actions/unknown-action/_quarto.yml`:

```yaml
project:
  type: website
website:
  title: Unknown action
  repo-url: https://github.com/example/docs
  repo-branch: main
  repo-actions: [edit, publish]
```

`repo-actions/unknown-action/page.qmd`:

```markdown
---
title: Unknown action
format: html
_quarto:
  tests:
    html:
      noErrors: true
      printsMessage:
        level: WARN
        regex: "Unknown repository action"
      ensureHtmlElements:
        - ["div.toc-actions a.toc-action"]
---

Text.
```

The `regex:` values are matched against the diagnostic **title**, so keep them free of backticks and of the punctuation the message template wraps around code spans — match on a distinctive substring rather than the whole sentence.

`page-level-true.qmd` — this one *can* live beside `actions.qmd` under the top-level `repo-actions/` config, because `repo-actions: true` in front matter is read from the top-level slot directly (D-6) and never merges with the website-scoped list. `Q-13-13` at `INFO`, on a page with **no TOC** (D-11 gates it on exactly that):

```markdown
---
title: Page level true
format: html
toc: false
repo-actions: true
_quarto:
  tests:
    html:
      printsMessage:
        level: INFO
        regex: "Page-level `repo-actions: true` ignored"
      ensureHtmlElements:
        - ["div.nav-footer-center div.toc-actions"]
---

Text.
```

**Why the two WARN fixtures carry `noErrors: true`, and the INFO one does not.** `printsMessage` does **not** clear the default gate: `spec.rs:216-223` drops `check_warnings` only for `noErrors` / `noErrorsOrWarnings` / `shouldError`, and `runner.rs:238-247` then runs `NoErrorsOrWarnings` unconditionally. A fixture that expects a WARN and says nothing else therefore fails on the default gate before its own assertion is even considered. `page-level-true.qmd` emits `INFO`, which the gate does not trip, so it needs nothing.

**`printsMessage` matches the diagnostic *title* only** — `runner.rs:319-326` captures `diag.title` and discards the problem and hints. Match a distinctive substring of the title, and keep backticks out of the regex.

- [x] **Step 4: Run them**

Run: `SMOKE_FILTER=repo-actions cargo nextest run -p quarto --test integration smoke_all`
Expected: PASS. The filter is a substring match on the path relative to `smoke-all/`.

Then run the whole suite once — the fixtures are auto-discovered, so a malformed one breaks every other smoke test:

Run: `cargo nextest run -p quarto --test integration smoke_all`

- [x] **Step 5: Prove the assertions are live**

A smoke-all fixture that silently fails to assert looks identical to one that passes. Temporarily add an impossible selector (`"div.this-cannot-exist"`) to `actions.qmd`'s must-match list, re-run the filtered command, confirm it **fails**, then remove it. Do not skip this — it is the only thing distinguishing a real fixture from a decorative one.

- [x] **Step 6: Keep the render output out of git**

These are the suite's first `type: website` fixtures, and a website render writes a whole `_site/` tree beside each `_quarto.yml`. `runner.rs:249-259` cleans only the output file and its `<stem>_files` directory, so `_site/` survives the run — and **`_site` is not in `.gitignore`** (checked: no entry). Every existing smoke-all fixture renders a single file in place, which is why this has never come up.

Add the ignore entry in this commit:

```
crates/quarto/tests/smoke-all/**/_site/
```

Then confirm the tree is clean after a run:

```bash
SMOKE_FILTER=repo-actions cargo nextest run -p quarto --test integration smoke_all
git status --short crates/quarto/tests/smoke-all/
```

Expected: no untracked output. If anything else appears, widen the ignore rather than committing build output.

- [x] **Step 7: Commit**

```bash
git add crates/quarto/tests/smoke-all/repo-actions .gitignore
git commit -m "Add repo-actions smoke-all fixtures (bd-repo-actions-missing-99ezd2fe)"
```

### Task 12: End-to-end verification against Quarto 1

Tests passing is necessary but not sufficient — see CLAUDE.md, "End-to-end verification before declaring success". This task drives the real binary and inspects real output.

**Files:** none (verification only; results are recorded in this plan)

- [x] **Step 1: Render the repro with the real binary**

```bash
cargo build --bin q2                      # from the worktree
Q2=$(git rev-parse --show-toplevel)/target/debug/q2

cd /tmp/q2-repro-repo-actions
rm -rf _site
"$Q2" render
```

- [x] **Step 2: Count the links**

```bash
# NOT `grep -c` — that counts matching *lines*, and the whole page body is one
# line, so it reports 1 or 2 no matter how many links there are. Task 0's `-c`
# was fine only because its expected answer was 0. Count matches, not lines:
grep -o "Edit this page\|View source\|Report an issue" _site/index.html | wc -l
grep -o "Edit this page\|View source\|Report an issue" _site/index.html | sort | uniq -c
```

Expected: **6** total (three actions × two placements), and `2` of each of the
three labels in the `uniq -c` breakdown. Task 0 recorded `0`.

- [x] **Step 3: Compare the emitted blocks against Q1's**

The canonical Q1 output is in the strand and at
`/Users/gordon/src/q2-positron-docs/llms-info/repros/repo-actions-missing/_site-q1/index.html`.
Extract both `toc-actions` blocks from q2's output and diff them against Q1's:

```bash
# BSD/POSIX grep has no lazy quantifier — `.*?` would run to the LAST
# `</div>` on the line, and the whole page body is one line. Match the
# opening tags, then read the surrounding markup directly.
grep -o '<div class="toc-actions[^>]*>' _site/index.html
grep -o 'class="toc-action"[^>]*' _site/index.html
```

If `rg` is available, `rg -oP '<div class="toc-actions.*?</div>' _site/index.html` does give the whole block (PCRE mode supports lazy quantifiers).

Q1's TOC block, for reference — **pretty-printed here for readability; the real output at `_site-q1/index.html:91` is a single unbroken line**, which is what q2's emitter produces too, so diff on content rather than on line breaks:

```html
<div class="toc-actions"><ul>
<li><a href="https://github.com/example/example-docs/edit/main/index.qmd" class="toc-action"><i class="bi bi-github"></i>Edit this page</a></li>
<li><a href="https://github.com/example/example-docs/blob/main/index.qmd" class="toc-action"><i class="bi empty"></i>View source</a></li>
<li><a href="https://github.com/example/example-docs/issues/new" class="toc-action"><i class="bi empty"></i>Report an issue</a></li>
</ul></div>
```

and the footer block is the same with `class="toc-actions d-sm-block d-md-none"`.

Differences to expect and accept: the repro's `repo-url` differs from the fixture in Task 0 if you changed it; q2's footer carries an extra `.container-fluid` wrapper (a pre-existing, documented divergence at `render_html.rs:181`). Any other difference is a defect — fix it.

- [x] **Step 4: Check the whole render is clean**

Confirm exit code 0 and no unexpected diagnostics in the transcript.

- [x] **Step 5: Record the evidence in this plan**

Append to the **Verification record** section at the bottom of this file: the exact invocation, the grep count before and after, a paste of one emitted `toc-actions` block, and an explicit statement that the output was inspected. This is required by CLAUDE.md, not optional.

- [x] **Step 6: Commit the plan update**

```bash
git add claude-notes/plans/2026-08-24-repo-actions.md
git commit -m "Record end-to-end verification for repo actions (bd-repo-actions-missing-99ezd2fe)"
```

### Phase 5 gate

- [x] Run `cargo nextest run --workspace`. Expect the Task 10 integration tests (+12) and the smoke-all fixtures — note that smoke-all fixtures do **not** each register as a nextest case: they are discovered inside the single `smoke_all` test, so the workspace count grows by 12, not 16. Confirm `smoke_all` itself still passes; a malformed fixture fails the whole aggregate test, not just its own file.

  **Result: 13191 passed, 199 skipped** against Phase 4's 13179/199 — **exactly +12, skipped unchanged, zero
  failures.** As predicted the four new smoke-all fixtures add no nextest cases of their own; they are discovered
  inside the single aggregate test, which passed: `PASS [6.760s] quarto::integration smoke_all::smoke_all`.
  `git status --short` empty afterwards — no `_site/` artifact escaped the new `.gitignore` rule. Phase 5 closed.

---

## Phase 6 — Documentation and follow-up

### Task 13: User-facing documentation

**Files:**
- Create: `docs/guides/projects/repo-actions.qmd`
- Modify: `docs/_quarto.yml` (the `guides/projects` sidebar list, lines 32-42)

There is **no `docs/websites/` directory.** `docs/` contains `about.qmd`, `authoring/`, `errors/`, `guides/`, `index.qmd`, `presentations/`. Website-project features live in `docs/guides/projects/`, which already holds the sibling-feature precedent from the breadcrumbs port.

- [x] **Step 1: Read the sibling page and match its shape**

```bash
cat docs/guides/projects/breadcrumbs.qmd
ls docs/guides/projects/
```

`breadcrumbs.qmd` came from the same MVP-exclusion list via the same kind of strand, so its structure, depth, and tone are the target. Document usage, not internals (CLAUDE.md): the `repo-url`, `repo-branch`, `repo-subdir`, `issue-url`, `repo-actions`, `repo-link-target`, `repo-link-rel` keys; that the links appear at the foot of the TOC and again in the page footer; that `repo-actions: false` works per page; that `repo-actions: true` does not (link to `Q-13-13`).

- [x] **Step 1b: Add the sidebar entry**

In `docs/_quarto.yml`, add `- guides/projects/repo-actions.qmd` to the `guides/projects` list, after `breadcrumbs.qmd`.

**This list is maintained by hand and no lint rule covers it** — `error-docs-sidebar-unlisted` polices only `errors/`. A page omitted here still renders but is unreachable by navigation, which is exactly how the errors sidebar drifted to 153 of 211 pages before a rule was written for it.

- [x] **Step 2: Note the two known limitations**

Non-GitHub hosts get GitHub-shaped `edit` / `source` URLs, and notebook `edit` links exist only on GitHub. Both are Q1 parity; link the follow-up strand from Task 15 once it exists.

- [x] **Step 3: Render the docs site to check the page builds**

Run: `cargo run --bin q2 -- render docs/`
**Never** use the system `quarto` binary — the docs site is built with Quarto 2 (CLAUDE.md).

- [x] **Step 4: Commit**

```bash
git add docs/
git commit -m "Document website repo-actions (bd-repo-actions-missing-99ezd2fe)"
```

### Task 14: Path-resolution contract inventory row

**Files:**
- Modify: `claude-notes/designs/path-resolution-model.md` — the **`### Conforming`** table (header at 138-140), appended after the `URL-space emitters` row at line 147

- [x] **Step 1: Add the row**

Add a row recording the third exit this feature introduces — a consumer that takes the pivot form (project-root-relative, forward slashes) and emits an **absolute external URL**, resolving through neither `page_url_for` nor a filesystem read:

The table's columns are **`Site | Keys | Mechanism`** — the middle column names the *config keys*, not the output. Match that:

```markdown
| `transforms/repo_actions_render.rs` + `quarto-navigation::repo_actions` | `repo-url`, `repo-branch`, `repo-subdir`, `issue-url` | absolute external URLs built from the pivot form (`page_relative_source`); **neither rule-2 exit** — no `page_url_for`, no filesystem read. `repo-subdir` is a *repository*-namespace path, outside this contract entirely. |
```

It belongs under `### Conforming`, not under `### VIOLATIONS`: the consumer takes the documented pivot form and never joins a config string onto a base it picked itself. Read the surrounding rows before writing it.

- [x] **Step 2: Commit**

```bash
git add claude-notes/designs/path-resolution-model.md
git commit -m "Record the external-URL exit in the path-resolution inventory (bd-repo-actions-missing-99ezd2fe)"
```

### Task 15: File the multi-host follow-up strand

**Files:** none (braid only — nothing to commit)

- [x] **Step 1: Create the strand**

```bash
braid create "repo-actions assume GitHub URL shapes on every host" \
  -t feature -p 3 -l parity -l websites \
  --deps discovered-from:bd-repo-actions-missing-99ezd2fe --json \
  -d "repo-actions builds edit/source URLs with GitHub's path shapes on every host. GitLab uses /-/edit/<branch>/<path> and /-/blob/<branch>/<path>; Gitea and Codeberg use /_edit/ and /src/. On those hosts the 'source' link is wrong for every file type, and the 'edit' link is dropped entirely for .ipynb pages (deliberately -- Q1 commit 5c2186680 suppresses notebook edit links, 967197b12 carves out github.dev only, and that allowlist never grew).

Ported as-is in bd-repo-actions-missing-99ezd2fe for Q1 parity; this strand covers doing better. Needs a host-detection table mapping a repo-url host to its edit/blob path shapes, plus a decision on what to do for unrecognized hosts.

Upstream: quarto-dev/quarto-cli#5301 (open, 2023-04-25), with #7155 and #12138 closed as duplicates. None of those reporters mention notebooks."
```

- [x] **Step 2: Note nothing is committed** — braid stores strands in the synced skein, not in git.

### Phase 6 gate

- [x] Run `cargo nextest run --workspace`. This phase touches only `docs/`, a design note, and the braid skein, so the delta must be **zero**.

  **Result: 13191 passed, 199 skipped** — identical to the Phase 5 gate. Zero delta, as required.
 A non-zero delta means a docs change reached compiled code — most likely an `error_catalog.json` edit that belonged in Phase 2.

### Task 16: Reconcile and close

- [x] **Step 1: Re-read this plan and verify every checkbox against reality.** Do not trust stale checkmarks — confirm each landed. Correct any that are wrong.

- [x] **Step 2: Full verification**

```bash
cargo xtask verify
cargo xtask lint
```

**Full `verify`, not `--skip-hub-build`.** The skip flag is appropriate only when nothing WASM-facing changed; this plan touches `quarto-core` and `quarto-navigation`, both of which `wasm-quarto-hub-client` depends on, and the WASM target builds separately from `cargo build --workspace`. `cargo xtask lint` is not optional either — Task 5 adds three error codes, and both `error-docs-page-missing` and `error-docs-sidebar-unlisted` are repo-level rules that only this command runs.

- [x] **Step 3: Final workspace run and delta report**

Run `cargo nextest run --workspace`; report the pass/skip delta against the Task 0 baseline and account for every test added.

**Full verification result — both green.**

- `cargo xtask verify` (full, *not* `--skip-hub-build`): **all 14 steps passed**, including the WASM leg and
  the hub-client build/test steps that `cargo build --workspace` cannot cover. Exit 0.
- `cargo xtask lint`: `All checks passed! (1045 files checked)` — covers `error-docs-page-missing` and
  `error-docs-sidebar-unlisted` for the three new `Q-13-*` codes, and `metadata-as-str` for the transform.

**Final workspace delta against the Task 0 baseline.** Baseline was **13130 passed, 199 skipped**; final is
**13191 passed, 199 skipped** — **+61 passed, skipped unchanged, zero failures.** Every added test accounted for:

| Source | Tests | Running total |
| --- | ---: | ---: |
| Task 2 — `repo_action_links()` unit tests | +19 | 13149 |
| Task 2 fix round — `none_still_leaves_the_issue_url_link` | +1 | 13150 |
| Task 3 — `repo_actions_to_html()` unit tests | +5 | 13155 |
| Task 4 — `PageFooter::center_append` unit tests | +3 | 13158 |
| Task 6 — `RepoActionsRenderTransform` unit tests | +12 | 13170 |
| Task 7 — TOC partial + Rust twin | +3 | 13173 |
| Task 8 — footer copy and synthesis | +5 | 13178 |
| Task 9 — pipeline ordering | +1 | 13179 |
| Task 10 — `ProjectPipeline` integration tests | +12 | 13191 |
| Task 11 — smoke-all fixtures | +0 | 13191 |

Task 11 adds 0 nextest cases by design: the four fixtures are discovered *inside* the single aggregate
`smoke_all` test, which passes. Task 5 (catalog + docs) and Tasks 13-15 (docs, design note, braid strand) add
no tests, and their phase gates each confirmed a zero delta.

- [ ] **Step 4: Commit the reconciled plan, then ask for permission to push.** Never push without it.

- [ ] **Step 5: Close the strand**

```bash
braid close bd-repo-actions-missing-99ezd2fe --reason "repo-actions render in both Q1 placements; verified end-to-end against the repro"
```

---

## Verification record

*Filled in by Task 12. Required by CLAUDE.md's end-to-end verification rule — a plan that reaches Task 16 with this section empty is not done. The three diagnostics are asserted mechanically by Task 11's smoke-all fixtures, so they need no hand-recorded transcript here.*

### Baseline (Task 0)

- Workspace tests: `cargo nextest run --workspace` → **13130 passed, 199 skipped** (112.7s, exit 0)
- `grep -c "Edit this page\|View source\|Report an issue" _site/index.html` → `0`

### After implementation (Task 12)

- **Invocation:**

  ```bash
  cargo build --bin q2
  Q2=$(git rev-parse --show-toplevel)/target/debug/q2
  cd /tmp/q2-repro-repo-actions && rm -rf _site && "$Q2" render
  ```

  Exit code **0**. Transcript was exactly the two normal lines — `Rendering project:
  /private/tmp/q2-repro-repo-actions (type: website)` and `Rendered 1 of 1 files to
  /private/tmp/q2-repro-repo-actions/_site`. **No diagnostics of any kind**, expected or
  otherwise (this project is correctly configured, so none should fire).

- **Link count: 6**, from `grep -o … | wc -l`, with `sort | uniq -c` giving `2` of each
  of the three labels — three actions × two placements. **Task 0 recorded `0`.**

  ```
     2 Edit this page
     2 Report an issue
     2 View source
  ```

- **Emitted TOC block** (single unbroken line in the real output; wrapped here only to fit):

  ```html
  <div class="toc-actions"><ul><li><a href="https://github.com/example/example-docs/edit/main/index.qmd"
  class="toc-action"><i class="bi bi-github"></i>Edit this page</a></li><li><a
  href="https://github.com/example/example-docs/blob/main/index.qmd" class="toc-action"><i
  class="bi empty"></i>View source</a></li><li><a
  href="https://github.com/example/example-docs/issues/new" class="toc-action"><i
  class="bi empty"></i>Report an issue</a></li></ul></div>
  ```

- **Emitted footer block:** identical to the above except the wrapper class, which is
  `class="toc-actions d-sm-block d-md-none"` — the responsive classes Q1 adds only when the
  TOC copy also landed.

- **Comparison against Quarto 1: byte-identical.** Both blocks were extracted from q2's
  output and from Q1's canonical render at
  `/Users/gordon/src/q2-positron-docs/llms-info/repros/repo-actions-missing/_site-q1/index.html`
  with `rg -oP '<div class="toc-actions.*?</ul></div>'`, and `diff` reported **no
  differences at all**:

  ```
  Q1 blocks: 2   q2 blocks: 2
  $ diff q1-blocks.txt q2-blocks.txt
  *** BYTE-IDENTICAL TO QUARTO 1 ***
  ```

  Every element of the port is confirmed by that equality: URL construction for all three
  actions (`edit/`, `blob/`, `issues/new`), the branch and path segments, first-link-only
  icons (`bi bi-github` on `edit`, `bi empty` on the rest — decision D-8), the `toc-action`
  anchor class, list structure, and the footer copy's responsive classes. The
  `.container-fluid` footer-wrapper divergence anticipated in Step 3 does not appear inside
  these blocks, so nothing needed to be excused.

- **Output inspected: yes** — by the orchestrating session, which ran the render, read the
  emitted HTML, and performed the Q1 diff above. Not inferred from the absence of errors.

### Diagnostics

Asserted by the Task 11 smoke-all fixtures (`no-repo-url.qmd`, `unknown-action.qmd`, `page-level-true.qmd`) via `printsMessage`. Record here only the confirmation that Task 11 Step 5's negative control failed as required:

- Negative control failed before removal: _(yes/no)_
