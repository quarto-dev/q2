# Migrating a Quarto 1 TS engine extension to q2 — the marimo engine as a worked example (bare-sql interop feature)

**Plan:** [2026-07-02-plan4c-marimo-validation.md](../plans/2026-07-02-plan4c-marimo-validation.md), Phase 4cG
**Primary source:** [2026-07-02-marimo-engine-q2-compat.md](2026-07-02-marimo-engine-q2-compat.md) (§1-§16, the full trail)
**Precedent:** [2026-07-02-julia-engine-migration-guide.md](2026-07-02-julia-engine-migration-guide.md) — same
audience and structure; read that one first if you haven't ported an engine to q2 before.
**Upstream source:** `~/src/quarto-marimo`, branch `q2-bare-sql-interop`, commits `2495a47`, `e8ec4fb`, `77c15c8`,
`2a2f312` (`main..q2-bare-sql-interop`).
**Audience:** an extension author (or a q2 contributor helping one) porting a real Q1 TypeScript engine extension
to q2, and the upstream `quarto-marimo` maintainers evaluating whether to merge the `q2-bare-sql-interop` branch.

## The headline result — different from Julia's

Julia's engine ported with **zero source changes**. Marimo's did not, and that's expected: this task was
explicitly **a feature build, not just a validation** (plan scope note). q2/team decided marimo should treat
bare `{sql}` as an `Interop` language (rides along whenever marimo is already present via a `python.marimo`
primary), matching knitr's own `sql: Interop` declaration — but the shipped upstream engine's `claimsLanguage`
returned `false` for bare sql, so delivering the behavior required source changes on **both** sides:

1. **q2-core** (`4c0`): widen static claims from one-per-language to **`Vec`-per-language**, so a single `sql`
   key can hold both a `whenClass`-gated primary claim and an unconditional interop claim.
2. **the marimo engine itself** (`4c0-eng`, upstream commits `2495a47`/`e8ec4fb`/`77c15c8`/`2a2f312`): make the
   live `claimsLanguage` return interop for bare sql, and make `execute()` actually run bare-sql cells when q2
   assigns them to marimo.

This was mandatory, not optional, because q2 **hard-errors on any static-vs-dynamic claim mismatch**
(`ts_engine.rs:286`) — a declared static `Interop` claim that the module's live `claimsLanguage` contradicts
fails every render. So unlike Julia, "port marimo to q2" is inseparable from "extend marimo to support the
bare-sql interop feature." Everything below is organized by the plan's own seven adaptation categories, plus
a section on the changes that landed in q2-core itself rather than in the engine.

## 1. `claimsLanguage` interop widening (+ the Option B claims map & Vec form)

**Upstream change:** `2495a47`, `src/marimo-engine.ts`. `claimsLanguage("sql", firstClass)` now returns
`{kind: "interop"}` whenever `firstClass !== "marimo"` (was `false`); all python cases and tagged/dotted sql
are unchanged. Return type widened from `boolean | number` to `boolean | number | LanguageClaim`.

**q2-core prerequisite (4c0):** static claims were previously **one claim per language key**. A single `sql`
key needed to express *two* different claims simultaneously — primary-when-tagged AND interop-otherwise — so
`_extension.yml`'s `claims:` values became a **Vec**:

```yaml
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
          - { whenClass: marimo, kind: primary, priority: 2 }   # {sql .marimo} self-activates
          - { kind: interop }                                    # bare {sql} rides along
        "sql.marimo":
          - { kind: primary, priority: 1 }
```

The combine rule (new code, not a refactor of an existing reducer): map each Vec entry via
`static_claim_to_language_claim` (yields `None` on `whenClass` mismatch), drop the `None`s, then reduce by an
explicit `ClaimKind` ordering — **Primary > Interop > Fallback**, priority as tiebreak only within equal kind.
Vec order must not matter: interop is deliberately listed *first* in the fixture precisely so a naive
"first-non-`None`" reducer is caught (compat doc §3, plan SC1's vacuity note) — the correct reducer picks
`Primary(2)` for `{sql .marimo}` even though `Interop` appears earlier in the list.

**For an extension author:** if your engine claims one language two different ways depending on context
(tagged-vs-bare, or any other conditional split), you need the Vec form — a single scalar claim per language
can't express it. If your engine's claims are all one-claim-per-language (the common case, like Julia's), the
old scalar form still works; the Vec is additive, not a breaking change to the schema.

### Interop contention between engines (priority, then candidate order)

If **two present engines** both hold an `Interop` claim on the same language (real case: marimo + knitr both
claim `sql` — knitr's builtin claim is `Interop(0)`, `crates/quarto-core/src/engine/knitr/mod.rs:245`), the
winner is deterministic (verified 2026-07-03 against `resolution.rs`):

1. **Highest interop priority wins** (T3, `resolution.rs:496-520`, strictly-greater comparison). An extension
   that needs to outrank a builtin's interop claim declares one: `- { kind: interop, priority: 1 }`.
2. **On a priority tie** (marimo's unprioritized `- { kind: interop }` defaults to 0, `types.rs:181` — so
   marimo-vs-knitr on `sql` IS a 0-0 tie today), the first candidate in `candidate_engines` order keeps the
   win (`resolution.rs:65`): explicitly-declared engines in front-matter order, then **extension engines in
   registration order, then builtins**. So implicitly, marimo (extension) beats knitr (builtin) for a doc's
   bare sql — arguably right, since the author installed and tagged marimo. Authors flip the tie with an
   explicit `engine:` list (its order feeds the candidate order), or preempt T3 entirely via a
   Fallback-claiming explicit engine (T2 — see the documented `[marimo, jupyter]` edge in the plan).

## 2. Bare-sql execution gate (`cellOwnedByMarimo`, argv threading, `extract.py`'s `BARE_SQL_FENCE_REGEX`) — the `handledLanguages` leave-alone contract

Claiming sql at resolution time isn't enough — the engine also has to *execute* the cell it now owns.
Three upstream changes, in order:

**a. `2495a47` — new predicate `cellOwnedByMarimo(cell, handledLanguages)`** in `lib/is-marimo-cell.ts`,
deliberately **not** folded into the existing `isMarimoCell(cell)` (whose only call site has no ownership
info to pass). `execute()` reads `options.handledLanguages` and derives `bareSqlOwned` from it, then threads
that boolean to `extract.py` as a new 4th positional argv (`bare_sql: yes|no`), after `input`/`mime`/`eval`.

**b. `e8ec4fb` — `extract.py`'s `BARE_SQL_FENCE_REGEX`.** marimo's own markdown parser has **no `.marimo`
gate at all** — the gate is entirely TS-side. marimo classifies a cell as SQL only when the fence is already
in the qmd-form `sql {.marimo}` (language before the brace); a sibling `SQL_DOT_FENCE_REGEX` pre-rewrites
`{sql .marimo}`/`{sql.marimo}` into that form. Bare `{sql}` wasn't covered, so it was misclassified as python
and threw a syntax error. `BARE_SQL_FENCE_REGEX` is the sibling that rewrites bare `{sql ...}` → `sql {.marimo
...}`, applied only when the new `bare_sql` argv flag says q2 assigned marimo bare-sql ownership on this
render. Factored as a standalone, unit-testable `rewrite_bare_sql(text, enabled)` (pure regex, no
duckdb/sqlglot/polars import needed for the unit tests).

**c. `77c15c8` — FINDING #4, the load-bearing correction: `handledLanguages` is a leave-alone set, not a
positive-ownership set.** The first implementation (`2495a47`) read
`(options.handledLanguages ?? []).includes("sql")` as "q2 assigned me sql" — backwards. q2's wire field is
documented (now — see the q2-core section below) and proven by a pre-existing q2 unit test
(`crates/quarto-core/src/engine/jupyter/text_execute.rs:600-655`, "sql must NOT be in jupyter's
handled_languages — it is owned by jupyter... not something it cedes") to be the **complement**:
`EngineResolution::handled_languages_for` is q2's built-in `HANDLED_LANGUAGES` set **union** every language
this render assigned to a *different* engine. Because q2's resolver assigns every language present in the
document an owner (or hard-fails) before `execute()` ever runs, "language absent from `handled_languages`"
and "language owned by me" coincide for any language the engine actually has a decision to make about — so
the sound check is the **negation**: `!handledLanguages.includes("sql")`. Both `bareSqlOwned` (in
`marimo-engine.ts`) and `cellOwnedByMarimo` (in `is-marimo-cell.ts`) were flipped to this complement; both
doc comments were corrected to state the leave-alone semantics explicitly and cite `resolution.rs:292`.

**The contract, spelled out for future TS engine authors** (this is the reusable lesson, not marimo-specific):
`handledLanguages` in your `execute()` options tells you which languages you should **leave alone** — your own
built-in set, plus anything another engine already owns in this render. It is never a list of languages
assigned *to you*. To ask "did q2 assign me ownership of language L," check that L is present in the document
**and absent** from `handledLanguages` — never search `handledLanguages` for your own engine's name (it never
appears there). Get this backwards and your interop/ownership gate silently never fires, with no error — the
render just quietly executes the cell as unowned plain text instead of routing it to you.

**Verification (all upstream, deno + pytest, cited not re-derived):** RED-by-revert with the pre-correction
test file showed 81 passed / 2 failed (exactly the two direct `cellOwnedByMarimo` gate assertions);
corrected assertions then 83/83 green. `pytest tests/python/`: 47/47 green (`extract.py` itself untouched by
the FINDING #4 fix — only the TS-side sense of the flag changed, not its meaning to `extract.py`).

## 3. API-shape notes

**Includes are file paths, not content (a q2-core gap this engine was the first to hit — see the q2-core
section below for the fix).** `marimo-engine.ts` returns `includes["include-in-header"]` as a **temp-file
path**, "like Jupyter does" per its own source comment — the Q1/Pandoc convention every existing TS-engine-host
consumer (jupyter, knitr) codes against. This is not something the extension needed to change; it was already
doing the standard thing. q2's `translate_includes` just didn't honor it for the engine-contributed wire
channel (fixed in q2-core, not the engine).

**`execute()` options — `handledLanguages` is the one field this feature newly *consumes*** (see §2). Every
other `execute()` option marimo reads (`target`, `format`, `metadata`) was already exercised by the
python-primary path validated in 4cB and needed no changes.

**No other QuartoAPI signature gaps found** for the surface marimo touches: `quarto.console`,
`quarto.system.pandoc` (pdf/latex/typst only, via `htmlToMarkdown`), `quarto.mappedString.fromFile`,
`quarto.markdownRegex` (`extractYaml`/`partition`/`breakQuartoMd` with a **custom cell regex** — correction 8,
`breakQuartoMd`'s 4th param `startCodeCellRegex?: RegExp` is supported). All non-`jupyter`, all Plan-2-complete
— matches Julia's finding of zero API gaps, just a different namespace subset.

## 4. Dropped/inert methods

- **`partitionedMarkdown` and `postprocess` — inert.** No wire message or dispatch case exists for either
  (correction 5). Same disposition as Julia found for the same two methods.
- **`checkInstallation` — inert in q2.** Grepped the complete `ToEngine` wire-message enum
  (`ts_protocol.rs:33-101` — `init`, `loadEngine`, `launchEngine`, `shutdown`, `claimsLanguage`, `claimsFile`,
  `markdownForFile`, `execute`, `intermediateFiles`, `dependencies`, `cancel`: eleven variants, no twelfth) and
  every occurrence of `checkInstallation`/`check_installation` across `ts_protocol.rs`/`ts_engine.rs`/
  `ts_process.rs`: zero. The only places the name appears anywhere in the tree are the TS type declaration
  (`ts-packages/quarto-types/src/execution-engine.ts:209`, an optional interface method) and the two fixture
  engines that implement it (julia, marimo). q2 never sends a wire message that would invoke it — there is no
  "runs during every launch" call site to cite. marimo's implementation (a no-op `delay(2000)` + spinner that
  checks nothing) is simply never called under q2.
- **`generatesFigures: true` — no consumer in q2** (correction 6). HTML flows through
  `includes["include-in-header"]` plus inline raw-`{=html}`/`![](…)` output from `render-output.ts`, not
  through a `store_html_dependencies`-style figure-registration path that would read this flag.
- **`canFreeze: false` — accepted-untested, controller-ratified, not re-derived here.** Wire → store →
  `TsEngine::can_freeze()` (`ts_engine.rs:614`) dead-ends at a `Debug` impl (`registry.rs:316`); confirmed
  `RenderOptions.use_freeze` is constructed `false` at every call site (`render.rs:643`,
  `pass2_renderer.rs:809,1066`) — no freeze-consulting code path exists in q2 today to bind a test to.
  Strand `bd-mx5x609r` holds the freeze-epic-time test spec for when q2 grows a real freeze mechanism.

## 5. `first_class`/dotted-language claims

**`whenClass` gates primary claims on the cell's `first_class` tag** (`{python .marimo}` → `("python",
Some("marimo"))`), while **dotted tokens are entirely separate language keys with no `first_class`**
(`{python.marimo}` → `("python.marimo", None)`, `{sql.marimo}` → `("sql.marimo", None)`) — verified with pampa
directly (plan correction 2): `engine_cell_lang` strips the outer `{…}` and returns the inner token verbatim,
so the dot-joined form is a distinct language token needing its own claim key, not a variant of the
space-separated form. Both forms are Q1-legacy syntax still supported (Gordon confirmed Q1's parser did the
same; the dotted form is discouraged but not removed).

**`claimsFile`'s whole-file short-circuit — the biggest gotcha for any content-inspecting engine, spelled out
here for future engine authors.** An engine's `claimsFile` answer operates at a **different, earlier layer**
than the per-language `claims:` map. `EngineClaimsFileStage` (`crates/quarto-core/src/stage/stages/
engine_claims_file.rs`) runs before `ParseDocumentStage`, asks every registered engine whether it claims the
**whole input file**, and — first claimer wins — records that as `ctx.claimed_engine_name`. Per
`engine_execution.rs:226`'s own comment, that whole-file claim **short-circuits ALL per-language tier
evaluation**, functionally identical to an explicit `engine: <name>` frontmatter declaration — it bypasses the
`claims:` map entirely, for every language in the document, not just the one that triggered the file-level
match. marimo's `claimsFile` is content-inspecting by default (no `claims-files:` key declared → answered
dynamically by loading the engine and regex-scanning the file for a `.marimo`-tagged fence via
`containsMarimoFence`/`MARIMO_CELL_REGEX`). This means:

- Any file containing so much as one matching fence gets marimo as the **whole-file** owner, regardless of how
  narrowly the per-language `claims:` map is scoped — arguably desirable for marimo specifically (a file with a
  marimo cell probably *is* a marimo file).
- **But it silently breaks multi-engine coexistence.** A doc with `{python .marimo}` **and** `{r}` (owned by
  knitr) rendered through the unmodified fixture collapses to `[marimo]` ownership only — knitr never runs, and
  the `{r}` cell is spliced back as raw, unexecuted source. No error; just quietly-wrong output (found in 4cD's
  SC16 row).
- It also forces an engine LOAD at file-claim time even for an otherwise fully-static, zero-load engine (per
  the 4c0 Vec design), because a content-inspecting `claimsFile` must spawn the module to run the regex scan.

**Guidance for extension authors and future q2 contributors:** declare `claims-files: []` (or a real static
extension list) in `_extension.yml` if your engine's per-language `claims:` map is meant to be the actual
resolution authority, or if you want multi-engine coexistence to work in documents that also contain your
tagged fence. Leaving `claims-files:` undeclared for a content-inspecting engine is a real, silent
coexistence hazard — general to any TS engine, not specific to marimo. (The fixture keeps the default
undeclared behavior deliberately, to also validate the short-circuit itself and its documented workaround —
tests derive a `claims-files: []` variant at test-setup time where genuine per-language resolution is needed.)

## 6. `deno.json`/mock remap

Upstream `~/src/quarto-marimo`'s own root `deno.json` (used for its own `deno test` suite) maps
`@quarto/types` to a **local test mock** (`./tests/mocks/quarto-types.ts`) and pins `path` to a bare
`https://deno.land/std@0.224.0/path/mod.ts` URL — fine for the engine's own unit tests, wrong for a real
build. **The fixture deliberately does not copy this file** (plan line 388, "Do NOT copy marimo's root
`deno.json`"). Instead, `q2 build-ts-extension` resolves config via q2's own workspace auto-detection (tier 3
— `find_workspace_root` walks up from `_extensions/marimo/` to the repo root, which contains
`ts-packages/quarto-api`), which supplies the **real** `@quarto/types`/`@quarto/api` and the `@std/*`
alias set from `resources/extension-build/deno.json` — the same import-map-parity fix Julia's port already
required (`e56da9c29`; no new alias was needed here, since marimo's imports — `path` only, no `fs/`, `log`, or
`encoding/`) are a subset of what that fix already covers.

**Guidance for extension authors:** if your repo ships its own `deno.json` for local dev/test convenience
(mocked types, pinned URL imports), do not copy it into a q2 build context — let q2's own
`resources/extension-build/deno.json` supply the real `@quarto/types`/`@quarto/api` bindings via workspace
auto-detection. Bringing your own `deno.json` into the fixture would shadow that and either fail to resolve
`@quarto/api` at all, or resolve against your own test mocks instead of the real API surface.

**Remote import left as-is.** The one fully-qualified URL import (`https://deno.land/std@0.224.0/async/
delay.ts`) was left unremapped per the brief — not required to alias, and `deno bundle` fetched/inlined it
without incident (resulting bundle contains a literal `function delay(...)` definition, not a live import —
offline-safe once built).

## 7. Loader-shim replacement (local rebundle vs. GitHub-release shim)

Upstream `_extensions/marimo/marimo-engine.js` (the file `_extension.yml`'s `path:` points at) is a
**GitHub-release downloader shim** — 1160 bytes, not a real bundle; it fetches the actual engine from a
release artifact at install time. This doesn't work in an offline, git-checked-out fixture, so the fixture's
`_extension.yml` instead points at the same relative filename, but populated by **locally rebundling**
`src/marimo-engine.ts` via `q2 build-ts-extension` — the real ~22 KB compiled engine, not the shim. Bundle
sanity checks used throughout: output size (22070 → 22033 bytes across the two SC19-adjacent rebundles,
consistent with like-for-like refactors, not functional changes), `grep -c marimo`/`grep -c '^export'` counts
matching the source, `deno check` clean, and no stray `@quarto/api` string markers (the engine only imports
*types* from `@quarto/types`, erased at bundle time, and references the `quarto` global at runtime).

Rebuilding requires the same `build-ts-extension` directory-resolution workaround Julia's guide already
documents in detail (§4b there): `q2 build-ts-extension <entry.ts>` doesn't work directly (must be a
directory or `_extension.yml` path), and `find_entry_ts`'s `<ext_dir>/src/<ext_dir_basename>.ts` convention
doesn't match a real upstream repo layout (`src/` sits at the repo root, sibling to `_extensions/`, not inside
it) — worked around with the same throwaway, never-committed symlink
(`_extensions/marimo/src -> ../../src`, created immediately before the build, removed immediately after,
confirmed absent both before and after via `find … -type l`).

One marimo-specific wrinkle `find_entry_ts` also survives without a workaround: its exact-name convention
would look for `_extensions/marimo/src/marimo.ts` (not `marimo-engine.ts`), but it falls back to "any single
`.ts` file in `src/`" when the exact-name candidate is absent — and the fixture's `src/` has exactly one file,
so the fallback resolves unambiguously today. This becomes ambiguous if a second `.ts` source is ever added to
`src/` (directory read order isn't a stable convention) — flagged as a latent gap for the same `--entry`
override follow-up Julia's guide already tracks for `build-ts-extension`, not fixed here.

## q2-core changes marimo forced (fixed in q2, no engine action needed)

Three changes landed in q2's own Rust code, not in the extension — marimo was the first real consumer to
exercise these paths (the synthetic echo-engine fixture that validated Plans 1a-3 never exercised a
space-separated `{lang .cls}` fence round-trip through the writer, nor the `include-in-header` wire channel
with a real temp-file-path payload). None of these required any change to `marimo-engine.ts`.

- **Pampa QMD-writer round-trip fix — `411380777`.** The parser encodes a space-separated `{lang .cls}` code
  fence's language as a literal bracket-wrapped class string (`{python .marimo}` → classes `["{python}",
  "marimo"]`), which `engine_cell_lang` unwraps on read. The writer had no mirror: `write_attr` blindly
  dot-prefixed every class, turning the fence back into the malformed `{.{python} .marimo}` on any
  `serialize_ast_to_qmd` round-trip — breaking every TS engine consuming such a cell (surfaced by marimo's
  `{python .marimo}` fixture failing to re-parse after a round-trip). Fix: a new `write_code_attr`, used only
  by `write_codeblock`/inline `write_code` (both confirmed to receive this bracket-wrapped encoding from the
  parser) — a class shaped like `{lang}` is written bare instead of dot-prefixed. Generic `write_attr` (divs,
  spans, links, images, tables) is untouched; no snapshot changes.
- **TS-engine includes read as file paths, not literal content — `13f697c85`.** `marimo-engine.ts` (and TS
  engines generally) send `include-in-header`/`-before-body`/`-after-body` as temp-**file paths**, mirroring
  Q1's `--include-in-header` contract that jupyter/knitr/marimo all code against. q2's `translate_includes`
  was folding those wire strings verbatim into `PandocIncludes`, whose internal contract is **content** — the
  native knitr engine already reads its include files before populating the same struct; the TS-engine wire
  path just never had. Result before the fix: rendered `<head>` contained the literal temp-file *path* as
  text instead of the file's contents, and marimo's injected header markers never reached the output.
  `translate_includes`/`read_include_contents` now read each wire value as a file path and store its content;
  `map_execute_result` is fallible (a protocol violation from the engine is a loud `ExecutionError::other`
  naming the engine, include key, and offending value — no content-vs-path sniffing heuristic). Doc comments
  on `TsPandocIncludes` and the `include_resolve.rs` fold now state the wire-vs-internal contract and cite the
  knitr precedent.
- **`ts_protocol.rs` doc pin on `TsExecuteOptions::handled_languages` — `b4f4f52bf`.** No logic change; a doc
  comment was added stating the leave-alone semantics (§2 above) and citing the FINDING #4 incident by name,
  so the next TS-engine author reads the contract before getting it backwards, instead of after.

**For the migration guide:** these three are "fixed in q2, no engine action needed" — an author porting a
*different* engine that hits a space-separated `{lang .cls}` fence round-trip, or that sends
`include-in-header` as a file path (the Q1-standard convention), or that reads `handledLanguages`, benefits
from all three automatically; nothing to do on the extension side.

**For the upstream PR / `quarto-marimo` maintainers:** these three are useful *context*, not something the PR
needs to carry — they were q2-side gaps, already fixed in q2 proper, unrelated to whether `quarto-marimo`
merges `q2-bare-sql-interop`. Cite them if asked "why did this take q2-core changes at all," but they are not
part of the engine diff being proposed upstream.

## Summary table

| Category | Finding |
|---|---|
| `claimsLanguage` interop widening | Upstream `2495a47`: bare `sql` → `{kind:"interop"}`. Required q2-core's Vec-per-language claims (4c0) so one `sql` key can hold both a primary and an interop claim. |
| Bare-sql execution gate | Upstream `2495a47`/`e8ec4fb`/`77c15c8`: new `cellOwnedByMarimo`, argv-threaded `bare_sql` flag, `extract.py`'s `BARE_SQL_FENCE_REGEX`. FINDING #4: `handledLanguages` is q2's leave-alone set, not positive ownership — get this backwards and the feature silently never fires. |
| API-shape | No signature gaps in the touched surface (console, pandoc, mappedString, markdownRegex incl. custom cell regex). `handledLanguages` newly consumed. Includes are file paths (see q2-core fixes). |
| Dropped/inert methods | `partitionedMarkdown`/`postprocess` inert (no wire case); `checkInstallation` inert in q2 (no wire case, grep-confirmed); `generatesFigures` no consumer; `canFreeze:false` accepted-untested (bd-mx5x609r). |
| `first_class`/dotted-language | `whenClass` gates space-separated claims; dotted (`python.marimo`) tokens are separate keys. `claimsFile`'s whole-file short-circuit bypasses ALL per-language `claims:` resolution and breaks multi-engine coexistence unless `claims-files: []` is declared — general TS-engine gotcha, not marimo-specific. |
| `deno.json`/mock remap | Upstream's own root `deno.json` (test mocks, pinned URL imports) deliberately not copied; q2's workspace auto-detection + `resources/extension-build/deno.json` supplies the real `@quarto/api`/`@quarto/types`. |
| Loader-shim replacement | Upstream `marimo-engine.js` is a GitHub-release downloader shim (1160 bytes); fixture rebuilds the real ~22 KB bundle locally via `build-ts-extension` + the same directory-resolution symlink workaround Julia's guide documents. |
| q2-core changes forced | Pampa QMD-writer bracket-class round-trip fix (`411380777`); TS-engine includes read as file paths not literal content (`13f697c85`); `ts_protocol.rs` doc pin on the leave-alone contract (`b4f4f52bf`). All "fixed in q2, no engine action needed." |

## Bottom line

**For an extension author porting a different TS engine:** the two load-bearing lessons this validation adds
beyond Julia's are (1) if any of your claims are conditional on cell tagging vs. bare use, you need the Vec
claims form, and your live `claimsLanguage` must agree with your static declaration exactly (q2 hard-errors on
mismatch); (2) `handledLanguages` in your execute options is always a **leave-alone** set — check for your
target language's *absence* from it to infer "I own this," never search it for your own name. If your engine's
`claimsFile` is content-inspecting (any dynamic answer, not a declared `claims-files:` list), declare
`claims-files: []` unless you specifically want whole-file ownership to override all other engines' per-language
claims in mixed documents.

**For the `quarto-marimo` upstream PR (`q2-bare-sql-interop` → `main`):** the branch is four commits
(`2495a47`, `e8ec4fb`, `77c15c8`, `2a2f312`), fully green upstream (83/83 deno, 47/47 pytest) and fully green
in q2's e2e suite (`marimo_engine_e2e.rs`, `marimo_resolution.rs` — SC8-SC19 all closed, `cargo nextest run -p
quarto-core` clean). The PR is self-contained: no q2-core change is a prerequisite for merging it upstream (the
three q2-core fixes above are q2-side infrastructure, already landed, not part of this diff). Net behavior
change for existing marimo users: bare `{sql}` cells in a document that also contains a `{python .marimo}` (or
`{sql .marimo}`) cell now execute via marimo instead of being left as plain unexecuted text — additive, no
existing claim or execution path is narrowed.
