# Julia engine: upstream fixes, static declarations, and bundling in q2 (epic, DRAFT)

> ## ⚠️ PROVISIONAL — NEEDS REVIEW
>
> This is a **first-pass draft written from research, not from a decision**. The
> sequencing, the scope, and the assignment of work to PRs are all proposals and
> have **not been agreed with anyone** — including PumasAI, who own the
> extension. Several load-bearing steps depend on external parties and external
> release timelines. Nothing here should be treated as settled, and no part of
> it has been started. **Review before acting on any of it.**

**Status:** preliminary draft — research done, not scoped into tasks, not started.
**Author context:** written 2026-09-03 off the back of the worker-leak
investigation (`0f243f64c` on `julia-orphan-triage`). Nothing here is committed
to upstream yet.

## Overview

Three workstreams, in increasing order of risk and dependency:

1. **The worker-leak bug fix** (plus two sibling fixes) — pure bug fixing,
   Q1-compatible, no schema implications.
2. **The static engine declarations** in `_extension.yml` (`name:`, `claims:`,
   `file-extensions:`) that let Quarto 2 resolve the engine in **pass 1 without
   loading it**. These are *rejected by Quarto 1 today* and cannot land alone.
3. **Bundling the julia engine into q2** as a vendored subtree, so `q2` ships
   with Julia support instead of requiring a separate extension install. This
   needs the engine's q2-flavoured declarations to exist somewhere stable
   first — i.e. it follows (2).

(2) is blocked on a `quarto-cli` schema change **and on that change reaching a
stable Quarto release**, which is why this is an epic rather than a PR. (1) is
independent and can go immediately.

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

### F8 — bundling: q2 already has the runtime hook; the gap is maintenance

Quarto 1 vendors whole extension repos as **git subtrees** under
`src/resources/extension-subtrees/<name>/`, kept in sync by a hidden dev
command (`src/command/dev-call/pull-git-subtree/cmd.ts`). That command is a thin
wrapper over `git subtree add/pull --squash`, with a hard-coded `SUBTREES` table:

```ts
{ name: "julia-engine",
  prefix: "src/resources/extension-subtrees/julia-engine",
  remoteUrl: "https://github.com/PumasAI/quarto-julia-engine.git",
  remoteBranch: "main" }
```

It finds the last split via `git log --grep="git-subtree-dir: <prefix>$"`, falls
back to `subtree add` when the prefix is new, and no-ops when there are no new
commits. Requires `QUARTO_ROOT`. Discovery side (`extension/extension.ts:680-695`):
extension lookup falls back to scanning
`resourcePath("extension-subtrees")/*/_extensions/<name>`, a **separate root**
from `resourcePath("extensions")`.

**q2 already has the equivalent runtime machinery.** `builtin_extensions_path()`
(`extension/mod.rs:43-80`) embeds `resources/extensions/` via `include_dir!`,
lazily extracts it to a temp dir through `ResourceBundle`, and has a WASM VFS
variant; `discover_extensions` takes a `builtin_extensions_dir` that is
**scanned first**, and it is already wired up from `project/mod.rs:2206` and
`stage/context.rs:298`. So there is **no discovery work to port** — an extension
dropped into the embedded bundle is found automatically.

**Sizes matter for the layout decision:**

| | size |
|---|---|
| `resources/extensions/` today (7 bundled extensions) | 712K |
| julia-engine `_extensions/` payload (what must ship) | **68K** |
| julia-engine **whole repo** (tests, `.github`, `src`, docs) | **14M** |

Q1's subtree vendors the *entire* repo. Embedding that wholesale via
`include_dir!` would put 14M of tests and CI config into every `q2` binary. So
q2 should subtree the full repo into `resources/extension-subtrees/julia-engine/`
(git cost only, mirroring Q1) but point `include_dir!` at just its
`_extensions/` subdirectory — 68K in the binary. **Open: is 14M in q2's git
history acceptable, or should we vendor a curated copy instead and give up
`git subtree`'s merge tracking?**

The genuinely new work is therefore the **maintenance command**, not the
runtime: port `pull-git-subtree` as `cargo xtask pull-extension-subtree`. xtask
is the right home — it already hosts every comparable maintenance/build task
(`build_agents_docs`, `build_hub_mcp_bundle`, `braid_snapshot`, …).

**Synergy with the fixture-drift problem:** if the bundled copy is subtreed from
the **q2 branch** (which carries the static declarations), then the bundled
extension already has `claims:` — q2 gets pass-1 static resolution for Julia out
of the box, and the hand-maintained test fixture could potentially be replaced
by (or derived from) the bundled copy, retiring the fork-drift item in Step 4.

*Not yet investigated:* `filterBundledSubtreeEngines` (`extension.ts:734`, used
at `render/pandoc.ts:446,1324`) strips bundled subtree engines out of the
metadata `engines` array handed to pandoc. Whether q2 needs an analogue depends
on how q2 surfaces `engines` in metadata — not traced.

## Proposed sequence

### Step 0 — talk to Julius first

Before any of the below: **discuss with Julius Krumbiegel / PumasAI.** Steps 1
and 2b add Quarto-2-only surface to *their* extension and constrain *their*
`quarto-required`. The conversation should cover:

- whether they are willing to carry q2-only keys at all (and if so, whether in
  `main` or only on a branch),
- the `quarto-required` bump and its cost to their users (see Step 2b),
- the offer in Step 2c (a q2 branch), which may be the outcome they prefer.

This gates 2b entirely and may change its shape. Do it early — it is cheap and
it is the step most likely to invalidate the rest of the plan.

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

**Then wait for a full, stable Quarto release carrying it.** *(Revised — an
earlier draft targeted a prerelease.)* A published extension whose
`quarto-required` names a prerelease would refuse to install for ordinary users
on stable Quarto, which is not something we should ask PumasAI to ship. This
makes the epic's critical path a **release cycle**, not weeks — which is exactly
why Step 2c exists.

### Step 2 — quarto-julia-engine

**(a) The bug fix — ready now, no dependencies.** Three commits currently on
`q2-close-busy-fix` (rebased onto upstream v0.2.1, bundle verified in sync):

- `b881e69` redirect the detached server's stdio to devnull
- `f7c9bfc` recover from a busy/failed oneShot worker close
- `4e6bc27` close the oneShot worker when the run fails (the leak)

Fully Q1-compatible; no schema dependency; independent of everything else above
and below. Needs a CHANGELOG entry (enforced by `EnforceChangelog.yml`).

*Open: one PR or three?* They are independently reviewable and the leak fix is
the one with hard evidence.

**(b) The static declarations — gated on Step 1 shipping in a stable release.**
Adds the `name:`/`claims:`/`file-extensions:` block, and must additionally:
- bump `quarto-required:` to the **stable** release from Step 1,
- bump `QUARTO_CLI_REV` in `ci.yml` to a commit containing the schema change,
- add a CHANGELOG entry,
- explain F2 (accept-and-ignore, not a Q1 behavior change) and F3
  (`file-extensions` != `validExtensions`) in the PR body.

**(c) Offer a q2 branch on `PumasAI/quarto-julia-engine` — the unblocker.**
Because 2b waits on a release cycle, offer to maintain a **branch** on the
upstream repo (name TBD, e.g. `q2`) carrying the static declarations, so people
who want to try **Julia in Quarto 2 before it ships** can point at it. This:

- gives early adopters a real, upstream-hosted path with no prerelease
  `quarto-required` and no fork of record,
- gives q2 a **stable remote to subtree from** for Step 5 (see F8) — the
  bundled copy would track this branch, not `main`,
- keeps `main` clean and Q1-only until Step 1's schema change is stable,
  which is likely what PumasAI would prefer anyway,
- lets us validate the declarations against real users before asking for them
  in `main`.

*Open: does the branch live on PumasAI (preferred — upstream-hosted, discoverable)
or on the `gordonwoodhull` fork (no permission needed)? This is part of the
Step 0 conversation.*

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

### Step 4 — q2: bundle the julia engine as a vendored subtree

**Goal:** `q2` ships with Julia support built in — no separate extension
install. Gated on Step 2c (a stable branch to subtree from). See **F8** for the
research; the headline is that q2 **already has the discovery + embedding
machinery**, so this is mostly maintenance tooling, not a port.

Work items (first pass, not scoped):

- **Vendor the subtree.** `git subtree add --squash` the Step 2c branch into
  `resources/extension-subtrees/julia-engine/`, mirroring Q1's layout. Decide
  the git-history cost first (14M — see F8 open question).
- **Embed only the payload.** Point a second `include_dir!` at
  `resources/extension-subtrees/julia-engine/_extensions` (68K), not at the
  subtree root, and expose it through the existing `ResourceBundle` /
  `builtin_extensions_path()` path. *Open: does `builtin_extensions_path`
  return one dir (requiring the subtree payload to be merged into the existing
  bundle) or should discovery accept a list of builtin roots, mirroring Q1's
  two separate roots?* — this is the main design decision in Step 5.
- **Port the maintenance command** as `cargo xtask pull-extension-subtree`
  (Q1: `src/command/dev-call/pull-git-subtree/cmd.ts`): a `SUBTREES` table, last-split
  detection via `git log --grep="git-subtree-dir: <prefix>$"`, `subtree add`
  when the prefix is new, `subtree pull --squash` otherwise, no-op when there
  are no new commits. Drop the `QUARTO_ROOT` env dependency — xtask already
  knows the repo root.
- **Decide the fixture's future.** If the bundled copy carries the static
  declarations, the hand-maintained fixture fork may be replaceable by (or
  derivable from) the bundled copy — which would retire the drift item in
  Step 5. Needs care: the julia e2e tests deliberately copy the fixture into a
  temp project.
- **Runtime prerequisites.** Bundling ships the engine, not Julia itself:
  QuartoNotebookRunner still instantiates on first use (network), and the
  engine host still needs Deno. Worth an explicit UX decision about what
  `q2` does on a machine with no Julia.
- *Not investigated:* whether q2 needs an analogue of
  `filterBundledSubtreeEngines` (F8).

### Step 5 — what else belongs (candidates, not yet decided)

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
- **Q2.** *(Resolved in this revision — target a stable release, not a
  prerelease.)* Remaining: how long is that cycle, and does it change what we
  do in the meantime beyond Step 2c?
- **Q3.** Step 0: what does Julius say? Everything in Step 2 is contingent on
  it. Specifically: q2-only keys in `main` or only on a branch; who hosts the
  Step 2c branch; and are they willing to bump `quarto-required` at all.
- **Q7.** Step 4 layout: one builtin-extensions root (merge the subtree payload
  into the existing embedded bundle) or teach discovery a **list** of builtin
  roots (mirroring Q1's separate `extensions` / `extension-subtrees` roots)?
- **Q8.** Is 14M of vendored repo acceptable in q2's git history for the sake of
  `git subtree`'s merge tracking, or do we vendor a curated 68K copy and accept
  manual syncing?
- **Q9.** Once Julia is bundled, what is the story on a machine without Julia
  installed — silent fallback to jupyter, or a diagnostic?
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
