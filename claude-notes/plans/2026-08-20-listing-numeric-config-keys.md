# Listing numeric config keys silently ignore unquoted YAML integers (bd-yjsz6hdu)

**Date:** 2026-08-20
**Braid:** bd-yjsz6hdu (bug, p2, label `listings`)
**Checkout:** main checkout, branch `main` @ `a72a2bb26` (no worktree — investigation only)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The class is fully enumerated (bigger than the strand's
three named keys — see inventory), the fix shapes are clear, and the main open
question is *altitude*: patch the listing sites onto the existing local helper,
or promote proper numeric accessors to `ConfigValue` and retire the three
independent hand-rolled variants that have now accumulated.

## Issue context

Filed 2026-08-20 by me, discovered during
bd-listing-ellipsis-no-matching-l963osy1 (PR #570, merged): numeric listing
options were read with `entry.value.as_plain_text().and_then(|s| s.parse().ok())`,
but an unquoted YAML integer arrives as `ConfigValueKind::Scalar(Yaml::Integer)`,
for which `as_plain_text()` returns `None` — the option is silently dropped and
the default kept. PR #570 fixed `max-description-length` via a new local helper
`parse_u32_scalar` (`as_int()` first, plain-text parse fallback) and left the
siblings to this strand.

## Dependency graph

Single edge: `discovered-from` → bd-listing-ellipsis-no-matching-l963osy1
(**closed**, PR #570). No dependents pin urgency; the class is latent until a
user sets one of the affected keys unquoted — which is the *natural* way to
write them (`page-size: 10`).

## What the code looks like today (full inventory, 2026-08-20)

Swept the workspace for `as_plain_text()` + `parse()` chains and bare
`as_int()` reads.

**Still broken — unquoted integer silently dropped** (the strand's core):

| Site | Key | Note |
|---|---|---|
| `listing/config.rs:494` | `page-size` | currently inert downstream (no pagination emission, bd-nbv80e33) but parsed & bound |
| `listing/config.rs:499` | `max-items` | `Option<…>` — quietly stays `None` |
| `listing/config.rs:538` | `grid-columns` | grid class `quarto-listing-cols-N` wrong → falls to default 3 |
| `listing/config.rs:893` | `feed: items:` | inside `parse_feed` — the audit the strand asked for found exactly this one |

**Broken for unquoted *floats*** (`Yaml::Real` handled nowhere):

- `revealjs/assemble.rs:238` `float_opt` — handles `as_int()` + string, but
  `margin: 0.2` arrives as `Scalar(Yaml::Real("0.2"))` → both probes miss →
  silently defaults to 0.1. (`margin` is the only `float_opt` consumer.)
  Confirmed `Yaml::Real` is what the real front-matter path produces
  (`pampa/src/pandoc/meta.rs:450`); `as_plain_text()` matches only
  String/Path/Glob/Expr/PandocInlines.

**Mirror-image miss — bare `as_int()` with no string fallback** (breaks on
*quoted* numbers, `"3"`, which in front matter become `PandocInlines`):
`document_profile.rs:798,905`, `revealjs/transform.rs:62`,
`revealjs/assemble.rs:235` (`int_opt`), `extension/read.rs:566,584`,
`template.rs:1240`. Lower severity (users rarely quote numbers) but the same
class seen from the other side.

**Existing hand-rolled robust readers — now three independent copies:**

1. `pampa/src/toc.rs:240` — `level`: `as_int().or_else(plain-text-parse)`.
2. `revealjs/assemble.rs:235-243` — `int_opt`/`float_opt` (float variant
   incomplete, see above).
3. `listing/config.rs:709` — `parse_u32_scalar` (PR #570).

`ConfigValue` (in-tree `quarto-pandoc-types`) offers only strict `as_int()` /
`as_bool()`; there is no `as_f64` at all (the revealjs comment says so
explicitly) and nothing lenient.

No repro fixture needed — the class is already pinned by a regression test
from PR #570 (`max_description_length_accepts_unquoted_integer` travels the
real YAML path); new tests would follow the same pattern per key.

## Proposed phases (draft)

- **Phase 0 — Tests (TDD).** Per-key regression tests through
  `parse_from_yaml` (unquoted int for the four listing sites; unquoted float
  for revealjs `margin` if in scope; quoted-string forms as regression
  guards). Verify each fails first.
- **Phase 1 — The accessor** (shape depends on Q1): either migrate the four
  listing sites onto `parse_u32_scalar`, or add lenient numeric accessors to
  `ConfigValue` (`quarto-pandoc-types`) — e.g. `as_i64_lenient()` /
  `as_f64_lenient()` accepting Integer, Real (float case), and
  parseable String/PandocInlines — and migrate.
- **Phase 2 — Consumers.** Rewire the in-scope sites; retire the local
  helpers that the chosen altitude obsoletes.
- **Phase 3 — Verification.** Full workspace tests; `cargo xtask verify`
  (quarto-pandoc-types/quarto-core → WASM leg); e2e render probe for one
  listing key + `margin` if in scope.
- **Phase 4 — Lint (optional, Q4).** xtask lint rule banning new
  `as_plain_text().…parse()` chains outside the blessed accessor, in the
  spirit of `metadata-as-str`.

## Open design questions for the user

1. **Altitude.** (a) Minimal: four one-line changes onto the existing
   `parse_u32_scalar`, listing-only. (b) Foundational: add lenient numeric
   accessors to `ConfigValue` in `quarto-pandoc-types` and migrate all three
   hand-rolled helpers + the four broken sites onto them. (c) Radical: teach
   `as_plain_text()` itself to render Integer/Real scalars as text — fixes
   the class at the root but changes semantics for every existing caller
   (blast radius includes the `metadata-as-str` lint's assumptions); I'd
   recommend against (c). My read: **(b)** — a third hand-rolled copy just
   appeared, which is the classic signal the abstraction wants to move down.
2. **revealjs `margin:` float bug.** Include here (natural under altitude (b),
   where `as_f64_lenient` exists anyway), or file as its own strand? My read:
   include under (b); file separately if we pick (a).
3. **Bare `as_int()` sites** (quoted-number miss, 7 call sites): migrate in
   this strand, or file as a separate lower-priority strand? My read:
   separate strand — different symptom, different severity, and this strand
   stays reviewably small.
4. **Lint rule.** Worth adding the `as_plain_text().…parse()` lint in this
   strand (Phase 4), a follow-up strand, or not at all? My read: follow-up
   strand — the pattern is subtle enough that it *will* be reintroduced, but
   the lint design (which chains to flag, how to bless the accessor's own
   fallback) deserves its own small scope.

## Risks / tradeoffs (draft)

- Altitude (b) touches `quarto-pandoc-types`, which `wasm-quarto-hub-client`
  depends on → full `cargo xtask verify` mandatory before push (WASM leg).
- Lenient accessors must define what happens for `Yaml::Real` when an
  *integer* is requested (`page-size: 2.5`?) — recommend: integer accessor
  rejects non-integral Real (keep default + stay silent, matching current
  error posture; a diagnostic would be new behavior worth its own question).
- The quoted-form tests double as guards that migrating off
  `as_plain_text()`-first doesn't regress the currently-working string path.
