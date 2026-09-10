# Add a Rust grammar to `quarto-highlight` (`rust` / `rs`)

**Status:** approved 2026-09-08, in progress.
**Strand:** bd-202u5bld (parent: bd-n7x2, syntax highlighting epic).

## Overview

`quarto-highlight` ships 13 built-in tree-sitter grammars (14 user-facing
classes). Rust is not one of them, so a ```` ```rust ```` or ```` ```rs ````
fence renders with `<pre class="rust">` and zero `hl-*` spans, in both
`q2 render` and the hub-client / `q2 preview` WASM path.

The registry is data-driven (`BUILTIN_ALIASES` + `BUILTIN_BUILDERS` in
`crates/quarto-highlight/src/registry.rs`) and every downstream consumer —
the render stage (`annotate_pandoc`), the SCSS theme translator
(`quarto-sass/src/highlight_theme.rs`), the LSP semantic tokens
(`quarto-lsp-core/src/types.rs::capture_to_token_type`), and the WASM export
(`wasm-quarto-hub-client`) — resolves through it. So this is the mechanical
"add a language" path the crate's `langs/mod.rs` doc comment describes, plus
the fixtures and prose that enumerate the built-in set.

### Feasibility findings (scouted 2026-09-08)

| Question | Answer |
|---|---|
| Crate | `tree-sitter-rust` **0.24.2** (tree-sitter/tree-sitter-rust, MIT). Already in the local cargo registry cache. |
| Runtime compat | Depends on `tree-sitter-language = "0.1"`, same as the other grammars; picks up the workspace `[patch]` to `crates/tree-sitter-language-wasm-shim`. Grammar ABI is `LANGUAGE_VERSION 15`, identical to `tree-sitter-python` 0.25 which already runs. No second `tree-sitter` in the graph. |
| Query source | Crate exposes `HIGHLIGHTS_QUERY` (also `INJECTIONS_QUERY`, `TAGS_QUERY`). We use only `HIGHLIGHTS_QUERY`, like `lua.rs`; **no vendoring under `resources/highlights/`** needed (that path exists only because `tree-sitter-julia` lacks the constant). |
| Injection/locals constraint | `highlights.scm` contains no `injection.*` / `local.*` captures, so `registry::tests::builtin_configs_have_no_injection_or_locals` stays green. Uses only `#match?` predicates, which Python's query already exercises. |
| Capture names | `keyword`, `punctuation.bracket`/`.delimiter`, `type`, `type.builtin`, `function`, `function.method`, `function.macro`, `string`, `operator`, `constructor`, `constant`, `constant.builtin`, `comment`, `comment.documentation`, `attribute`, `variable.parameter`, `variable.builtin`, `property`, `label`, `escape`. **All** resolve via the longest-dotted-prefix fallback in both `capture_token` (SCSS) and `capture_to_token_type` (LSP legend) — no theme or legend changes. |
| WASM | `scanner.c` includes `<wctype.h>` and calls `iswalpha`/`iswdigit`/`iswspace`; `tree-sitter-bash` (already built for wasm32) uses the same symbols, provided by `crates/wasm-c-shim`. `build.rs` is the standard `cc` two-file build. Expected to link cleanly; verified by the full `cargo xtask verify` (hub-build leg). |
| Docs | `docs/` does not enumerate built-in languages (`themes.qmd` only documents `highlight-style:`), so no docs change. The prose in the smoke fixture `01-builtin-python.qmd` and its README do enumerate them and must be updated. |

Class choice: canonical key `rust`, alias `rs` (mirrors `python`/`py`,
`julia`/`jl`).

## Checklist

### Phase 1 — tests first (TDD)

- [x] `crates/quarto-highlight/tests/integration/all_languages.rs`: add
      `("rust", "fn main() {}\n", "keyword")` and `("rs", …)` cases. Run;
      expect `every_case_class_resolves_to_a_grammar` to fail on `rust`.
- [x] `crates/quarto-highlight/tests/fixtures/builtin-snippets.json`: add a
      `rust` entry with a snippet that exercises a keyword, a macro call, a
      lifetime-free type annotation, a string, and a `///` doc comment
      (e.g. `/// Greets.\nfn greet(name: &str) -> String {\n    format!("hi, {name}")\n}\n`).
      This drives both the insta golden (`golden.rs`) and the hub-client
      vitest harness (`hub-client/src/services/highlight.wasm.test.ts`)
      from one file, so the two paths cannot drift. Expect `golden_all_builtins`
      to panic with "`rust` is not a registered class".
- [x] Smoke-all fixture: add `crates/quarto/tests/smoke-all/highlighting/09-builtin-rust.qmd`
      with `ensureFileRegexMatches` for `<pre class="sourceCode rust"`,
      `hl-keyword">fn</span>`, and a `hl-function-macro` span over `println`.
      Expect failure (no `sourceCode` class, no spans).

### Phase 2 — implementation

- [x] `crates/quarto-highlight/Cargo.toml`: add `tree-sitter-rust = "0.24"`
      under the built-in grammar crates block.
- [x] `crates/quarto-highlight/src/langs/rust.rs`: `build()` mirroring
      `lua.rs` — `tree_sitter_rust::LANGUAGE.into()`, `"rust"`,
      `tree_sitter_rust::HIGHLIGHTS_QUERY`, `""`, `""`.
- [x] `crates/quarto-highlight/src/langs/mod.rs`: `pub(crate) mod rust;`
      (alphabetical, after `r`).
- [x] `crates/quarto-highlight/src/registry.rs`: `("rust", &["rs"])` in
      `BUILTIN_ALIASES`; `("rust", crate::langs::rust::build)` in
      `BUILTIN_BUILDERS`.
- [x] Run `cargo nextest run -p quarto-highlight`; accept the new
      `integration__golden__rust.snap` via `cargo insta review` **after
      reading it** (report the span list in the commit message per the
      snapshot policy).

### Phase 3 — bookkeeping and verification

- [x] Update prose that enumerates the built-in set: the paragraph in
      `smoke-all/highlighting/01-builtin-python.qmd` ("The 14 built-in
      languages…" → 15, add `rust`/`rs`) and the README table row for
      the new fixture.
- [x] `claude-notes/research/syntax-highlighting-grammar-crate-scout.md`:
      add the Rust row (crate, version, symbol, commit SHA, query path,
      license).
- [x] `cargo build --workspace`, `cargo nextest run --workspace`. (Green via `cargo xtask verify` steps 3 and 5.)
- [x] **Full** `cargo xtask verify` (not `--skip-hub-build`): steps 1–7 green (wasm32 link OK); step 8 fails only on the pre-existing Node 26 `localStorage` failures (bd-lh30hlvd, reproduced on clean main); `test:wasm` (133) and `test:integration` (119) run separately, green.
      `quarto-highlight` is linked into `wasm-quarto-hub-client`, so the
      wasm32 C build of `scanner.c` is only exercised by the hub-build leg.
- [x] End-to-end: `cargo run --bin q2 -- render <probe>.qmd` with ```` ```rust ````
      and ```` ```rs ```` blocks; grep the HTML for
      `<pre class="sourceCode rust"` and `hl-function-macro`. Record the
      invocation and a snippet of the output below.
- [x] Preview path: rebuild WASM (`cd hub-client && npm run build:wasm`,
      `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`) and confirm
      the preview iframe highlights the same probe (stale-WASM trap, see
      CLAUDE.md § Verifying Rust changes in `q2 preview`).
- [x] Commit (branch `braid/bd-202u5bld-add-rust-grammar-quarto`); push awaits approval.

## Details

### Measured baseline (2026-09-08, main @ b7e7c96a4)

Probe (`probe.qmd`: a ```` ```rust ```` block, a ```` ```rs ```` block, and a
```` ```python ```` control block), rendered with
`cargo run --bin q2 -- render probe.qmd`. Output inspected:

```
<pre class="rust code-with-copy"><code>fn main() { let x: u32 = 1; println!(&qu…
<pre class="rs code-with-copy"><code>fn main() {}
<pre class="sourceCode python"…            ← control: 2 × hl-keyword spans
```

Neither Rust block gets the `sourceCode` class or any `hl-*` span; the
Python control block does. The class is passed through untouched because
`pick_first_resolvable_class` (`annotate.rs`) finds no registry entry for
`rust` or `rs`.

### Why no `resources/highlights/rust/`

The design plan's rule (2026-04-19, "Queries sourced from crate-exposed
`HIGHLIGHTS_QUERY` constants (drift-free)") applies: the crate exposes the
constant, so vendoring would only create a second copy to keep in sync.
Vendor only if we later need to patch the query (e.g. to add captures the
upstream lacks), and document the delta in the vendored file's header as
Julia's does.

### Out of scope

- `INJECTIONS_QUERY` (doc-comment markdown, `macro_invocation` token trees):
  the crate forbids injections by design (bd-v9pi6ty9). Not needed for
  first-class Rust highlighting.
- Any theme/legend additions: none required (see capture table above).
- Docs page listing supported languages: none exists today; if one is
  written later, lift the smoke fixtures per the README's note.

### Risks

- **wasm32 link of `scanner.c`**: low; same libc surface as bash. If it
  fails, the fix belongs in `crates/wasm-c-shim`, not in a cfg-gate on the
  grammar (all built-ins must be present on both targets so the render
  and preview paths agree).
- **Bundle size**: tree-sitter-rust's `parser.c` is large (Rust grammar is
  among the bigger ones). Note the WASM size delta in the verification
  step; the phase-3 plan tracked ~200–250 KB per grammar as acceptable.

### End-to-end verification (2026-09-08, worktree branch)

`cargo nextest run -p quarto-highlight`: 34 passed. Smoke fixture
`09-builtin-rust.qmd`: passed (all four regexes matched).

Same probe as the baseline, rendered from the worktree with
`cargo run --bin q2 -- render probe.qmd`. Output inspected:

```
<pre class="sourceCode rust"      ← was: <pre class="rust code-with-copy"
<pre class="sourceCode rs"        ← was: <pre class="rs code-with-copy"
<span class="hl-keyword">fn</span><span class="hl-function">main</span>…
<span class="hl-type-builtin">u32</span> … <span class="hl-function-macro">println</span>
<span class="hl-function-macro">!</span>(<span class="hl-string">&quot;hi {x}&quot;</span>)
```

Upstream-query behaviours worth knowing (not bugs on our side):
integer literals are captured as `constant.builtin` (→ `Constant` colour);
`let`-bound identifiers and `->` carry no capture.

Preview path (fresh WASM: `npm run build:all` via verify step 7, then
`cargo xtask build-q2-preview-spa`, `cargo build --bin q2`):
`q2 preview --no-browser --port 47311 probe.qmd`, loaded in headless
Chromium via Playwright (the Chrome MCP extension was not connected).
The preview iframe's DOM was inspected:

```
<pre class="sourceCode rust"><code class="sourceCode rust">[keyword:fn] [function:main]… [type-builtin:u32] …
<pre class="sourceCode rs"><code class="sourceCode rs">[keyword:fn] [function:main]…
```

Screenshot confirmed the default palette colours the tokens (keywords,
function names, `u32`, the literal, `println!`, the string).
