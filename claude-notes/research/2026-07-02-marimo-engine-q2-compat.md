# Marimo engine extension — q2 compatibility log (Plan 4c Phase 4cA)

**Plan:** `claude-notes/plans/2026-07-02-plan4c-marimo-validation.md`, Phase 4cA
**Upstream source:** `~/src/quarto-marimo` (machine-local checkout, branch
`q2-bare-sql-interop`, HEAD `e8ec4fb704ea21ed29b072fb4d77bbb448816045`
— "Python: bare-sql fence rewrite gated on TS-threaded ownership flag (SC6)").
The engine-side 4c0-eng changes landed as `2495a47` ("TS engine: bare-sql
Interop claim + execution-ownership gate (q2 plan4c 4c0-eng)") and `e8ec4fb`.
**Fixture destination:** `crates/quarto-core/tests/fixtures/extensions/marimo/`
**Deno version used for rebundle:** `deno 2.9.0 (stable, release,
aarch64-apple-darwin)`, `v8 14.9.207.2-rusty`, `typescript 6.0.3`

Companion to `claude-notes/research/2026-07-02-julia-engine-q2-compat.md`
(Plan 4 Phase 4A); same methodology, applied to the marimo engine. This log
covers only Phase 4cA (fixture setup + rebundle). Rendering (4cB+) is out of
scope here.

## 1. Fixture copy — included / excluded

Copied individually (not `rsync`, since only specific files are needed —
the upstream repo root is a live user working tree with unrelated scratch
`.qmd` files, `_site/`, `.venv/`, etc., not a clean extension-only repo like
`~/src/quarto-julia-engine`):

- `src/marimo-engine.ts` → fixture `src/marimo-engine.ts`
- `lib/cell-execution-regex.ts`, `lib/is-marimo-cell.ts`,
  `lib/render-output.ts` → fixture `lib/` (from `<repo>/lib/`, **not**
  `src/lib/` — the repo has no `src/lib/`; `marimo-engine.ts` imports
  `../lib/*.ts`, so `lib/` must be a sibling of `src/` in the fixture, same
  relationship as upstream)
- `_extensions/marimo/extract.py`, `_extensions/marimo/command.py` →
  fixture `_extensions/marimo/` (co-located next to the rebundled engine
  output — see §2 for why)

**Excluded** (per the plan and brief, verbatim):
- `_extensions/marimo/marimo-deprecated.lua` — deprecation shim, irrelevant
  to the engine itself; the plan explicitly says not to contribute it.
- `_extensions/marimo/marimo-engine.js` — this is upstream's **GitHub-release
  downloader shim** (1160 bytes, not a real bundle), not the actual engine.
  The fixture's `_extension.yml` instead points at the locally rebundled
  output produced in §2.
- `_extensions/marimo/__pycache__/` — Python bytecode cache, build artifact.
- Everything else in the upstream repo root (`.venv/`, `_site/`, `deno.json`,
  `deno.lock`, `pixi.lock`, `uv.lock`, `pyproject.toml`, `tests/`,
  `tutorials/`, the untracked scratch `.qmd` files, etc.) — repo-only,
  matches the julia fixture's exclusion rationale (not fixture content).

No secrets found in the copied files (`grep -il "token\|secret\|api_key\|apikey\|password"` on all six copied files: no hits).

## 2. Why `extract.py`/`command.py` are co-located with the bundle

`marimo-engine.ts` resolves both runtime files the same way
(`constructUvCommand` / the `execute` handler, both via
`dirname(fromFileUrl(import.meta.url))`):

```ts
const currentDir = dirname(fromFileUrl(import.meta.url));
const extractPath = join(currentDir, "extract.py");
...
const scriptPath = join(currentDir, "command.py");
```

`import.meta.url` resolves to the directory of the **loaded bundle** at
runtime, i.e. wherever `_extension.yml`'s `path:` points once claimed. So
`extract.py`/`command.py` must sit next to the rebundled
`marimo-engine.js`, inside `_extensions/marimo/` — exactly the pattern
julia's `.jl` runtime files use relative to `julia-engine.js` (§7 of the
julia compat log). No copy-to-temp step exists in q2's TS-engine loader
that would break this co-location assumption (same as julia — unconfirmed
end-to-end here since no render is in scope for 4cA; flagged for 4cB).

## 3. `_extension.yml` — written fresh (not adapted from upstream)

Upstream `_extensions/marimo/_extension.yml`:

```yaml
title: Quarto Marimo Extension
version: 0.4.5
quarto-required: ">=1.9.20"
contributes:
  engines:
    - path: marimo-engine.js
  filters:
    - marimo-deprecated.lua
```

Fixture `_extension.yml` (per the brief's Option B claims map, verbatim):

```yaml
title: Quarto Marimo Extension
author: marimo-team
version: 0.4.5
quarto-required: ">=1.9.20"
contributes:
  engines:
    - path: marimo-engine.js
      name: marimo
      claims:
        python:
          - { whenClass: marimo, kind: primary, priority: 2 }
        "python.marimo":
          - { kind: primary, priority: 1 }
        sql:
          - { whenClass: marimo, kind: primary, priority: 2 }
          - { kind: interop }
        "sql.marimo":
          - { kind: primary, priority: 1 }
    filters: (not contributed — see below)
```

Differences from upstream, each an intentional adaptation (not a bug):

1. **`author:` added.** q2's extension reader
   (`crates/quarto-core/src/extension/discover.rs`) requires `author`;
   upstream marimo (like upstream julia) has none. Same divergence julia's
   §3 already documented. Value chosen: `marimo-team` (the upstream GitHub
   org, `github.com/marimo-team/quarto-marimo`).
2. **`name: marimo` + `claims:` map added.** This is the 4c0 "Option B"
   static-claims form (Vec-per-language; `sql` carries two claims —
   a `whenClass`-gated primary claim and an unconditional `interop`
   claim). This is what lets q2's Pass-1 resolver statically know marimo
   claims `{python .marimo}`/`{sql .marimo}` as primary and rides along
   on bare `{sql}` as an interop participant, without spawning the engine
   to ask dynamically (mirrors `claimsLanguage` in `marimo-engine.ts`
   line-for-line — see §4 below). Bare `{python}` is intentionally
   **not** claimed (no claims entry for plain `python`), matching
   `claimsLanguage`'s `return false` fallthrough for that case.
3. **`filters:` (the `marimo-deprecated.lua` contribution) dropped.** Per
   the brief: the deprecation shim is irrelevant to a working-engine
   fixture and would need its own file copy for no purpose here.
4. **`path:` repointed** from the upstream downloader shim
   (`marimo-engine.js`, same filename but different content/purpose) to
   the fixture's own locally rebundled output at the same relative path —
   see §5.
5. `claims-files` deliberately **not** declared, same rationale as julia's
   §3: marimo's `claimsFile` is content-inspecting
   (regex-scans the file for a marimo cell fence via
   `containsMarimoFence`/`MARIMO_CELL_REGEX`), so leaving it undeclared
   keeps Pass-1 zero-spawn resolution intact — `file-extensions` isn't
   declared either, since marimo doesn't claim by extension (`validExtensions`
   is `.qmd`/`.md`, the universal set, not a distinguishing marker).

## 4. `claimsLanguage` (dynamic, TS) vs. the fixture's static `claims:` map

Cross-checked line-for-line against `marimo-engine.ts`'s
`claimsLanguage` (lines 162-182):

```ts
claimsLanguage: (language, firstClass) => {
  if ((language === "python" || language === "sql") && firstClass === "marimo") {
    return 2;                          // whenClass: marimo, priority 2
  }
  if (language === "python.marimo" || language === "sql.marimo") {
    return 1;                          // priority 1
  }
  if (language === "sql") {
    return { kind: "interop" };        // bare sql rides along
  }
  return false;                        // bare python: unclaimed
}
```

Every branch has a corresponding static entry in the fixture's `claims:`
map (§3) except the branch order/fallthrough logic itself (static claims
don't need to encode "check this before that" — the resolver's own
claim-kind semantics do that). This is a **read-only cross-check**, not a
modification — `marimo-engine.ts` is committed byte-identical to upstream
(§6).

## 5. Rebundle — provenance and the `find_entry_ts`/symlink workaround

Same two structural mismatches julia's §4 hit, confirmed empirically
before working around them (read `claude-notes/research/2026-07-02-julia-engine-q2-compat.md`
§4/§8 first, per the brief):

1. `q2 build-ts-extension src/marimo-engine.ts` (a `.ts` file, not a
   directory) fails — `resolve_extension_dir` only accepts a directory or
   a literal `_extension.yml` path.
2. `q2 build-ts-extension _extensions/marimo` (the real extension
   directory, containing `_extension.yml`) fails on `find_entry_ts`: no
   `_extensions/marimo/src/` exists (the fixture's dev `src/` sits at the
   fixture root, sibling to `_extensions/`, mirroring every real upstream
   Quarto-1 extension repo's layout — the same reason julia's `src/` lives
   at the fixture root instead of inside the shipped package dir).

Note also a **third**, marimo-specific non-issue: `find_entry_ts`'s naming
convention is `<ext_dir_basename>.ts` (i.e. it would look for
`_extensions/marimo/src/marimo.ts`, not `marimo-engine.ts`) — but the
function falls back to "any `.ts` file in `src/`" when the exact-name
candidate is absent, and the fixture's `src/` has exactly one `.ts` file,
so the fallback resolves unambiguously. No rename or fixture-layout
workaround was needed for this part.

Worked around exactly as julia did — a **local, non-committed, one-time
symlink**:

```
_extensions/marimo/src -> ../../src     # created only for the build, removed after
```

`deno bundle` canonicalizes the symlinked entry path before resolving
relative imports (`../lib/*.ts` from `src/marimo-engine.ts`), so this
correctly resolves to the fixture's real `lib/` (sibling of the real
`src/`), not a `lib/` relative to `_extensions/marimo/`. The symlink was
removed immediately after the build; confirmed absent before staging
(`find crates/.../marimo -type l` → no output).

Exact invocation used (from the workspace root, `q2` built at this
worktree's HEAD, release binary):

```
$ ./target/release/q2 build-ts-extension crates/quarto-core/tests/fixtures/extensions/marimo/_extensions/marimo -v
⚠️  deno bundle is experimental and subject to changes
Bundled 76 modules in 31ms
  crates/quarto-core/tests/fixtures/extensions/marimo/_extensions/marimo/marimo-engine.js 21.55KB

Built: .../marimo/_extensions/marimo/src/marimo-engine.ts → .../marimo/_extensions/marimo/marimo-engine.js
```

Config resolved via workspace auto-detection (tier 3 — `find_workspace_root`
walked up from `_extensions/marimo/` to the repo root, which contains
`ts-packages/quarto-api`), same as julia: `@quarto/types` from local
workspace source, `@std/*` (`path`) from `jsr:` per
`resources/extension-build/deno.json`'s existing aliases (no new alias
needed — julia's §1 import-map parity work already added everything
marimo's imports use: `path` only, no `fs/`, `log`, or `encoding/`
namespaces are imported by `marimo-engine.ts`).

**Remote import (`delay`).** `https://deno.land/std@0.224.0/async/delay.ts`
is a fully-qualified URL import, left as-is per the brief (not required to
remap). `deno bundle` fetched and inlined it without incident (warm local
`DENO_DIR`/`jsr` cache from the julia rebundle in the same session); the
resulting bundle contains a literal `function delay(ms, options = {})`
definition (grep-confirmed), not a live import — offline-safe once built.

**Bundle sanity (smoke check, per the brief — no render attempted):**
- Output exists: `_extensions/marimo/marimo-engine.js`, 22070 bytes (up
  from the upstream shim's 1160-byte downloader stub — confirms this is a
  real bundle, not an accidental copy of the shim).
- `grep -c marimo` → 37 hits; `grep -c '^export'` → 1 (`export default
  marimoEngineDiscovery`, matching the source).
- `deno check crates/.../marimo-engine.js` → clean, no type errors, no
  output (exit 0).
- No `@quarto/api`/`quarto-api` string markers in the bundle — expected,
  same reasoning as julia's §4: `marimo-engine.ts` only imports **types**
  from `@quarto/types` (erased at bundle time) and references the
  `quarto` global at runtime (host-injected, never imported).

## 6. `marimo-engine.ts` / `lib/*.ts` / `extract.py` / `command.py` — source modifications

**None.** All six copied files are byte-identical to
`~/src/quarto-marimo` at HEAD `e8ec4fb` (`diff` empty for every file,
confirmed individually — see the task report for the full listing). No
edit was needed to make the bundle build or `deno check` pass.

## 7. Regression / build verification

- `cargo build --bin q2 --release`: green.
- `cargo nextest run -p quarto-core`: 2627 passed, 33 skipped, 0 failed
  (includes the existing julia J1-J6 rows and echo/P2 rows — no
  regression from adding the inert marimo fixture data).
- `cargo nextest run -p quarto -E 'test(build_ts_extension)'`: 15/15
  passed (the echo-engine e2e driver + all `build_ts_extension` unit
  tests — unaffected by the new fixture, since nothing references it
  yet).

## 8. Open items / concerns carried to 4cB

1. **Runtime co-location — CONFIRMED (4cB attempt 1, 2026-07-02).** §2's
   `import.meta.url` reasoning holds: the bundle correctly resolved
   `command.py`/`extract.py` co-located in `_extensions/marimo/` and `uv run`
   spawned both successfully (see §9). No relocation issue.
2. **`find_entry_ts`'s naming convention doesn't match `marimo-engine.ts`
   directly** (§5) — works today only because of the "any `.ts` in `src/`"
   fallback. If a second `.ts` file is ever added to the fixture's `src/`
   (e.g. a `constants.ts` companion, mirroring julia's), the fallback
   becomes ambiguous (`read_dir` order is not a stable convention) and the
   build would need the same `--entry`-override extension julia's §8.1
   already flagged for `build-ts-extension`. Not an issue today (`src/`
   has exactly one file); worth remembering if 4cB+ ever needs to add a
   second TS source file to this fixture.
3. **`author: marimo-team` is a judgment call**, not sourced from any
   upstream metadata field (upstream has no `author:` at all — same as
   julia). If marimo's actual maintainers state a preferred display name,
   update this value; it has no behavioral effect beyond satisfying q2's
   required-field check.
4. **`claimsLanguage`'s dynamic branches were cross-checked, not
   exercised.** §4 is a static read of the TS source against the fixture's
   `claims:` map; the actual Pass-1 resolver behavior (does q2 correctly
   assign priority-2/priority-1/interop per the map, matching what
   `claimsLanguage` would return if asked dynamically) is 4cB+'s job to
   verify against a real render, same division of labor as julia's
   4A (static claims declaration) vs. 4B (first real render, §9 of the
   julia log). **Partially confirmed by 4cB attempt 1 (§9): the static
   `python: [{whenClass: marimo, kind: primary, priority: 2}]` claim DID
   correctly route the `{python .marimo}` cell's language ownership to
   marimo** (the engine host resolved marimo as owner and spawned its
   subprocess) — the render still fails downstream, but for a reason
   unrelated to claim resolution (see §9).

## 9. 4cB attempt 1 (2026-07-02) — resolved version + BLOCKING finding

**Resolved marimo version:** `marimo==0.23.13` (python 3.13.7,
`cpython-3.13.7-macos-aarch64-none`), resolved via `uv run --with marimo` —
consistent with the environment facts' `uv.lock` constraint (`>=0.23.1`).

**Manual invocation:**
```
cargo run --bin q2 -- render <scratch>/minimal.qmd -v
```
against a tempdir copy of the fixture (`_extensions/marimo/` +
`_quarto.yml` + `minimal.qmd`), per the e2e driver pattern.

**Result: render FAILS, not yet green.** Static claim resolution and the
marimo subprocess chain (Deno bundle load → `uv run` package resolution →
`command.py` → `uv run … extract.py`) all worked correctly and marimo's own
Python-side parser executed the cell (`extract.py` exited 0, `count: 1`).
The failure is downstream: a **pampa QMD-writer round-trip bug** on the
`{lang .firstclass}` (space-separated) fence syntax mangles the cell before
marimo's TS-side `execute()` can recognize its own output slot, and the
mangled text then fails a later tree-sitter re-parse
(`Error: Parse error / unexpected character or token here`).

Root cause, evidence, and the exact file/line pointers are recorded in the
plan file's 4cB section
(`claude-notes/plans/2026-07-02-plan4c-marimo-validation.md`, "BLOCKING
FINDING") and in the task report (`.superpowers/sdd/task-4cB-report.md`) —
not duplicated here to avoid drift between the two. Short version: this is
**not** a marimo-engine defect and **not** in 4cB's authorized fix scope
(pampa's writer is core q2 infrastructure); it needs controller-level
triage before 4cB (and therefore SC8) can go green.

## 10. 4cB attempt 2 (2026-07-02) — resume after the pampa fix; SECOND blocker found

The pampa QMD-writer fix (`411380777`) resolved §9's blocker — the render now
**succeeds** (exit 0) and produces HTML containing the correct evaluated
result `2` (`<pre class='text-xs'>2</pre>` inside a `<marimo-island>` block).
Static claim resolution, the `uv`/marimo subprocess chain, and marimo's own
cell execution are all confirmed working end-to-end.

**But a second, independent blocker prevents the brief's/SC8's specific
marker requirement from being met:** the rendered `<head>` contains, as
literal text, the temp file's *path* (e.g.
`/var/folders/.../marimo-header-….html`) rather than that file's contents.
Confirmed by reading the temp file directly off disk — it genuinely does
contain `__MARIMO_EXPORT_CONTEXT__` and `<marimo-code hidden>` — so the
break is entirely on the q2-consumption side, not in `extract.py`'s header
construction. Root cause: `ts_engine.rs::translate_includes` treats every
engine-contributed `include-in-header` wire value as literal content
(matching `IncludeResolveStage`'s documented architecture — engine-contributed
`PandocIncludes` are folded verbatim, never file-read, unlike knitr's
native-Rust `convert_includes` which does read the file at its own path
before populating the same struct), while `marimo-engine.ts` sends a
temp-file *path*, "like Jupyter does" per its own comment — a Q1/Pandoc-style
assumption this q2 code path doesn't honor for the engine-contributed
channel. No other TS-engine fixture has exercised this wire field before, so
the mismatch was previously latent (matching plan correction #6's note that
"4cC validates the real sink").

Full evidence + exact file/line pointers: plan file 4cB section ("BLOCKING
FINDING #2") and `.superpowers/sdd/task-4cB-report.md`. Both possible fixes
(q2-core drain reads file paths, or marimo-engine.ts sends literal content)
are outside this task's scope — controller call.

## 11. 4cB attempt 3 (2026-07-02) — closing summary: render DONE; SC8 GREEN, RED pending sign-off

FIX #2 (`13f697c85`) resolved §10's blocker. Firsthand re-render (fresh
rebuild, fresh scratch dir) confirms the full success criterion: `2` present
via `<marimo-cell-output><pre class='text-xs'>2</pre></marimo-cell-output>`,
**and** `__MARIMO_EXPORT_CONTEXT__` + `<marimo-code hidden>` present in
`<head>` (the temp-path leak is gone). **Resolved marimo version:
`marimo==0.23.13`** (python 3.13.7), consistent across all three attempts.

Test Seam Spec SC8 is written
(`crates/quarto-core/tests/integration/marimo_engine_e2e.rs`, registered
alphabetized in `main.rs`) and GREEN — a real, unmocked, full-chain render
(deno + uv gated, `eprintln!` SKIP lines, conjunctive assertions on the
marker and the executed-output markup).

**A third, independent finding surfaced proving RED-by-revert**: SC8's
frozen spec text names the revert "remove the `python` claim from
`_extension.yml`" — applying exactly that, alone, does **not** redden the
test. Root cause: `EngineClaimsFileStage`'s whole-file `claims_file` check
(which runs before, and independent of, per-language `claims:` resolution)
dynamically loads marimo's own unmodified `claimsFile` JS function, which
does its own raw-text regex scan for a `.marimo` fence — present in
`minimal.qmd` regardless of the per-language YAML edit — and that alone
short-circuits ALL per-language tier evaluation (`engine_execution.rs:225`'s
own comment confirms this is by design, mirroring an explicit `engine:
marimo` declaration). A corrected revert — adding `claims-files: []`
alongside removing the `python:` claim, which disables the dynamic
content-inspecting `claims_file` path — was tested and DOES produce genuine
RED (`Error: Engine 'jupyter' is registered but its runtime is not
available`), then reverted (fixture restored byte-identical, `git diff`
confirmed clean). **Approved by controller 2026-07-02**; applied for the
final RED capture (documented verbatim in the test file's doc comment),
then re-reverted so the committed fixture stays pristine. Full trace in the
plan file's 4cB "BLOCKING FINDING #3" and the task report.

## 12. Extension-author note: `claimsFile` whole-file claims short-circuit ALL per-language tiers

Worth calling out for anyone authoring or reasoning about a TS engine's
`_extension.yml`, not just marimo: an engine's `claimsFile` answer (whether
static, via a declared `claims-files:` key, or dynamic, via the live JS
`claimsFile` function when `claims-files:` is absent) operates at a
**different, earlier layer** than the per-language `claims:` map.
`EngineClaimsFileStage` (`crates/quarto-core/src/stage/stages/
engine_claims_file.rs`) runs before `ParseDocumentStage`, asks every
registered engine whether it claims the **whole input file**, and — first
claimer wins — records that engine as `ctx.claimed_engine_name`. Per
`engine_execution.rs:225`'s own comment, that claim **"short-circuits ALL
tier evaluation and returns exactly that engine"** — functionally identical
to an explicit `engine: <name>` frontmatter declaration, entirely bypassing
whatever the `claims:` map says about individual languages.

This means a content-inspecting `claimsFile` (the marimo default: no
`claims-files:` key declared, so `claimsFile` is answered dynamically by
loading the engine and scanning the file's raw text) can make an engine own
a document **regardless of** how narrowly its per-language `claims:` map is
scoped. For marimo specifically, this is arguably intentional/desirable
upstream behavior (any file containing a `.marimo`-tagged fence is a marimo
file, full stop) — but it means the `claims:` map's per-language
`whenClass`/`priority` entries are, in practice, largely moot for whole-file
ownership decisions once a single matching fence is present; they only
start to matter for finer-grained language-vs-language competition *within*
an already-claimed file (e.g. bare `{sql}` interop-gating per Option B).

**Plan-6 nuance (flagged, not resolved here):** a content-inspecting
`claimsFile` (no static `claims-files:`) forces an engine LOAD at the
`EngineClaimsFileStage` pipeline stage — i.e. even an otherwise fully-static
engine (one whose per-language `claims:` map never needs to load the
module, per 4c0's zero-load static-claims design) still pays a load cost at
file-claim time if it doesn't declare `claims-files:`. Declaring
`claims-files: []` (or a real extension list) for a static-claims engine
would avoid this — worth a nuance note for whichever plan next tightens
zero-load guarantees end-to-end.

## 13. Task 4cB2 (2026-07-02) — dynamic-path parity DONE; bare-sql interop BLOCKED on a second, independent defect

**Parity for the plain case is done and green.** The dynamic-claims fixture
variant (identical engine, `claims:` map dropped from `_extension.yml`,
derived per-test in `marimo_engine_e2e.rs` rather than committed as a second
bundle) renders the same python-only `minimal.qmd` SC8 uses with the same
result: `p4cb2_dynamic_path_parity_minimal_render_matches_static` is GREEN.
This proves the legacy dynamic path (`ts_engine.rs:668`'s `claims_language`
else-branch: `ensure_loaded` + a live `ClaimsLanguage` wire call) resolves
`{python .marimo}` ownership identically to the static `claims:` map.

**Bare-sql interop parity (the plan's SC9 row) is BLOCKED — a second,
independent defect, not just the anticipated §12 vacuity.** Per the plan
brief's Risk 1, the evidence-first procedure was run to completion:

1. On the claims-less fixture AS-IS (no `claims-files:` override — same gap
   §12 describes for the static fixture), rendering a
   `{python .marimo}` + bare `{sql}` doc produces the SAME whole-file
   short-circuit §12 already documents: temporary, reverted instrumentation
   (`eprintln!` in `engine_execution.rs`, `git diff` clean afterward) showed
   `ownership={}` / `claimed_engine_name=Some("marimo")` — vacuous, exactly
   as predicted.
2. Applying the pre-authorized `claims-files: []` fix to the tempdir-only
   variant disables the short-circuit: `claimed_engine_name=None`,
   `ownership={"python": "marimo", "sql": "marimo"}` — the dynamic
   per-language `ClaimsLanguage` wire path genuinely and correctly resolves
   BOTH languages to marimo. This confirms 4c0-eng's `claimsLanguage`
   interop change is correct at the resolver level.
3. **But the rendered HTML still does not show marimo executing the sql
   cell** — `<pre class="{sql} code-with-copy"><code>SELECT 1 + 1 AS
   x</code></pre>`, a plain unexecuted code block, not
   `<marimo-cell-output>` — despite `ownership["sql"]=="marimo"` being
   correct. Root cause: `marimo-engine.ts`'s `execute()` computes
   `bareSqlOwned = (options.handledLanguages ?? []).includes("sql")`
   (mirrored in `lib/is-marimo-cell.ts`'s `cellOwnedByMarimo`), on the
   assumption — stated explicitly in that file's doc comment — that
   `handledLanguages` is a *positive* "q2 assigned me this language" set.
   It is not: `EngineResolution::handled_languages_for` (`resolution.rs:292`)
   is documented as, and — via the pre-existing, passing
   `jupyter/text_execute.rs:600-655` unit test ("sql must NOT be in
   jupyter's handled_languages — it is owned by jupyter... not something it
   cedes") — proven to be, the **leave-alone** set: HANDLED_LANGUAGES ∪
   languages owned by *other* engines. Because marimo owns sql here, "sql"
   is correctly *excluded* from `handled_languages_for("marimo")` — so
   `bareSqlOwned` evaluates `false` exactly when marimo's ownership is
   correct, and the execute()-time splice never fires.
4. A confirmatory revert (Risk 1's literal SC4 experiment, run against the
   `claims-files:[]`-fixed variant): reverting the bundle's `claimsLanguage`
   bare-sql branch (`{kind:"interop"}` → `false`) makes the render fail
   outright (`Error: Engine 'jupyter' is registered but its runtime is not
   available...` — sql falls through every tier to jupyter's unavailable
   T4 fallback). This is a real, non-vacuous difference from step 2's
   outcome (success vs. failure), proving the dynamic wire call genuinely
   affects resolution — but neither state satisfies "marimo executes the
   sql cell," because that is gated by the separate defect in step 3,
   independent of the claim's value.

**Why this isn't fixed here.** Both ends of a fix are out of this task's
authorized scope: `marimo-engine.ts`/`is-marimo-cell.ts` are excluded
fixture/engine source (only the pre-authorized `claims-files: []` tweak was
sanctioned, not flipping `bareSqlOwned`'s sense), and the durable fix is
architecturally bigger than a one-line flip anyway — a bare
`!handledLanguages.includes(lang)` on the engine side cannot distinguish "I
own this language" from "nobody owns this language" (exactly the ambiguity
the plan's SC15 presence-gating negative case exists to catch), so a sound
fix needs quarto-core to expose a new, *positive* "languages assigned to
you" wire field distinct from the existing leave-alone `handledLanguages` —
a wire-protocol change, not a fixture tweak. Reported for controller
triage; see the plan file's 4cB2 section and Test Seam Spec SC9 row
annotation for the parallel write-up, and
`.superpowers/sdd/task-4cB2-report.md` for the full task report.

**Note on unit-test blindness (why SC5 didn't catch this):** 4c0-eng's
`cellOwnedByMarimo` unit tests (`is-marimo-cell.test.ts`, SC5) hand-feed
`handledLanguages` arrays matching the *same* (incorrect) assumption the
implementation makes — e.g. `cellOwnedByMarimo(bareSqlCell, ["sql"])` →
`true` — so they pass regardless of which convention is right. Only an
end-to-end test wired to q2-core's REAL resolved value (this task, SC9 —
explicitly framed in the plan as "the first end-to-end load→ask→resolve
with a real engine") could surface the mismatch, because only it exercises
the actual wire value rather than a hand-picked stand-in.

### FINDING #4 fix landing (2026-07-02, task 4cB2-fix)

**Controller-ratified fix: leave-alone semantics, engine-side flip — NO
q2 wire-protocol change.** This supersedes this section's earlier framing
(above) that a sound fix "needs quarto-core to expose a new, *positive*
'languages assigned to you' wire field." The controller's call: q2's
resolver already assigns every language present in the document an owner,
or hard-fails, before `execute()` ever runs — so for any language the
engine actually has a decision to make about, "absent from the leave-alone
set" and "owned by me" coincide, and the ambiguity flagged above ("cannot
distinguish 'I own this language' from 'nobody owns this language'") never
actually arises in practice. A bare complement (`!handledLanguages.includes(lang)`)
is therefore sound as-is, without a new wire field.

- **Upstream (`~/src/quarto-marimo`, branch `q2-bare-sql-interop`, commit
  `77c15c8`):** `src/marimo-engine.ts`'s `bareSqlOwned` and
  `lib/is-marimo-cell.ts`'s `cellOwnedByMarimo` both flipped to
  `!handledLanguages.includes("sql")`; both doc comments corrected to state
  the leave-alone semantics and point at `resolution.rs:292`.
  `tests/is-marimo-cell.test.ts`'s two `cellOwnedByMarimo` gate assertions
  (fed under the old, backwards convention) corrected to match: bare-sql
  cell + `handled=[]` → owned (`true`); + `handled=["sql"]` → NOT owned
  (`false`). RED captured against the flipped implementation with the
  pre-correction test file (81 passed / 2 failed — exactly the two direct
  gate assertions); corrected assertions then GREEN. Full deno suite: 83/83
  green (unchanged count — 2 cases corrected in place, none added/removed).
  `pytest tests/python/`: 47/47 green, `extract.py` untouched (the fix is
  TS-side only; the argv-derived `bare_sql` flag's *meaning* to `extract.py`
  didn't change, only which TS expression computes it).
- **q2 (`crates/quarto-core/tests/fixtures/extensions/marimo/`):**
  `src/marimo-engine.ts` and `lib/is-marimo-cell.ts` copied byte-identical
  from the upstream commit above (diff-verified); rebundled via the §5
  symlink-workaround procedure (`_extensions/marimo/src -> ../../src`,
  `q2 build-ts-extension crates/quarto-core/tests/fixtures/extensions/marimo/_extensions/marimo -v`,
  symlink removed immediately after — confirmed absent). Bundle sanity:
  `grep bareSqlOwned` shows `!(options.handledLanguages ?? []).includes("sql")`;
  `grep handledLanguages.includes` shows the negated
  `cellOwnedByMarimo` expression; `deno check` on the bundle is clean.
  `crates/quarto-core/src/engine/ts_protocol.rs`'s `TsExecuteOptions::handled_languages`
  field gained a doc comment stating the leave-alone semantics and this
  finding, so the next TS-engine author doesn't repeat it — no logic change
  in q2-core.
- **Manual e2e re-proof (bare-sql interop, both fixture paths):** the
  claims-less dynamic-path variant (`claims-files: []` override,
  §13 step 2's scenario) and the committed static fixture (whole-file
  `claims_file` short-circuit path, §12) were both re-rendered against the
  same `{python .marimo}` + bare `{sql}` doc
  (`pyproject` deps `["duckdb","sqlglot","polars","pyarrow"]`). Both now
  show marimo's executed sql output
  (`<marimo-cell-output><pre class='text-xs'>...</pre></marimo-cell-output>`)
  for the sql cell, where before the fix both showed a plain unexecuted
  `<pre class="{sql} code-with-copy">` block. See
  `.superpowers/sdd/task-4cB2-fix-report.md` for exact invocations and
  output snippets.
- SC9 remains **not** committed as a passing automated test in this task
  (per the brief, a separate resume owns completing SC9's test file) — this
  section and the plan file's SC9 row / 4c0-eng B2 bullet record that the
  underlying defect blocking it is now fixed.

### SC9 completion (2026-07-03, task 4cB2-completion) — CLOSED GREEN

With FINDING #4 fixed, the SC9-completion follow-up landed the committed
test: `sc9_bare_sql_interop_dynamic_path_marimo_executes_sql_cell` in
`crates/quarto-core/tests/integration/marimo_engine_e2e.rs`. Two changes
beyond the fix itself:

1. **`write_claims_less_extension_yml` now always appends
   `claims-files: []`.** This was the anti-vacuity correction task 4cB2's
   original Risk-1 evidence justified (step 2 above) — without it, EVERY
   dynamic-path test (including the already-green
   `p4cb2_dynamic_path_parity_minimal_render_matches_static`) risks the
   whole-file `claims_file` short-circuit (§12) silently deciding ownership
   before any per-language `ClaimsLanguage` wire call runs. Folding it into
   the shared helper means the derivation is anti-vacuous by construction
   for every current and future test built on `setup_marimo_project_dynamic`,
   not just SC9.
2. **The SC9 test itself**, asserting the SAME behavioral-proxy observable
   task 4cB2 originally chose — the rendered HTML's marimo island for the
   sql cell — now GREEN because the execute()-time gate is fixed: a
   `<marimo-table>` island with `data-data` carrying the escaped JSON
   `[{"x":2}]` (the `SELECT 1 + 1 AS x` result), inside `<marimo-cell-output>`,
   conjunctive with the python cell's markers (same style as SC8).
   RED-by-revert (SC4's named revert) reproduces the pre-fix hard failure
   verbatim (`Error: Engine 'jupyter' is registered but its runtime is not
   available...`) when run against the SAME `claims-files:[]`-fixed
   variant — restored byte-identical afterward, re-confirmed GREEN.

Verification: `cargo nextest run -p quarto-core -E 'test(marimo_engine_e2e)'`
— 3/3 green (SC8, the 4cB2 parity test, and SC9). Full evidence + exact
invocations in `.superpowers/sdd/task-4cB2-report.md` (appended section).
4cB2 is now fully closed — all three deliverables (fixture-variant harness,
python-only parity, bare-sql interop parity) done and green.

## 14. Task 4cC (2026-07-03) — widget render + SC10, `include-in-header` real sink validated

Phase 4cC's job: prove the *real* HTML sink for marimo output — plan
correction 6's "HTML flows through `includes["include-in-header"]` +
inline raw-`{=html}`/`![](…)` output from `render-output.ts`, not
`store_html_dependencies`" — against an actual `mo.ui` widget, not just the
bare `1 + 1` scalar SC8/SC9 exercise.

**Fixture.** Committed `widget.qmd` at the marimo fixture root (alongside
`minimal.qmd`): one `{python .marimo}` cell, `mo.ui.slider(1, 10, value=5)`.
Kept to plain `mo.ui` (no matplotlib/altair) — same warm-uv-cache reasoning
as `minimal.qmd`.

**Manual e2e render (repo rule, done before any test code).** Built
`target/release/q2`, scratch-copied `_extensions/marimo/` + `widget.qmd`
(no `_quarto.yml` — mirrors the test harness's `setup_marimo_project`,
standalone-file discovery off the directory-relative `_extensions/`
convention), ran `q2 render widget.qmd`. Render succeeded; inspected the
output HTML directly:

- `<head>` (routed via `includes["include-in-header"]`, a `PandocIncludes`
  temp file the engine's bundled `execute()` writes `marimoExecution.header`
  into — corresponds to upstream `marimo-engine.ts` ~300-310): contains the
  `__MARIMO_EXPORT_CONTEXT__` trust-marker `<script>` and a
  `<marimo-code hidden>...</marimo-code>` tag carrying the URL-encoded
  notebook source, plus the islands runtime `<script type="module"
  src="https://cdn.jsdelivr.net/npm/@marimo-team/islands@.../main.js">`.
- `<body>`: the widget's raw `{=html}` output (`render-output.ts`'s
  non-mime-sensitive branch, `result += "```{=html}\n" + output.value +
  "\n```\n\n"`, which becomes a Pandoc `RawBlock` the HTML writer emits
  verbatim — no DOM postprocessor, no `store_html_dependencies`) is a
  `<marimo-island data-app-id="main" data-cell-id="Hbol"
  data-reactive="true">` wrapping `<marimo-cell-output>` →
  `<marimo-ui-element ...><marimo-slider data-initial-value='5'
  data-start='1' data-stop='10' .../></marimo-ui-element>` →
  `</marimo-cell-output>` → `<marimo-cell-code hidden>...</marimo-cell-code>`
  → `</marimo-island>`.

This firsthand-inspected shape is exactly what plan correction 6 predicted
and what SC8/SC9 already implied for the header half — 4cC is the first
task to confirm it for widget/figure *body* output specifically.
`generatesFigures: true` was correctly **not** asserted (no q2 consumer);
no `store_html_dependencies` path was asserted or found.

**SC10 (frozen Test Seam Spec row).** Committed test
`sc10_widget_render_shows_header_include_and_body_island` in
`marimo_engine_e2e.rs`, using the SAME static-claims fixture path as SC8
(`setup_marimo_project`, no `_extension.yml` rewrite — the widget doc has
no bare-sql cell to force the dynamic path, and the row's gate is
marimo/uv, not dynamic-vs-static). Conjunctive assertions, mirroring the
SC8/SC9 discriminator-pair style:

1. Header-content marker (`__MARIMO_EXPORT_CONTEXT__` or `<marimo-code`) —
   the `include-in-header` sink itself.
2. Body island markup (`<marimo-island` and `<marimo-ui-element`) — proof
   the widget was executed and spliced in, not left as source.

**RED-by-revert.** In a TEMPDIR-ONLY copy of the fixture bundle (the
committed `crates/quarto-core/tests/fixtures/extensions/marimo/` stayed
`git diff`-clean throughout), neutered the engine's `include-in-header`
population in the bundled `marimo-engine.js` (corresponding to upstream
`marimo-engine.ts` ~300-310):

```diff
-          if (outputFormat === "html" && marimoExecution.header) {
+          if (false && outputFormat === "html" && marimoExecution.header) {
```

Unlike SC8/SC9's revert, this one does **not** touch ownership/resolution
— marimo still owns and executes the cell, so the render still **succeeds**
outright (no hard render-failure RED mode here). Re-rendering the same
`widget.qmd` through the reverted bundle and grepping the output:

```
$ grep -n "MARIMO_EXPORT_CONTEXT\|marimo-code\|marimo-island\|marimo-ui-element\|<head\|</head" widget.html
3:<head>
12:</head>
...
25:<marimo-island
...
31:    <marimo-ui-element object-id='...' ...><marimo-slider .../></marimo-ui-element>
34:</marimo-island>
```

`<head>` (lines 3-12) now contains neither header marker — discriminator
half 1 would fail with this test's own assertion message — while
`<marimo-island>`/`<marimo-ui-element>` (lines 25-34) are still present —
confirming half 2 alone would NOT have caught the broken wiring, which is
exactly why the row's assertions are conjunctive. Fixture restored
byte-identical afterward (`git diff` clean); test re-confirmed GREEN
against the pristine fixture.

**Verification:** `cargo nextest run -p quarto-core -E
'test(marimo_engine_e2e)'` — 4/4 green (SC8, the 4cB2 parity test, SC9,
SC10). One full `cargo nextest run -p quarto-core`: 2634 passed, 33
skipped, 0 failed (the julia e2e tests the brief flagged as historically
transient ran clean this time, no isolation needed). Full invocations +
snippets in `.superpowers/sdd/task-4cC-report.md`.

## 15. Task 4cD-e2e (2026-07-03) — the four remaining 4cD e2e rows: two
findings worth carrying forward (coexistence + error-catch)

Closed the 4cD checklist's three remaining unchecked items (sql-only
self-activation *renders*, sql-interop *both execute*, two-engine
coexistence *each executes only its owned cells*) plus SC18 (error
handling), via four new tests in `marimo_engine_e2e.rs`:
`sc13_e2e_tagged_sql_self_activation_renders`,
`sc14_e2e_static_sql_interop_both_execute_via_marimo`,
`sc16_e2e_marimo_knitr_coexistence_leaves_each_other_alone`,
`sc18_e2e_execute_catch_shows_error_marker_not_crash`. All four GREEN, all
four RED-by-revert-verified in a tempdir-only copy of the bundle (never the
committed fixture — `git diff` stayed clean throughout). Two findings are
worth flagging for whoever next touches this fixture or the upstream
engine:

### Finding A — the whole-file `claims_file` short-circuit (SC8's finding
#3) also breaks two-engine coexistence, not just ownership vacuity

SC8's BLOCKING FINDING #3 (§12 in this doc) already established that the
committed fixture's absent `claims-files:` key makes `EngineClaimsFileStage`
ask the (excluded, unmodified) engine's *dynamic* `claimsFile` to
regex-scan the whole file for any `.marimo` fence — and if found, that
whole-file claim (`ctx.claimed_engine_name`) short-circuits ALL per-language
tier resolution, bypassing the `claims:` map entirely. Every fixture-derived
test so far (SC8, SC9, SC13, SC14) either doesn't have a second competing
engine in its doc, or already uses the ratified `claims-files: []`
derivation (SC9/4cB2's `write_claims_less_extension_yml`) for other
reasons.

SC16 is the first row with a SECOND, *independently owned* engine in the
same document (`{r}` → knitr). Rendering `{python .marimo}` + `{r}` through
the unmodified committed fixture confirmed the short-circuit breaks
coexistence outright: `resolve_engines`'s `claimed` seed collapses the
sequence to exactly `[marimo]` (empty ownership map), knitr never runs, and
the `{r}` cell is spliced back as raw, unexecuted source
(`<pre class="{r} code-with-copy"><code>1 + 1</code></pre>` — not knitr's
`[1] 2`). The fix used is the same one already ratified for SC9/SC14's
dynamic path: derive a `claims-files: []` variant at test-setup time
(`setup_marimo_project_dynamic`), which disables the short-circuit and lets
per-language resolution genuinely decide.

**Upstream implication (not fixed here, flagged for the extension-author
migration guide, 4cG):** any real marimo extension user who wants
multi-engine coexistence in one document needs `claims-files: []` in their
`_extension.yml` (or an equivalent static claims-files list) — otherwise
ANY `.marimo`-tagged doc silently loses coexistence with other engines,
with no error, just quietly-wrong output. This is a q2-core /
extension-authoring gotcha independent of marimo specifically; it applies
to any TS engine whose `_extension.yml` omits `claims-files:`.

### Finding B — `execute()`'s outer catch is unreachable from cell-content
syntax errors; marimo's own per-cell isolation gets there first

SC18's frozen row names `execute()`'s outer try/catch (marimo-engine.ts
~319-329) and suggests a syntactically-bad cell body (`def (:`) as the
trigger. Empirically, that trigger does NOT reach the outer catch: a
`{python .marimo}` cell containing `def (:` renders successfully (exit 0)
and produces `<pre class="marimo-error">SyntaxError: invalid syntax
(<unknown>, line 1)</pre>` in the body — `extract.py`'s own `_ParseError`
sentinel (a `try`/`except Exception` wrapped around each `app.add_code(...)`
call, by its own doc comment written precisely "to surface parse-time
exceptions... that would otherwise be swallowed") catches it INSIDE the
Python subprocess, per-cell, before the subprocess ever exits non-zero.
Runtime exceptions (undefined names, etc.) are similarly soft-caught by
marimo's own dataflow-graph execution model, producing
`application/vnd.marimo+error` mime-renderer islands rather than a process
crash (observed independently while debugging SC13's companion-import-cell
requirement).

Net effect: `execute()`'s outer catch is reachable only by a failure
OUTSIDE marimo's own per-cell/runtime error isolation — i.e. a genuine
subprocess-level failure (non-zero exit from `uv run`, a bad
`command.py`/`extract.py` invocation, a `JSON.parse` failure on malformed
output, etc.), not "bad code in a cell". The test uses an unresolvable
`pyproject` dependency name to trigger this reliably and portably (no
network/registry flakiness risk — the failure is a local `uv` resolution
error against a name guaranteed not to exist).

**Upstream implication (4cG):** the engine's error-handling contract is
narrower than the row's phrasing suggested — it's a *subprocess-failure*
handler, not a *notebook-cell-error* handler (marimo's own per-cell
isolation already owns the latter, and arguably does a better job of it —
inline, localized error markup instead of a whole-render fallback). Worth
noting in the extension-author migration guide as a "your engine probably
wants BOTH layers" pattern: per-cell isolation (marimo has it) for cell
content errors, and an outer catch (this row) for infrastructure/subprocess
failures.

**Verification:** `cargo nextest run -p quarto-core -E 'test(marimo_engine_
e2e) or test(marimo_resolution)'` — 15/15 green (1 pre-existing, unrelated
SC8 resource leak flagged by nextest, not a failure). One full `cargo
nextest run -p quarto-core`: 2645 run, 2640 passed, 5 failed (all
`julia_engine_e2e`, the plan's documented transient), 33 skipped; isolated
re-run of just `julia_engine_e2e` single-threaded: 7/7 green, confirming
the failures are parallel-execution resource contention, not a regression
from this task's changes.

## 16. Task 4cE (2026-07-03) — SC19 rebundle, env modes, `checkInstallation`, `canFreeze` close-out

Closes Phase 4cE and the whole plan's engine-fixture-facing work.

### SC19 fixture rebundle

Upstream moved `~/src/quarto-marimo` (`q2-bare-sql-interop`) from `77c15c8`
(the FINDING #4 fix already rebundled at q2 `b4f4f52bf`) to `2a2f312`
("Factor `buildCommand(metadata)` out of `execute()`'s env-mode branch
(SC19)"). Diffed every copied file before touching anything:
`src/marimo-engine.ts` differs (the `buildCommand` extraction — 28
insertions/18 deletions, `diff -u` confirms the pre-refactor inline
`if (useExternalEnv) {...} else {...}` block became a call to a new
exported `buildCommand(metadata, extractPath, getUvFlags = 
constructUvCommand)`); `lib/cell-execution-regex.ts`,
`lib/is-marimo-cell.ts`, `lib/render-output.ts`, `_extensions/marimo/
command.py`, `_extensions/marimo/extract.py` are all byte-identical
(`diff` empty) — confirms the brief's prediction that `2a2f312` "touched
only marimo-engine.ts + a new test file."

Recopied `src/marimo-engine.ts` only. Rebundled with the same symlink
workaround as §5 (`_extensions/marimo/src -> ../../src`, created
immediately before `q2 build-ts-extension`, removed immediately after;
`find … -type l` confirmed no symlink left in the tree before AND after):

```
$ ./target/release/q2 build-ts-extension \
    crates/quarto-core/tests/fixtures/extensions/marimo/_extensions/marimo -v
Bundled 76 modules in 23ms
Built: .../src/marimo-engine.ts → .../marimo-engine.js
```

Bundle sanity: `grep -c buildCommand marimo-engine.js` → 3 (definition,
call site inside `execute()`, export-object entry), bundle size 22033
bytes (vs. the prior 22070-byte bundle — consistent with a like-for-like
refactor, not a functional change). No stray `@quarto/api` markers
(same reasoning as §5).

**Regression gate (the rebundle's own gate, per the brief):**
`cargo nextest run -p quarto-core -E 'test(marimo_engine_e2e) or
test(marimo_resolution)'` → **15/15 green** against the rebundled engine
(one benign LEAK annotation on a different row than usual — nextest's
leak detector samples whichever subprocess happens to still be
finishing when it checks, not a fixed test; still a PASS, not a
failure, consistent with the LEAK precedent already recorded in this
plan's other task reports). One full `cargo nextest run -p quarto-core`:
**2645 passed, 0 failed, 33 skipped** — no julia transient this run, no
isolation re-run needed.

### `external-env` vs `uv` — manual e2e proof (not a committed test)

SC19's unit seam (upstream, `buildCommand`) already binds the *shape* of
command selection. What's left provable e2e is that `external-env: true`
genuinely works against a real ambient python with marimo installed —
attempted per the brief's "ATTEMPT it cheaply" instruction, in scratchpad
(not committed):

```
$ uv venv <scratch>/venv
$ uv pip install --python <scratch>/venv/bin/python marimo   # resolved 0.23.13
```

Document (`external-env.qmd`, front-matter `external-env: true`):

```
---
title: "Marimo External-Env"
external-env: true
---

```{python .marimo}
import marimo as mo
21 + 21
```
```

Rendered with `<scratch>/venv/bin` prepended to `PATH`:

```
$ PATH="<scratch>/venv/bin:$PATH" ./target/release/q2 render external-env.qmd -v
```

Exit 0; rendered HTML contains `42` and the marimo markers
(`__MARIMO_EXPORT_CONTEXT__`, `marimo-cell-output`, `marimo-island`).

**Rigor check (stronger than the brief asked for):** re-ran with `uv`
*entirely absent* from `PATH` (`which uv` → not found, confirmed) — same
scratch venv only, plus `deno` and `/usr/bin:/bin`. The render still
succeeded, exit 0, same `42` + markers. This proves the code path taken
was genuinely `buildCommand`'s `useExternalEnv` branch (`["python",
extractPath]`) and never fell through to the `uv` branch — if the
external-env branch were broken or bypassed, this run would have failed
outright with "uv: command not found."

**Not committed as an automated test.** Every existing skip-gate in this
suite (`deno_available()`, `uv_available()`, `rscript_available()`,
`knitr_r_package_available()`) is a cheap version-check probe with no
side effects. A faithful gate for this feature would instead need to
*construct* an ambient marimo python — `uv venv` + `uv pip install
marimo` — as part of test setup: a heavier, network-dependent,
disk-writing fixture step unlike any existing row, and exactly the kind
of setup-time flakiness the brief's "do NOT commit a flaky test" guard
is aimed at. Recorded here as the manual proof instead, per the brief's
explicit "otherwise record it as a manual proof" alternative.

### `checkInstallation` — inert in q2 (finding, not a new test)

Grepped the complete `ToEngine` wire-message enum
(`crates/quarto-core/src/engine/ts_protocol.rs:33-101`: `init`,
`loadEngine`, `launchEngine`, `shutdown`, `claimsLanguage`, `claimsFile`,
`markdownForFile`, `execute`, `intermediateFiles`, `dependencies`,
`cancel` — eleven variants, no twelfth) and every occurrence of
`checkInstallation`/`check_installation` across
`ts_protocol.rs`/`ts_engine.rs`/`ts_process.rs`: zero. The only places the
name appears in the whole tree are the TS type declaration
(`ts-packages/quarto-types/src/execution-engine.ts:209`, an optional
method on the interface) and the two fixture engines that implement it
(julia, marimo) — q2 never sends a wire message that would invoke it.
There is no call site to cite as "runs during every launch, so the green
e2e suite already proves it errors-free" (the disposition the brief
offered as the alternative) — the correct disposition is **inert in q2**
(correction-6 style), ticked with this finding, no new test.

### `canFreeze: false` — accepted-untested (pre-decided, cited only)

Per the brief, this disposition was controller-ratified upstream of this
task and is cited, not re-derived: `canFreeze` flows wire → store →
`TsEngine::can_freeze()` (`crates/quarto-core/src/engine/ts_engine.rs:614`)
and dead-ends at a `Debug` impl
(`crates/quarto-core/src/engine/registry.rs:316`); confirmed (read-only)
that `RenderOptions.use_freeze` is constructed `false` at every call site
(`render.rs:643`, `pass2_renderer.rs:809,1066`) — no freeze-consulting
code path exists in q2 today. Strand `bd-mx5x609r` holds the
freeze-epic-time test spec.

## FINDING #5 (2026-07-03) — marimo renders via `q2 render` but does NOT splice into `q2 preview`

Discovered while adding the Phase 4cH browser-level canary. The `q2 preview`
delivery chain records the marimo capture server-side but the executed marimo
output never reaches the SPA pane — the pane keeps showing the inert source
cell. Strand **bd-5jxcio5d** tracks closing the gap.

**Root cause.** The preview capture-splice
(`crates/quarto-core/src/engine/capture_splice.rs`,
`derive_cell_outputs`/`is_cell_wrapper`) maps each source engine cell to the
next `::: {.cell}` (a `Div` with class `"cell"`) block in the executed
markdown. Echo and julia emit that wrapper (julia via the engine-host's
`mdFromCodeCell`; the echo fixture's source comment calls the wrapper
"load-bearing... a bare paragraph → no splice"). **Marimo does not**: its
engine (`lib/render-output.ts`, `src/marimo-engine.ts`) returns each executed
cell as a bare ```` ```{=html} ```` block carrying
`<marimo-island>`/`<marimo-cell-output>` custom elements — zero `class="cell"`.
That shape renders correctly at the `q2 render` tier (the writer emits the raw
HTML verbatim), but the `.cell`-anchored preview splice has no wrapper to map,
so it leaves the cell as raw source. This is an architectural gap, not a bug in
the marimo engine (its island output is intentional).

**Evidence.** `q2 render` of a `{python .marimo}` / `40 + 2` doc produces
`<marimo-cell-output><pre class='text-xs'>42</pre></marimo-cell-output>`; the
same doc under `q2 preview` records `recorded engine capture(s) engines=marimo`
in the server log yet the pane still shows the inert `40 + 2` code block after a
bounded wait. Pinned by the SC21-NEG canary
(`q2-preview-spa/e2e/engine-capture-splice-marimo.spec.ts`), which is EXPECTED
to redden when bd-5jxcio5d lands (at which point it flips to a positive splice
test).

**Include-in-header in the pane vs static render (checklist observation, not a
gate).** In static `q2 render`, the marimo `include-in-header` content
(`__MARIMO_EXPORT_CONTEXT__` script + `<marimo-code hidden>`) is emitted into
`<head>`. That header content is what carries the notebook source — and, as a
secondary finding, the literal spaced `40 + 2` survives there inside a
`notebookCode:` JS-string (NOT only URL-encoded as `40%20%2B%202`). So any
"source absent from the pane" assertion for a FIXED (spliced) world must scope
to the pane BODY, excluding the head script. In the current (unspliced) preview
pane the head-script content does not surface as executed output at all — the
pane is the inert source cell.

**Implementer warning — entry-only revert vs whole-map drop (two different
resolution paths).** SC8's ratified two-part revert removes ONLY the `python:`
claim ENTRY (keeping the `claims:` key and the other entries) and adds
`claims-files: []`. That genuinely reddens (render fails jupyter-unavailable):
with the static `claims:` map still present, `ts_engine`'s static short-circuit
answers `claims_language` from the map alone — the missing `python` key resolves
to a static None and marimo does not claim the cell; the dynamic `claimsLanguage`
wire call fires ONLY when there is no static map at all, so it is never consulted.
SC8's committed RED is therefore NOT stale, and the dynamic `claimsLanguage` path
predates SC8 (4c0-eng only added the bare-sql interop arm to it). The SC21-NEG
canary uses this literal entry-only form. The trap, worth one sentence: dropping
the WHOLE `claims:` map (4cB2's claims-less variant, `write_claims_less_extension_yml`)
is a DIFFERENT revert — with no static map, `self.claims` is `None`, so the
dynamic `claimsLanguage` fires and re-claims `{python .marimo}`
(`language==="python" && firstClass==="marimo" → 2`), and the render does NOT
redden. The two forms engage static-map-lookup vs dynamic-wire resolution; only
the entry-only form is SC8's revert.
