# Feature porting: pitfalls and lessons

A living, append-only companion to
[`feature-porting.md`](feature-porting.md). Implementing agents
(Phase 2) **append entries as part of their port's PR** whenever they
hit something the process doc didn't predict — a trap, a pattern
worth reusing, a Q1 behavior class that will recur. The review
process **explicitly reviews this file's diff** on every
`feature-port` PR: new entries are part of the deliverable, and a
port that hit visible trouble but added no lesson is a review
question in itself.

Entry format: a bold one-line claim, then the mechanism and the
move. Tag each entry with the PR it came from. Newest at the bottom;
never rewrite old entries (correct them with a follow-up entry).

---

## From PR #492 (project profiles, bd-fu16z22k)

- **Q1's schema is load-bearing.** Q1 validates config via YAML
  schemas Q2 doesn't have; a closed-object check Q1 got for free
  must be hand-written in Q2 (profiles: the `profile:` key shape).
  Budget an explicit validation step for any ported config surface.

- **Q1 env-var mutation is a pattern, not an accident.** Several Q1
  features work by writing env vars (`QUARTO_PROFILE`, dotenv
  loading). Q2 policy is *never mutate the process environment* —
  carry the value as data, apply to children via `Command::env`, and
  special-case any shortcode/API that must see the resolved value in
  preference to the real env.

- **The `q2` bin crate's tracing targets are `q2::…`**, not
  `quarto::…` — logging added there was invisible until
  `verbose_to_filter` gained a `q2=` directive. The general shape:
  when adding logs, verify they actually print at the intended `-v`
  level through the real binary before writing a test against them.

- **`Attr` is a map.** Pandoc-style repeated attributes don't exist
  in q2 (`Attr.2` is a `LinkedHashMap`); Q1 semantics relying on
  duplicate keys need a redesign (profiles: comma-OR replaced
  repeated-attribute OR).

- **`.local` file order.** Q1's local-override files are
  `_quarto.yml.local` (`.yml` *then* `.local`), not
  `_quarto.local.yml`.

- **Signature widening conflicts semantically, not textually.** A
  port that adds a parameter to a shared constructor compiles
  locally and still fails PR CI if main gained a new caller in the
  meantime — git merges cleanly, the merge doesn't compile.
  `git fetch && git rebase origin/main` + a full workspace build
  **immediately before opening the PR**, and again whenever PR CI
  fails in code you didn't touch.

- **Don't `git add -A` while fixing rebase fallout.** In-progress
  local files (this document's ancestor, once) ride along into the
  wrong commit. Stage the fix explicitly, or check `git status`
  before committing.

- **Search the skein before designing.** Two existing strands had
  already scoped parts of the profiles work and captured a wrinkle
  (Q1's dotenv `QUARTO_PROFILE` bootstrap) that fresh research
  would have found late or not at all. Prior-art search is a Phase 1
  step, not an optimization.

- **The stale-binary trap.** `cargo nextest run -p <crate>` rebuilds
  test binaries, not `target/debug/q2`; a manual E2E check against
  the old binary can "fail" for code that is actually correct (or
  worse, "pass" for code that isn't there). Rebuild `-p quarto`
  before trusting any manual binary run.

- **A diagnostic nobody prints is invisible.** Pushing warnings into
  a vector proves nothing; verify the vector reaches a CLI print
  site, and test the message through the real binary's stderr
  (profiles: `config_diagnostics` happened to be printed by both
  drivers, but that was checked, not assumed).

- **Config-driven activation makes features testable everywhere.**
  The smoke-all runners can't pass CLI flags, but a feature that can
  activate from config alone (`profile.default`) gets exercised by
  all three runners — native, WASM, Playwright — with one fixture.
  When designing a ported feature, keep a no-flags activation path;
  it's also what makes the WASM/hub story work at all.

- **Concurrent sessions contaminate machine-level evidence.** A
  full-workspace test run failed with timeouts caused by another
  session's build saturating the machine, and orphan-process counts
  were double-attributed for the same reason. Before blaming (or
  filing) machine-level symptoms, check `ps` for concurrent test
  runs and re-measure alone. (Same session, the flip side: a
  reproducible orphan-kernel leak *was* real and became bd-hxhnnlzs
  — fixed by another session the same day.)
