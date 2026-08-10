# Feature porting: Quarto 1 → Quarto 2

A two-phase process for porting a Quarto 1 feature to Q2, distilled
from the project-profiles port (bd-fu16z22k, PR #492,
`claude-notes/plans/2026-08-10-project-profiles-port.md`). The
long-term goal is to run Phase 2 as an independent agent workflow;
this document is the process contract both phases follow.

**The tension every port must resolve:** we want the feature to feel
like Quarto 1, but Q1's weak source-location infrastructure made it
guess at user intent instead of validating. Q2 errs toward strict
validation with good, actionable, span-carrying diagnostics. Phase 1
exists to decide — with the user — where each feature lands on that
line. Phase 2 executes those decisions without re-litigating them.

---

## Phase 1 — scoping (user + agent, interactive)

All user involvement happens here. The output is a plan document an
independent agent can execute without asking further questions.

### 1. Track the work

- Create a braid strand for the port (`braid create … --json`).
- **Search the skein for prior art before designing anything**:
  `braid list` / grep the snapshot for the feature name. In the
  profiles port, two existing strands (bd-ev8mk1rp, bd-mlj6) had
  already scoped parts of the work and captured wrinkles (e.g. Q1's
  dotenv bootstrap) that the fresh research would have found late.
  Link them (`related`), and close them at the end if implemented.

### 2. Research — three parallel investigations

Run these as parallel background agents; each produces a
self-contained report:

1. **Q1 implementation** (`external-sources/quarto-cli`): exact
   files, functions, line numbers; the real algorithm including
   undocumented behavior, silent fallbacks, and bugs (the profiles
   port found: whitespace producing empty profile names, silent
   mixed-shape groups, an undocumented Posit Connect auto-detect,
   `--profile` implemented by env-var mutation). Ask for verbatim
   snippets of the core logic — the report must stand alone.
2. **Q1 documentation** (`external-sources/quarto-web`): the
   *documented contract* — syntax, precedence, examples worth
   reusing as test cases, documented warts (e.g. "metadata-files
   are not resolved in profiles"), and gaps where the docs are
   silent (those gaps become explicit design decisions).
3. **Q2 architecture** (this repo): where the feature plugs in —
   the seams (specific files/functions), existing types to reuse,
   diagnostic-code ranges, **terminology collisions** (in the
   profiles port, "profile" already meant `DocumentProfile`), and
   the print/plumbing paths a new config value must reach.

Also check **in-flight PRs** that overlap (`gh pr list`): PR #486
was mid-flight during the profiles port; the plan sequenced the
dependent phase after its merge and everything else before, so
nothing blocked.

### 3. Synthesize into three artifacts

- **Divergence table** — one row per behavior where Q2 will differ
  from Q1, with the Q1 behavior, the Q2 behavior, and why. This is
  the heart of the port: it drives the design questions, becomes
  test cases, and ends up (in user-facing form) in the docs page.
- **Strictness list** — every place Q1 is silent where Q2 will emit
  a diagnostic, with proposed severity and a new `Q-*` code from the
  right subsystem range.
- **Architecture proposal** — the seam(s), new types/fields, and the
  cross-cutting surfaces the feature must touch (see the Phase 2
  audit list below), so their cost is visible before approval.

### 4. Decide with the user

Bring the genuinely user-owned decisions as structured questions
(AskUserQuestion for crisp choices; prose for nuanced ones), with a
recommendation each. Typical categories:

- scope (which sub-features now, which deferred to strands);
- fidelity vs. adaptation for each divergence-table row;
- severities (silent / warning / error) for the strictness list;
- naming/CLI surface questions.

Record every answer in a **"Decisions locked"** section of the plan
with the date. Phase 2 treats these as settled.

### 5. Write the plan

`claude-notes/plans/YYYY-MM-DD-<feature>-port.md`, containing:
overview; terminology warnings; condensed Q1 reference (so Phase 2
never has to re-research); divergence table; decisions locked;
architecture; **phased checklist where every phase's first item is
its failing tests**; deferred/out-of-scope list with strand IDs;
coordination notes for in-flight PRs. Point
`claude-notes/plans/CURRENT.md` at it and reference it from the
strand. Iterate with the user until they give the go-ahead — that
go-ahead is what authorizes Phase 2's autonomy.

---

## Phase 2 — implementation (agent, autonomous)

The agent works the plan alone. No design questions back to the
user; if execution reveals that a locked decision is wrong or a
genuinely new decision appears, stop and report rather than
improvise (a workaround that undoes a locked decision means the plan
was not good enough — CLAUDE.md's rule).

### Execution loop, per plan phase

1. **Tests first, observed failing.** Write the phase's tests before
   its implementation and run them: assertion failures against a
   stub or missing wiring, not compile errors. Unit tests for pure
   logic; integration tests at the crate level; **binary-driven
   tests** (`env!("CARGO_BIN_EXE_q2")`) for anything CLI-visible —
   they also solve env-var isolation (`.env_remove()` on the child).
2. Implement to green.
3. **Phase gate**: full workspace `cargo nextest run` (monorepo —
   crate-local green is not enough), clippy, fmt, the
   `claude-notes/instructions/review.md` checklist.
4. **Commit-and-continue at the clean boundary** — one plan phase
   per commit, with a commit message that summarizes behavior, key
   decisions, and test evidence. (Policy in CLAUDE.md §Git
   Workflow.)
5. Update the plan checklist as items complete; record E2E evidence
   inline (exact invocation + observed output snippet).

### End-to-end verification is not optional

Tests passing is necessary, not sufficient (CLAUDE.md
§End-to-end verification). Each user-visible phase gets a real
`cargo run --bin q2 -- …` invocation with the output inspected. Two
traps from the profiles port:

- **Stale binary**: `cargo nextest -p <crate>` rebuilds test
  binaries, *not* `target/debug/q2`. Rebuild `-p quarto` before
  trusting a manual E2E run — a "failing" feature may just be an
  old binary.
- **Real engines**: if the feature touches engine subprocesses,
  verify with a live kernel once (a jupyter cell printing the
  variable), not just the spawn-site unit test.

### Cross-cutting audit list

Q2 features rarely live in one file. For each new config
value/file/flag, sweep these (all bit the profiles port):

- **Source tracking**: every new file whose values merge into config
  must join the `bind_config_source` candidate lists *and* the
  `MetadataMergeStage` register closure, or diagnostics degrade to
  span-less. Grep `extension_manifest_paths` for the full list of
  candidate sites.
- **Cache keys**: new inputs that change render output must join
  `Pass1KeyInputs` (count-prefixed, order-sensitive if order is
  semantic) or stale pass-1 results get served.
- **All construction sites** of any widened struct (the compiler
  finds them — but budget for them; `RenderScriptsContext` had 6).
- **Both pipelines**: native + WASM/preview (`build_transform_pipeline`
  and its preview variant; `StageContext`). The WASM leg only
  compiles under `cargo xtask verify` — run the full verify before
  declaring done.
- **Print paths**: a diagnostic pushed somewhere nobody prints is
  invisible; verify the vector you append to reaches the CLI (and
  through the real binary, not just in-process).
- **Terminology**: honor collisions identified in Phase 1
  consistently (never bare `profiles` in the port; comments where
  both meanings meet, e.g. the cache key).

### Discovered work

File it immediately as a strand with `--deps discovered-from:<id>`
and move on — do not scope-creep the port. This includes
infrastructure bugs found en route (the profiles port filed a
jupyter kernel leak that another session fixed the same day) and
sub-features whose sound implementation is bigger than the port
needs (preview `--profile` threading).

### Shipping (divergence from the default no-PR policy)

When the plan is complete and `cargo xtask verify` (full, WASM leg)
is green, the agent — **without further permission** — does:

1. Push the work to a feature branch
   (`feature/<strand-id>-<slug>`), never to `main`.
2. Open a PR labeled **`feature-port`**. The label is the workflow
   tag: a separate review process picks these up, reviews, merges,
   and closes the strand. The implementing agent does *not* merge.
3. The PR body carries the full session summary — the same report
   the agent would give the user: a **commit-per-phase table**,
   design highlights (precedence rules, policy compliance,
   terminology, strictness upgrades), the test plan with counts and
   red-first evidence, E2E evidence, gates run, incidental fixes,
   and the deferred-work strand list. The PR is the durable record;
   assume the reviewer has not seen the session.
4. Comment the PR URL on the strand; leave the strand open for the
   review process to close.

### Handoff checklist (what must exist when Phase 2 ends)

- [ ] PR open, labeled `feature-port`, body = full summary
- [ ] One commit per plan phase, each with workspace-green evidence
- [ ] Plan doc fully checked off, E2E evidence recorded inline
- [ ] Docs page(s) under `docs/` rendered with q2 and inspected,
      including a user-facing "Differences from Quarto 1" section
- [ ] A smoke-all fixture if the feature can activate without CLI
      flags (config-driven activation exercises all three runners,
      including WASM)
- [ ] Deferred-work strands filed and linked; superseded strands
      closed
- [ ] New pitfalls/lessons appended to
      `feature-porting-lessons.md` (or a conscious "nothing new")
- [ ] Strand updated with the PR URL

---

## Pitfalls and lessons

The accumulated pitfalls-and-lessons list lives in its own
append-only file:
[`feature-porting-lessons.md`](feature-porting-lessons.md).

- **Phase 1**: read it before designing — several entries are
  Q1-behavior classes that recur across features.
- **Phase 2**: read it before implementing, and **append an entry
  (tagged with the PR) whenever the port hits something this
  process doc didn't predict**. The addition ships in the port's
  PR.
- **Review process**: explicitly review that file's diff on every
  `feature-port` PR. New entries are part of the deliverable; a
  port that visibly hit trouble but added no lesson is itself a
  review question.
