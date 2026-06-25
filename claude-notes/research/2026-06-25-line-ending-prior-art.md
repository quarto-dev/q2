# Line-ending handling — prior art & tooling survey

**Date:** 2026-06-25
**Companion to:** `2026-06-25-line-ending-handling-analysis.md` (q2's
current state + the settled "preserve" policy).
**Purpose:** Before designing the preservation work, survey how the
reference implementation (Pandoc), the broader Rust ecosystem, and our
own dependency (comrak) handle CRLF/LF — and what reusable tooling
exists. Verified against source where possible; web/deepwiki claims are
flagged as such.

---

## Three strategies (the framing this survey produced)

The field does not split into "preserve vs normalize." There are three
distinct, shipped strategies:

1. **Pure preserve** — bytes kept verbatim end-to-end; the writer emits
   the input's line-ending convention. Cleanest source maps (offsets are
   trivially byte-accurate because nothing was rewritten). Largest
   reader+writer surface to get right. *This is q2's decision, and what
   doctemplate already does (#329).*

2. **Pure normalize** — strip all `\r` on input, process in LF, re-apply
   a chosen convention at the output boundary. Simple; but the internal
   byte stream no longer matches the original file, so byte-accurate
   source maps back to the original are lost unless separately tracked.
   *This is Pandoc.*

3. **Normalize + correction map** — normalize to LF for processing, but
   record where bytes were removed so every reported offset/position can
   be translated back to the original file. Middle ground: simple
   processing, honest positions, at the cost of a mapping table and the
   bugs that come with it. *This is rustc, and — crucially — what comrak
   already does internally.*

### The axis that actually matters: offset/source-info alignment

"Preserve vs normalize" is the wrong axis. The real question is: **do the
source byte offsets stay valid indices into the original input bytes?**

q2 has that as a *hard requirement* — source maps must point back at the
exact bytes of the original `.qmd`, because that is what makes Windows
source maps clean and quarto-hub's collaborative positions correct. q1
(and Pandoc) never had that requirement: they keep no byte-accurate map
to the original, so they can normalize and forget. **That difference in
requirement — not taste — is why q1 can normalize and q2 cannot.**

Given q2's requirement, the choice collapses to the two options that
*keep offsets aligned*:

- **(1) preserve** — bytes untouched, so offsets are trivially valid;
- **(3) normalize + correction map** — bytes rewritten, but a map
  (rustc's `normalized_pos`) translates every offset back to the
  original.

Strategy (2) — normalize and forget — is the *only* one ruled out, because
it is the only one that breaks offset alignment.

q2 chose (1): it satisfies the requirement the cheapest-to-reason-about
way — no bytes touched means no map to maintain and no map-offset bugs
(cf. rustc #149568). The escape hatch the team agreed on ("if review
shows we must normalize somewhere, clarify the policy") is, concretely,
**falling back to (3) at a specific boundary** — most likely the
commonmark reader (see comrak below), where (1) is impractical and comrak
already maintains the correction-map-style positions for us.

---

## Pandoc (strategy 2) — verified against `DEV_CONTRIB/pandoc-1`

- **Input: strips every CR at decode.** `filterCRs = B.filter (/=13)`
  in `src/Text/Pandoc/Class/PandocMonad.hs:482` and
  `src/Text/Pandoc/UTF8.hs:106,121`. Runs at UTF-8 decode, before any
  reader sees the text. So CRLF *and* lone CR both collapse to LF; the
  readers assume `\n`-only input. (DeepWiki confirmed the same.)
- **Output: choose convention at the I/O handle, not in writers.**
  `data LineEnding = LF | CRLF | Native` (`App/Opt.hs:71`), default
  `Native` (`Opt.hs:847`), surfaced as `--eol crlf|lf|native`
  (`CommandLineOptions.hs:571`). Applied via
  `hSetNewlineMode h (NewlineMode eol eol)` (`UTF8.hs:81`); the writers
  themselves only ever emit `\n`. The OS handle translates on write.
- **Implication for q2.** Pandoc can normalize because it keeps *no*
  byte-accurate source map back to the original CRLF input. q2's entire
  rationale (source-map cleanliness → quarto-hub) is what forbids
  strategy 2 for us. We deliberately diverge from Pandoc here. But the
  **output mechanism is worth stealing**: an `--eol`-style setting
  applied at the output boundary keeps writer code convention-agnostic
  instead of threading EOL through every `writeln!`. (For pure-preserve,
  the "setting" defaults to "match this document's input convention.")

## rustc (strategy 3) — the closest precedent to our problem

- rustc normalizes CRLF→LF (and strips BOM) when a `SourceFile` is
  created, for parsing. This made `byte_start/byte_end` in the JSON
  diagnostic output off-by-one per preceding CRLF — which **broke
  rustfix on Windows** (rust-lang/rust#65029).
- Fix (PR #65074): `SourceFile.normalized_pos: Vec<NormalizedPos>`
  records "locations of characters removed during normalization", and
  `normalized_byte_pos` corrects reported offsets back to the original
  file. u32 offset tracking.
- Cautionary tale: `normalized_byte_pos` later had a relative-vs-absolute
  position bug (rust-lang/rust#149568). The correction-map approach is
  proven but fiddly — exactly the kind of subtle offset bug q2 is trying
  to avoid by preferring strategy 1.

## comrak (strategy 3, already in our tree) — DeepWiki, needs source confirm

- Our `readers/commonmark.rs` delegates to comrak. comrak normalizes
  CRLF→LF internally per the **CommonMark spec mandate** (spec §2.1
  treats LF, lone CR, and CRLF all as line endings), consuming `\r\n` as
  one break. **No option disables this.**
- But comrak reports `Sourcepos` (1-based line/col) relative to the
  **original** input, tracking a `column_offset` for consumed CR/LF —
  i.e. it already implements strategy 3.
- **Consequence:** on the commonmark reader path, pure-preserve (1) is
  not achievable without forking comrak. This boundary is the prime
  candidate for the documented escape hatch → accept strategy 3 here,
  and make sure our wrapper carries comrak's original-relative positions
  through rather than recomputing against normalized text.

## CommonMark spec

Line endings (LF, lone CR, CRLF) are all "line endings" and conforming
parsers normalize them; insertion of a final LF is also spec behavior.
This is *why* comrak can't offer an opt-out — and why any qmd construct
that must remain byte-faithful should be handled by our own
tree-sitter-qmd path, not delegated to a CommonMark engine.

---

## Reusable Rust tooling

| Crate | Role | Fit for q2 |
|---|---|---|
| **`line-index`** (rust-analyzer) | `TextSize` offset ↔ `(line,col)`, UTF-8/16/32, `WideEncoding` for LSP | Strong — quarto-hub is a collaborative editor; LSP position encoding (UTF-16) is exactly its `WideLineCol`. Candidate for the offset↔position layer regardless of strategy. `cargo add line-index`. |
| **`line-ending`** | detect + normalize + convert LF/CRLF/CR; `consume_line_ending()` on `Peekable<Chars>` | Useful for **detecting a document's convention** (needed for round-trip preserve) and for the output-EOL step. |
| **`normalize-line-endings`** | iterator: any of `\r\n`/`\r`/`\n` → `\n` | Building block if a boundary opts into strategy 3. |
| **`newline_normalizer`** | normalize via `Cow::Borrowed`, ~4.66 ns, no alloc when input unchanged | Fast path; the no-alloc-when-unchanged property is nice for the common LF case. |
| rustc `normalized_pos` pattern | not a crate — a design to copy if we ever do strategy 3 with byte-offset output | Reference design + its known bug (#149568) as a test to write up front. |

---

## Further research — findings (completed 2026-06-25)

### tree-sitter + CRLF — byte-accurate, no normalization

tree-sitter core operates on raw bytes and does **not** normalize. Row
increments on `\n` only; `\r` is part of the line's content and counts
toward both column and byte range (`ts_lexer__do_advance`). So the parse
tree's `Point`/`Range` stay byte-accurate on CRLF. **Correction to the
companion analysis:** the suspected drift at `treesitter.rs:1371` is
*not* a bug — `raw_text.split('\n')` keeps the `\r` in each substring, so
`line.len() + 1` is byte-correct on CRLF. The reader bugs are purely in
our post-extraction string handling (half-strip, silent normalize),
never in tree-sitter or its byte offsets.

### quarto-cli (q1) — normalizes to LF (strategy 2)

q1 normalizes via `lines()` in `src/core/lib/text.ts`, early in
preprocessing, before Pandoc/Lua; it does not preserve the convention in
output. So q1 follows strategy 2, like Pandoc. **Consequence:** q2's
preserve is a deliberate q2-only improvement, *not* a q1↔q2 parity
requirement — parity comparisons may legitimately differ in EOL, and
that is expected, not a parity bug.

### comrak — strategy 3, source-confirmed

`comrak-0.52.0/src/parser/mod.rs:205-211`: the `feed` loop consumes `\r`
then optional `\n` as one line ending (CRLF as a unit), and tracks
offsets for `Sourcepos`. No opt-out. Confirms the commonmark reader path
cannot do pure-preserve without forking comrak → escape-hatch (strategy
3) boundary.

### Git operational layer — `--renormalize` leaves the working tree

`git add --renormalize .` after adding `.gitattributes ... text eol=lf`
stages the CRLF→LF conversion **in the index**, but does **not** rewrite
the working-tree files — `foo` stays CRLF on disk until a re-checkout
(`git rm --cached -r . && git checkout .`, or delete + checkout). So the
determinism strand is two steps: (1) add the attribute + renormalize +
commit, (2) re-checkout to refresh the 704 already-CRLF working files.

### quarto-hub / Automerge — already pure-preserve; line/col layer is LF-only

This is the *endpoint* of the rationale, and the finding validates it:
**no CRLF normalization exists anywhere in the hub pipeline.** Text flows
byte-for-byte disk → Automerge → VFS → WASM parser:

- Disk → Automerge: `quarto-hub/src/sync.rs:78` `read_to_string` (no
  normalization), written verbatim via `update_text` (`sync.rs:134`).
- Automerge stores the whole file as one Automerge `Text` blob
  (`quarto-automerge-schema/src/index.ts:253`), byte-agnostic.
- Automerge → VFS → WASM: `preview-runtime/.../wasmRenderer.ts:255` →
  `wasm-quarto-hub-client/src/lib.rs:356` `content.as_bytes()`, verbatim.
- Monaco ↔ Automerge: edits spliced by UTF-16 offset with no transform
  (`quarto-sync-client/src/client.ts:1161`); Monaco EOL is **not**
  configured (`Editor.tsx:97-121`) — it auto-detects.

So byte offsets in source maps already match the VFS bytes exactly —
precisely the property the preserve policy is meant to protect. **But the
line/column layer assumes LF-only in three places**, counting `\r` as an
ordinary column char:

- `crates/quarto-source-map/src/utils.rs:22` (`offset_to_location`)
- `crates/comrak-to-pandoc/src/source_location.rs:35` (line table)
- `hub-client/src/utils/diffToMonacoEdits.ts:32` (`offsetToPosition`)

On CRLF content these still produce **byte-accurate byte offsets**, but
**line/column drifts by one per CRLF line**. The TS one feeds Monaco
`Position`s directly, so it is a likely edit-misplacement bug on CRLF
documents in the hub. Under the preserve policy CRLF *does* reach these
functions, so "what does column mean under preserve" must be answered for
the line/col layer, not just the pampa writers. This is the strongest
candidate for Windows-dev-led testing.

### LSP position encoding (carry-over)

The hub uses UTF-16 code-unit offsets end-to-end
(`quarto-sync-client/src/types.ts:24`), matching Monaco + Automerge's
WASM build. `line-index`'s `WideEncoding`/`WideLineCol` models exactly
this if we adopt it for the offset↔position layer.

## Sources

- Pandoc: local clone `DEV_CONTRIB/pandoc-1` (`UTF8.hs`, `PandocMonad.hs`,
  `App/Opt.hs`, `App.hs`, `CommandLineOptions.hs`); DeepWiki jgm/pandoc.
- rustc: rust-lang/rust#65029, PR #65074, #149568;
  `rustc_span::SourceFile`.
- comrak: DeepWiki kivikakk/comrak (source confirmation still TODO).
- Crates: line-index (rust-analyzer), line-ending, normalize-line-endings,
  newline_normalizer (crates.io / lib.rs).
