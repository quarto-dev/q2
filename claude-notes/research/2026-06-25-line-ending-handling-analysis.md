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

## The decision Chris owns (policy for *real* user CRLF documents)

Pinning fixtures makes tests green but says nothing about what happens
when a real Windows user feeds a CRLF `.qmd`. Two coherent positions:

- **A — Preserve (extend #329 to pampa).** Make reader+writer chain
  CRLF-transparent: strip the *full* `\r\n` where a trailing newline is
  removed (fix `fenced_code_block.rs:70`), escape `\r` faithfully in
  native, and have writers emit the input convention. Faithful, matches
  doctemplate, but a real lift (writers hardcode `\n` everywhere,
  structural newlines included) and needs the offset-drift spots
  (`treesitter.rs:1371`) audited.

- **B — Normalize content to LF, deliberately and documented, with
  offsets kept correct.** This is what comrak + inline-math already do
  de facto. Simpler output, but **contradicts the stated #329
  precedent** and re-opens the offset-drift risk #157 rejected.

These are independent of the green-tests work. My read: do the safe
decomposition now (below), and decide A-vs-B as a separate design call —
but stop treating "fix the snapshots" as if it answers the policy
question.

---

## Recommended decomposition (independent, smallest-first)

1. **`.gitattributes` LF pin** for pampa fixtures + snapshots (mirror the
   doctemplate precedent). Requires `git add --renormalize` (or re-checkout)
   since the working tree is already CRLF. Fixes the content-driven
   failures the #329 way. *Determinism, not normalization.*
2. **Forward-slash the filename** in ASTContext / JSON writer via
   `quarto_util::to_forward_slashes`. Fixes `json/001`. Safe — metadata,
   no offset impact. (Best applied at ingress in `with_filename` so
   diagnostics also get a stable path.)
3. **Writer/reader correctness, defense-in-depth** (so real CRLF content
   never produces garbage even when not pinned):
   - `write_safe_string`: add `'\r' => write!(buf, "\\r")`.
   - `fenced_code_block.rs:70`: strip a full `\r\n` line-ending unit, not
     a bare `\n`.
4. **Policy decision A-vs-B** for real CRLF documents — separate design
   doc; do not bundle.
5. **Incidental:** error-corpus snapshot tests are no-ops (stale glob);
   file a strand.

## Strands

- **bd-238o** — "Port 3 known pampa Windows fixes from quarto-markdown."
  Re-evaluate against #329: its fix #1 (`write_safe_string \r`) and #3
  (json path separator) stand; fix #2 (CRLF normalization in test reads)
  is the **wrong approach for q2** — replace with `.gitattributes`
  pinning.
- **bd-1v25u93e** — pampa snapshot mismatches (native/007 stale header +
  json/001 backslash). Covered by items 1–2 above.
