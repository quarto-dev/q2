# `.md` render support (bd-6d2wj4zp)

**Status:** design questions D1–D9 resolved with Carlos 2026-08-07 (see "Design
decisions"). One follow-up refinement adopted: the default render list is expressed
*literally* as `render: ["**/*.qmd"]` (see S2′). Implementation not yet started —
awaiting explicit go-ahead.

**Strand:** bd-6d2wj4zp — "Render .md files as inputs (explicit render-list opt-in;
ignore engine specs with warning)". Related: bd-xxul (the original "non-.qmd input
extensions" deferral from websites Phase 1 — this plan settles its `.md` half;
`.ipynb` rides separately with bd-19nc56ao).

## Overview

Support plain `.md` files as render inputs. Desired semantics (from the 2026-08-07
session):

1. `.md` files participate in a **project** render only when matched by explicit
   `project.render` globs — never via default input discovery.
2. Once included, a `.md` file behaves **exactly like a `.qmd`** file (same parser,
   shortcodes, includes, transforms, formats).
3. Engine specifications in a `.md` file are **ignored with a warning diagnostic**
   (not honored, not a hard error).

Motivating example: the Posit Connect docs port at
`~/Desktop/daily-log/2026/08/05/q2-connect-docs/docs-quarto-2` — 176 `.md` + 205
`.qmd` files, rendered via:

```yaml
project:
  render:
    - "**/*.md"
    - "**/*.qmd"
    - "!licenses/dashboard.licenses.md"
    - "!licenses/go.licenses.md"
    - "!news/NEWS.md"
    ...
```

Its navbar/sidebar reference `.md` files directly (`file: admin/index.md`), and its
`.md` sources use shortcodes (`{{< env … >}}`), raw HTML blocks, and `format:`
front matter — but **zero** `engine:` keys and zero executable cells. So the
"identical to `.qmd`, engines ignored" policy covers the real corpus exactly.

## What Quarto 1 actually does (research summary)

Research pass over `external-sources/quarto-cli` (HEAD `2e66958`, v1.10). Key
findings, with the surprises flagged:

- **Q1 includes `.md` in the render list *by default*** — this is the big
  divergence from what we want. Input discovery has no extension allow-list; a
  file is an input iff some engine claims it (`src/project/project-context.ts:932`),
  and the markdown engine claims `.md`/`.markdown`
  (`src/execute/markdown.ts:39-42`). With no `project.render`, a full directory
  walk picks up every `.md`. So our "explicit opt-in only" rule is a **deliberate
  departure from Q1**, not a match. (In practice most Q1 sites either have no stray
  `.md` or, like the Connect docs, use explicit globs anyway.)
  - Exception: **book** projects overwrite `project.render` with the chapter list
    (`src/project/types/book/book-config.ts:265`), so books effectively already
    have opt-in semantics.
- **Engine selection never sees a `.md` file's front matter.** The markdown engine
  claims `.md` by extension *before* YAML is read (`src/execute/engine.ts:320-330`),
  so `engine: jupyter` in a `.md` is **silently ignored** and overwritten to
  `markdown` (`src/command/render/render-contexts.ts:336`). Our warning is an
  improvement over Q1's silence.
- **Executable cells in `.md` are a hard error at execute time**:
  "You must use the .qmd extension for documents with executable code"
  (`src/execute/markdown.ts:78-98`). Notes: the check is `=== ".md"` so
  `.markdown` files escape it; and the regex counts *any* brace-fenced cell
  (`{ojs}`, `{mermaid}`, `{dot}` included), not just computational ones.
- **Otherwise `.md` is pipeline-identical to `.qmd`**: the qmd Lua reader is
  installed unconditionally for every Pandoc invocation
  (`src/command/render/pandoc.ts:1021`), so shortcodes, fenced-div handling, and
  the filter chain all apply. Includes too (`src/project/project-shared.ts:463`).
- **Freeze is disabled** for `.md` (`canFreeze: false`, `src/execute/markdown.ts:46`);
  `.md` inputs never touch `_freeze/`.
- **Default ignore globs** exclude `README.?([Rrq])md`, `CLAUDE.md`,
  `CLAUDE.local.md`, `AGENTS.md`, `AGENTS.local.md`, `*.llms.md`, `_*`, `.*`, plus
  `.gitignore` entries (`src/project/project-context.ts:878-886`), and `keep-md`
  intermediates (`<stem>.<format>.md`) are subtracted from the input set.
- **`.md` → `.md` output collision** is special-cased: `format: gfm` on `foo.md`
  outputs `foo-gfm.md` instead of clobbering the source
  (`src/command/render/output.ts:188-193`).

## Current q2 state (code map)

- **There is exactly one hard `.qmd` gate in the render path**:
  `has_qmd_extension` in `crates/quarto-core/src/project/discovery.rs:132`, used by
  the walker (`walk_qmd`, `:369`) and the candidate filter (`is_renderable_qmd`,
  `:94`). The module doc (`:9-13`) explicitly defers `.md`/`.ipynb` to a follow-up
  (that's bd-xxul). Exclusion rules (underscore/dot components, `node_modules`,
  case-insensitive `README` stem, output-dir) live in `is_renderable_qmd` and apply
  to **both** the default-walk and render-pattern paths — `.md` inherits them for
  free. Note `expand_patterns` (`:165`) matches patterns against the
  already-filtered walked set, so the walk itself must learn about `.md`, not just
  the filter.
- **Glob machinery is ready.** The shared glob API from #460/#461
  (`crates/quarto-core/src/glob/`) deliberately keeps extension policy out of the
  matcher — `claude-notes/designs/glob-semantics.md` §"What does *not* belong in
  `GlobOptions`" says discovery policy is the enumerator's job. `RENDER` consumer
  options have `default_positive: None`. The pinned consumer-options test at
  `glob/mod.rs:148-181` must be updated if any defaults change.
- **Single-file `q2 render foo.md` already almost works.** `classify_inputs`
  (`crates/quarto/src/commands/render.rs:209`) never checks the extension outside a
  project (the `:279` comment says "must be a `.qmd` file" but the code only
  rejects directories), and `ProjectContext::discover` wraps a single input with no
  extension check (`project/mod.rs:541`). Known gap: `detect_single_input_format`
  (`render.rs:1165-1171`) is `.qmd`-gated, so a `.md` file's front-matter `format:`
  is silently ignored and falls back to `"html"`.
  Inside a project, `q2 render foo.md` fails with Q-7-6 ("Input Excluded From
  Render List") whose hint — check `project.render` and underscore conventions —
  is misleading for the extension case.
- **q2's engine machinery makes "no execution for `.md`" nearly free.** Engine
  detection is metadata-only (`crates/quarto-core/src/engine/detection.rs` —
  explicit `engine:` key, engine map, or engine-specific top-level key like
  `jupyter:`; **no** code-cell sniffing), defaulting to the no-op `markdown`
  engine. `EngineExecutionStage` (`stage/stages/engine_execution.rs:193`) skips
  `markdown` and passes the AST through. So a `.md` with no engine metadata
  already does exactly the right thing; the only new behavior is *detecting and
  warning about* a non-trivial engine spec on a `.md` input. The stage can see the
  input path via `ctx.document.input`.
- **Pre-built seam:** `SourceType { Qmd, Markdown, Ipynb, Rmd }` at
  `crates/quarto-core/src/stage/data.rs:170-201` exists and is currently dead code
  — it's the intended dispatch point from bd-xxul.
- **pampa has no literal-markdown reader** — `"markdown"` normalizes to `"qmd"`
  (`crates/pampa/src/options.rs:257-263`). So "`.md` behaves precisely like
  `.qmd`" is not just easy, it's the *only* available semantics without new work.
  (Happily it's also what Q1 does and what we want.)
- **Other `.qmd`-shaped gates that affect the motivating example:**
  - Nav/body link classification: `transforms/navigation_href.rs:186/:243/:333`
    (`ends_with(".qmd")`) — non-`.qmd` hrefs are treated as static resources, so
    `file: admin/index.md` in a sidebar and `[x](other.md)` in a body would not be
    rewritten to `.html`. The dep graph's body-link edges derive from the same
    resolver (`project/dependency_graph.rs:147`).
  - Preview: `crates/quarto-hub/src/discovery.rs:122-131` (only `.qmd` enters the
    VFS), `crates/quarto-hub/src/watch.rs:244-247` (`is_qmd_file` watch filter),
    and TS side `ts-packages/preview-renderer/src/types/project.ts:45` plus iframe
    link handling (`iframePostProcessor.ts:252`, `iframeLinkHandlers.ts:114`).
  - Listing default: `glob/mod.rs:107` `default_positive: Some("*.qmd")`;
    `sidebar.auto` index detection hard-codes `index.qmd`
    (`transforms/sidebar_auto.rs:321`).
  - Diagnostics prose that says `.qmd`: Q-5-13 problem text (`discovery.rs:317`),
    Q-7-7 ("No renderable `.qmd` files matched", `render.rs:1372`), Q-7-6 hint,
    `Q-PROJECT-EMPTY` hint (`orchestrator.rs:1048`).
- **Output-path hazard:** `Format::output_path` just swaps the extension
  (`format.rs:463`), and `gfm`/`commonmark` map to `"md"` (`format.rs:286-287`) —
  so `foo.md` with `format: gfm` would produce `output == input` and **overwrite
  the source**. Q1 guards this; q2 currently doesn't.

## Proposed semantics (spec)

- **S1 — Renderable extensions.** A *renderable source* is `.qmd` (always) or
  `.md` (conditionally, per S2/S3). `.markdown` is not supported (D1).
- **S2 — Project discovery.** With no `project.render` key, discovery renders
  `.qmd` only — unchanged; `.md` files are invisible (deliberate divergence from
  Q1's default-inclusion). With `project.render` present, the walk collects both
  extensions and a candidate is included iff it matches a positive pattern and no
  negative one — for `.qmd` and `.md` alike. "Explicit" means *matched by an
  explicitly written render pattern*, not "the pattern syntactically mentions
  `.md`" (D2, option (a)). All existing exclusions (underscore/dot components,
  `node_modules`, README stem, output-dir) apply to `.md` unchanged.
- **S2′ — The default is a literal render list (adopted in review).** The
  invariant users can be told, verbatim: **omitting `project.render` is exactly
  equivalent to writing `render: ["**/*.qmd"]`.** There is no hidden ".md off by
  default" extension policy — the default *pattern* is `.qmd`-shaped, and writing
  your own render list replaces it, at which point globs mean what they say.
  This is achievable **by construction**, not just documentation:
  - The matcher's `**` matches zero segments (`**/*.qmd` matches root-level
    `about.qmd` — pinned test `glob/matcher.rs:243-244`), and a single-pattern
    match preserves walk order, so the match set *and order* of
    `render: ["**/*.qmd"]` equal the default walk's.
  - Implementation (refined during Phase 1): the default lives in the
    **enumerator**, not in `GlobOptions::RENDER.default_positive` —
    `discovery::effective_render_patterns` prepends
    `DEFAULT_RENDER_PATTERN` (`**/*.qmd`) whenever the author wrote no
    positive pattern (no `render:` key, or negations only), and everything
    flows through one walk-then-match path. Two reasons over the
    `default_positive` route: (1) the glob-layer injection only fires for
    negation-only lists, so the empty case would have needed enumerator
    logic anyway; (2) with raw-level detection, a *broken* positive pattern
    (`Q-5-14`/`Q-5-15`) yields an empty set plus its diagnostic instead of
    silently falling back to rendering everything. This also matches
    `glob-semantics.md` ("the walk is the enumerator's job") and leaves the
    pinned consumer-options table untouched.
  - Pleasant consequence: a **negation-only** render list
    (`render: ["!drafts/**"]`) now means "all `.qmd` minus these" — which is
    exactly the intent documented at `glob/mod.rs:137-139` ("a render list of
    only exclusions means walk the project, minus these") but which the current
    code does *not* implement (`expand_patterns` iterates positive globs only →
    empty set → `Q-PROJECT-EMPTY`). The injection fixes that latent bug, and
    keeps `.md` out of negation-only lists, consistently with the invariant.
  - The `consumer_options_are_as_documented` pinned test (`glob/mod.rs:148-181`)
    and the RENDER doc-comment must be updated.
  Note the built-in exclusions (README, underscore/dot, `node_modules`, D4 list,
  output-dir) are **not** part of the pattern default — they are enumerator-level
  filters applied in both modes, so the invariant holds without writing them as
  negations. See D4′ for their interaction with explicit positive patterns.
- **S3 — Single-file render.** `q2 render foo.md` works outside a project
  (front-matter `format:` honored — fixes the `detect_single_input_format` gap).
  Inside a project, the file must be in the render list, same as `.qmd`; the
  not-in-render-list diagnostic gains extension-aware wording.
- **S4 — Parse/pipeline semantics.** Identical to `.qmd`: same pampa parse, same
  metadata handling, shortcodes, includes, transforms, themes, formats. No
  literal-markdown mode (none exists; Q1 also applies qmd semantics to `.md`).
- **S5 — Engines.** A `.md` input never executes engines. If engine detection on
  a `.md` yields anything non-trivial — an explicit `engine:` key (string, array,
  or map) or an engine-specific top-level key (`jupyter:`, `knitr:`) — emit a new
  warning diagnostic (Q-code, see below) anchored at the offending metadata key,
  and skip all engine execution for that document. Executable cells without engine
  metadata pass through unexecuted — exactly like a `.qmd` without an `engine:`
  key in q2 today (deliberate divergence from Q1's hard error; see D3).
- **S6 — Output paths.** `foo.md` → `foo.html` via the existing mechanism. New
  guard: if a format's `output_path(input) == input`, refuse with an error
  diagnostic rather than overwriting the source (the `foo.md` + `format: gfm`
  case). See D7 for whether we adopt Q1's `foo-gfm.md` rename instead.
- **S7 — Links and navigation.** A project-relative link or nav `file:`/`href:`
  entry pointing at a **render-list member** is a source-document link and gets
  rewritten to its output href (`admin/index.md` → `admin/index.html`), for `.md`
  exactly as for `.qmd`. A link to a `.md` file *not* in the render list stays a
  static-resource link (see D6). Required for the Connect docs navbar/sidebar.
- **S8 — Listings / sidebar-auto.** Defaults stay `.qmd`-flavored (listing
  `default_positive: "*.qmd"`, sidebar-auto `index.qmd`). Explicit listing
  `contents:` globs matching render-list `.md` files should work, but this is a
  stretch goal (see D8).
- **S9 — Preview.** `q2 preview` treats `.md` render-list members like `.qmd`:
  synced into the VFS, watched for changes, links intercepted. Scoped as the final
  implementation phase (see D9).

### Q1 ↔ Q2 divergence table (all deliberate)

| Behavior | Q1 | Proposed Q2 |
|---|---|---|
| `.md` in default (glob-less) discovery | included | **excluded** |
| `engine: jupyter` in `.md` | silently ignored | ignored **with warning** |
| ```` ```{python} ```` cell in `.md` | hard error at execute | passes through unexecuted (same as engine-less `.qmd`), per D3 |
| `.md` + `format: gfm` | renamed output `foo-gfm.md` | **error** (D7) |
| `.markdown` extension | supported (with buggy cell check) | **not supported** (D1) |
| Explicit render entry naming an ignored file (e.g. `AGENTS.md`) | rendered (hidden ignores apply to walk only) | still excluded; `Q-5-13` explains (D4′) |
| Negation-only `project.render` | all inputs minus exclusions | same, `.qmd`-only via injected `**/*.qmd` (S2′ — currently a latent empty-set bug in q2) |
| Freeze for `.md` | n/a (`canFreeze: false`) | n/a — q2 has no freeze yet; `.md` never executes so nothing to freeze |

## New diagnostic: engine spec ignored on `.md`

- **Proposed code:** `Q-2-40` (markdown subsystem; next free after Q-2-39). The
  condition is document-level (fires for single-file renders too), so the
  `project` subsystem (`Q-5-*`) is wrong; there is no engine subsystem and this
  doesn't seem to justify allocating one. Alternative if reviewers prefer: keep a
  `Q-5-*`/`Q-7-*` code. (See D5.)
- **Shape** (builder pattern per `example_embed.rs:551`):
  - title: "Engine Specification Ignored for Markdown Input"
  - problem: "`.md` documents never execute engines; the `engine:` specification
    has no effect."
  - detail: names the requested engine(s).
  - hint: "Rename the file to `.qmd` if you need executable code."
  - location: the `engine:` (or `jupyter:`/`knitr:`) key's `SourceInfo` from the
    metadata `ConfigValue` map.
- **Emission site:** `EngineExecutionStage::run`, gated on
  `SourceType::from_path(&ctx.document.input) == Some(SourceType::Markdown)` —
  bringing the dead `SourceType` seam to life as bd-xxul intended.
- **Process:** catalog entry in `crates/quarto-error-catalog/error_catalog.json`
  (`since_version: "99.9.9"`), presence test in `lib.rs`, docs page
  `docs/errors/markdown/Q-2-40.qmd` (`status: stub` acceptable), then run
  `scripts/audit-error-codes.py` manually (not part of `xtask verify`).

## Design decisions (resolved with Carlos, 2026-08-07)

- **D1 — `.markdown` extension: NO, `.md` only.** `.markdown` is a one-line
  follow-up if ever requested. (Q1 supports it but its cell check misses it,
  suggesting no real usage.)
- **D2 — "Explicit inclusion" = matched by any positive render pattern** (option
  (a)): `render: ["chapters/*"]` picks up `chapters/notes.md`. Accepted
  conditional on the default render list being articulable as an actual render
  list a user could have written — which S2′ makes literally true
  (`render: ["**/*.qmd"]`). The opt-in moment is writing a `project.render` key
  at all. Carlos also asked for an SSG-landscape sanity check on keeping `.md`
  out of the default at all; the survey (session 2026-08-07) supported the
  opt-in stance: SSGs that render `.md` by default (Hugo, Eleventy, MkDocs,
  Docusaurus, Astro) all do so from a *dedicated content directory*, which is
  itself the opt-in boundary; Quarto renders general project trees where `.md`
  is heavily non-content (README/CHANGELOG/NEWS/licenses/agent files — Q1's
  reactively-grown ignore list and the Connect project's own negations are the
  evidence). Sphinx's "must be referenced in a toctree" is the closest analogue
  to our choice. Jekyll's front-matter-sniffing middle ground was considered
  and rejected as implicit magic. The strictness argument seals it: qmd is a
  strict dialect, and diagnostics about files the user never meant to publish
  are the worst kind of error.
- **D3 — Executable cells in `.md`: silent passthrough** (identical to
  engine-less `.qmd`). Revisit when/if q2 adds cell-language engine inference.
- **D4 — Adopt Q1's ignore list as built-in exclusions**: `CLAUDE.md`,
  `CLAUDE.local.md`, `AGENTS.md`, `AGENTS.local.md`, `*.llms.md`, in both
  discovery paths.
  - **D4′ (minor, open):** built-in exclusions are hard filters — an explicit
    positive pattern (even a literal `render: [AGENTS.md]`) does not override
    them; the user sees `Q-5-13` ("matched nothing"), whose prose should grow a
    mention of built-in exclusions. Q1, for comparison, applies its hidden
    ignore globs only to the walk, not to an explicit render list. Revisit only
    if a real need to render such a file appears.
- **D5 — Q-code: `Q-2-40`** (markdown subsystem). Carlos's additional argument:
  the warning fires for `q2 render foo.md` with no `_quarto.yml`, and users
  don't think of that as a "project", so `Q-5-*` would mislabel it.
- **D6 — Links to non-render-list `.md`: static-resource classification, no new
  warning** for now.
- **D7 — `output == input` collision: hard error** with an actionable hint
  (mention `output-file:`), not Q1's silent rename. Principle worth recording
  (Carlos): Q1 was structurally unable to emit good errors (no source tracking,
  Pandoc interop) and so veered toward "guess what the user meant"; Q2 leans
  "be more strict, but ensure good, actionable diagnostics follow".
- **D8 — Listings/sidebar-auto over `.md`: out of scope.** Follow-up strand if
  the Connect port hits it. `LISTING.default_positive` stays `*.qmd`.
- **D9 — Preview: in scope as final phase**, split into its own
  session/strand if it turns out heavy. It will definitely be wanted (the
  porting workflow is preview-driven).

## Appendix: how other SSGs treat `.md` inputs (survey, 2026-08-07)

Recorded because "why doesn't Quarto render my `.md` by default?" will recur in
user discussions. The question underneath is: *what is the opt-in boundary that
separates content from non-content markdown?*

| Tool | Renders `.md` by default? | Opt-in boundary |
|---|---|---|
| Hugo | yes | dedicated `content/` directory |
| MkDocs | yes | dedicated `docs/` directory |
| Docusaurus | yes | dedicated `docs/` + `blog/` directories |
| Astro | yes | dedicated `src/pages/` directory |
| Pelican / Zola | yes | dedicated `content/` directory |
| Eleventy | yes | **none** — input dir defaults to project root; compensates with built-in `node_modules` ignore + `.eleventyignore` + gitignore integration |
| Jekyll | conditionally | **front-matter gate** — `.md` with YAML front matter renders; without it, the file is copied as a static asset |
| Sphinx (MyST) | no | **explicit reference** — documents must appear in a `toctree`; orphans produce warnings |
| Quarto 1 | yes | **none** — walks the project tree; compensates with a reactively-grown ignore list (README, then `CLAUDE.md`, then `AGENTS.md`, then `*.llms.md`, …) |
| Quarto 2 (this plan) | **no** | **explicit render list** — default ≡ `render: ["**/*.qmd"]`; write your own list to include `.md` |

Observations:

1. **Every "renders `.md` by default" tool except Eleventy has a dedicated
   content directory.** The directory *is* the explicit opt-in: putting a file
   in `content/` is the same deliberate act as adding a glob to
   `project.render`. Those tools aren't evidence against opt-in — they're
   evidence that everyone needs *some* boundary.
2. **Tools that treat the project root as the source tree all develop
   compensating machinery**, because real project roots are full of
   non-content markdown (README, CHANGELOG, NEWS, CONTRIBUTING, LICENSE.md,
   vendored licenses, agent-instruction files). Eleventy grew ignore-file
   integration; Q1 grew its hidden ignore-glob list one embarrassment at a
   time; the Connect docs project *still* needs `!news/NEWS.md` and
   `!licenses/*.licenses.md` on top of Q1's defaults. Default-inclusion in a
   general project tree is structurally a whack-a-mole.
3. **Jekyll's front-matter sniffing** is the interesting middle ground (content
   self-identifies), but it's implicit magic — rendering behavior changes
   because of file *contents*, and a stray `---` block in a README flips it.
   Rejected for Q2.
4. **Sphinx is the closest relative** — a documentation tool whose sources live
   among other files, requiring explicit reference into the document tree, with
   diagnostics for orphans. That is essentially the Q2 position.
5. **The Q2-specific argument on top of the landscape:** qmd is a *strict*
   dialect. Arbitrary `.md` files not written for Quarto will produce parse
   diagnostics — and errors about files the user never asked to publish are
   the worst kind of error. Opt-in means every `.md` in the render list is
   there because someone asserted it is qmd-dialect content.
6. **Discoverability mitigation** so the opt-in default doesn't read as "Quarto
   ignored my files" silence: `Q-PROJECT-EMPTY` hints at present-but-unmatched
   `.md` files (Phase 1).

## Implementation plan

TDD throughout: each phase starts with failing tests. Phases are ordered so the
render path (the Connect-docs `q2 render` use case) lands before preview.

### Phase 1 — Discovery (project render list) — **DONE 2026-08-07**

- [x] Tests (in `discovery.rs` unit tests + orchestrator integration tests):
  - `.md` NOT discovered when `render_patterns` is empty
    (`md_is_not_discovered_without_render_patterns`)
  - **S2′ equivalence**: no-`render:`-key discovery ≡ `render: ["**/*.qmd"]`,
    same files *and order* (`default_discovery_equals_explicit_qmd_globstar`)
  - `.md` matched by explicit pattern IS included — extension glob, literal
    path, directory-shaped pattern per D2(a)
    (`md_is_included_when_matched_by_render_patterns`,
    `md_only_pattern_does_not_include_qmd`)
  - negation globs exclude `.md` (`negation_excludes_md_matches`)
  - negation-only render list = all `.qmd` minus exclusions, no `.md`
    (`negation_only_render_list_subtracts_from_the_default` — fixed the
    latent empty-set behavior)
  - underscore/dot/README/output-dir exclusions apply to `.md`
    (`md_respects_builtin_exclusions`)
  - D4 ignore list excluded even when matched
    (`agent_instruction_md_files_never_render`); literal positive naming one
    still excluded + Q-5-13 (`literal_pattern_naming_an_excluded_file_is_diagnosed`)
  - `.qmd`-only behavior unchanged (all 20 pre-existing discovery tests green
    without modification)
  - `unmatched_md_files` behavior (`unmatched_md_reports_optin_candidates`,
    `unmatched_md_skips_builtin_exclusions`, `unmatched_md_shrinks_as_patterns_match`)
  - CLI e2e: `empty_project_with_md_files_hints_at_render_list_optin`,
    `rendering_md_not_in_render_list_hints_at_optin` (render_cli_e2e.rs)
- [x] Default supplied by the enumerator: `effective_render_patterns` +
  `DEFAULT_RENDER_PATTERN` in discovery.rs (see S2′ mechanism note);
  `GlobOptions::RENDER` doc-comment updated, `default_positive` stays `None`,
  pinned consumer-options test untouched
- [x] Discovery generalized: single `walk_sources` (`.qmd` + `.md`) +
  `select_from_walk` replaces the walk/expand split; `is_renderable_source`
  with D4 `is_agent_instruction_md`; module docs rewritten
- [x] Q-5-13 prose → "renderable source file", mentions built-in exclusions
- [x] `Q-PROJECT-EMPTY` extra hint counts unmatched `.md` files and shows
  `"**/*.md"` (via new pure helper `discovery::unmatched_md_files`, called
  only in the empty-set path so `ProjectContext`/`discover_project_files`
  signatures stay unchanged)
- [x] Q-7-6 hint is extension-aware (`render_list_exclusion_hint`); Q-7-7
  prose → "renderable source files"
- [x] Full workspace suite green: 11011 passed / 197 skipped (one unrelated
  `quarto-hub` admin-collect flake passed in isolation and on re-run)
- [ ] `Q-7-6`/`Q-7-7` wording: extension-aware hint for `.md`-not-in-render-list
  (`render.rs:1358-1378`)

### Phase 2 — Engine-ignored warning — **DONE 2026-08-07**

- [x] Stage-level tests (engine_execution.rs): `engine: jupyter` and top-level
  `jupyter:` on `.md` → exactly one Q-2-40, AST passthrough; no engine
  metadata → silent; explicit `engine: markdown` → silent (harmless no-op,
  warning would be noise); unknown engine on `.md` → Q-2-40 only, no
  availability-fallback warning (skip happens before engine resolution)
- [x] E2e test (`md_with_engine_spec_renders_with_q_2_40_warning`): opted-in
  `.md` with `engine: jupyter` renders successfully through the real binary,
  Q-2-40 on stderr, content in output HTML
- [x] Catalog entry (after Q-2-39) + presence test + docs page
  `docs/errors/markdown/Q-2-40.qmd` (status: stub)
- [x] Emission in `EngineExecutionStage::run` gated on
  `SourceType::from_path(&ctx.document.input)` — the dormant `SourceType`
  seam is now live; diagnostic anchored at the `engine:` (or engine-name)
  metadata key via `ConfigMapEntry::key_source`
- [x] `scripts/audit-error-codes.py`: 171/171 consistent

### Phase 3 — Single-file path + output guard — **DONE 2026-08-07**

Findings that adjusted the plan (research-report conclusions were stale):

- Front-matter `format:` on a standalone `.md` was **already honored for
  native formats** — per-document format resolution in the pipeline reads
  front matter regardless of extension (`md_front_matter_format_revealjs_yields_reveal_deck`
  passed before any fix; kept as a pin). The `.qmd` gate in
  `detect_single_input_format` mattered for the **non-native bail-out**: a
  `.md` with `format: pdf` slipped past the early "not yet supported"
  refusal and rendered HTML bytes into `doc.pdf`. Fixed + pinned
  (`md_with_non_native_format_gets_early_refusal_like_qmd`).
- The D7 collision is **not** reachable via `format: gfm` today (non-native
  formats bail; `determine_output_paths` maps unknown formats to `.html`) —
  but `q2 render doc.qmd --output <abs path of doc.qmd>` **silently
  destroyed the source file** (verified live). The guard lives in
  `determine_output_paths` (`render_to_file.rs`) — the single chokepoint all
  native single-file *and* project pass-2 renders funnel through — and
  refuses `output == input` with an actionable error. Covers today's
  `--output` data-loss bug and the future md-output-format default. Generic
  render error, no dedicated Q-code (as decided).

- [x] Tests: unit (`output_equal_to_input_is_refused`), e2e overwrite refusal
  preserving the source byte-for-byte
  (`output_equal_to_input_refuses_and_preserves_source`), `.md` revealjs pin,
  `.md` non-native early refusal
- [x] `detect_single_input_format` accepts `.md`
- [x] `output == input` guard in `determine_output_paths`
- [x] Stale "must be a `.qmd` file" comment at the SingleDoc fallthrough fixed
- [x] Full workspace suite green (11022 passed)

### Phase 4 — Links, navigation, dependency graph — **DONE 2026-08-07**

Finding that shrank the phase: the href → output **rewriting was already
membership-based** — both nav and body resolution go through
`ProjectIndex::lookup_by_source`, which is keyed by source path and contains
whatever discovery selected. Once Phase 1 put `.md` files in the render list,
sidebar `file: notes.md` and body `[x](notes.md)` resolved to `notes.html`
with no further change (pinned by e2e before any Phase-4 edit). The
`.ends_with(".qmd")` gates control only two things:

1. **Miss diagnostics** (Q-13-1..4/7): kept `.qmd`-only **deliberately** per
   D6 — a `.md` miss may legitimately be a static resource; comments at both
   gate sites now say so.
2. **Pass-1 body-link target extraction** (`resolve_doc_relative_target`),
   which feeds `DocumentProfile.body_link_targets` → dependency-graph edges:
   extended to `.md`. Non-render-list `.md` targets are dropped by the graph
   builder's index lookup, as designed.

- [x] E2e (`md_pages_get_nav_and_body_links_rewritten`): website project with
  sidebar `file: notes.md`, `.qmd`→`.md` and `.md`→`.qmd` body links, all
  rewritten to `.html`; plus a subset-render (Mode B) leg pinning that links
  into `.md` pages still rewrite when only the linking page renders
- [x] Unit (`target_md_resolves_like_qmd`): `.md` targets extract like
  `.qmd`, including `..` normalization and `.md`-source → `.qmd`-target
- [x] D6 comments at both miss-diagnostic sites
- [x] `host_relative_qmd` / `dependency_graph.rs` verified path-generic — no
  change needed (graph filters by index, listing binding is extension-blind)
- [x] Full workspace suite green (11024 passed)

Note: the graph edges' pass-2 *augmentation* effect (always-render pages
re-rendering when a linked `.md` changes) is exercised only at the unit
level — an e2e needs always-render listing pages, which are out of scope
per D8; noted for the listing follow-up strand.

### Phase 5 — Preview (scope per D9)

- [ ] Rust: `quarto-hub/src/discovery.rs:122` VFS sync + `watch.rs:244` filter
  accept `.md`
- [ ] TS: `preview-renderer` source-file checks (`types/project.ts:45`,
  `iframePostProcessor.ts:252`, `iframeLinkHandlers.ts:114`)
- [ ] Full WASM/SPA rebuild chain + `cargo xtask verify` (not `--skip-hub-build`)

### Phase 6 — End-to-end verification + docs — **DONE 2026-08-07** (verify run below)

- [x] **End-to-end with real Connect-docs sources** (per the CLAUDE.md e2e
  policy). Invocation: copied `admin/index.md` + `user/index.md` verbatim
  from the Connect port into a scratch project with
  `render: ["**/*.md", "**/*.qmd"]`, a sidebar `file: admin/index.md`, and a
  body link `[Admin Guide](admin/index.md)`, then
  `CONNECT_VERSION=2026.08 cargo run --bin q2 -- render <dir>` →
  "Rendered 3 of 3 files". Output inspected:
  - `_site/admin/index.html` exists; sidebar hrefs are page-relative and
    correct from both root (`admin/index.html`) and the admin page itself
    (`index.html` / `../index.html`)
  - body link rewrote to `href="admin/index.html"`
  - inline `{{< env CONNECT_VERSION >}}` expanded to `2026.08` in both `.md`
    and `.qmd` (verified in a paired fixture)
  - **discovered (not `.md`-related):** shortcodes in *metadata fields*
    (`title:`/`subtitle:`) expand empty — identically for `.qmd` and `.md`.
    Filed as bd-wpoiv8pq (discovered-from bd-6d2wj4zp). The two render
    warnings in the slice were the unimplemented `include` shortcode, also
    orthogonal.
- [x] Mixed-fixture coverage lives in the inline e2e fixtures
  (`md_pages_get_nav_and_body_links_rewritten`,
  `md_with_engine_spec_renders_with_q_2_40_warning`,
  `empty_project_with_md_files_hints_at_render_list_optin`, discovery unit
  fixtures) — no separate corpus directory needed; smoke-all gains nothing
  the e2e tests don't already assert
- [x] User docs: new page `docs/guides/projects/render-list.qmd` (render-list
  semantics, the default ≡ `**/*.qmd` invariant, `.md` opt-in, never-render
  list, Q-2-40 pointer), added to the docs sidebar; `cargo run --bin q2 --
  render docs/` → 184 of 184 files, no warnings from the new page, its
  `.qmd` cross-links rewrote to `.html`
- [ ] `cargo xtask verify` (full, WASM leg included) — running at session
  end; no snapshot files changed anywhere in this strand
- [ ] Close-out on bd-6d2wj4zp after Phase 5 decision (preview split to its
  own session per D9)

### Phase 5 status

Deferred to a dedicated follow-up session per D9's escape hatch — it touches
`quarto-hub` Rust, `ts-packages/preview-renderer` TS, and the WASM/SPA
rebuild chain, and the render-path work (Phases 1–4, 6) stands on its own.
The strand stays open until preview lands or is split into its own strand.

## References

- Q1 research (this session, agent report): `src/project/project-context.ts:888-1009`,
  `src/execute/engine.ts:310-330`, `src/execute/markdown.ts:23-107`,
  `src/command/render/output.ts:188-193`, `src/command/render/render-contexts.ts:336`
- q2 code map (this session, agent report): see file:line citations inline above
- `claude-notes/plans/2026-04-23-websites-phase-1.md` §"File-list expansion" (the
  original deferral), `claude-notes/designs/glob-semantics.md` (enumerator-owns-
  extension-policy), `claude-notes/plans/2026-07-20-ipynb-surface-syntax-design.md`
  (the `.ipynb` sibling)
