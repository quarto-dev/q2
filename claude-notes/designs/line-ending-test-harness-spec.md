# Line-ending test harness — completeness spec

**Status:** Spec (2026-06-26). Implements `bd-tuf04qgu` (harness) and is
the coverage contract for the line-ending preserve epic `bd-eehxwr29`.
**Reads first:**
[byte-offset-invariant.md](byte-offset-invariant.md) (the contract),
[line-ending-preserve.md](line-ending-preserve.md) (the work),
[`../research/2026-06-26-line-ending-gap-characterization.md`](../research/2026-06-26-line-ending-gap-characterization.md)
(the observed bytes the cases below are built from).

## Two buckets (the requirement)

- **Bucket A — current behavior, every identified I/O boundary + related.**
  Golden / characterization. **Green today, no code change.** For an
  already-correct boundary, A == the desired behavior and stays a
  permanent regression guard. For a buggy boundary, A pins the *current
  bug*; it intentionally **breaks when the fix lands**, at which point the
  paired B test goes green and the A characterization is deleted/flipped.
- **Bucket B — desired Windows CRLF behavior.** Assertion tests. **Red
  until support lands** (drives the fix). Where the target behavior is not
  yet decided (`bd-hn2ddyhf` column semantics, `bd-1ll7kj9e` comrak
  strategy), the B test is written **`#[ignore]`** with the blocking strand
  named, so the gap is visible in the suite and un-ignored on decision.

Every boundary gets **both** an A and a B (TS uses `it.skip` for the
ignore case). The four input variants per boundary: **LF, CRLF, lone-CR,
mixed**. The minimal B assertion at every boundary: *feed CRLF, every
reported offset still indexes the original bytes.*

## Coverage matrix

Legend — B-now: `pass` (already correct, A==B), `FAIL` (red until fix),
`ignore` (decision-gated), `pass?`/`(unprobed)` (owning strand has not yet
written or run the A test to confirm — tracked as an open verification
step, not a fourth bucket).

### Readers / ingress

| Boundary | file | A (pin current) | B (desired CRLF) | B-now | Owner | Home |
|---|---|---|---|---|---|---|
| tree-sitter parse | `scanner.c` | CRLF → byte-accurate `Point`/`Range` | offsets index original bytes | pass | (guard) `bd-tuf04qgu` | pampa |
| BOM ingress | `readers/qmd.rs` | BOM counted, not leaked | offsets valid under leading BOM | pass | (guard) `bd-tuf04qgu` | pampa |
| fenced code strip | `treesitter_utils/fenced_code_block.rs:70` | lone `\r` left in CodeBlock/RawBlock | full `\r\n` unit consumed, internal CRLF intact | FAIL | `bd-xyn4kk3k` | pampa |
| native string quoting (read-back via writer) | — | see native writer row | — | n/a (duplicate of native writer row) | `bd-ske10iyd` | pampa |
| inline-math soft-break | `treesitter.rs:476` | rejoined `\n` (normalized) | rejoined with original convention | FAIL | `bd-qmtp61ms` | pampa |
| `strip_continuation_prefix` | `treesitter.rs:422,428` | split/rejoin `\n` (normalized) | original convention preserved | FAIL | `bd-qmtp61ms` | pampa |
| bare-CR-at-EOF injection | `main.rs:217`, `qmd.rs:79` | `ends_with('\n')`-only → appends `\n` after lone CR | inject matching convention (or none) | FAIL | `bd-qmtp61ms` | pampa |
| grid-table offset math | `treesitter.rs:1371` | byte-correct on CRLF | offsets valid on CRLF | pass | (guard) `bd-tuf04qgu` | pampa |
| commonmark/comrak | `readers/commonmark.rs` | comrak normalizes CRLF→LF, original-relative Sourcepos | offsets resolve to original via carried Sourcepos (strategy 3) + rustc #149568 regression | ignore | `bd-1ll7kj9e` | pampa |
| YAML block scalar | quarto-yaml | CRLF → `SoftBreak`, `Str` clean | offsets through provenance pool index original | pass? (unverified) | `bd-aowdiufr` | quarto-yaml |
| XML text/attr | quarto-xml `parser.rs` | (unprobed) | offsets index original CRLF text + attr | pass? (unprobed) | `bd-w9imk3bi` | quarto-xml |

### Writers / egress

| Boundary | file | A (pin current) | B (desired CRLF) | B-now | Owner | Home |
|---|---|---|---|---|---|---|
| qmd writer | `writers/qmd.rs` | structural LF + content CRLF → **mixed** | round-trip input convention | FAIL | `bd-3ecwq37k` | pampa |
| html writer | `writers/html.rs` (52 `writeln!`) | structural LF, content `\r` passthrough | emit input convention | FAIL | `bd-3ecwq37k` | pampa |
| native writer | `writers/native.rs:11` | raw `\r` unescaped + lone trailing `\r` | `\r` escaped as `\r` (Pandoc `show`) | FAIL | `bd-ske10iyd` (escape), `bd-3ecwq37k` (structural EOL) | pampa |
| json writer | `writers/json.rs`, `json_stream.rs` | `\r` escaped correctly | `\r` escaped (regression) | pass | (guard) `bd-tuf04qgu` | pampa |

### Offset / position layer

| Boundary | file | A (pin current) | B (desired CRLF) | B-now | Owner | Home |
|---|---|---|---|---|---|---|
| `map_offset` byte-exactness | `writers/qmd.rs:2742,2769` | drifts 1 byte/line when writer emits LF on CRLF | byte-exact offsets into code bodies on CRLF | ignore (gated on `bd-3ecwq37k`) | `bd-3ecwq37k` | pampa |
| line↔col (Rust) | `quarto-source-map/utils.rs:8`, `file_info.rs:85`, `comrak-to-pandoc/source_location.rs:28` | `line_breaks` LF-only, omits bare CR | line ending is one unit; CRLF/lone-CR counted as a break | ignore (gated on `bd-hn2ddyhf`) | `bd-hebi97on` | quarto-source-map, comrak-to-pandoc |
| line↔col (TS) | `hub-client/src/utils/diffToMonacoEdits.ts:32` | LF-only, misplaces on CRLF | Monaco-correct positions on CRLF | ignore (gated on `bd-hn2ddyhf`) | `bd-b1291v6g` | hub-client (vitest) |

### Hub pipeline / Lua

| Boundary | file | A (pin current) | B (desired CRLF) | B-now | Owner | Home |
|---|---|---|---|---|---|---|
| disk → automerge → VFS → WASM | `quarto-hub/src/sync.rs:78,134`, `wasm-quarto-hub-client/src/lib.rs` | verbatim byte flow, no transform | CRLF reaches WASM parser byte-identical | pass | `bd-5im5ey38` (Rust + TS) | quarto-hub / wasm crate / sync-client (TS) |
| Lua `file:read("*l")` | `lua/io_wasm.rs:171` | strips trailing `\r` (intentional) | n/a — characterization only, document intentional | pass | `bd-tuf04qgu` (gap G3) | pampa |

## Gaps found by this review

- **G1 — shared input-builder home.** The harness needs a builder that
  turns one base string into LF / CRLF / lone-CR / mixed variants, usable
  from pampa, quarto-yaml, quarto-xml, quarto-source-map, comrak-to-pandoc.
  `quarto-test` is the wrong home (it runs QMD-embedded smoke assertions).
  Options: (a) a tiny dev-only `quarto-line-ending-testkit` crate as a
  `dev-dependency`; (b) inline a trivial builder per crate (matches
  doctemplate's `crlf_preservation`, which just writes `"\r\n"` literals —
  no helper). Recommend **(b)** for the leaf cases (trivial, no new crate)
  and a 3-line shared `fn variants(base: &str)` only if duplication bites.
  **Decision needed in `bd-tuf04qgu`.**
- **G2 — hub verbatim-flow** — **resolved:** owned by new strand
  `bd-5im5ey38` (Rust WASM-entry guard + `quarto-sync-client` vitest, both
  regression guards passing today). The TS line/col converter remains
  `bd-b1291v6g`; this strand is the verbatim byte flow only.
- **G3 — Lua `*l` strip is uncharacterized.** Intentional, local, not a
  source-map boundary — but should still have one A characterization test
  documenting it as intentional so a future reader doesn't "fix" it. Fold
  into `bd-tuf04qgu`.
- **G4 — already-correct boundaries need explicit guards.** tree-sitter,
  BOM, grid-table, and json `\r` escape are all correct today and have *no
  strand*. (Hub verbatim flow is the same kind of already-correct
  boundary, but is already covered — see G2 — by `bd-5im5ey38`.) The
  remaining four are pure regression guards and must be listed as A==B
  assertions owned by `bd-tuf04qgu`, or a silent regression could
  reintroduce a bug with nothing to catch it.

## Build order (once approved)

1. `bd-mv2ggmr5` pin (Layer 0 gate; in-source tests don't need it, but
   fixture-reading A tests do).
2. `bd-tuf04qgu`: shared builder decision (G1), then per-boundary A+B
   modules. All A green; B `FAIL`/`ignore` as the matrix says. This is the
   red baseline the Layer-1 fixes turn green.
3. Layer-1 fixes (`bd-xyn4kk3k`, `bd-ske10iyd`, `bd-qmtp61ms`) flip their B
   green and retire their A characterization.
4. Decisions (`bd-hn2ddyhf`, `bd-1ll7kj9e`) un-ignore their B tests; then
   `bd-hebi97on`/`bd-b1291v6g`/comrak implement against them.
5. `bd-3ecwq37k` writer-EOL flips the writer + `map_offset` B tests green.

## What "everything" means here

Every row above is either **paired** (has an A and a B) or a **named
exception**, and every row has a named owner (or a flagged gap). The only
B tests that cannot be authored as red-now are the three decision-gated
ones (column semantics ×2 implementations, comrak strategy) — and those
are present as `#[ignore]`/`it.skip` stubs so the suite still *names* the
missing coverage.

Three rows are named exceptions — two pending, one permanent. Coverage is
**not** complete until the two pending ones are resolved:

- YAML block scalar and XML text/attr are `pass?`/`(unprobed)` — their
  owning strands (`bd-aowdiufr`, `bd-w9imk3bi`) still owe the A test that
  confirms the row. Until that A test exists and passes, these rows are
  outstanding work, not coverage.
- Lua `*l` strip is, by design, outside B-bucket coverage: it documents
  intentional behavior (G3), not a Windows CRLF gap, so it has no B
  target and never will.

So the harness reaches full coverage of every identified I/O boundary once
G1–G4 are resolved *and* the YAML/XML A tests land and pass — not before.
