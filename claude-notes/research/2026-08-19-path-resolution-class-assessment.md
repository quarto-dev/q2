# Path resolution as a bug *class*: why PR #524 did not fix issue #455

**Date:** 2026-08-19
**Trigger:** user request to assess GH issue #455 ("include-in-header resolves
relative to each input file, not project root") against PRs merged since it was
filed — expectation was that it had been fixed; verification shows it has not.
**Status:** reviewed 2026-08-19; user approved direction. Applied same day:
§6.1 CLAUDE.md bullet, §6.2 contract promotion
(`claude-notes/designs/path-resolution-model.md` is now normative), §6.4
braid bookkeeping, and the §6.3 lint filed as bd-40luf359. A `PathLike`
trait variant of §6.3 was considered and rejected (costs over benefits);
the nominal-enforcement benefit is retained via the distinctively named
accessor form. Remaining open work: the §6.2 mechanism convergence and
B/E-group fixes (bd-oejuizi9, bd-hjv5o, bd-oqoozmtr, bd-rdcvjy2s) and the
lint itself (bd-40luf359).
**Related:** `claude-notes/designs/path-resolution-model.md` (the model),
bd-oejuizi9 (the bug), bd-hjv5o (the deferred audit),
`claude-notes/plans/2026-08-13-site-root-relative-paths.md` (PR #524),
`claude-notes/plans/2026-05-20-bd-qor9a-metadata-path-resolution.md` (the mechanism).

---

## 1. Empirical verification (main @ e6ac236d, 2026-08-19)

Issue #455's exact fixture (project `_quarto.yml` with
`format.html.include-in-header: [custom-header.html]`, inputs `index.qmd` and
`sub/index.qmd`), rendered with `cargo run --bin q2 -- render <fixture>`:

| Variant | Result |
|---|---|
| `- custom-header.html` in `_quarto.yml` | **Still broken, verbatim.** Q-5-4 looking for `<fixture>/sub/custom-header.html`; `_site/index.html` has the header, `_site/sub/index.html` does not. |
| `- /custom-header.html` (leading `/`) | **Also broken.** Treated as OS-absolute `/custom-header.html`; Q-5-4 ×2; header missing from *all* pages. |
| Same key moved to project-root `_metadata.yml` | **Silently ignored for every input** (root and sub), zero warnings. Worse than the issue's report, which only observed the root-input case. |

Resolution is still `doc_dir.join(rel_path)` in
`crates/quarto-core/src/stage/stages/include_resolve.rs:499`
(`read_include_file`); the module docs (`include_resolve.rs:45-49`) still
describe resolution as document-relative "for the first cut."

The intended semantics are settled (cscheid's comment on #455, and rule 1 of
`designs/path-resolution-model.md`): **no leading `/` → relative to the
directory of the file that declared the path; leading `/` → project root.**
For `_quarto.yml` the declaring dir *is* the project root, so the reporter's
expectation and the Q2 rule coincide on this fixture.

## 2. Why #524 did not naturally involve a fix

Four cooperating mechanisms, ordered from surface to root cause.

### 2.1 Two path spaces, and the decree was scoped to one

#524's design decree ("a Quarto path with a leading `/` means
site-root-relative, uniformly" — decision 4 of the 2026-08-13 plan) governs
**URL space**: strings that survive into emitted HTML (image `src`, link
`href`, logo). The plan's carve-out section *explicitly excluded filesystem
space* — paths the renderer reads at build time. `include-in-header` is
filesystem-space, so it sat on the far side of a deliberate scope boundary.
The boundary was reasonable for shipping #524; the failure is that nothing
recorded "the same decree must eventually govern filesystem space too" as
actionable work.

### 2.2 The survey methodology could not see this site

#524's Q4 survey enumerated leading-`/` *interpretation sites* — code that
does something with a leading slash — by searching for such handling.
`include_resolve.rs` contains no leading-`/` handling at all (bare
`Path::join`, which silently treats `/x` as OS-absolute), so it was invisible
to the survey. **A grep for implementations of a convention is structurally
blind to sites that fail to implement it.** The complement search — "every
site that consumes a config-originated path," which this assessment ran (§4)
— is the sweep that finds the class.

### 2.3 The knowledge existed, fragmented and unlinked

Before #455 was even filed, three artifacts already described its exact class:

- **bd-hjv5o** (2026-05-20) — the deferred "generalization audit" from
  bd-qor9a, listing `include-in-header`, `css`, `theme`, `template`,
  `bibliography`, listing `contents:` by name.
- **bd-oejuizi9** (2026-06-10) — the bug itself, for theme/css/include.
- **`designs/path-resolution-model.md`** (2026-06-10) — the two-rule model,
  including a "where the model is NOT yet applied" section naming this gap.

None of the three were linked to each other; none were linked to #455; #524's
investigation cited none of them and re-derived its own partial map by grep.
The design note is a *descriptive index* ("Status: Index / consolidation
note"), not a contract anyone is instructed to consult, so nothing routed
#524's author through it.

### 2.4 No shared representation: every consumer picks its own base

This is the root cause and the one the user's question targets. A path
authored in config reaches its consumer as a **bare string**, and each
consumer independently chooses a base directory to join it against. The
correct machinery exists — but adoption is opt-in per call site, and there are
now **three coexisting correct mechanisms** plus the incorrect ad-hoc joins:

1. **`resolve_metadata_path`** (`transforms/navigation_href.rs:583`, from
   bd-qor9a): SourceInfo provenance → declaring file's dir → project-root-
   relative. Adopted by navigation surfaces only (sidebar/navbar/footer
   generate transforms). Caveat: `_quarto.yml`'s FileId is typically not in
   the per-document SourceContext, so the helper *degrades to raw* for
   `_quarto.yml`-declared values — correct only because its callers happen to
   assume project-root-relative input.
2. **`ConfigValueKind::Path` + `adjust_paths_to_document_dir`**
   (`project/mod.rs:253-296`): rebases `Path`-kind values from declaring dir
   to consuming-doc dir at metadata-merge time. Applied to explicit `!path`
   tags and to extension-contributed values (which are force-marked via
   `FRAGMENT_PATH_PATTERNS` / `FORMAT_ASSET_PATTERNS` / `PATH_VALUED_KEYS`).
   User-authored plain strings are never marked, so this mechanism never sees
   them.
3. **Per-layer explicit bases for `format.html.css`**
   (`project/format_css.rs:104-130`, commit 37758160, 2026-08-14): the merge
   passes the correct `layer_base` per source (project dir / `_metadata.yml`
   parent / doc dir), marks the value `Path`, handles leading `/` at project
   root. This is the *newest* correct implementation — built two weeks ago,
   for one key.

The sharpest single illustration: **`css:` and `include-in-header:` are
declared in the same `_quarto.yml` block and appear in the same extension
path-pattern tables, and `css:` was fixed correctly on 2026-08-14 while
`include-in-header` was not** — because the css fix (bd-format-css-not-copied)
was scoped to the key its bug report named, exactly as #524 was scoped to the
surfaces its origin project hit. Each fix is locally rational; the class
survives every one of them.

A second asymmetry falls out of mechanism 2: an **extension-contributed**
`include-in-header` / `theme` / `template` / `filters` value is marked `Path`
and rebased correctly, while the **user-authored** value of the same key in
`_quarto.yml` is not. Same key, different behavior by declarer.

## 3. Timeline of the class

| Date | Event |
|---|---|
| 2026-02-17 | `!path` + `adjust_paths_to_document_dir` for `_metadata.yml` (dir-metadata plan) |
| 2026-05-20 | bd-qor9a lands `resolve_metadata_path` for navigation; Phase 6 generalization audit **deferred to bd-hjv5o** (names include-in-header explicitly) |
| 2026-06-10 | bd-oejuizi9 filed (theme/css/include per-doc-dir); `designs/path-resolution-model.md` written, gap section included |
| 2026-08-06 | **GH #455 filed** (include-in-header instance of the class) |
| 2026-08-07 | #455 discussion settles normative semantics (declaration-site-relative; `/` = project root) |
| 2026-08-14 | `format.html.css` instance fixed correctly (37758160) — class untouched |
| 2026-08-18 | **PR #524 merged** — URL-space decree; filesystem space carved out; class untouched |
| 2026-08-19 | This assessment: #455 verified still broken in all three variants |

## 4. Inventory of config-path consumption sites (snapshot)

Full sweep, grouped by the base-directory convention actually in use. This
snapshot is frozen evidence for this assessment; the **living** version of
this table belongs in `designs/path-resolution-model.md` once promoted to a
contract (§6.2).

### A. Declaration-site-relative (conforming to rule 1)

| Site | Keys | Mechanism |
|---|---|---|
| `transforms/{navbar,sidebar,footer}_generate.rs` | nav hrefs, logos | `resolve_metadata_path` (SourceInfo) |
| `glob/provenance.rs` `BaseDirContext` | `listing.contents`, front-matter `resources:` | SourceInfo root-file provenance; leading `/` → project root |
| `project/format_css.rs` + 3 call sites in `metadata_merge.rs` | `css` | explicit per-layer `layer_base`; leading `/` → project root |
| `project/mod.rs` fragment rebase, `extension/{paths,read}.rs` | extension-contributed theme/css/include-*/template/filters/etc. | force-marked `ConfigValueKind::Path`, rebased at merge |

### B. Consuming-document-dir (violates rule 1 for any non-frontmatter declarer)

| Site | Keys | Leading `/` |
|---|---|---|
| `stage/stages/include_resolve.rs:499` | `include-in-header` / `-before-body` / `-after-body` | OS-absolute (**#455**) |
| `stage/stages/apply_template.rs:199,415` | `template`, `template-partials` | OS-absolute |
| `filter_resolve.rs:255-273` | `filters` | OS-absolute |
| `quarto-sass/src/themes.rs:469-475` (+ `compile_theme_css.rs`, `revealjs/theme.rs`) | `theme` custom `.scss` | OS-absolute |
| `transforms/title_banner.rs:200` | `title-block-banner` image probe | OS-absolute |
| `project_resources.rs:721,835` | engine/filter-declared resources | OS-absolute |

### C. Project-root-relative (correct for `_quarto.yml`-only keys)

`website.favicon` (`website_config.rs:112`), navbar-logo/footer-image copy
hooks (`website_post_render.rs`), `project.render` / `project.resources`
(`discovery.rs:409`, `project_resources.rs:494`), sidebar `auto:`
(`sidebar_auto.rs:275`), `brand:` (`quarto-sass/src/config.rs:462`),
`{{< include >}}` shortcode leading-`/` (`include_expansion.rs:689`, the
bd-w9koo1i2 fix — markdown-space, not YAML).

### D. Emission-time page-relative rewriting (URL space, post-#524)

`resolve_static_resource_href` / `resolve_root_relative_resource_href` and
their callers (link_rewrite images, navbar/footer render, favicon link,
format_css emission, example_embed, listing item image rebase).

### E. No base at all (resolved against process CWD — additional gap)

| Site | Keys |
|---|---|
| `pampa/src/citeproc_filter.rs:133-135` | `csl` |
| `pampa/src/citeproc_filter.rs:151-153` | `bibliography` |

CWD-relative is worse than doc-dir-relative: behavior varies with the
directory `q2` is invoked from. Filed as a discovered-from strand (§6.4).

## 5. Assessment against the user's three-level framing

1. **"Same representation, explicitly, so behavior cannot deviate."** Correct
   diagnosis. The representation half-exists (`ConfigValueKind::Path`,
   SourceInfo provenance, the annotation table
   `pampa/src/pandoc/meta_annotations.rs` — which even has an unused
   `Interpretation::Path` variant awaiting schema-driven marking). What is
   missing is (a) marking user-authored strings for known path-shaped keys,
   and (b) a rule that consumers may not choose a base directory themselves.
2. **Temporary plain-text sweep instruction.** Agreed, and it should point at
   a *specific artifact* (the promoted contract with the inventory table), not
   just say "sweep" — §2.2 shows a naive sweep (grep for handlers) misses
   exactly the broken sites.
3. **Long-term non-constructibility.** The project already has the enforcement
   idiom: `cargo xtask lint`. The `metadata-as-str` rule is the exact
   precedent — same shape of bug (a per-call-site wrong default that type
   signatures permit), same remedy (ban the raw accessor pattern outside
   blessed modules, with `lint:allow` escape hatch).

## 6. Proposed remediation (for review — not yet applied)

### 6.1 Horizon 1: plain-text instructions (this week)

- **CLAUDE.md**: add a short "Path resolution is a bug class" bullet: any bug
  report or fix touching how a config-authored path is resolved must read
  `claude-notes/designs/path-resolution-model.md` and check the fix against
  the full consumption-site inventory there, not just the reported key.
  Scope-outs are fine but must be recorded as strands linked to bd-oejuizi9.
- **Braid hygiene**: link bd-oejuizi9 ↔ bd-hjv5o ↔ bd-r1y48cx0 as related;
  reference GH #455 from bd-oejuizi9. (Done, §6.4.)

### 6.2 Horizon 2: one mechanism, applied across the class

- Promote `designs/path-resolution-model.md` from index note to **normative
  contract** (peer of `transform-pipeline-phases.md`): the two rules, the two
  spaces (URL/filesystem — both governed by rule 1 + a rule-2 analogue), the
  blessed helpers, the living inventory table with per-site conformance
  status, and an author rule: *a consumer of a config-originated path MUST
  NOT choose its own base directory; it consumes an already-resolved value or
  calls a blessed resolver with provenance.*
- Converge the three correct mechanisms. The css `layer_base` approach
  (mechanism 3) is the most general — it works for `_quarto.yml` (where
  SourceInfo-based lookup degrades, §2.4) and runs at merge time, before any
  consumer. Recommended target: **generalize the css marking to every key in
  a shared path-shaped-key table** (unifying `FRAGMENT_PATH_PATTERNS`,
  `FORMAT_ASSET_PATTERNS`, `PATH_VALUED_KEYS`, the css matcher, and the
  annotation table into one registry), so values arrive at consumers already
  `Path`-marked and declaration-dir-resolved, and `adjust_paths_to_document_dir`
  handles the rest uniformly. Consumers like `include_resolve.rs` then drop
  their `doc_dir.join` for a plain read of the resolved value.
- Fix the B-group and E-group sites via that mechanism (bd-oejuizi9 for
  include/theme; new strands for template/filters/title-banner/bibliography).
  Leading `/` in filesystem space anchors at project root, matching css and
  the include shortcode.

### 6.3 Horizon 3: non-constructibility for new code

- **New xtask lint rule** (working name `config-path-base`): flag
  `join`-ing a config-originated string onto a base directory outside a
  blessed-module list (the resolver modules + metadata_merge), the same
  enforcement shape as `add-file-with-id` and `metadata-as-str`. With the
  H2 registry in place, the lint's blessed list is small and the rule is
  mostly "did you bypass the registry."
- Optional stronger form once consumers are migrated: make the raw-string
  read of a `Path`-kind value the awkward path (accessor returns the resolved
  form; raw access needs an explicitly named method), so the type system
  carries most of the weight and the lint only guards the perimeter.

### 6.4 Braid bookkeeping (applied with this assessment)

- Re-verification comment on bd-oejuizi9 (c-el1kfz7p, 2026-08-19).
- `related` links: bd-oejuizi9 ↔ bd-hjv5o, bd-oejuizi9 ↔ bd-r1y48cx0.
- New strand: `bibliography`/`csl` resolve against process CWD
  (discovered-from bd-oejuizi9).
- New strand: leading-`/` unsupported in filesystem-space config keys
  (include slots et al.) — the #455 workaround gap, discovered-from
  bd-oejuizi9. (bd-rdcvjy2s; the CWD strand above is bd-oqoozmtr.)
- New strand: the §6.3 lint rule — bd-40luf359, related to bd-oejuizi9.
