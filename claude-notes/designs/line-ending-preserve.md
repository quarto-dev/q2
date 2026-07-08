# Line-ending preserve — design spec

**Status:** Design (2026-06-26). Implementation tracked by epic
`bd-eehxwr29` and its children.
**Foundational contract:** [byte-offset-invariant.md](byte-offset-invariant.md)
— line-ending preservation is one consumer of that invariant. Read it
first; this spec does not restate the rationale.
**Background research:**
[`2026-06-25-line-ending-handling-analysis.md`](../research/2026-06-25-line-ending-handling-analysis.md)
(current state),
[`2026-06-25-line-ending-prior-art.md`](../research/2026-06-25-line-ending-prior-art.md)
(strategies, Pandoc/rustc/comrak, hub findings).

## Goal

Make q2 honour the **preserve** policy for line endings end to end: CRLF
input renders CRLF output, LF renders LF, and nothing in between silently
rewrites the byte stream. PR #329 implemented this for doctemplate only;
the rest of q2 is accumulated accident (some paths preserve, some silently
normalize, one half-strips, the writers preserve nothing). This spec
extends #329 to all of q2.

The policy is not a goal in itself — it is the cheapest way to satisfy the
byte-offset invariant (strategy 1: untouched bytes ⇒ trivially valid
offsets). Where preserve is impossible, the invariant doc's strategy-3
escape hatch applies (comrak only, today).

## Approach: characterize, then fix, then design — gated by a harness

The work splits into three layers, enforced by the dependency graph so
`braid ready` reveals them in order. The ordering principle: **build the
oracle first, fix the mechanical bugs against it, and let the oracle's
observations feed the genuine design decisions.**

A distinction that dissolves the "tests-first vs design-first" tension:

- **Characterization tests** record what the code *does now* (feed CRLF,
  read the offset comrak emits, observe whether a strip leaves a `\r`).
  Uncontested — they assert reality.
- **Assertion tests** lock in what the code *should* do. They require a
  ratified contract.

The harness produces characterization data cheaply; the mechanical fixes
turn characterization tests into assertion tests; the design decisions are
made *from* the observed bytes, not a whiteboard.

## Layer 0 — determinism + the oracle

The common prerequisite for everything else.

### `bd-mv2ggmr5` — pin pampa fixtures + snapshots to LF (`.gitattributes`)

There is no `.gitattributes` under `crates/pampa`, so ~704 tracked files
flip to CRLF on a Windows checkout with `core.autocrlf=true`, producing the
7 failing snapshot/roundtrip/corpus tests. Add `text eol=lf` for the
fixture and snapshot globs (mirroring
`crates/quarto-doctemplate/.gitattributes`), then `git add --renormalize`
+ re-checkout (renormalize stages the index conversion but leaves the
working tree CRLF until re-checkout). **This is controlling on-disk bytes
for determinism — not in-code normalization.** Replaces `bd-238o`'s
rejected "normalize on test read" approach.

**Invariant:** pinning `.snap` files to LF is safe *only while no snapshot
ever captures CRLF-preserving output*. CRLF round-trip assertions live
in-source (inline expected strings), never in a `.snap`, or the pin
silently flattens them.

### `bd-tuf04qgu` — in-source CRLF test harness

A shared helper that constructs CRLF / LF / lone-CR / mixed inputs in code
and asserts (a) output convention matches input and (b) content bytes
between structural tokens survive. In-source so it runs on Linux CI, not
only on a Windows checkout (the #329 / #139 pattern). A Windows checkout
additionally verifies the real autocrlf path the in-source tests cannot
reach. Also stands up CRLF unit-test variants for the three offset
converters (below), since the provenance test surface is LF-only today
(zero CRLF offset assertions).

> The 2026-06-18 per-line-provenance plan that would have owned a
> `T-N*`/`T-E*` offset-regression seam is **shelved** (verified
> 2026-06-26): none of its symbols exist in `main` and
> `write_impl_tracked` (`qmd.rs:2769`) is still top-level-only. Build CRLF
> offset tests fresh against the *current* `write_with_source_info`; align
> with that plan's frozen seam design only if it is revived.

## Layer 1 — byte-faithfulness (mechanical TDD, blocked by the harness)

No design debate; "preserve" already decides each. The invariant: feed
CRLF, the byte survives the round-trip and the offset still indexes the
original bytes.

- **`bd-xyn4kk3k` — fenced code-fence half-strip.**
  `treesitter_utils/fenced_code_block.rs:70` pops only `\n`, leaving a lone
  trailing `\r` in `CodeBlock`/`RawBlock` text on CRLF. Direct cause of the
  stray `\r` in snapshot `native/007`. Consume the full CRLF line-ending
  unit; preserve internal CRLF content.
- **`bd-ske10iyd` — native writer `\r` escape.**
  `writers/native.rs` `write_safe_string` (lines 11–23) escapes `\\`, `"`,
  `\n` but not `\r`, so a raw CR leaks into serialized strings. Pandoc's
  Haskell `show` escapes `\r`. Add the arm.
- **`bd-qmtp61ms` — remove silent CRLF→LF rewrites.** Two tree-sitter-qmd
  post-extraction sites rejoin with a hardcoded `\n`: inline-math
  soft-break (`treesitter.rs:476`) and `strip_continuation_prefix`
  (`treesitter.rs:422,428`). Both: rejoin with the *original* convention.
  **Scope excludes comrak** — that boundary is strategy-3 (see Layer 2).
  (`treesitter.rs:1371` was checked and is byte-correct on CRLF; not in
  scope.)

## Layer 2 — design-gated (blocked by the harness)

The genuine design work. Each needs the harness's observations or a
ratified contract before assertions can be written.

### `bd-hn2ddyhf` — decide: column semantics under preserve

Under preserve, CRLF reaches the line/column layer, where three Rust
converters and one TS converter count `\r` as an ordinary column char, so
line/column drift one per CRLF line (byte offsets stay correct). The
decision: **is a line ending (CRLF/CR/LF) one logical column unit that
does not advance the column?** Forcing input: Monaco excludes the line
ending from its line/column model, so if the hub must match Monaco the
answer is forced. This strand resolves it **once**; two implementations
consume the answer:

- **`bd-hebi97on`** (Rust) — `quarto-source-map/src/utils.rs:8`,
  `file_info.rs:85`, `comrak-to-pandoc/src/source_location.rs:28`. The two
  `quarto-source-map` converters are redundant; **consolidate to one
  CRLF-aware converter** rather than fixing both.
- **`bd-b1291v6g`** (TS) — `hub-client/src/utils/diffToMonacoEdits.ts:32`;
  reconcile with Monaco position semantics. Windows-testable end to end
  against a running hub with a CRLF document.

### `bd-3ecwq37k` — writers emit the input convention

The largest gap: no writer chooses output EOL from the input convention;
every structural newline is a hardcoded `\n` (`writeln!`). Design questions
to settle here: per-document detection of the dominant ending; mixed
endings in one document; a document with no trailing newline; an explicit
override akin to Pandoc's `--eol crlf|lf|native` applied at the output
boundary (so writer code stays convention-agnostic instead of threading
EOL through every `writeln!`). doctemplate (#329) is the in-tree precedent.

**Byte-exactness precondition (current bug, verified 2026-06-26).**
`write_with_source_info` → `map_offset` (`qmd.rs:2742`/`2769`) tiles one
linear piece per top-level block; `map_offset` into a code body is
byte-exact only when output bytes equal source bytes. If the writer emits
LF on CRLF input, every code-body newline shrinks one byte in output and
`map_offset` drifts one byte per line — silently. So this strand is a
precondition for engine offset-exactness on CRLF (line *count* is
unaffected; only *byte* exactness). See byte-offset-invariant.md
§"Byte-exactness precondition".

### `bd-1ll7kj9e` — decide: comrak boundary (strategy 3?)

`readers/commonmark.rs` delegates to comrak, which normalizes CRLF→LF per
CommonMark spec §2.1 with **no opt-out** (source-confirmed
`comrak-0.52.0/src/parser/mod.rs:205-211`). The one boundary where
strategy 1 is impossible without forking. Decision: accept strategy 3
(normalize + correction map) — carry comrak's original-relative
`Sourcepos` (it tracks a `column_offset`) through the wrapper, rustc's
`normalized_pos` pattern (write its known bug #149568 as a test up front) —
or fork. **P2 and related to `k-n74s`** ("Add CommonMark reader to pampa
using comrak-to-pandoc"): the reader is being added now, so decide
alongside it.

## Sequencing (dependency graph)

```
bd-mv2ggmr5 (pin, ready now)
   └─blocks─> bd-tuf04qgu (harness)
                 └─blocks─> bd-xyn4kk3k, bd-ske10iyd, bd-qmtp61ms   (Layer 1)
                 └─blocks─> bd-hn2ddyhf ─blocks─> bd-hebi97on, bd-b1291v6g
                 └─blocks─> bd-3ecwq37k
                 └─blocks─> bd-1ll7kj9e (related: k-n74s)
```

`braid ready` surfaces only the pin at the start; the rest unlocks as the
spine lands. The serial gate is intentional — without the oracle the rest
cannot be done responsibly.

## Out of scope

- **Path separators** (`bd-dff27o04`) — the JSON-writer backslash issue is
  a Windows/Linux path-metadata difference, *not* a line-ending issue
  (normalizing separators shifts no offset). Detached from this epic.
- **Error-corpus no-op tests** (`bd-5wy2i4mc`) — stale-glob cleanup found
  during the investigation; unrelated; standalone.
- **Per-line-provenance reimplementation** — shelved (see Layer 0 note);
  this spec only requires writer-EOL to keep the *current* engine path
  CRLF-exact.

## Non-source assets — outside the preserve policy

The preserve policy governs **document source** (see byte-offset-invariant.md
§Scope). Two asset classes fall outside it and must not be conflated with the
strands above:

- **SCSS → CSS (parsed).** `quarto-sass` normalizes CRLF→LF once at
  `parse_layer()` (`layer.rs:112`); grass then re-emits LF regardless of input.
  User-CRLF theme / `_brand.yml` SCSS is safe — it cannot leak CRLF into the
  compiled CSS. This is a sanctioned normalize-and-forget (no offset map
  exists), not a policy violation. Root cause + fix: `bd-3fgnmlco` (closed).
- **Vendored served assets (`reveal.css`, `reveal.js`, embedded JS/CSS).**
  Embedded via `include_str!` / `include_bytes!` and served byte-for-byte;
  they carry no offsets and CRLF vs LF is browser-identical. On a Windows
  checkout `core.autocrlf` rewrites the committed-LF copies to CRLF, so a
  byte-exact test against an always-LF reference (e.g. the npm dist)
  false-positives on EOL alone.

  **Sanctioned pattern: normalize EOL inside the test comparison**, not a
  `.gitattributes` LF pin. `vendored_reveal_assets_match_npm_package`
  (`crates/quarto-core/src/revealjs/assemble.rs`) does exactly this — it is a
  *content-drift* check (catch a reveal.js version bump); line endings are
  noise in it (`bd-ol45i8oe`). A `.gitattributes` pin is checkout-dependent (an
  existing CRLF working copy stays stale until `git add --renormalize` +
  re-checkout) and needs per-dir upkeep. Note this is a **different** case from
  the fixture/snapshot pins (`bd-mv2ggmr5`, doctemplate, citeproc): those pin
  *inputs to a determinism test*, where LF-on-disk is the thing under test —
  not served product bytes.

## Open items

- **Vendored-asset EOL coverage** — a 2026-07-08 audit found ~9 of ~10
  embedded vendored resource dirs unpinned and untested for EOL; only the
  reveal drift test exercises the class, and no `cargo xtask lint` enforces
  EOL coverage on `include_str!` / `include_bytes!` targets under `resources/`.
  This is cosmetic on the current architecture (served bytes, no offsets;
  shipped WASM + native binaries are CI-built on Linux → LF), so it is tracked
  as a decide-if-worth-it item on `bd-aamrec3j`, not a correctness gap. Pursue
  a code fix (normalize-on-embed, checkout-independent) or a coverage lint only
  if cross-build-host byte determinism becomes a stated goal.
- **Column semantics** (`bd-hn2ddyhf`) is unresolved — it gates the
  line/col implementations and decides what their tests assert. The
  2026-06-26 probes sharpen it: `astContext.line_breaks` omits bare CRs
  entirely (it is LF-only), so the failure is a missing line break, not a
  miscounted column. See
  [`../research/2026-06-26-line-ending-gap-characterization.md`](../research/2026-06-26-line-ending-gap-characterization.md).
- **BOM** — resolved as preserve-correct at ingress (offsets count it, no
  leak); not a separate strand. Writer re-emit folds into `bd-3ecwq37k`.
- **YAML / XML readers** — newly characterized boundaries (YAML block
  scalars consume CRLF → `SoftBreak`; XML unprobed). Catalog rows added in
  byte-offset-invariant.md; XML needs an in-crate probe strand, YAML needs
  offset-validity confirmation before classification.
- **bare-CR-at-EOF injection** — `ends_with('\n')`-only check appends `\n`
  after a trailing bare `\r`. Low priority; candidate to fold into
  `bd-qmtp61ms`.

## Dev workflow notes

- Strands live in the braid skein (epic `bd-eehxwr29`); nothing to commit
  for issue work.
- Layer 0 (`bd-mv2ggmr5`) is the only ready strand; start there. The
  `.gitattributes` change needs `git add --renormalize` + re-checkout on a
  Windows tree.
- Verify line-ending fixes per `CLAUDE.md` end-to-end: drive the real
  binary on a CRLF fixture and inspect bytes (`xxd`), not just unit tests.
