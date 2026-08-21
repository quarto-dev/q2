# Listing numeric config keys silently ignore unquoted YAML integers (bd-yjsz6hdu)

**Date:** 2026-08-20
**Braid:** bd-yjsz6hdu (bug, p2, label `listings`)
**Checkout:** main checkout, branch `braid/bd-yjsz6hdu` (off `main` @ `a72a2bb26`)
**Status:** Design aligned 2026-08-21; executing.

## Design decisions (2026-08-21, aligned with user)

1. **Altitude (b)**: lenient numeric accessors on `ConfigValue` in
   `quarto-pandoc-types` (`as_int_lenient` / `as_f64_lenient`); migrate the
   three hand-rolled readers + the four broken listing sites onto them.
   Option (c) (making `as_plain_text` render numbers) rejected.
2. **revealjs `margin:`** float bug included here.
3. **Bare `as_int()` sites**: an additional phase in this strand (not a
   separate strand), with **stacked, independently reviewable commits** —
   accessors, then the broken-site migrations, then the bare-`as_int` phase.
4. **Lint rule**: follow-up strand — filed as **bd-clqzz2rl**.

Accessor semantics (from the risk note, now decided): the integer accessor
accepts `Yaml::Integer` and parseable string/inlines forms and **rejects
`Yaml::Real`** (a `page-size: 2.5` keeps the default, silently — matching the
current error posture; diagnostics would be new behavior out of scope). The
float accessor accepts Integer, Real, and parseable string/inlines forms.

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

## Work items

Commit structure (stacked): **A** accessors, **B** broken-site fixes,
**C** bare-`as_int` migration. Each commit green on its own.

- **Phase 0 — Failing tests for the broken sites (before commit B's fixes).**
  - [ ] `parse_from_yaml` regression tests: unquoted `page-size`,
    `max-items`, `grid-columns`, `feed: items:`; quoted-string forms as
    guards for the currently-working path.
  - [ ] revealjs `margin:` as `Yaml::Real` (via `new_scalar`) →
    `v["margin"] == 0.2`; guard: int `margin` and default.
  - [ ] Verify each fails at HEAD.
- **Phase 1 — Accessors (commit A).**
  - [ ] `as_int_lenient() -> Option<i64>` and `as_f64_lenient() -> Option<f64>`
    on `ConfigValue` (`quarto-pandoc-types/src/config_value.rs`) with unit
    tests (Integer, Real, quoted string, PandocInlines, garbage, Real-to-int
    rejection, whitespace trim).
- **Phase 2 — Broken-site + hand-rolled-reader migration (commit B).**
  - [ ] listing `config.rs`: `page-size`, `max-items`, `grid-columns`,
    `feed.items` → via `parse_u32_scalar`, itself reimplemented over
    `as_int_lenient` (keeps the u32 clamp in one place).
  - [ ] revealjs `int_opt`/`float_opt` → thin wrappers over the accessors
    (or deleted if call sites read cleanly); fixes `margin:`.
  - [ ] `pampa/src/toc.rs:240` `level` → `as_int_lenient` (behavior-neutral
    consolidation; it was already robust).
- **Phase 3 — Bare-`as_int` sites, quoted-number miss (commit C).**
  - [ ] Failing tests first (quoted `"3"` / front-matter-inlines forms), then
    migrate: `document_profile.rs:798` (`order:`), `:905`
    (`extract_u32_field` — `reading-time-minutes`, `word-count`),
    `revealjs/transform.rs:62` (`slide-level:`), `extension/read.rs:566,584`
    (claim `priority`).
  - [ ] **Excluded with rationale:** `template.rs:1240` — that `as_int()` is
    a type-dispatch arm in `config_value_to_template_value` (bool → int →
    null → …), not an option read; string-to-int coercion there would change
    template semantics.
- **Phase 4 — Verification.**
  - [ ] Full workspace tests; full `cargo xtask verify`
    (quarto-pandoc-types → WASM leg); snapshot audit.
  - [ ] E2E probe through `cargo run --bin q2 -- render`: a listing with
    unquoted `grid-columns` + a revealjs doc with `margin: 0.2`; inspect
    output; record here.

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
