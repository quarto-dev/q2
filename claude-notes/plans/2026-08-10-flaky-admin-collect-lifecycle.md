# Flaky test: admin_collect_lifecycle fails intermittently in full-workspace runs (bd-u0tldu4z)

**Date:** 2026-08-10
**Braid:** bd-u0tldu4z (bug, p3 as filed — but see verdict)
**Branch:** `main` (main checkout; no worktree — investigation only, per `/investigate-beads`)
**Status:** Part A implemented on `main` (user go-ahead 2026-08-10: "do Part A of
bd-eb2wnxkp here"). bd-u0tldu4z stays open for a future dedicated stress
verification; moved-store scenario confirmed out of scope; Parts B/C of the
eb2wnxkp plan remain open questions on that strand.

## Triage verdict

**Root cause identified — bd-u0tldu4z is a duplicate manifestation of
bd-eb2wnxkp** (doc-id case-folding on case-insensitive filesystems), not a
timing/parallel-load race as the strand description hypothesized. The fix is
bd-eb2wnxkp's already-written plan
(`claude-notes/plans/2026-07-28-doc-id-identity-from-paths.md`, on branch
`braid/bd-eb2wnxkp-listdocidsfilesystem-reconstructs-doc-ids`), whose Part A is
exactly the migration-free case-variant recovery the user proposed this
session (evaluated below). Recommendation: link `caused-by`, keep bd-u0tldu4z
open only as the "confirm the test is stable under a stress loop after the fix"
tracking item, and green-light bd-eb2wnxkp's plan.

## The evidence (new this session)

The strand asked for the one thing nobody had: captured failure output (both
prior sightings were fail-fast full-workspace runs). A 150-iteration stress
loop of the two `admin_collect_lifecycle` tests on `main` @ `59bbccb9`
(macOS/APFS) produced **3 failures in 142 iterations (~2%/iteration)**, all
with the same signature — ids identical in 27 of 28 characters, differing
**only in the case of character index 1**:

| # | iter | test / line | left (path-derived) | right (true id) |
|---|------|-------------|--------------------|-----------------|
| 1 | 21  | `collect_lifecycle_quarantine_restore_purge` :117 | `2CPAD…` | `2cPAD…` |
| 2 | 35  | `collect_lifecycle_quarantine_restore_purge` :117 | `2VuTE…` | `2vuTE…` |
| 3 | 142 | `collect_reverification_skips_rereferenced_candidate` :205 | `2RS54…` | `2rS54…` |

Logs + loop script: `flaky-admin-collect-lifecycle-investigation/`.

This kills the "timing/state sensitivity under parallel load" hypothesis in
the strand description: the loop ran the tests **in isolation** (3 tests per
iteration, no workspace load) and still failed at the background rate. The
failure is a function of the random doc-id draw, not of load. It *looks*
load-correlated only because a full-workspace run is one more draw from a ~2-3%
Bernoulli, and full runs are when people watch.

Failure #3 is at `admin_collect_lifecycle.rs:205` — the same line as the
2026-08-09 GitHub Actions macOS flake recorded on bd-eb2wnxkp (c-tjflqt1c).
That CI occurrence showed two *entirely different* ids rather than a case
flip; that variant is also explained by the same root cause: that test asserts
`manifest.candidates[0].doc_id == orphan_id` **without first asserting
`candidates.len() == 1`**. When the *live* doc (rather than the orphan) is the
one whose path-derived id is mis-cased, it fails the live-set membership check
and wrongly joins the candidate list; `candidates[0]` is then the mis-cased
live id — a completely different string from `orphan_id`. One mechanism, two
victims, two signatures.

Note also: all five observed collisions across both strands have a leading
character in `{2,3}` — the bs58check leading character of a fixed-width
payload is heavily skewed, which is why the collision rate (~2-3% per store)
is far above the naive uniform-alphabet estimate (~0.1%/pair). The rate is
real; the tests will keep flaking until the recovery lands.

## Evaluation of the proposed workaround (this session's question)

> Could we search for the prefix (and files) case-insensitively — try `AbC`,
> then `abc`, `aBc`, … — so existing deployments keep their filesystem layout?

**Yes — and this is already the chosen design in bd-eb2wnxkp's plan (Part A,
`recover_doc_id`), with one crucial refinement: the case-variant search must be
validated by the bs58check checksum, not by mere case-insensitive matching.**
Details and corrections:

1. **The splay prefix is 2 characters, not 3** (samod `key_to_path`:
   `component.chars().take(2)`), so there are **at most 4** case variants
   (fewer when a prefix char is a digit, which has no case). Cheap.

2. **Direction matters.** *Lookup* by a known id (`load_range([id])`,
   `doc_dir(id)`) needs no search on macOS/Windows — the case-insensitive
   filesystem resolves any casing to the folded directory automatically. The
   broken direction is **enumeration**: `list_doc_ids_filesystem` reads
   *directory names* and reconstructs ids, and a folded level-1 dir yields the
   first-creator's casing for every id under it. So the case-variant search is
   applied at reconstruction time: generate the ≤4 case variants of the 2-char
   prefix, and let `DocumentId::from_str` (base58**check**, 4-byte checksum)
   pick the true one — exactly one variant parses, at ~2⁻³² false-accept odds.

3. **Why the checksum is essential:** two genuinely distinct valid ids can
   differ only in case. Naive case-insensitive matching would conflate them —
   trading the false-orphan bug for a wrong-document bug. bd-eb2wnxkp's plan
   already rejected unvalidated case-insensitive comparison for this reason;
   checksum validation is what makes the search sound.

4. **The user's core motivation is fully preserved:** zero on-disk changes, no
   migration for existing deployments (which the plan also demands — layout is
   samod's, shared with the JS implementation).

Corner cases inventoried (most already in the eb2wnxkp plan; the starred ones
are new from this session and should be folded in):

- **Level-2 dirs keep true casing** — created fresh under the (possibly
  folded) level-1 dir; APFS/NTFS are case-preserving. Only the 2-char prefix
  needs variants. A level-2 fold requires two docs whose 26-char rests also
  case-fold (~negligible for random ids); the `Ambiguous`/`Unidentifiable`
  hard-fail arms catch it if it ever happens.
- **Variants outside the base58 alphabet** (`0 O I l` excluded): fail to
  parse; natural rejection, no special case.
- **Digit-only prefixes:** no case variants; the as-read id must parse or is
  `Unidentifiable`.
- ★ **The `st/orage-adapter-id` entry:** every store has an `st/` level-1 dir
  (adapter identity, a *file* at level 2, skipped via `is_dir`). A doc id
  starting `St`/`ST`/`sT` folds **into** that pre-existing `st/` dir on
  macOS/Windows, so its id reads back as `st…`. `recover_doc_id` handles it,
  but this deserves an explicit test — it is the one collision every store is
  guaranteed to be primed for.
- ★ **Store copied case-insensitive → case-sensitive FS** (dev macOS →
  Linux prod restore): a folded dir survives the copy, and on Linux the
  exact-case `load_range`/`doc_dir` lookup **misses** it — the doc loads
  empty and becomes invisible (skipped, neither live nor candidate). This is
  the one place the *lookup*-direction case-variant search would matter.
  Recommend: out of scope for the fix (the mandatory recovery `warn!` already
  tells the operator the store has folded dirs); note it in the plan's
  out-of-scope list.
- **Unicode/NFD normalization:** N/A — base58 is ASCII.
- **Case-sensitive platforms run the same code** — the as-read id parses as
  its own single variant; no platform gating, deterministic tests everywhere.

## Issue context

Filed 2026-08-09 (discovered during bd-nv4p0eb1 verification), p3 bug.
Two comments record recurrences on 2026-08-10 (a different test in the same
family, and the original test again), both fail-fast, no captured output.
Description's hypothesis: "likely timing/state sensitivity … under parallel
load" — now disproven (see evidence).

## Dependency graph

- **discovered-from:** bd-nv4p0eb1 (closed — the add-file-with-id lint audit).
  Purely incidental: the flake surfaced during that strand's full-workspace
  verification; no semantic connection.
- **No other edges** — but a strong latent edge exists: bd-eb2wnxkp
  (in_progress, p1) describes the same mechanism, measured the same rate, and
  its comment c-tjflqt1c already logged one of this family's CI failures.
  **Action item: add `caused-by` bd-eb2wnxkp.**

## What the code looks like today

Unchanged from bd-eb2wnxkp's plan: `list_doc_ids_filesystem`
(`crates/quarto-hub/src/admin/scan.rs:80`) still does
`format!("{prefix}{rest}")` over raw dir names; `normalize_id`
(`classify.rs:162`) is still a string strip; ids flow as `String` through
`LoadedDoc`/`live_doc_ids`/`collect`. Reproduced at HEAD (`59bbccb9`) at
~2%/iteration. Pre-flight `cargo xtask verify --skip-hub-build` green.

## Proposed phases (draft)

- [x] Phase 0 — Link `caused-by` bd-eb2wnxkp; correct the strand description's
  load hypothesis (comment with captured evidence).
- [x] Phase 1 — **Part A implemented** (user-scoped: Part A only, on `main`):
  - `recover_doc_id(prefix, rest)` in `crates/quarto-hub/src/admin/scan.rs`:
    tests the ≤4 case variants of the 2-char splay prefix against
    `DocumentId::from_str` (bs58check), dedups by parsed id preferring the
    as-read casing (legacy-UUID hex parses case-insensitively — several
    casings, one id — and must not read as Ambiguous; returning the as-read
    *string* also keeps hypothetical UUID-form stores byte-compatible, leaving
    instance B untangled for Part B). Zero matches → `Unidentifiable`,
    multiple distinct ids → `Ambiguous`; both hard-fail.
  - `list_doc_ids_filesystem` is now fallible; `hub admin scan` (main.rs) and
    `collect()` abort on error ("refusing to collect: …"); recovery that
    changes casing emits `tracing::warn!`.
  - Over-broad module-doc safety claim narrowed (re-verification covers
    staleness, not systematic mis-identification).
  - Tests (TDD, red confirmed before implementation): 5 unit tests with
    checksum-verified fixtures (incl. the ★ `st/` collision id
    `StntkRJtG7hVPkKPY4Qkeu6f5bZ` and digit-prefix `26tzBsg…`); 2
    deterministic hand-built-splay integration tests (platform-independent);
    1 end-to-end collect test (live doc with case-renamed splay dir is not
    quarantined; runtime case-insensitivity probe, skips honestly on
    case-sensitive filesystems). New file
    `tests/integration/admin_doc_id_recovery.rs`.
  - Test hygiene: `collect_reverification_skips_rereferenced_candidate` now
    asserts `candidates.len()` before indexing.
  - E2E through the real binary (per end-to-end verification policy):
    `cargo run --bin hub -- admin scan --data-dir <hand-built store> --json`
    on a store with a folded `2C/` dir emitted
    `WARN … on_disk=2C/PADPZ85aBLaaLaLrS2BNcVza1n
    recovered=2cPADPZ85aBLaaLaLrS2BNcVza1n` and the manifest carried the true
    id; with a `zz/not-a-doc-id` pair added, the binary refused
    ("does not correspond to any valid document id … refusing to guess") and
    exited 1. Output inspected in both runs.
- [x] Phase 1b — **Belt-and-braces collect guard** (user answered eb2wnxkp
  open question 4 "yes, defense in depth" on 2026-08-11):
  `locate_all_doc_dirs` + `locate_verified` in
  `crates/quarto-hub/src/admin/collect.rs` map every recoverable id to the
  on-disk director(ies) whose *actual names* round-trip to it via
  `recover_doc_id`. A candidate that does not round-trip to exactly one real
  directory is skipped (with reason) regardless of its liveness verdict, and
  the quarantine rename source comes from this map — never from `doc_dir`
  construction + filesystem case-folding. `doc_dir` remains only for
  constructing restore destinations, documented as such. Tests: 3 unit tests
  (folded dir located by actual name; absent id refused; duplicate-dir
  ambiguity refused on case-sensitive filesystems, resolved where the
  filesystem folds) + 1 integration test (folded orphan still correctly
  quarantined under its true id). Note: the guard's refusal path is
  unreachable through the CLI on a healthy store *by design* (every other
  layer already defends); it is exercised at unit level and through
  `collect()` itself, the same function the CLI dispatches to.
- [x] Phase 1c — User answered eb2wnxkp open question 2 (2026-08-11): fix
  stays in quarto-hub (done); upstream proposal filed as **bd-3uw7uufa**
  (related: bd-eb2wnxkp).
- [ ] Phase 2 — Future dedicated verification (bd-u0tldu4z stays open for
  this, per user): 150+-iteration stress loop over the
  `admin_collect_lifecycle` + `admin_scan_real_store` families
  (`stress.sh` in the investigation dir); close bd-u0tldu4z on a clean loop,
  citing the count. (A first post-fix loop was run this session — see strand
  comment — but the dedicated verification pass remains open.)

## Open design questions for the user

1. **Strand disposition.** Close bd-u0tldu4z now as duplicate-in-effect of
   bd-eb2wnxkp (its fix covers this), or keep it open as the post-fix
   stress-verification tracking item (my lean — it is the only artifact that
   remembers the *test-side* verification obligation)?
2. **Go-ahead for bd-eb2wnxkp's plan?** It has been awaiting review since
   2026-07-28 with 5 open questions of its own; the flake family has now been
   sighted 5 times. This session's finding is that its Part A is exactly the
   migration-free workaround you proposed. Do you want to answer its open
   questions and start implementation (in its existing branch/worktree)?
3. **Moved-store scenario.** Confirm the ★ case (folded store copied to a
   case-sensitive filesystem) stays out of scope — the alternative is adding
   case-variant search to the lookup direction too, which widens the change
   for a scenario we have never observed.

## Risks / tradeoffs (draft)

- Closing this strand without the stress-loop phase would repeat the original
  trap: at a 2-3% rate, a single green run means nothing.
- The eb2wnxkp branch is 13 days old (based on `main` @ `270d58b5`); expect a
  rebase before implementation.
