# Byte-offset invariant — source positions stay valid indices into the original input

**Status:** Active (established 2026-06-26). Foundational contract; the
line-ending preserve epic (`bd-eehxwr29`) is one consumer.
**Companion docs:** the `SourceInfo` *shape* contract is
[provenance-contract.md](provenance-contract.md); the *encoding* split
(UTF-8 vs UTF-16) at the WASM wire is
[attribution-encoding-contract.md](attribution-encoding-contract.md).
This doc states the cross-cutting *position-validity* rule those two assume.
**Background:** the line-ending state analysis and prior-art survey in
[`claude-notes/research/2026-06-25-line-ending-handling-analysis.md`](../research/2026-06-25-line-ending-handling-analysis.md)
and
[`2026-06-25-line-ending-prior-art.md`](../research/2026-06-25-line-ending-prior-art.md).

## The invariant

> **Every byte offset q2 reports for a piece of source must remain a valid
> index into the *original*, unmodified input bytes** — the bytes as they
> sit on the user's disk (or in the collaborative store), before any
> internal rewriting.

`SourceInfo` ranges, diagnostic spans, crossref anchors, engine
error-line mapping, and the hub's editor positions are all offsets. They
are only meaningful if they point back at the exact bytes the user can
see. The moment an internal step rewrites bytes *and forgets it did*,
every offset downstream of that step is silently wrong against the file
the user is looking at.

## Why q2 has this requirement (and q1 / Pandoc do not)

q2's source maps power two things q1 never had:

1. **Clean Windows behaviour.** A `.qmd` on Windows is CRLF. If offsets
   drift by one per preceding `\r`, diagnostics and edits land on the
   wrong byte — the rustfix-on-Windows failure (rust-lang/rust#65029) is
   the canonical example of exactly this.
2. **Collaborative editing in quarto-hub.** Cursors, selections, and
   edit splices are offsets into a *shared* document. They must agree
   across collaborators and across the disk⇄store boundary.

q1 and Pandoc keep **no** byte-accurate map back to the original, so they
are free to normalize line endings and forget. q2 cannot. **This
difference in requirement — not taste — is the whole reason the rules
below exist.**

## The real axis: offset alignment, not "preserve vs normalize"

There are three strategies for handling a convention-bearing byte (a
`\r`, a BOM) at an internal boundary:

| # | Strategy | Offset alignment | Verdict |
|---|---|---|---|
| 1 | **Preserve** — bytes untouched end to end | trivially valid (nothing moved) | **default** |
| 2 | **Normalize + forget** — strip `\r`, process in LF, never record what was removed | **broken** — offsets index the normalized stream, not the original | **forbidden** |
| 3 | **Normalize + correction map** — strip `\r` for processing, record removals, translate every reported offset back (rustc's `normalized_pos`) | valid (via the map) | **sanctioned escape hatch** |

The question is never "should we preserve or normalize?" It is **"do the
reported offsets still index the original bytes?"** Strategies 1 and 3
both answer yes; strategy 2 answers no. **Strategy 2 is the only one
ruled out.**

## Decision rule

1. **Default to preserve (strategy 1).** Bytes untouched means offsets are
   valid for free, with no correction map to maintain and no map-offset
   bugs (rustc's `normalized_pos` later grew exactly such a bug —
   rust-lang/rust#149568). Cheapest to reason about; pick it unless a
   boundary makes it impossible.
2. **Use normalize + correction map (strategy 3) only where a third-party
   engine normalizes with no opt-out.** At such a boundary, carry the
   engine's original-relative positions (or build a rustc-style
   `normalized_pos`) so offsets still resolve to the original bytes.
   **Each such boundary must be named in the catalog below.**
3. **Never use normalize + forget (strategy 2) anywhere.**
4. **Never normalize the cross-user canonical byte store.** See the hub
   boundary below — this is a special, load-bearing case of rule 3.

## Boundary catalog

Where each strategy applies today. Add a row when a new boundary is
introduced.

| Boundary | `file:line` | Strategy | Notes |
|---|---|---|---|
| BOM at ingress | `readers/qmd.rs` (verbatim) | **1 preserve** | Verified 2026-06-26: a 3-byte UTF-8 BOM is counted in offsets and not stripped or leaked into text ("Title" reported at `[5,10)` under the BOM; `total_length` includes it). No correction map needed (cf. rustc, which strips BOM). Writer re-emit of a leading BOM is a `bd-3ecwq37k` concern, not an ingress bug. |
| tree-sitter-qmd parse | `scanner.c`; tree-sitter core | **1 preserve** | Byte-accurate on CRLF natively: row advances on `\n` only, `\r` counts in column + byte range. No normalization; offsets are sound. |
| post-extraction string handling (readers) | `treesitter_utils/fenced_code_block.rs:70`, `treesitter.rs:422,428,476` | **1 preserve** *(currently buggy)* | Must rejoin/strip with the original convention. Today some paths half-strip or silently rewrite CRLF→LF — genuine bugs under this contract (`bd-xyn4kk3k`, `bd-qmtp61ms`). |
| qmd / native / html writers | `writers/*.rs` | **1 preserve** *(largest gap)* | Structural newlines are hardcoded `\n` (`writeln!`); no writer emits the input convention. Round-trip requires a writer-EOL model (`bd-3ecwq37k`). Until then, engine `map_offset` into code bodies drifts on CRLF (byte-exactness precondition — see below). |
| line ↔ column converters | `quarto-source-map/src/utils.rs:8`, `file_info.rs:85`, `comrak-to-pandoc/src/source_location.rs:28`, `hub-client/src/utils/diffToMonacoEdits.ts:32` | **1 preserve, CRLF-aware** | Byte offsets stay valid here; only *line/column* drifts because the line table is LF-only. Verified 2026-06-26 with a bare-CR doc: the grammar treats lone `\r` as a break (`Header` + `Para`/`SoftBreak`), but `astContext.line_breaks` records **only** the LF — the bare CRs are absent, so line/col misattributes every lone-CR line (not merely "counts `\r` as a column char"). Decision needed: a line ending is one logical column unit (`bd-hn2ddyhf`), then make the converters CRLF-aware (`bd-hebi97on`, `bd-b1291v6g`). The two Rust converters are redundant — consolidate to one. |
| YAML reader (block scalars) | quarto-yaml | **decide** | Verified 2026-06-26: a multi-line block scalar's internal CRLF is consumed into a `SoftBreak` — not preserved as content bytes (`Str` values are clean). Front-matter is metadata, not body; likely classifiable "local, intentional" like the Lua row, but offset validity through the provenance pool is unconfirmed. |
| XML reader (quick-xml) | quarto-xml `parser.rs` | **decide (unprobed)** | `trim_text_start/end` disabled; offsets via `reader.buffer_position()`. quick-xml does not normalize text nodes by default, but XML-spec §2.11 attribute-value normalization may. Needs a dedicated in-crate probe before assigning a strategy. |
| commonmark reader (comrak) | `readers/commonmark.rs`; `comrak-0.52.0/src/parser/mod.rs:205-211` | **3 normalize + map** | comrak normalizes CRLF→LF per CommonMark spec §2.1 with **no opt-out**. The one boundary where strategy 1 is impossible without forking. comrak reports original-relative `Sourcepos` (tracks a `column_offset`), so carry those through rather than recomputing against normalized text (`bd-1ll7kj9e`). |
| hub: disk → automerge → VFS → WASM | `quarto-hub/src/sync.rs:78` (verbatim `read_to_string`) → `:134` (`update_text`, no transform between — only a SHA256 for change detection); `wasm-quarto-hub-client/src/lib.rs` `content.as_bytes()` | **1 preserve (mandatory)** | The automerge `Text` blob is the **shared, cross-user canonical byte store**. It must hold the user's verbatim bytes (a Windows user's CRLF included): normalizing it would desync collaborative offsets across users and require per-user CRLF restoration on disk write-back. The in-browser parser is tree-sitter (CRLF-native), so the browser needs **no** normalization — the temptation to "process LF-only in WASM and restore offsets" would replace the small CRLF-aware line/col fix with a correction-map machine. Do not normalize here. |
| Lua file I/O | `lua/io_wasm.rs:171` | local, intentional | `file:read("*l")` strips a trailing `\r`, matching Lua line semantics; local to the Lua sandbox, not a source-map boundary. |

## Byte-exactness precondition (writer ⇄ engine provenance)

`write_with_source_info` → `map_offset` (`crates/pampa/src/writers/qmd.rs:2742`,
`write_impl_tracked` at `:2769`) tiles the output with one linear piece per
top-level block. `map_offset` into a code body is byte-exact **only when
output bytes equal source bytes**. Under CRLF input that holds **only if the
writer preserves the input convention**: if the writer emits LF while the
source is CRLF, every internal newline in a code body shrinks one byte in
output, so `map_offset` drifts one byte per code line — silently. This is a
current bug (verified 2026-06-26), independent of the shelved per-line-
provenance reimplementation. Writer-EOL preservation (`bd-3ecwq37k`) is
therefore a precondition for engine offset-exactness on CRLF documents. Line
*count* is unaffected (`write_soft_break` is 1:1); only *byte* exactness is at
risk.

## Test obligation

The provenance test surface is currently **implicitly LF-only**: no test
asserts an offset against CRLF input. Any code that converts, tiles, or
reports offsets must have a CRLF (and lone-CR, and mixed) regression
variant, built **in source** so it runs on Linux CI — not only on a
Windows checkout (on-disk fixtures are pinned to LF for determinism;
`bd-mv2ggmr5`). The shared harness and per-boundary coverage live in
`bd-tuf04qgu`. The minimal assertion at every boundary is: *feed CRLF,
confirm every reported offset still indexes the original bytes.*

## References

- Line-ending analysis + prior-art:
  [`2026-06-25-line-ending-handling-analysis.md`](../research/2026-06-25-line-ending-handling-analysis.md),
  [`2026-06-25-line-ending-prior-art.md`](../research/2026-06-25-line-ending-prior-art.md).
- Gap characterization (live-byte probes for BOM, bare-CR, YAML, CRLF
  round-trip): [`2026-06-26-line-ending-gap-characterization.md`](../research/2026-06-26-line-ending-gap-characterization.md).
- rustc correction-map precedent: rust-lang/rust#65029 (the Windows
  rustfix break), PR #65074 (`normalized_pos`), #149568 (its later bug).
- CommonMark spec §2.1 (line endings); comrak
  `src/parser/mod.rs:205-211` (no opt-out, original-relative `Sourcepos`).
- Pandoc's output-boundary EOL model (`--eol`, `hSetNewlineMode`) as the
  shape to steal for the writer-EOL setting — prior-art doc, Pandoc
  section.
