# Migrate remaining glob consumers onto the shared glob API

**Braid strand:** bd-mt7a6uc4 (task, P3) — `discovered-from:bd-v7ixzsp5`
**Stacks on:** PR [#460](https://github.com/quarto-dev/q2/pull/460)
(`bugfix/bd-v7ixzsp5-listing-contents-globs`), branch
`braid/bd-mt7a6uc4-glob-consumer-migration`
**Parent plan:** `claude-notes/plans/2026-08-06-listing-glob-provenance.md`
(decision 3)
**Status:** draft — awaiting review. No code written yet.

> **Base-branch caveat.** #460 has not been through CI (GitHub Actions
> outage, 2026-08-06). This branch stacks on it anyway; if review
> changes #460, rebase before landing. Nothing here should merge before
> #460 does.

## Overview

#460 introduced `crates/quarto-core/src/project/listing/glob_resolve.rs`
as the seed of an internal, base-directory-anchored glob API:
provenance-resolved base dirs, lexical `../` normalization with
project-root clamping, `!` negation, single-view matching. It is
currently listing-shaped — it takes `&[ListingContents]`, emits
`Q-12-17`, and lives under `project/listing/`.

The rest of the tree still resolves globs three other ways, each with
its own matcher, its own base-directory rule, and its own silent
failure modes. This plan generalizes the #460 seed into one module,
writes down a single normative semantics table, and migrates the other
consumers onto it.

The goal is **one glob semantics**, not merely shared code: after this
work, a pattern means the same thing in `contents:`, `project.render`,
`resources:`, and `sidebar.auto:`.

## Consumer inventory (state of the tree at #460 HEAD)

| # | Consumer | Site | Base directory | Candidate source |
|---|---|---|---|---|
| A | listing `contents:` | `project/listing/glob_resolve.rs` + `transforms/listing_generate.rs` + `project/dependency_graph.rs` | **declaring file's dir** (provenance) | in-memory `ProjectIndex` profiles (`.qmd` only), no fs |
| B | `project.render` | `project/discovery.rs::expand_patterns` | project root (always) | `SystemRuntime` walk of `.qmd`, pre-filtered |
| C | `resources:` (project + document) | `project_resources.rs::expand_one` | project root (project-level) / **host document's dir** (document-level) | `glob` crate over the real filesystem, any extension |
| D | `sidebar.auto:` | `transforms/sidebar_auto.rs::normalize_pattern` | n/a — not a glob (see below) | in-memory profiles |
| E | preview resources | `quarto-preview/src/config.rs` | delegates to C with anchor = deck dir | follows C automatically |

### Semantic axes — what each consumer does today

| Axis | A listings (post-#460) | B `project.render` | C `resources:` | D `sidebar.auto` |
|---|---|---|---|---|
| `*` | one segment | one segment | one segment (`glob` crate) | **stripped** — pattern becomes a prefix |
| `**` | zero+ segments | zero+ segments | zero+ segments | **stripped** |
| `?` | yes | yes | yes | no |
| `[abc]` classes | no | no | **yes** (`glob` crate) | no |
| bare literal dir | matches everything beneath | **matches nothing** | `dir/**/*` | prefix match (everything beneath) |
| leading `/` = project root | **silently dropped** | **silently no-match** | yes (documented Q1 parity) | n/a |
| `../` | normalized, clamped to root, `Q-12-17` on escape | **silently no-match** | allowed, escape → `Q-5-1` | n/a |
| `!` negation | yes | **silently ignored** | **silently ignored** | **silently ignored** |
| per-pattern "matched nothing" diagnostic | no | no (project-wide `Q-7-7` only) | no | `sidebar_auto.rs:52` hint on empty |
| fs access | none (WASM-safe) | via `SystemRuntime` | direct `glob::glob` + `canonicalize` (not WASM-safe) | none |

Two axes deserve their own note:

- **D is not a glob implementation at all.** `normalize_pattern` strips
  `*.qmd`, `**`, `*`, and trailing `/`, then prefix-matches. So
  `docs/*.qmd`, `docs/**`, `docs/`, and `docs` are all the same
  pattern, and `docs/*.qmd` matches `docs/deep/nested.qmd`. Migrating D
  is therefore a real behavior change, not a refactor — **accepted**
  (Carlos, 2026-08-06); see decision D6.
- **C is the only fs-backed expander**, and the only one that cannot run
  in hub-client today. A and B match against a candidate list someone
  else produced; C calls `glob::glob` and `Path::canonicalize` directly,
  which on `wasm32-unknown-unknown` compile but cannot see the VFS. See
  §"WASM / VFS" below.

### Divergence from Q1 (for the record)

Q1 routes both `project.render` and `resources:` through
`resolveGlobs`/`resolvePathGlobs` (`external-sources/quarto-cli/src/core/path.ts:227`),
which in `auto` mode:

- treats a leading `!` as an exclusion (negation) — both consumers;
- expands a literal existing directory to `dir/**/*`;
- **prefixes `**​/` to any pattern not anchored with `/`** — so Q1's
  `*.qmd` means "anywhere in the tree".

q2 deliberately does not adopt the implicit `**/` prefix (that is the
"`*.qmd` vs `**/*.qmd` inconsistency" the strand names). We keep `*` =
one segment everywhere and require `**/` to be written. That divergence
should be stated once, in the contract doc, rather than rediscovered per
consumer (see decision D5).

## Defects this migration fixes

Each becomes a failing test in Phase 0/1 before any implementation.

1. **`_metadata.yml`-declared `resources:` resolve against the wrong
   base.** Document-level resources anchor at the *host document's*
   directory regardless of which file declared them
   (`project_resources.rs::collect_static_resources`). This is exactly
   #460's defect 2, one metadata key over. `RawResourcePattern` already
   carries `SourceInfo`, so the fix is the #460 resolver applied to a
   different key.
2. **`!` negation is unsupported in `project.render` and `resources:`,
   and fails differently in each.** Q1 supports it in both.
   `project.render` matches the `!…` entry literally, it drops out
   silently, and the excluded file renders anyway. `resources:` is
   *loud but wrong*: the `!…` entry falls through to the literal-path
   branch and aborts the whole render with
   `Declared resource '<root>/!data/secret.csv' does not exist on disk`
   (`project_resources.rs:800`, exit 1) — while still publishing the
   file the author was trying to exclude. Confirmed in Phase 0.
3. **Bare directory in `project.render` matches nothing.**
   `render: ["index.qmd", "posts"]` renders only `index.qmd` and says
   `Rendered 1 of 1 files` — no diagnostic at all, because the render
   set is non-empty so even `Q-7-7` never fires. Q1 and listings both
   mean "everything beneath".
4. **`../` handling is three different things.** Clamped-and-diagnosed
   in listings, error-checked in resources, silently no-matching in
   `project.render`.
5. **Leading `/` is three different things.** Honored in resources,
   silently dropped in listings (`join_and_normalize` discards the
   empty segment, so `/posts/*.qmd` from `sub/index.qmd` becomes
   `sub/posts/*.qmd`), silently no-matching in `project.render`.
6. **`sidebar.auto: [docs/*.qmd]` silently means `docs/**`** — no
   diagnostic, and the user's stated intent is discarded.
7. **No per-pattern "matched nothing" diagnostic anywhere.** A typo'd
   glob is silent in all four consumers. This is the single largest
   usability win available here (decision D7).
8. **Character classes work in `resources:` and nowhere else**
   (Phase 0, f7). `resources: ["data/fig-[0-9].csv"]` correctly
   publishes `fig-3.csv` and skips `fig-x.csv`; the same class in a
   listing `contents:` matches nothing, silently. D1's resolution fixes
   this by construction — every consumer gets the `glob` vocabulary.

## Phase 0 findings — observed behavior at `2a37d56e`

Eight fixture projects (session scratchpad `phase0/`), each rendered
with `./target/debug/q2 render .` and the output tree/HTML inspected.
Every predicted defect reproduced; two predictions needed correcting
(f2b, f3), both now fixed in the list above.

| Fixture | Declared | Expected (post-migration) | **Observed today** |
|---|---|---|---|
| `f1-resources-dirmeta` | `blog/_metadata.yml`: `resources: ["data/*.csv"]`, host `blog/deep/index.qmd` | publishes `blog/data/from-blog.csv` | publishes `blog/deep/data/from-deep.csv` — anchored at the **host doc**, not the declaring file (defect 1) |
| `f2-render-negation` | `render: ["*.qmd", "!draft.qmd"]` | `draft.qmd` excluded | `draft.html` rendered; `Rendered 3 of 3`, no diagnostic (defect 2) |
| `f2b-resources-negation` | `resources: ["data/*.csv", "!data/secret.csv"]` | `secret.csv` not published | **render aborts**, exit 1: `Declared resource '<root>/!data/secret.csv' does not exist on disk` — *and* `_site/data/secret.csv` is written anyway (defect 2, louder than predicted) |
| `f2c-…-literal` | `resources: ["data", "!data/secret.csv"]` | same | same abort; both CSVs published |
| `f3-render-bare-dir` | `render: ["index.qmd", "posts"]` | `posts/a.qmd`, `posts/b.qmd` rendered | only `index.html`; `Rendered 1 of 1 files`, **no diagnostic** (defect 3 — worse than predicted: `Q-7-7` never fires because the set is non-empty) |
| `f4-render-leading-slash` | `render: ["/index.qmd", "/sub/*.qmd"]` | both rendered | `Q-PROJECT-EMPTY`, `Rendered 0 of 0` (defect 5) |
| `f5-listing-leading-slash` | `sub/index.qmd`: `contents: ["/posts/*.qmd"]` | root `posts/a.qmd`, `b.qmd` listed | listing renders **empty**, no diagnostic (defect 5, listings side) |
| `f6-sidebar-auto` | `sidebar: auto: "docs/*.qmd"` | only `docs/top.qmd` | both `docs/top.html` **and** `docs/deep/nested.html` in the sidebar (defect 6) |
| `f7-classes` | `resources: ["data/fig-[0-9].csv"]` + `contents: ["posts/p[0-9].qmd"]` | both class-filtered | resources: correct (`fig-3.csv` only). listing: **empty**, silently (defect 8) |

Two structural confirmations:

- **`project.render` is read only from `_quarto.yml`/`.yaml`**
  (`ProjectConfig::parse_config`, `project/mod.rs:625`), so its base dir
  is trivially the project root; carrying `SourceInfo` (D8) buys
  diagnostics, not correctness. `as_str()` there already accepts
  `ConfigValueKind::Glob`, so `!glob`-tagged entries are not dropped.
- **Consumer sweep is complete.** A fresh grep for ad-hoc pattern
  handling (`contains('*')`, `trim_end_matches("*")`, `GLOB_CHARS`,
  `fn *glob*`) surfaces only the four consumers plus the two documented
  exclusions (`system-runtime/sandbox.rs`, `qmd-syntax-helper`).

## Design

### Module shape

Extract a `crates/quarto-core/src/glob/` module:

```
glob/
  mod.rs        — public API, GlobOptions, contract doc pointer
  pattern.rs    — GlobPattern, negation split, join + lexical normalize
                  + project-root clamp   (from glob_resolve.rs)
  provenance.rs — SourceInfo -> base dir (from glob_resolve.rs)
  matcher.rs    — segment matcher        (from project/discovery.rs)
  expand.rs     — SystemRuntime-driven expansion over a walked tree
                  (new; replaces the `glob` crate use in
                  project_resources.rs — decision D1)
```

`project/listing/glob_resolve.rs` shrinks to a listing adapter
(`ListingContents` → `PatternSet`, `Q-12-17` emission).
`project/discovery.rs` keeps only discovery (walk + exclusions) and
calls the shared matcher. Nothing in the module may touch `std::fs`
directly: the pure layer (resolve + match) stays WASM-safe, and the
expansion layer goes through `SystemRuntime` so it is testable with the
in-memory runtime and usable from hub-client.

### API sketch (to be refined during Phase 1)

```rust
/// One resolved pattern: project-relative, normalized, forward slashes.
pub struct GlobPattern { pub pattern: String, pub negated: bool }

/// Per-consumer knobs. The *semantics* are shared; this exists only
/// for differences we can justify in the contract doc.
pub struct GlobOptions {
    /// A literal directory matches everything beneath it.
    pub directory_rule: bool,
    /// Positive set injected when only negations were written.
    pub default_positive: Option<&'static str>,
    /// Restrict matches to these extensions (B: `qmd`; C: none).
    pub extensions: Option<&'static [&'static str]>,
    /// Emit a "matched nothing" diagnostic per pattern.
    pub warn_on_empty: bool,
}

pub struct Resolution { pub globs: Vec<GlobPattern>, pub escaped: Vec<EscapedGlob> }

/// Provenance -> base dir -> normalized project-relative patterns.
pub fn resolve(
    raw: impl IntoIterator<Item = (String, SourceInfo)>,
    ctx: &BaseDirContext<'_>,   // source_context, project_dir, fallback_dir
    opts: &GlobOptions,
) -> Resolution;

/// Pure matching against a project-relative candidate path.
pub fn matches(globs: &[GlobPattern], candidate: &str) -> bool;

/// Filesystem expansion for consumers that need real files (C).
pub fn expand(
    globs: &[GlobPattern],
    project_root: &Path,
    runtime: &dyn SystemRuntime,
    opts: &GlobOptions,
) -> Result<Vec<PathBuf>, GlobError>;
```

`expand` walks from the longest literal prefix of each pattern rather
than the project root, so a pattern like `data/**/*.csv` does not walk
`_freeze/` or `node_modules/` (see risk R2).

### WASM / VFS — how fs-backed expansion works in hub-client

The API must work on both targets, so **no layer may touch `std::fs`**.
The pure layer (resolve + match) already satisfies that. The expansion
layer gets there by going through `SystemRuntime`, which both targets
already implement:

| Need | Trait method | Native | WASM |
|---|---|---|---|
| list a directory | `dir_list` | `fs::read_dir` | `vfs.list_directory` |
| is it a directory | `is_dir` / `path_exists` | `fs::metadata` | `vfs.is_directory` |
| containment check | `canonicalize` | resolves symlinks | lexical normalize (no symlinks in the VFS) |

`crates/quarto-system-runtime/src/wasm.rs:318` already implements all
three against the automerge-backed VFS, so `expand(&dyn SystemRuntime)`
runs in hub-client unchanged. Three consequences to honor:

1. **`canonicalize` replaces `Path::canonicalize`.** Today
   `project_resources.rs::canonicalize_within_project` calls std
   directly and falls back to `lexical_normalize` when the path does not
   exist. Routing through the runtime keeps native symlink-escape
   detection (the reason the containment check canonicalizes at all)
   and gives WASM the lexical behavior the fallback already had.
2. **VFS paths are `/project/`-prefixed** (CLAUDE.md, "VFS Path
   Conventions"). Since every pattern is resolved to a *project-relative*
   string before expansion and joined to the runtime's project root at
   walk time, the prefix composes without special-casing — but a WASM
   test must pin it, because this is exactly the kind of thing that
   silently half-works.
3. **Testability improves on both targets.** With expansion behind the
   trait, the existing in-repo mock runtimes (e.g.
   `stage/context.rs:421`) can drive expansion tests without touching a
   real filesystem, and `WasmRuntime` + a seeded VFS gives us a genuine
   WASM-side test.

This is the main reason to prefer D1's "one matcher" answer: it is not
just tidiness, it is what makes `resources:` expansion possible in
hub-client at all.

### Contract document

Write `claude-notes/designs/glob-semantics.md` alongside the existing
`document-profile-contract.md` / `transform-pipeline-phases.md`: one
normative table (the axes above, resolved to a single column), the Q1
divergences, and the rule for adding a new glob consumer. Back it with a
table-driven test that asserts each consumer's `GlobOptions`, so a
future divergence shows up as a test diff instead of a surprise.

## Decisions (Carlos, 2026-08-06)

| # | Question | Decision |
|---|---|---|
| D1 | Unify the matcher without losing `[...]` classes, and without hitting the filesystem. | **Resolved by investigation** (2026-08-06): no fork or new library is needed. `glob`'s *matching* half is already pure — `glob::Pattern::matches_with` does no I/O; only the `glob()` walker touches `std::fs`, and that is the half we replace with `SystemRuntime` enumeration. Adopt `glob::Pattern` as the matcher engine for **all** consumers and delete the hand-rolled `wildcard_match`/`segment_match`. Full syntax kept, zero new dependencies, VFS-capable. See §"D1 in detail". |
| D2 | Leading `/` = project root, everywhere? | **Yes.** A `/`-leading pattern resolves against the Quarto project root, in every consumer. Side benefit: listings regain an escape hatch for the pre-#460 project-relative behavior (`contents: /posts/*.qmd`). |
| D3 | `!` negation everywhere? | **Yes.** |
| D4 | Bare literal directory = everything beneath, everywhere? | **Yes.** |
| D5 | Keep `*` = one segment (reject Q1's implicit `**/` prefix)? | **Yes** — the better default. Q1 projects migrating will have to adjust; our diagnostics (D7) and future tooling catch it. |
| D6 | Migrate `sidebar.auto` to real globs? | **Yes.** Behavior changes are expected and welcome here. Concretely: `sidebar.auto: [docs/*.qmd]` today normalizes to the prefix `docs` and therefore includes `docs/deep/nested.qmd`; after migration `*` is one segment, so nested files drop out and the author writes `docs/**/*.qmd` to keep them. |
| D7 | Add a per-pattern "matched nothing" diagnostic? | **Yes** — warning-level, reusing each subsystem's code family (`Q-12-*` listings, `Q-7-*` render, `Q-5-*` resources) with one shared message builder. This is the migration aid for D5. |
| D8 | Carry `SourceInfo` on `project.render` patterns? | **Yes.** |
| D9 | `DocumentProfile` v8 → v9 for resolved resource patterns? | **Yes**, same shape as #460, with the profile-cache invalidation check. |
| D10 | Ship the behavior changes silently (0.x, per parent-plan decision 1)? | **Yes**, plus changelog entries. |

### D1 in detail — matching is already separable from walking

The `[...]` risk was: `resources:` is the only consumer on the `glob`
crate, whose vocabulary (`[abc]`, `[a-z]`, `[!abc]`, `[*]` escapes) is
strictly larger than q2's hand-rolled `discovery.rs::wildcard_match`.
A naive swap would make `resources: ["figures/fig-[0-9].png"]` match
only a file literally named `fig-[0-9].png` — figures silently stop
being copied, no error until a reader hits a broken image.

**That trade-off turns out to be false.** The `glob` crate already ships
the two halves separately:

- `glob::glob()` / `Paths` — the directory walker. This is the only part
  that uses `std::fs` (`glob-0.3/src/lib.rs:77`), and it is the part we
  replace with `SystemRuntime` enumeration.
- `glob::Pattern::matches_with(&str, MatchOptions)` — a **pure** matcher.
  No I/O, no filesystem notion at all; it matches a pattern against a
  string we supply from any source, including a VFS listing.

We were simply using the walking half. Verified on the pinned toolchain
(2026-08-06, scratch crate, all assertions passing) that
`MatchOptions { require_literal_separator: true, require_literal_leading_dot: false, case_sensitive: true }`
gives exactly the semantics D2–D5 call for, with classes intact:

```
*.qmd          matches about.qmd,  NOT sub/about.qmd      (D5: one segment)
**/*.qmd       matches sub/about.qmd AND about.qmd        (** = zero+ segments)
docs/**/*.qmd  matches docs/a/b.qmd
fig-[0-9].png  matches fig-3.png                          (classes kept)
fig-[!0-9].png does NOT match fig-3.png
a[*]b.qmd      matches a*b.qmd                            (literal-* escape)
data/*         matches data/.hidden                       (dotfile parity)
a?c.qmd        does NOT match a/c.qmd                     (? does not cross)
```

So: adopt `glob::Pattern` as the matcher engine behind our API, feed it
the forward-slash project-relative candidate strings we already build,
and delete `wildcard_match`/`segment_match`. Every consumer *gains*
character classes, `quarto-core` keeps exactly the dependencies it has
today, and resource patterns keep byte-identical syntax — the
regression risk disappears rather than being mitigated.

Use `matches_with(&str, …)`, **not** `matches_path`: the latter is
separator-sensitive, and we normalize to forward slashes on all
platforms.

#### The upgrade path we are not taking yet

`globset` (ripgrep's matcher) is the richer option: it adds `{a,b}`
brace alternates and compiles N patterns into one automaton, which
would suit the "match each candidate against every pattern of a
listing" loop. Verified it compiles clean for `wasm32-unknown-unknown`
on our pinned toolchain, and its semantics under `literal_separator(true)`
match the table above.

It is not free, though: it is currently in our tree only through a
**proc-macro** (`rust-embed-impl`), i.e. host-only, so adopting it as a
runtime dep would add `globset` + `bstr` + `log` to the WASM bundle
(`aho-corasick`/`regex-automata`/`regex-syntax`/`memchr` are already
there via `regex`, a real dep of pampa and quarto-sass). Since the API
wraps the matcher, swapping engines later is a one-file change — which
is the point of having the API. Revisit if we want brace alternates or
if per-listing matching shows up in a profile.

Three sibling hazards remain in the enumeration swap and need pinning
tests regardless:

1. **Exclusion-policy leakage.** `discovery.rs`'s walker skips `_`- and
   `.`-prefixed components, `node_modules`, and `README`. Those are
   *discovery* policy, not glob semantics. Resource expansion must not
   inherit them, or `resources: [".nojekyll"]` and `_data/x.csv` break.
   `GlobOptions` carries the exclusion set; default is empty.
2. **Dotfile matching.** `require_literal_leading_dot: false` keeps
   `data/*` matching `data/.hidden`, which is what both the `glob()`
   walker and the internal matcher do today. Parity — pin it so it stays.
3. **Separator crossing.** `Pattern::matches` with *default* options lets
   `*` cross `/`; the `glob()` walker hides that by walking
   directory-by-directory. Moving to the pattern API makes the option
   load-bearing: `require_literal_separator: true` is what enforces D5.
   Pin it, because getting it wrong silently widens every pattern in the
   tree.
4. **Malformed patterns now surface.** `Pattern::new` rejects things the
   hand-rolled matcher accepted silently (e.g. `**` that is not a whole
   path component). Phase 1 must decide where that error goes — most
   likely D7's diagnostic family, pointing at the YAML scalar via the
   `SourceInfo` we already carry.

## Phases

### Phase 0 — confirm the defect inventory (before any code) ✅

- [x] Build fixture projects reproducing defects 1–6 and record observed
      behavior at branch HEAD (`q2 render`, output inspected). Nine
      fixtures, results in §"Phase 0 findings" above.
- [x] Confirm defect 2's resources sub-case: a `!…` entry is **not**
      silent — it aborts the render with "does not exist on disk"
      (`project_resources.rs:800`) while still publishing the file the
      author meant to exclude.
- [x] Confirm `project.render` is only ever read from `_quarto.yml`
      (`ProjectConfig::parse_config`) — base dir is trivially the
      project root; provenance buys diagnostics only.
- [x] Grep the tree for glob consumers this inventory missed — none;
      only the four consumers plus the two documented exclusions.
- [x] Bonus: confirm character classes work in `resources:` today and
      match nothing in listings (f7) — the empirical basis for D1.

### Phase 1 — extract the API (pure refactor, no behavior change)

- [ ] Move matcher + resolver into `crates/quarto-core/src/glob/`;
      listing keeps a thin adapter; `project/discovery.rs` keeps only
      discovery. Existing tests move with the code and must pass
      unchanged.
- [ ] `GlobOptions` (incl. the exclusion-policy field, default empty) +
      the table-driven options test.
- [ ] Leading-`/` = project root in the resolver (D2), with the listing
      case pinned (`contents: /posts/*.qmd` from `sub/index.qmd`).
- [ ] Swap the hand-rolled matcher for `glob::Pattern` + the fixed
      `MatchOptions` (D1); delete `wildcard_match`/`segment_match`.
      Existing discovery/listing tests must pass unchanged — that is the
      evidence the swap is semantics-preserving where it should be.
- [ ] Decide + wire where `Pattern::new` compile errors surface (hazard
      4), pointing at the YAML scalar.
- [ ] Pinning tests for the sibling hazards: exclusion-policy isolation,
      dotfile matching, separator non-crossing, class patterns.
- [ ] `claude-notes/designs/glob-semantics.md`.
- [ ] `cargo xtask verify --skip-hub-build` green before moving on.

### Phase 2 — `project.render`

- [ ] Failing tests: negation, bare directory, leading `/`, `../`
      clamping, per-pattern empty-match diagnostic.
- [ ] Carry `SourceInfo` on render patterns (D8); migrate
      `expand_patterns` to the shared API; keep the exclusion rules
      (underscore/hidden/`node_modules`/output dir/README) where they
      are — they are discovery policy, not glob semantics.
- [ ] Diagnostic registration + docs stub page.

### Phase 3 — `resources:`

- [ ] Failing tests: `_metadata.yml` base dir (defect 1), negation,
      `[...]` classes preserved, leading `/` unchanged, bare directory
      unchanged, out-of-project still `Q-5-1`.
- [ ] Resolve resource patterns at profile-extraction time; profile
      v8 → v9 (D9) + profile-cache invalidation check.
- [ ] Swap `glob::glob` for `expand()` over `SystemRuntime` (D1);
      route the containment check through `runtime.canonicalize()`;
      drop the `glob` dependency from `quarto-core/Cargo.toml`.
- [ ] WASM/VFS test: seed a `WasmRuntime` VFS under `/project/` and
      expand a resource pattern against it (the `/project/` prefix
      composition from §"WASM / VFS" note 2).
- [ ] Verify `quarto-preview`'s two call sites (E) follow without
      source changes, or adjust them.

### Phase 4 — `sidebar.auto`

- [ ] Failing tests pinning the new semantics; explicit test that
      `docs/*.qmd` no longer matches `docs/deep/nested.qmd` and that
      `docs/**/*.qmd` does.
- [ ] Migrate `normalize_pattern`/`matches_prefix` to the shared API.
- [ ] Changelog + docs note for the behavior change.

### Phase 5 — diagnostics + docs

- [ ] Shared "matched nothing" builder wired into all migrated
      consumers (D7); catalog entries + docs error pages; audit script
      clean.
- [ ] `docs/` user-facing page describing glob semantics once (the
      parent plan deferred a listings guide to bd-2nb6i1qv — coordinate
      so the two do not duplicate).

### Phase 6 — verification

- [ ] `cargo nextest run --workspace`.
- [ ] `cargo xtask verify` (full, incl. hub-client WASM leg — the glob
      module is WASM-reachable through the listing transform).
- [ ] End-to-end `q2 render` on every Phase-0 fixture, output inspected,
      invocations + snippets recorded in this plan.
- [ ] Render `docs/` and diff against the base branch; explain any
      difference.
- [ ] `hub-client/changelog.md` if anything under `hub-client/` moves.

### Phase 7 — bookkeeping

- [ ] Close bd-mt7a6uc4; file follow-up strands for anything deferred.

## Explicitly out of scope

- `crates/qmd-syntax-helper/src/utils/glob_expand.rs` — a CLI utility
  globbing cwd-relative arguments with no project and no provenance.
- `crates/quarto-system-runtime/src/sandbox.rs` path wildcards — a
  security allowlist, a different problem domain. Its matcher is ad hoc
  (`// Simple glob matching - full implementation in k-485`); worth a
  separate strand, not this one.
- `!glob`-tagged values outside `resources:` (e.g. `include-in-header`).
  `include_resolve.rs` reads `K::Glob(s)` as a plain string and never
  expands it — a latent feature gap. File a strand; do not fix here.
- CLI `q2 render <glob>` arguments — shell-expanded before q2 sees them.
- `glob::glob` in pampa's test corpora.

## Risks

- **R1 — base branch unverified.** #460 has no CI (GitHub outage). Any
  review change there forces a rebase here.
- **R2 — resources expansion performance.** Replacing `glob::glob` with
  a runtime walk could regress on large trees. Mitigation: walk from
  each pattern's longest literal prefix, honor the discovery exclusions,
  and measure on a fixture before/after per
  `claude-notes/instructions/performance-profiling.md`.
- **R3 — ~~silent `[...]` regression~~ retired.** D1's resolution keeps
  the full `glob` vocabulary; classes are gained, not lost. What remains
  is `MatchOptions` drift: a wrong `require_literal_separator` silently
  widens every pattern in the tree. Pinned by test.
- **R3b — exclusion-policy leakage** into resource expansion would drop
  legitimate dot/underscore resources. Mitigation: pinning test.
- **R4 — profile version churn.** #460 already bumped v7 → v8; this adds
  v9. Confirm cache invalidation once, in Phase 3.
- **R5 — scope creep.** This touches the render list and the resource
  publisher — the two places where a mistake means missing output files.
  Each phase must be independently green and independently revertable.
