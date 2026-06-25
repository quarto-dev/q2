# Line-ending handling across q2 I/O — deep analysis

**Date:** 2026-06-25
**Trigger:** 7 pampa snapshot/roundtrip/corpus tests fail on a Windows
checkout. Investigation showed the failures are *pre-existing* Windows
CRLF artifacts, not regressions from the merged PRs #340/#341. Chris
asked to step back and understand what q2 actually does with line
endings at every I/O boundary, rather than reach for ingress
normalization (which PR #329 explicitly rejected).

**Method:** four parallel source-mapping passes (readers/grammar,
writers, fixtures/`.gitattributes`, path separators), each citing
`file:line`, then spot-verification of the load-bearing claims.

---

## The contract we are supposed to follow (PR #329, fixes #157)

> "Conventions are preserved, not normalized." CRLF input renders CRLF
> output; LF renders LF. Ingress CRLF→LF normalization was rejected
> twice because it silently rewrites bytes and **shifts every source
> offset by one per preceding `\r`, which drifts diagnostics** — fatal
> for q2's source-map foundation. Test determinism is achieved by
> pinning fixtures to LF via `.gitattributes`, with the CRLF path
> covered by in-source tests. A writer-side `--eol` override (like
> Pandoc's) is a separate future concern.

PR #329 implemented this for **doctemplate only**. The finding below is
that **the rest of q2 does not honor this contract** — it has no
coherent line-ending policy at all.

---

## The map: what each boundary actually does

### Readers / grammar (ingress)

- **`readers/qmd.rs`** passes input bytes **verbatim** to tree-sitter —
  no CR stripping (✓ aligned with #329). Trailing-newline injection
  (`qmd.rs:79`, `main.rs:217`) checks `ends_with(b"\n")` only; safe for
  normal CRLF files, mishandles a pathological bare-CR-at-EOF.
- **tree-sitter scanner** (`tree-sitter-qmd/.../scanner.c`) is
  CRLF-aware: it consumes `\r\n` as one unit into structural
  line-ending tokens. Content *between* those tokens is captured raw, so
  CRLF survives into the AST.
- **The behavior is inconsistent past that point** — this is the headline:
  | Path | `file:line` | CRLF behavior |
  |---|---|---|
  | code-fence trailing strip | `treesitter_utils/fenced_code_block.rs:70` | `if content.ends_with('\n') { content.pop() }` pops **only `\n`**, leaving a **lone trailing `\r`** in `CodeBlock`/`RawBlock.text`. **Half-strip bug.** Direct cause of the stray `\r` in snapshot `native/007`. |
  | inline math soft-breaks | `treesitter.rs:476` | rewrites soft-break to `\n` — **silently normalizes** |
  | `strip_continuation_prefix` (lists/quotes) | `treesitter.rs:422,428` | splits on `\n`, rejoins with `\n` — **silently normalizes** CRLF→LF inside nested content |
  | grid-table offset math | `treesitter.rs:1371` | `offset += line.len() + 1` assumes 1-byte `\n`; on CRLF the `+1` is wrong → **latent source-offset drift** (exactly the #329 hazard, already present) |
  | commonmark reader | `readers/commonmark.rs` (comrak) | comrak normalizes CRLF→LF per CommonMark spec — **silently normalizes** |
  | YAML / XML readers | quarto-yaml, quarto-xml | pass verbatim; quick-xml may apply XML-spec `\r\n`→`\n` internally |
  | Lua file I/O | `lua/io_wasm.rs:171` | strips trailing `\r` from `file:read("*l")` — intentional, local to Lua |

  So pampa **preserves** CRLF in some paths, **silently normalizes** in
  others, **half-strips** in one, and **drifts offsets** in another.
  None of it is a decision; it is accumulated accident.

### Writers (egress)

- **`writers/native.rs` `write_safe_string` (lines 11–23)** escapes
  `\\`, `"`, `\n` but **not `\r`** → a raw CR byte leaks into the
  serialized string. Pandoc's Haskell `show` escapes `\r` as `\r`. This
  is the second contributor to `native/007` (the internal `\r\n`'s CR).
  This fn is the only string-quoter in the file; used for every
  string-bearing node.
- **`writers/json.rs`** — both the Value path (serde) and the streaming
  path (`json_stream.rs` `ESCAPE[0x0D]`) escape `\r` correctly (✓).
- **`writers/html.rs`, `writers/qmd.rs`** pass content `\r` through
  verbatim; **every structural newline is a hardcoded `\n`** (`writeln!`).
- **No pampa writer chooses output EOL from the input convention.** They
  all emit `\n` unconditionally. So pampa does **not** implement #329's
  "preserve end-to-end" — doctemplate does, pampa does not.

### Path separators (a *different* class of bug, tagged along)

- `astContext.files[].name` is emitted **verbatim** from
  `ast_context.filenames[idx]` on both write paths
  (`json.rs:1812` value, `json.rs:3967` streaming). Origin:
  `readers/qmd.rs:98` → `ASTContext::with_filename(path.to_string_lossy())`.
  On Windows the snapshot test's glob yields `tests\snapshots\json\001.qmd`
  → backslashes in output → `json/001` mismatch.
- **This is NOT a line-ending issue and NOT content.** It is file-name
  *metadata*. Normalizing it to forward slashes does **not** shift any
  source offset, so #329's prohibition does not apply.
- The canonical helper **already exists**: `quarto_util::to_forward_slashes`
  (`quarto-util/src/path.rs:23`). PR #340 used it for the Lua side.
  HTML resource paths, DocumentProfile, listings, and preview-deps all
  already normalize. **The JSON writer / ASTContext is the lone holdout.**

### Fixtures / determinism

- **Zero `.gitattributes` EOL pin anywhere under `crates/pampa/`.** Only
  `quarto-doctemplate` (template fixtures) and `tree-sitter-doctemplate/grammar`
  pin LF; root `.gitattributes` carries only the beads merge driver.
- Under pampa, **704 tracked files are `i/lf w/crlf`** → flip to CRLF on
  any Windows checkout with `core.autocrlf=true` (390 `.qmd`, 210
  `.snap`, plus `.rs`/`.md`/`.json`). Workspace-wide it is ~2000+.
- Every fixture read is `std::fs::read_to_string` with **no** post-read
  normalization; raw bytes go straight to the parser.

### Incidental finding (separate bug)

- The error-corpus snapshot tests (`test_error_corpus.rs:258,335`) glob
  `resources/error-corpus/*.qmd`, which now matches **zero files** (the
  `.qmd` live in `case-files/`). Those two functions lack the
  `assert!(file_count > 0)` guard, so they **pass vacuously** and 84
  committed `.snap` files are stale/unexercised. Worth its own strand.

---

## Diagnosis: three problems were conflated as "line endings"

1. **Test determinism** (the immediate 7 failures). Fixtures/snapshots
   aren't pinned, so autocrlf flips them. #329's answer — `.gitattributes
   eol=lf` — is the right, *safe* fix. This is controlling on-disk bytes,
   **not** normalizing in code.

2. **Engine line-ending policy** (the deep issue). q2 has no coherent
   policy. #329 declared "preserve, don't normalize" but only doctemplate
   implements it; pampa is an accidental mix (preserve / silently
   normalize / half-strip / offset-drift). The writers preserve nothing.

3. **Path separators** (a red herring). Pure metadata normalization,
   already solved everywhere except the JSON writer; safe to fix at
   ingress with the existing helper.

Pinning `.gitattributes` fixes the **content**-driven failures
(`native/007`, roundtrip, corpus, html/json writer tests) by keeping
CRLF out of the fixtures. It does **not** fix `json/001` — that
backslash comes from the Windows glob *path*, not file content, so it
needs the path fix regardless of EOL.

---

## Policy decision: PRESERVE (settled)

Decided 2026-06-25: the policy is **preserve, do not normalize**,
declared repo-wide (extend #329 from doctemplate to all of q2). The
approach is to declare the policy first, then treat each line-ending
failure it surfaces as a genuine bug; only if review proves a real need
to normalize somewhere do we clarify the policy.

Rationale: the less we touch a `.qmd` text, the cleaner the diffs and the
better the source maps work — and the better the source maps, the better
quarto-hub.com works. Preserving the byte stream is what keeps Windows
source maps honest.

**Consequences for this analysis:**

- The "silently normalize" reader paths are now **genuine bugs**, not
  acceptable behavior:
  - `treesitter.rs:476` inline-math soft-break → `\n`
  - `strip_continuation_prefix` (`treesitter.rs:422,428`) rejoin → `\n`
  - `readers/commonmark.rs` (comrak) CRLF→LF
- `treesitter.rs:1371` offset drift is the exact source-map damage the
  policy exists to prevent — a priority bug.
- **No writer choosing EOL from input convention** is the largest gap.
  Under preserve, content `\r\n` must round-trip; the qmd writer in
  particular must emit the input convention rather than forcing `\n`.
  (This is just unfinished pampa, not intended behavior.)
- The half-strip (`fenced_code_block.rs:70`) and the missing native
  `\r` escape remain bugs, now framed as "faithfully preserve the
  byte stream" rather than "harden against CRLF".

The escape hatch (normalize *somewhere* if review proves a genuine need,
then clarify the policy) stays open but is not the default for any
finding here.

---

## Recommended decomposition (under the preserve policy)

*Determinism* (test infrastructure, orthogonal to engine policy):

1. **`.gitattributes` LF pin** for pampa fixtures + snapshots (mirror the
   doctemplate precedent). Requires `git add --renormalize` (or
   re-checkout) since the working tree is already CRLF. Keeps committed
   fixtures LF so LF-asserting tests are deterministic; the CRLF path is
   covered by in-source tests (the #329 pattern). *Not normalization.*

*Genuine bugs surfaced by declaring the policy* (Carlos: "recognize each
failure as a genuine bug"):

2. **Forward-slash the filename** in ASTContext / JSON writer via
   `quarto_util::to_forward_slashes`. Fixes `json/001`. Consistent with
   the policy — it is file-name *metadata*, not `.qmd` text, and does not
   shift source offsets. Best applied at ingress in `with_filename` so
   diagnostics also get a stable path.
3. **Faithful byte-stream preservation in the parse→AST path:**
   - `fenced_code_block.rs:70`: consume a full `\r\n` line-ending unit
     (not a bare `\n`), so no lone `\r` is left and internal CRLF content
     is preserved intact.
   - `write_safe_string` (native): add `'\r' => write!(buf, "\\r")`.
   - Kill the silent CRLF→LF rewrites: `treesitter.rs:476` (inline math),
     `strip_continuation_prefix` (`treesitter.rs:422,428`), and decide how
     to handle comrak's spec-mandated normalization in `commonmark.rs`.
   - **Fix the offset drift** at `treesitter.rs:1371` (`+1` vs `+2` on
     CRLF) — top priority, it is the source-map damage the policy guards.
4. **Writers emit the input convention** (the largest gap): no writer
   currently chooses EOL from input. The qmd writer especially must
   round-trip CRLF rather than forcing `\n`. Likely needs a writer-side
   EOL setting (the future `--eol` override #329 mentioned). Larger
   design; scope separately.

*Incidental:*

5. Error-corpus snapshot tests are no-ops (stale glob `resources/error-corpus/*.qmd`
   matches zero files; missing `file_count > 0` guard); 84 `.snap` are
   unexercised. File a strand.

## Strands

- **bd-238o** — "Port 3 known pampa Windows fixes from quarto-markdown."
  Re-evaluate against #329: its fix #1 (`write_safe_string \r`) and #3
  (json path separator) stand; fix #2 (CRLF normalization in test reads)
  is the **wrong approach for q2** — replace with `.gitattributes`
  pinning.
- **bd-1v25u93e** — pampa snapshot mismatches (native/007 stale header +
  json/001 backslash). Covered by items 1–2 above.
