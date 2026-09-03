# Julia engine: upstream fixes + static engine declarations (epic, DRAFT)

**Status:** preliminary draft — research done, not scoped into tasks, not started.
**Author context:** written 2026-09-03 off the back of the worker-leak
investigation (`0f243f64c` on `julia-orphan-triage`). Nothing here is committed
to upstream yet.

## Overview

Two things want to go upstream to `PumasAI/quarto-julia-engine`, and they have
very different risk profiles:

1. **The worker-leak bug fix** (plus two sibling fixes) — pure bug fixing,
   Q1-compatible, no schema implications.
2. **The static engine declarations** in `_extension.yml` (`name:`, `claims:`,
   `file-extensions:`) that let Quarto 2 resolve the engine in **pass 1 without
   loading it**. These are *rejected by Quarto 1 today* and cannot land alone.

The second is blocked on a `quarto-cli` schema change, which is why this is an
epic rather than a PR.

## Key research findings

### F1 — Q1's `external-engine` schema is closed; all four keys are rejected

`src/resources/schema/definitions.yml:295-304` (both `external-sources/quarto-cli`
and the dev checkout `~/src/quarto-cli`):

```yaml
- id: external-engine
  schema:
    object:
      closed: true            # <-- no additional properties
      properties:
        path: { path: { description: ... } }
      required: [path]
```

Verified empirically against the dev build (`quarto` 99.9.9), one key at a time:

| key | result |
|---|---|
| `path` only | ACCEPTED |
| `name` | REJECTED |
| `claims` | REJECTED |
| `file-extensions` | REJECTED |
| `claims-files` | REJECTED |

Failure is at `readExtension` → `readAndValidateYamlFromFile` — the extension
fails to **load**, so it is a hard render error, not a warning. Shipping the
declarations upstream without the schema change would break the extension for
every Q1 user.

### F2 — q2 deliberately inverts the Q1 jupyter/julia default

Q1 (`execute/jupyter/jupyter.ts`): jupyter's `claimsLanguage` returns true for
`julia`, with the comment *"jupyter has to claim julia so that julia may also
claim it without changing the old behavior of preferring jupyter over julia
engine by default."* So **in Q1, jupyter wins `{julia}` by default.**

q2 (`engine/jupyter/mod.rs:168-173`): jupyter is a universal `Fallback(0)`, and
the comment states the Julia extension's `Primary(1)` wins when installed (kind
dominates priority), with `{julia}` still reaching jupyter via the T4 fallback
tier when the extension is absent.

**Consequence for the schema PR:** if Q1 ever *honors* `claims:`, it would flip
its own jupyter/julia default. The schema change should therefore be
**accept-and-ignore** (pure forward-compatibility for Quarto 2), explicitly
*not* new Q1 behavior. This needs to be stated in the PR description or a
reviewer will reasonably read it as a behavior change.

### F3 — `file-extensions: [.jl]` is not a transcription of `validExtensions()`

The engine's `validExtensions: () => []`. In Q1 that is correct because
`validExtensions` is a **global admission gate** (`execute/engine.ts:311-317`):
if *no* engine lists the extension the file is rejected before any `claimsFile`
runs — and **jupyter** lists `.jl` via `kJupyterPercentScriptExtensions`, so the
gate passes and julia's `claimsFile` is still consulted.

In q2, `file-extensions` is a **per-engine can-handle pre-filter**, so `.jl`
must be declared or q2 never asks julia about a `.jl` file. This matches
`claude-notes/designs/engine-resolution.md:131-134`, but it means the same key
name carries different semantics in the two systems. Needs an explicit note in
the PR, and ideally a description in the schema itself.

### F4 — `claims-files` correctly omitted; `.jl` input is still not zero-load

julia's `claimsFile` is `isPercentScript(file, [".jl"])` — a content sniff. q2's
`FileClaim` is `{ extension }` only; `content-pattern` is **not implemented**
(referenced in a `types.rs` comment; the plan exists at
`claude-notes/plans/2026-07-07-plan7a-static-content-pattern-claims.md`).

So the pass-1 zero-load win currently covers **`{julia}` cells in `.qmd`**, not
`.jl` percent-script input — julia still loads to answer the file claim.
Finishing that half is Plan 7a, not this epic, but the epic should say so.

### F5 — static claims lookup is case-sensitive; dynamic claiming is not (CONFIRMED)

- Dynamic: `claimsLanguage: (language) => language.toLowerCase() === "julia"`.
- Static: `parse_claims_map` inserts `entry.key.clone()` with **no** lowercasing
  (unlike `file-extensions`, which normalizes to undotted lowercase at parse),
  and `lookup_static_claim` does an exact `claims.get(language)`.
- The language token is **never** normalized on the way in: `engine_cell_lang`
  (`capture_splice.rs:86-100`) returns the `{lang}` class body verbatim, and
  `walk_block_for_langs` stores it verbatim. No `to_lowercase` anywhere in
  `resolution.rs`.

So a `{Julia}` cell claims **dynamically** but not **statically** — the static
declaration is not the "complete replacement" §3.3 promises. This is a q2 bug,
independent of anything upstream.

### F6 — upstream CI pins quarto-cli by full commit hash

`.github/workflows/ci.yml`:

```yaml
QUARTO_CLI_REPO: quarto-dev/quarto-cli
QUARTO_CLI_REV: 97e7649bf14607cf39cda13f013185a4146e047b # v1.9.35
```

CI checks out that exact quarto-cli rev and runs `./configure.sh`. So the
static-declarations PR must **also bump `QUARTO_CLI_REV`** to a commit
containing the schema change, or its own CI fails. Easy to miss.

Upstream CI also **verifies the bundled JS is up to date** with `src/`, and
`EnforceChangelog.yml` requires a CHANGELOG entry on every PR.

### F7 — `quarto-required` is inert in q2

q2 parses `quarto-required` into `Extension` but never compares it
(`ts_engine.rs:2901` — "carrier (inert in 1c)"). So bumping it constrains **Q1
users only**; q2 is unaffected either way.

## Proposed sequence

### Step 1 — quarto-cli: loosen the `external-engine` schema

Add `name`, `claims`, `file-extensions`, `claims-files` to the `external-engine`
definition (or drop `closed: true`), documented as **accepted and ignored by
Quarto 1**, reserved for Quarto 2 resolution.

Notes:
- `src/resources/schema/json-schemas.json` is a **generated** artifact
  (`src/core/schema/json-schema-from-schema.ts:168`); the PR must include the
  regenerated file. *Open: exact regeneration command — not yet confirmed.*
- Decide whether to type the keys properly (better errors, more review surface)
  or accept them loosely.
- Land it, then get it into a **prerelease** so downstream can depend on it.

### Step 2 — quarto-julia-engine: two PRs

**(a) The bug fix.** Three commits currently on `q2-close-busy-fix` (rebased
onto upstream v0.2.1, bundle verified in sync):

- `b881e69` redirect the detached server's stdio to devnull
- `f7c9bfc` recover from a busy/failed oneShot worker close
- `4e6bc27` close the oneShot worker when the run fails (the leak)

Fully Q1-compatible; no schema dependency; can go **immediately**, independent of
everything else. Needs a CHANGELOG entry (enforced).

*Open: one PR or three?* They are independently reviewable and the leak fix is
the one with hard evidence.

**(b) The static declarations.** Adds the `name:`/`claims:`/`file-extensions:`
block, and must additionally:
- bump `quarto-required:` to the step-1 prerelease,
- bump `QUARTO_CLI_REV` in `ci.yml` to the schema-change commit,
- add a CHANGELOG entry,
- explain F2 (accept-and-ignore, not a Q1 behavior change) and F3
  (`file-extensions` ≠ `validExtensions`) in the PR body.

**Risk:** requiring a *prerelease* in a published extension's
`quarto-required` is user-hostile — it would make the extension refuse to
install on stable Quarto. See open question Q2.

### Step 3 — q2: fix the static-claim case sensitivity (F5)

Independent of upstream; can land any time. Options:

1. Lowercase claim keys at parse in `parse_claims_map` **and** lowercase the
   language at lookup in `lookup_static_claim` (mirrors how `file-extensions`
   already normalizes at parse). Preferred — normalizes both sides at the
   boundary, consistent with existing precedent.
2. Normalize the language once, further upstream (at `engine_cell_lang` /
   `walk_block_for_langs`). Wider blast radius: that token feeds more than claim
   lookup.

TDD: a failing test with a `{Julia}` cell against a static `claims: {julia: ...}`
registry, RED before the fix.

### Step 4 — what else belongs (candidates, not yet decided)

- **Fixture drift management.** The q2 fixture is a hand-maintained fork
  (`claude-notes/plans/2026-04-16-julia-validation.md`). After this epic it
  carries **two permanent deliberate deviations**: the `claims:` block (q2-only
  schema) and the Bug C comments in
  `start_quartonotebookrunner_detached.jl` (code byte-identical to upstream).
  Nothing checks fixture-bundle ≡ fixture-TS (upstream CI does; q2 has no
  equivalent), and nothing tracks fixture-vs-upstream drift. Candidate: a
  documented refresh procedure, optionally an `xtask lint` rule.
- **Plan 7a (`content-pattern`)** to make `.jl` percent-script input zero-load
  too (F4) — the other half of "pass-1 happy".
- **Long-term home for the declarations if upstream declines.** q2's fixture
  stays a fork indefinitely; alternatively the design doc's author-side
  document-level `engines: [{julia: {claims: ...}}]` table could carry them
  without touching `_extension.yml`. Worth deciding *before* asking upstream.
- **`author:` field drift.** Fixture says `Quarto Julia Engine`; upstream has
  none; a local stash adds `PumasAI`. Trivial, but pick one.
- **Housekeeping:** `~/src/quarto-julia-engine` has `stash@{0}` holding
  `.gitignore` + a `q2-test-unknown-key: hello` probe (that probe's question is
  now answered by F1 — unknown keys are rejected). Drop or apply.

## Open questions

- **Q1.** Should the quarto-cli schema change *type* the new keys (full
  property schemas) or just stop being `closed`? Typing them documents the
  contract and gives good errors, but invites the question "what does Quarto 1
  do with these?" — to which the answer is "nothing" (F2).
- **Q2.** Is requiring a prerelease in upstream's `quarto-required` acceptable
  to PumasAI at all? If not, PR 2b probably has to **wait for a stable Quarto
  release** carrying the schema change — which changes the epic's timeline from
  weeks to a release cycle. This is the single biggest scheduling risk.
- **Q3.** Has any of this been discussed with PumasAI / jkrumbiegel? Adding
  Quarto-2-only keys to *their* extension needs buy-in, and the answer changes
  how PR 2b should be pitched.
- **Q4.** PR 2a: one PR or three?
- **Q5.** Does q2 want the fixture to track upstream mechanically (refresh
  script + drift lint) or stay a hand-maintained fork?
- **Q6.** Is `{Julia}` (non-lowercase language) actually reachable in practice,
  or is the F5 fix purely defensive? It is a real divergence either way, but it
  affects priority.

## Not doing / out of scope

- Implementing `claims:` semantics **in Quarto 1** (F2 — it would flip Q1's
  jupyter/julia default).
- Plan 7a itself (tracked separately).
- Any braid strands: this is plan-scoped work, so items live in this
  checklist when the epic is actually scheduled.
