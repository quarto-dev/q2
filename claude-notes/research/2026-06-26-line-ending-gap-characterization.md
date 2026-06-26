# Line-ending gap characterization — live-byte probes

**Date:** 2026-06-26
**Companion to:** the settled design in
[`../designs/byte-offset-invariant.md`](../designs/byte-offset-invariant.md)
and [`../designs/line-ending-preserve.md`](../designs/line-ending-preserve.md),
and the state analysis
[`2026-06-25-line-ending-handling-analysis.md`](2026-06-25-line-ending-handling-analysis.md).
**Trigger:** before building the in-source CRLF harness (`bd-tuf04qgu`),
Chris asked to characterize the boundaries the analysis left *unverified*
(XML/YAML reader normalization, bare-CR-at-EOF, BOM) by observing actual
bytes, then decide catalog rows + strands from what the code really does.

**Method.** Crafted exact-byte probe files (`xxd`-verified) and ran the
already-built `target/debug/pampa.exe` (the docs branch is code-identical
to `main` for these files, so no rebuild was needed). All byte
observations were taken from output written to a **file**, not a pipe —
see the methodology note below.

## Methodology note (load-bearing for the harness)

`pampa … -t json | jq … | xxd` shows CRLF (`0d 0a`) on every JSON
structural newline on Windows — that CR is added by **jq's own
pretty-printer / stdout text handling**, not by pampa. Writing the same
JSON straight to a file (`-t json > out.json`) shows **zero** CRLF. So:

- **Verify line-ending bytes by writing to a file**, never through a
  `jq`/stdout pipe.
- The harness (`bd-tuf04qgu`) compares Rust `String`s in-process, so it is
  immune to this; but any *probe* that inspects rendered bytes must avoid
  the pipe.

## Findings

### BOM — preserve-correct (not a bug)

Input `EF BB BF '# Title' \n \n 'hello' \n` (18 bytes):

- Heading renders `Str "Title"` — **no BOM leak** into text content.
- `astContext`: `total_length: 18` (BOM counted), and the "Title" run is
  reported at byte range `[5,10)`, which is exactly "Title" **in the
  BOM-bearing original** (`#`=3, ` `=4, `T`=5 …). Offsets index the
  original bytes *including* the BOM.

So BOM at ingress is strategy-1 preserve: not stripped, not leaked,
offsets valid. **No offset-level bug.** The only residual question is
whether a *writer* re-emits a leading BOM on round-trip — that folds into
the writer-EOL strand (`bd-3ecwq37k`), not a standalone bug. Contrast
rustc, which strips the BOM and needed `normalized_pos` to repair offsets;
q2 avoids that class entirely here.

### Bare CR — grammar vs. line-table inconsistency

Input `'# H' \r 'line2' \r 'no-final-newline'` (26 bytes, lone CRs as
separators, no LF):

- The **grammar** treats lone `\r` as a line break:
  `Header 1 [Str "H"]` + `Para [Str "line2", SoftBreak, Str "no-final-newline"]`.
- But `astContext.line_breaks` is **`[26]`** — only the injected trailing
  `\n` (total_length went 26 → 27 via the "Missing Newline at End of
  File" Q-7-1 injection). **The two bare CRs at bytes 3 and 9 are not in
  the table.**

Consequence: byte offsets stay valid, but **line/column derived from
`line_breaks` misattributes every lone-CR line** — the converters don't
just "count `\r` as a column char" (the analysis framing), they don't see
a bare CR as a line break at all. This is the concrete, testable failure
behind `bd-hn2ddyhf` (column-semantics decision) and `bd-hebi97on` /
`bd-b1291v6g` (the converters).

Secondary, low priority: a file ending in a bare `\r` hits the
`ends_with('\n')`-only injection check (`main.rs:217`, `qmd.rs:79`),
appending `\n` to yield a trailing `\r\n` — arguably altering the doc.
Pathological; unowned.

### CRLF body round-trip — produces MIXED endings

Input uniform CRLF: `'# H' \r\n \r\n '```' \r\n 'code1' \r\n 'code2' \r\n '```' \r\n`.

**qmd writer output** (bytes, to file):

```
# H\n  \n  ```\n  code1\r\n  code2\r\n  ```\n
```

Structural newlines emit **LF**; code-body content keeps **CRLF**. A
uniform-CRLF document round-trips to a **mixed-ending** document. This is
the demonstration for `bd-3ecwq37k` (no writer chooses EOL from the input
convention).

**native writer output** reproduces both Layer-1 bugs from bytes — the
code body serializes as `"code1\r\ncode2\r"`:

- raw `0d` emitted **unescaped** where Pandoc's `show` would write `\r`
  (`bd-ske10iyd`, `write_safe_string` missing the `'\r'` arm);
- a **lone trailing `\r`** after `code2` from the fence half-strip
  (`bd-xyn4kk3k`, `fenced_code_block.rs:70` pops only `\n`).

Together these fully explain the stray `\r` in snapshot `native/007`.

### YAML block scalar — line endings consumed (not preserved)

Front-matter `desc: |` with body `line1\r\nline2` →
`MetaInlines [Str "line1", SoftBreak, Str "line2"]`. The internal CRLF is
consumed into a `SoftBreak`; `Str` values are clean (`"line1"`,`"line2"`),
no `\r` survives as content. Likely acceptable (YAML block-scalar
semantics; front-matter is metadata, not body bytes), but it means YAML
multi-line scalars do **not** round-trip CRLF. Offset validity through the
provenance pool was **not** confirmed in this pass.

### XML — unprobed

Not reachable through `pampa` with a quick fixture. `quarto-xml` already
disables `trim_text_start/end` and derives offsets from
`reader.buffer_position()`. quick-xml does not normalize text-node line
endings by default, but XML-spec §2.11 attribute-value normalization may
apply. **Needs a dedicated probe test inside the crate** (feed
`<a x="l1\r\nl2">t1\r\nt2</a>`, assert the stored text/attr bytes and that
the source ranges index the original).

## Proposed changes (for review — not yet applied to strands)

**Boundary-catalog rows to add** (in `byte-offset-invariant.md`):

| Boundary | Strategy | Note |
|---|---|---|
| BOM at ingress | 1 preserve | Counted in offsets, not stripped/leaked; verified `[5,10)` for "Title" under a 3-byte BOM. Writer re-emit is a `bd-3ecwq37k` concern. |
| YAML reader (block scalars) | (decide) | Multi-line scalar CRLF → `SoftBreak`; not preserved as content. Confirm offset validity, then classify (likely "local, intentional" like Lua I/O). |
| XML reader (quick-xml) | (decide, unprobed) | `trim_text` off, offsets via `buffer_position`. Probe text-node + attr normalization before assigning a strategy. |

**Sharpen** the line↔column catalog row and `bd-hn2ddyhf` text to cite the
`line_breaks`-omits-bare-CR finding (the testable failure), not only
"counts `\r` as a column char".

**Possible new strands** (Chris to approve):

- XML reader line-ending characterization (probe → catalog row → strategy).
- bare-CR-at-EOF injection edge (low priority; or fold into `bd-qmtp61ms`).

**No new strand for BOM** — resolved as preserve-correct; writer-BOM
folds into `bd-3ecwq37k`.

## Harness implications (`bd-tuf04qgu`, on hold)

The harness should mirror `quarto-doctemplate`'s
`mod crlf_preservation` (paired CRLF + a `*_unchanged` LF guard, inline
strings, in-source so it runs on Linux CI). The boundaries above give the
concrete cases:

- **characterization** (record current, buggy, behavior; flip to assertion
  on the linked fix): fenced lone-CR (`bd-xyn4kk3k`), native raw-CR
  (`bd-ske10iyd`), soft-break CRLF→LF (`bd-qmtp61ms`), qmd-writer mixed
  output (`bd-3ecwq37k`), `line_breaks` omits bare CR (`bd-hn2ddyhf`).
- **assertion now** (already correct): BOM offset validity, byte offsets
  under CRLF/lone-CR/mixed (the byte-offset invariant), LF behavior
  unchanged.

Each known-buggy case carries a `// characterizes bd-XXXX; flips to
assertion on fix` comment so a green harness is never mistaken for correct
behavior.
