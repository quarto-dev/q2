# Heading id written as {id="..."} emits two id attributes (bd-heading-id-attr-duplicated-xbpcmejr)

**Date:** 2026-08-18
**Braid:** bd-heading-id-attr-duplicated-xbpcmejr (p2, bug, label `markdown`)
**Checkout:** main checkout, branch `main` @ `0c3542d0`
**Status:** Approved 2026-08-19 — design questions answered by the user; implementation in progress.

## Design decisions (user-aligned, 2026-08-19)

1. **Last-id-wins, matching pandoc.** Promotion happens positionally in the
   child loop of `process_commonmark_attribute`. Note: q2's grammar enforces
   component order (id, then classes, then kv) — `{id="kv" #short}` is a
   parse error at HEAD (probed empirically), so the only reachable mixed case
   is `{#short id="kv"}` (kv wins) plus duplicate kv keys (last wins). Both
   fall out of in-loop processing.
2. **Pure bug fix** — no warning, no escape hatch for documents relying on the
   old passthrough behavior.
3. **Writer fallback is id-only** here. The analogous class-charset roundtrip
   hazard is deferred to **bd-fffjzi5s**.
4. **Split-class source spans point at the whole quoted value.** Per-word
   sub-spans deferred to **bd-0vfgz2cl** (p4, label `quarto-source-map`).

## Triage verdict

**Ready to design.** The strand's root-cause analysis is accurate, the single
choke point it hoped for exists (`process_commonmark_attribute`), pandoc's
authoritative semantics are confirmed empirically, and the one complication the
strand did not mention — the qmd writer cannot round-trip a slashy identifier
through the `{#...}` shorthand — has a clear fix in `write_attr`.

## Issue context

Filed 2026-08-18 by claude, out of the Posit Connect docs port (origin strand
`br-heading-id-attr-duplicated-fl8f35pp` in the Connect-docs-port skein — a
*different* skein, so no edge exists in this one). Pandoc accepts the
identifier both as `{#shorthand}` and as key-value `{id="..."}` and treats both
as THE identifier. q2 handles only the shorthand; the kv form stays in the
attribute map, the identifier slot stays empty, an auto slug is generated, and
the HTML writer emits **two `id=` attributes** on the sectionized `<section>` —
an HTML parse error. Real-world impact: 1,364 duplicate-id elements and all 159
remaining broken fragments in the Connect docs port at 0.23.0. The kv form is
the *only* way to express the site's slash-containing anchors
(`get-/v1/users/-guid-/keys`), which the shorthand grammar rejects.

## Dependency graph

**Empty** — `braid dep tree` / `dep list` show no edges in this skein. The
origin context lives in the Connect-docs-port skein (br-heading-id-attr-duplicated-fl8f35pp).
No incoming pressure from other strands; urgency comes from the Connect docs
port fragment-parity goal described in the strand body.

## What the code looks like today

All paths verified at `main` @ `0c3542d0`; symptom reproduced (see
`heading-id-kv-attribute-investigation/observed-2026-08-18.md`).

- **Single reader-side choke point exists.**
  `process_commonmark_attribute` in
  `crates/pampa/src/pandoc/treesitter_utils/commonmark_attribute.rs:14` is the
  only place a `commonmark_specifier` becomes an `Attr` (sole caller:
  `treesitter.rs:1250`). Every attribute consumer — headings
  (`postprocess.rs:967`), spans/links (`span_link_helpers.rs`), divs
  (`fenced_div_block.rs`), code spans (`code_span_helpers.rs`), fenced code
  blocks incl. `{python ...}` (`language_specifier.rs:47` consumes the
  commonmark attr downstream), captions (`caption.rs`), tables/sections —
  receives the tuple this function builds. Promoting reserved keys here fixes
  every element type at once. The merge sites the strand lists
  (`postprocess.rs:970`, `:1390`, `:1618`) then need **no changes**: they copy
  `inline_attr.attr` verbatim, which becomes correct input.
- **The bug is confirmed for heading, span, div, and code** (not just
  headings): all four leave `id` in attr.2 with attr.0 empty; pandoc promotes
  in all four.
- **Pandoc semantics (probed empirically, pandoc 3.x):**
  - kv `id` fills the identifier slot; **last id wins** across both forms and
    duplicates (`{#a id="b"}` → `b`; `{id="a" #b}` → `b`; `{id="a" id="b"}` → `b`).
  - kv `class` values are **split on whitespace** and appended to classes in
    source order (`{class="x" .y class="z"}` → `["x","y","z"]`).
  - Only `id` and `class` are reserved; everything else stays in the map.
  - Auto-id generation still applies when no id of either form is present.
- **Writer-side complication (not in the strand).** `write_attr`
  (`crates/pampa/src/writers/qmd.rs:415`) and `write_code_attr` (`:457`)
  always emit the identifier as `#id`. The shorthand token charset is
  `[#][._A-Za-z0-9-]+` (`grammar.js:572`); a promoted slashy id written as
  `{#get-/v1/x}` would **hard-error on re-parse**, breaking qmd round-tripping
  for exactly the motivating real-world input. The writer must emit
  `id="..."` kv form when the identifier is not shorthand-expressible.
  (Classes are safe: promoted class values are whitespace-split words, and any
  value outside the `.class` charset (`grammar.js:582`) could get the same
  `class="..."` fallback for symmetry — see design question 3.)
- **Auto-id/dedup path needs no change**: `with_header`
  (`postprocess.rs:931-974`) already keys on `attr.0` being empty; a promoted
  id flows through the explicit-id branch exactly like a shorthand id.

## Work items

### Phase 0 — Failing tests (TDD)

- [x] New `crates/pampa/tests/integration/test_kv_attr_promotion.rs`
      (registered alphabetized in `main.rs`), covering:
      heading kv id (slashy + plain) fills attr.0, no `id` left in attr.2,
      no auto slug; `{#short id="kv"}` → `kv`; `{id="one" id="two"}` → `two`;
      `{.y class="x z"}` → classes `["y","x","z"]` (source order,
      whitespace-split); span/div/code-span/fenced-code (`{python id="x"}`)
      promotion; auto id still generated when only kv `class` present;
      `attr_source.id` is `Some` for promoted ids.
- [x] Writer tests: slashy id round-trips as `{id="..."}` (not `{#...}`);
      plain promoted id normalizes to `{#plain}` and re-parses to the same
      attr.
- [x] Run the new tests; verify they fail as expected.

### Phase 1 — Reader promotion

- [x] `process_commonmark_attribute`: promote kv `id` → attr.0 (in-loop,
      last-wins; `attr_source.id` = value span) and kv `class` →
      whitespace-split words appended to attr.1 (one class-source entry per
      word, whole value span); neither key enters attr.2.

### Phase 2 — Writer fallback (id-only)

- [x] `write_attr` + `write_code_attr` in `writers/qmd.rs`: emit `id="..."`
      kv form when the identifier has chars outside `[._A-Za-z0-9-]`
      (else keep `#id` shorthand).

### Phase 3 — Sweep + verify

- [x] Grep for consumers reading `attr.2` key `"id"` that would now see it
      moved (expect none).
- [x] `cargo nextest run --workspace`; review and document any `.snap` churn.
- [x] End-to-end: `cargo run --bin q2 -- render` a fixture with
      `## H {id="get-/v1/x"}`, inspect the HTML for exactly one `id=` on the
      section, record invocation + snippet here.
- [ ] `cargo xtask verify` (full, WASM leg included — pampa changes flow into
      the hub client).

### Phase 4 — Docs

- [x] Check `docs/` qmd-syntax pages for attribute documentation; document the
      kv `id`/`class` forms if attributes are covered there. **Outcome: no-op** —
      neither `docs/guides/authoring/markdown-basics.qmd` (a stub: headings
      section lists only levels, `Lists`/`Syntax Characters` are empty FIXME
      headings) nor `dev-docs/syntax-notes.md` documents attribute syntax at
      all, so there is no existing coverage to extend. When the markdown-basics
      page grows an attributes section, it should mention both the `{#id}`
      shorthand and the `{id="..."}` kv form.

## End-to-end verification record (2026-08-19)

Invocation (real binary, not a library call):

```
cargo run -q --bin q2 -- render <scratch>/e2e/index.qmd --to html
```

Fixture headings: `## List API keys {id="get-/v1/users/-guid-/keys"}`,
`## Plain heading {id="plain-explicit"}`, `## Control {#shorthand-id}`.
Observed output (inspected in the generated `index.html`):

```html
<section id="get-/v1/users/-guid-/keys" class="section level2">
<section id="plain-explicit" class="section level2">
<section id="shorthand-id" class="section level2">
```

This matches the strand's pandoc/Q1 reference line for line; a grep for
elements carrying two `id=` attributes found zero. The full workspace run
was 12874/12874 with **no `.snap` changes** — no existing fixture used the
kv form.

## Open design questions for the user (answered 2026-08-19 — see Design decisions)

1. **Last-id-wins fidelity.** Pandoc resolves multiple ids (any mix of `#x` and
   `id="y"`) as last-one-wins. Matching that exactly means promotion must
   happen *in the child loop* (positional), not as a post-pass. Adopt pandoc's
   last-wins, or prefer first-wins/warn on conflict? (Recommendation:
   match pandoc; conflicts are rare and parity is the goal.)
2. **Silent behavior change for existing kv users.** Any document currently
   using `{id="x"}`/`class="..."` *expecting* a passthrough data attribute
   changes meaning. This matches pandoc/Q1, so I'd treat it as a pure bug fix
   with no warning/escape hatch — agree?
3. **Writer fallback scope.** Minimum fix is id-only (`id="..."` when not
   shorthand-expressible). Should the same fallback also cover class values
   that don't fit the `.class` token charset (`[A-Za-z][A-Za-z0-9_-]*`), which
   is a latent roundtrip hazard independent of this bug — here, or as a
   separate follow-up strand?
4. **AttrSourceInfo granularity for split classes.** `class="c1 c2"` yields two
   classes from one source span. Is pointing both class-source entries at the
   whole value span acceptable (simple), or do you want per-word sub-spans
   (fiddly, needs offset math inside the quoted value)?

## Risks / tradeoffs (draft)

- **Snapshot churn**: any existing fixture using kv id/class will shift
  (identifier slot populated, auto slug gone). Expect `.snap` diffs across
  pampa/quarto-core; each needs eyeballing per the snapshot policy.
- **Downstream consumers reading `attr.2["id"]`**: a grep for readers of the
  kv `id` key should be part of Phase 3 (none expected, but cheap to check).
- **Pre-flight verify** took three environment fixes, all unrelated to this
  bug; the Rust legs (12837 tests) were green throughout:
  1. ts-packages leg: `@quarto/engine-host-deno` could not resolve
     `@quarto/api/*` / `@quarto/types` — this checkout's root `npm install`
     predated those packages. Fixed with `npm install` from the repo root.
  2. hub-client `test:wasm` smoke-all: 4 highlighting-fixture failures from a
     WASM artifact predating the PR #547 highlight-theme-translator merge.
     Fixed with `cd hub-client && npm run build:wasm`.
  3. quarto-hub-mcp bundle test: `@esbuild/darwin-arm64` missing because the
     committed root `package-lock.json` has no `@esbuild/*` platform entries
     (npm optional-deps lockfile bug) — any clean install reproduces this.
     Machine-local workaround `npm install --no-save @esbuild/darwin-arm64@0.28.0`;
     repo fix filed as **bd-9itqqqe6** (discovered-from this strand).
