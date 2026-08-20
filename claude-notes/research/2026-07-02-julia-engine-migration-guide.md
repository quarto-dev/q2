# Migrating a Quarto 1 TS engine extension to q2 — the Julia engine as a worked example

**Plan:** [2026-04-16-julia-validation.md](../plans/2026-04-16-julia-validation.md), Phase 4G
**Primary source:** [2026-07-02-julia-engine-q2-compat.md](2026-07-02-julia-engine-q2-compat.md) (§1-§14)
**Audience:** an extension author (or a q2 contributor helping one) porting a
real Q1 TypeScript engine extension — `ExecutionEngineDiscovery.init()` +
`Engine.claimsLanguage`/`launch`/`execute` — to run under q2's Deno
engine-host subsystem (Plans 1a/1b/1c, 2, 3).

## The headline result

> **UPDATE 2026-07-02 (bd-h4rhohhy): no longer literally zero-changes.** The
> claim below — "zero source changes to *port* to q2" — still holds for
> *feature/API compatibility*: nothing in `julia-engine.ts` had to change to
> make the engine *run* under q2. But two pre-existing **engine defects**
> (present in Q1's copy too) surfaced through q2's `preview` engine-capture
> path and were fixed upstream on branch `q2-close-busy-fix`, so the fixture's
> `julia-engine.ts` is now **modified** (and the bundle is no longer the
> byte-identical `d9d5120…`): a oneShot close/busy recovery (Bug A) and a
> detached-server stdio-inheritance fix (Bug C). These are **engine bug fixes,
> not q2 adaptations** — they apply equally to Q1. See compat log §15 and the
> `worker-close.ts` module. The porting lesson below is unchanged; the "zero
> changes forever" phrasing was optimistic.

**`julia-engine.ts` needed zero source changes *to port*.** The bundle q2's
`build-ts-extension` produced from the (then-)untouched upstream
`src/julia-engine.ts` was **byte-identical** to the bundle Q1's own
`quarto call build-ts-extension` produced from the same source (`diff` empty,
MD5 `d9d5120eb94b187903a43fb500e65eea`, 44512 bytes — compat log §4; the
current, *bug-fixed* bundle is `82bff64…`, 45323 bytes — §15). Every
`quarto.*` call the engine makes (30 call sites across 7 namespaces) resolves
against `@quarto/api`; every native Deno API it uses (`Deno.Command`,
`Deno.connect`, `crypto.subtle`, file I/O) runs unmodified under real Deno.

That means: **for an extension author, "port to q2" is not a TypeScript
porting exercise.** The `@quarto/api` surface and the Deno bundling toolchain
were built to be a faithful drop-in for what a Q1 engine already expects. The
actual migration work — everything below — is entirely on the **ecosystem
side**: how the extension is packaged/declared for q2's static engine
resolution, how it's built, and what document-processing behaviors the q2
render pipeline does or doesn't yet replicate around the engine. (The Bug A/C
fixes above are a separate category again: not porting work, but *engine
maintenance* q2's harder exercise of the engine happened to expose.)

## 1. Import path adjustments

**None required in the engine source.** The one prerequisite is a
**repo-side** fix, not an author-side one: q2's shipped
`resources/extension-build/deno.json` (and `deno.workspace.json`) were
missing the bare-specifier aliases Q1's own
`src/resources/extension-build/import-map.json` provides for Deno stdlib
imports (`"path"`, `"path/posix"`, `"log"`, `"log/"`, `"fs/"`, `"encoding/"`
→ pinned `jsr:@std/*` packages). `julia-engine.ts` imports `"path"`,
`"fs/exists"`, and `"encoding/base64"` as bare specifiers — exactly Q1's
convention — and without the alias, `deno bundle` cannot resolve them.
Restoring **Q1 import-map parity** (commit `e56da9c29`, compat log §1) is
what let the upstream source bundle unmodified; an extension author does not
need to do anything if the q2 install they're bundling against already has
this parity (it's shipped, not per-extension).

**If you're porting a *different* Q1 engine and it imports something outside
this alias set:** check q2's `resources/extension-build/deno.json` first —
the fix is almost certainly adding the missing alias there (matching Q1's
`import-map.json` entry), not rewriting the extension's imports.

## 2. API signature differences

**None found.** The 2026-07-02 audit (compat log §7, reconciled in §9)
enumerated every `quarto.*` call site in `julia-engine.ts` — 30 calls across
`console`, `format`, `jupyter`, `mappedString`, `markdownRegex`, `path`,
`system` — and checked each against the `@quarto/api` implementation. All 30
exist with matching signatures; the earlier plan-prose count ("8
namespaces / 25 calls") was a different counting methodology, not evidence of
a gap — reconciled with no issues found.

**If you're porting a different engine:** grep it for `quarto\.[a-zA-Z]+\.[a-zA-Z]+`
and cross-check each namespace/method against `ts-packages/quarto-api/src/`.
The 7 namespaces the Julia engine exercises are a solid coverage sample but
not exhaustive — an engine that calls something like `quarto.pandoc.*` or
`quarto.project.*` may hit surface this plan didn't exercise.

## 3. Missing QuartoAPI methods

**None stubbed for this engine.** All 6 `quarto.jupyter.*` members
`julia-engine.ts` calls (`assets`, `isPercentScript`,
`percentScriptToMarkdown`, `resultEngineDependencies`, `resultIncludes`,
`toMarkdown`) are among `makeJupyter`'s 7 *implemented* methods — none hit
the `NotImplemented` throwers that exist elsewhere in `@quarto/api` for
methods no engine has needed yet (compat log §9).

## 4. Behavioral differences (the real migration work)

This is where the actual adaptation cost lives — not in the engine's own
code, but in three places: **packaging/declaration**, **the build tool**, and
**document-processing completeness gaps in q2's engine-host layer** that
Julia was the first real TS engine to expose.

### 4a. `_extension.yml` — q2-native static-claiming keys (packaging)

Q1's `_extension.yml` for an engine extension is minimal:

```yaml
title: Quarto Julia Engine Extension
version: 0.1.0
quarto-required: ">=1.9.0"
contributes:
  engines:
    - path: julia-engine.js
```

q2 requires **additional, q2-native keys** for **static claiming** — the
mechanism that lets q2 resolve `{julia}` cells to this engine *without*
spawning the Deno subprocess (zero-load resolution, J9/V-4):

```yaml
contributes:
  engines:
    - path: julia-engine.js
      name: julia
      claims:
        julia:
          kind: primary
          priority: 1
      file-extensions:
        - .jl
```

`claims`/`file-extensions` have no Q1 equivalent — Q1 always loads the engine
dynamically and asks it `claimsLanguage()` at resolution time. `claims` lets
q2 skip that round trip; `file-extensions` is a can-handle pre-filter only
(not a content-inspecting claim — Julia's dynamic `claimsFile` for `# %%`
percent scripts stays undeclared, deliberately, so it doesn't force Pass-1
spawning). See `claude-notes/designs/engine-resolution.md` §3.3 for the full
static-vs-dynamic claim contract.

**A real q2-vs-Q1 divergence, also caught here:** q2's extension reader
(`crates/quarto-core/src/extension/discover.rs`) makes `author` a **required**
field. Upstream `_extension.yml` has none (neither does Q1's schema). Without
it, discovery silently drops the extension — the symptom is *not* an error,
it's the `{julia}` cell rendering as an inert, unexecuted code block with a
repeated WARN log (`missing required 'author' field`). **Add `author: <name>`
to the extension's `_extension.yml`** when porting to q2 (compat log §9,
Failure 1).

### 4b. `build-ts-extension`'s directory-resolution convention doesn't fit a real Q1 extension repo layout (build tooling)

The literal `q2 build-ts-extension <entry.ts>` invocation an author would
reach for **does not work** against a real upstream Q1 extension repo layout
— two structural mismatches, neither fixed by editing the extension:

1. `PATH` must be a directory (or an `_extension.yml` path), not a `.ts`
   file — `q2 build-ts-extension src/julia-engine.ts` fails with "No
   `_extension.yml` found in `src/julia-engine.ts`."
2. `find_entry_ts` hardcodes the convention `<ext_dir>/src/<ext_dir_basename>.ts`
   — i.e. it expects the TS source to live *inside* the shipped
   `_extensions/<name>/` package. That's true for q2's own synthetic flat
   echo-engine fixture, but **false for every real upstream Quarto-1
   extension repo**, where `src/` sits at the repo root, sibling to
   `_extensions/` (exactly `~/src/quarto-julia-engine`'s own layout, which
   the plan explicitly preserved rather than reshaping). `q2
   build-ts-extension _extensions/julia-engine` fails with "No TypeScript
   entry point found. Expected `src/julia-engine.ts` inside
   `_extensions/julia-engine`."

**Workaround used in this plan** (compat log §4/§8): a local, throwaway,
**never-committed** symlink, created only for the duration of the build:

```bash
ln -s ../../src _extensions/julia-engine/src   # from the extension repo root
q2 build-ts-extension _extensions/julia-engine -v
rm _extensions/julia-engine/src                # remove immediately after
```

`deno bundle` canonicalizes the symlink before resolving relative imports, so
the emitted module-path banners read the true `src/julia-engine.ts` path —
the output is indistinguishable from a bundle built against the real source
tree. **This is the single largest "gotcha" for anyone reproducing the
rebuild step**, and it is a q2 build-tool limitation, not anything
extension-author-controllable. Flagged as a follow-up (compat log §8.1):
extend `find_entry_ts`/`resolve_extension_dir` to accept an explicit
`--entry <ts-path>` override, or document the symlink workaround directly in
`build-ts-extension --help`. Out of scope for this plan (a `crates/` change);
tracked as a real gap for whoever picks up general extension-authoring
ergonomics.

### 4c. The Julia notebook needs its own Julia environment, separate from the engine's (project setup, same in both Q1 and q2)

Not a q2-vs-Q1 difference, but a real trap for anyone porting a document that
`using`s packages: `QuartoNotebookRunner` launches each notebook's Julia
*worker* with `CWD` = the **notebook's own directory**, `JULIA_PROJECT=@.`
(search upward for the nearest `Project.toml`) — this is independent of the
*engine's own* runtime `Project.toml` under `_extensions/julia-engine/`. A
document with `using Plots` needs its own `Project.toml`/`Manifest.toml` at
(or above) the document's directory, committed alongside the document, or the
render fails with `ArgumentError: Package Plots not found in current path`
(compat log §10). This applies identically under Q1 — it's QNR's own
behavior — but is worth calling out explicitly in migration docs because it's
easy to attribute to "the port" when it's actually "how Julia notebooks
always worked."

**CI-cost callout (flagged in the 4CD review as a Minor, deliberately
deferred to this doc rather than fixed there):** the committed
`Manifest.toml` for the plan's `Plots`-using fixture is **40 KB**, and the
*first* CI run against a cold Julia depot pays a real network + package
install + precompile cost (`Pkg.instantiate()` on that Manifest) before any
document renders — this is not a `Project.toml` merely declaring intent, it's
a full dependency lockfile that CI must materialize. An extension-author repo
that ships example/test documents using real packages should expect this
same cold-start tax in CI, and should budget for it (a persistent Julia depot
cache across CI runs is the standard mitigation — not implemented here, out
of this plan's scope).

### 4d. q2-side completeness fixes — Julia was the first real consumer, not an engine-adaptation issue

Three fixes landed **in q2's Rust/TS engine-host code**, not in the extension.
These are q2 catching up to behavior Q1 already had, exposed because Julia
was q2's first real-world TS engine with actual document semantics (the
echo-engine test fixture that validated Plans 1a-3 emits its own markdown
directly and never exercises these paths):

- **Execute-visibility defaults** (commit `94a639a64`, compat log §9 Failure
  3). Q1 merges the *writer format's* execute defaults
  (`include`/`eval`/`output`/`echo`/`warning` = `true`, base case) into
  document metadata during format resolution, before partitioning it for the
  engine. q2 had no such layer — `format.execute` reached the engine with
  only the frontmatter's own keys, so any cell option `jupyterToMarkdown`
  checks (`shouldInclude`) that wasn't explicitly set came back `undefined`
  → every cell silently dropped, empty document body. Fixed with a host-side
  `applyExecuteDefaults(format)` that fills the **base** defaults only.
  **Known residual gap, not fixed here:** Q1 additionally overrides
  `echo`/`warning` to `false` specifically for *presentation-family* output
  formats (revealjs, beamer, pptx, dashboard — confirmed by reading
  `quarto-cli`'s `formats-shared.ts`/`formats.ts`/`format-pdf.ts`/
  `format-dashboard.ts`; **not** for plain HTML/PDF/LaTeX, contrary to an
  earlier, overstated note in this compat log — see §14). q2's defaults are
  target-format-agnostic, so a document targeting revealjs/beamer/pptx/
  dashboard through a TS engine will currently echo source by default where
  Q1 hides it. Tracked as **bd-cymkcyaf**.
- **Execute source map** (commit `6a5f80fc4`, compat log §9 Failure 2). q2's
  wire protocol sent an empty source map for every execute request — fine
  for engines that never inspect provenance, but `julia-engine.ts`'s
  `buildSourceRanges` maps every input line back to its origin, and an empty
  map made QuartoNotebookRunner crash (`maximum([])` over zero source
  ranges). Fixed by building a real per-line source map from the existing
  `ExecutionContext` provenance already carried on the Rust side — no wire
  protocol or type change, the field already existed and round-tripped in
  the echo tests; it just wasn't being populated.
- **Supporting-directory expansion** (commit `650a6b694`, bd-677297ca, compat
  log §11/§14 addendum). `julia-engine.ts` faithfully reports its whole
  `<stem>_files` directory as a single `supporting` entry (matching Q1's own
  convention) — q2's project resource copier only knew how to copy
  individual **files**, so a website render of a document with a real
  file-based figure failed with "the source path is neither a regular file
  nor a symlink." Fixed by expanding a reported supporting entry that
  resolves to a directory into its contained files at the point the resource
  report is built; the report itself, resource resolution, and the copy step
  stayed file-only and unchanged.

None of these three required any change to `julia-engine.ts` — they are, in
each case, q2's engine-host or pipeline catching up to a document-processing
behavior Q1 already had, surfaced by running a real engine end-to-end rather
than the synthetic echo fixture.

### 4e. Known, still-open behavioral gaps (not fixed, tracked for follow-up)

- **bd-cymkcyaf** — format-agnostic execute defaults (§4d above; corrects and
  narrows an earlier overstated note).
- **bd-uf4epv4w** — smart-typography mangling of machine-facing metadata
  strings (`--`/`---`/`...`/quotes get converted to typographic equivalents
  by q2's always-on markdown parse of frontmatter, before a TS engine ever
  sees them). Concretely broke `julia: exeflags: ["--threads=2"]` at the
  document level (`–threads=2`, en dash) — QNR then tried to open a file by
  that mangled name. Workaround for extension authors today: avoid
  `--`/`---`/`...`/smart-quote trigger sequences in machine-facing
  frontmatter strings (e.g. `-t2` instead of `--threads=2`), or set the
  option at the **project** level (`_quarto.yml`), where ProjectConfig
  strings stay literal.
- **bd-l9jhy5u0** — `julia-engine.ts` leaks a QNR worker on execute error
  (missing try/finally around the oneShot `close` call,
  `src/julia-engine.ts:742-749`). Confirmed present under **both** Q1 and q2
  (same source, byte-identical bundle) — a genuine upstream engine bug, not
  a q2 adaptation issue. Candidate for an upstream fix report.
- **bd-m1jeqhhz** — q2 has no equivalent of Q1's `quarto call engine julia
  status/kill/log/close/stop` daemon-management CLI surface. A document
  without `execute: daemon: false` defaults to a detached Julia server (per
  `isInteractiveSession && !runningInCI`) that q2 cannot introspect or stop
  short of reading the transport file out-of-band. More acute under `q2
  preview` (an inherently interactive session) than `q2 render`.

## Summary table

| Category | Finding |
|---|---|
| Import path adjustments | None in the extension; one repo-side fix (import-map parity, shipped) |
| API signature differences | None found (30/30 call sites match) |
| Missing QuartoAPI methods | None (all 6 `jupyter.*` members implemented) |
| `_extension.yml` packaging | Add q2-native `claims`/`file-extensions`; add required `author` |
| Build tooling | `build-ts-extension` doesn't fit a real Q1 repo layout — needs a temp symlink workaround (follow-up: `--entry` flag) |
| Project setup | Notebook needs its own `Project.toml` (same under Q1; CI cold-start cost callout) |
| q2 completeness fixes (not engine adaptation) | Execute-visibility defaults, execute source map, supporting-dir expansion — all q2-side, zero engine changes |
| Open behavioral gaps | bd-cymkcyaf (presentation-format defaults), bd-uf4epv4w (metadata mangling), bd-l9jhy5u0 (worker leak, upstream too), bd-m1jeqhhz (no daemon mgmt CLI) |

## Bottom line for an extension author

If your Q1 TS engine only uses the `@quarto/api` surface this plan exercised
(`console`, `format`, `jupyter`, `mappedString`, `markdownRegex`, `path`,
`system`, plus native Deno/Web Crypto), **expect your engine source to port
with zero changes.** Budget your actual migration effort for: (1) adding the
two q2-native `_extension.yml` keys plus `author`, (2) the `build-ts-extension`
symlink workaround (or waiting for the `--entry` flag follow-up), and (3)
being aware of the still-open behavioral gaps above if your documents exercise
presentation formats, machine-facing frontmatter strings with `--`/`---`, or
rely on daemon mode.

**One caveat to "port with zero changes"** (bd-h4rhohhy, 2026-07-02): *porting*
took zero changes, but q2's `preview` engine-capture path — a harder,
longer-lived exercise of the engine than one-shot `render` — later exposed two
**latent engine bugs** (present in Q1 too) that did require engine source
changes: a oneShot close-on-busy that discarded captures (Bug A) and a
detached-server stdio leak that could corrupt the host wire channel (Bug C).
Fixed upstream on `q2-close-busy-fix`; see compat log §15. The lesson: "zero
changes to port" is not "zero changes forever" — running an engine harder finds
engine bugs, and those are upstream-engine maintenance, not q2 adaptation.
