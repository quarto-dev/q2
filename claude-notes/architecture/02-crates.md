# Diagram 2 — Crate & Package Map

**SVG:** [`crates.svg`](./crates.svg) · **Set index & conventions:** [`README.md`](./README.md)

Companion diagrams: [Render pipeline](./01-pipeline.md) ·
[hub-client Automerge structure](./03-hub-client-automerge.md) ·
[q2 vs hub-client (build & WASM)](./04-q2-preview-wasm.md).

---

## How to read this

Same three-tier drill-down as the rest of the set
(**diagram → guide → source**). The SVG groups the workspace into a handful of
**subsystems** (labeled bands) and shows the principal dependency direction; it
does *not* draw every crate-to-crate edge. This guide lists the full adjacency
so any crate name in the diagram can be traced to its `crates/<name>/`
directory. Numbered markers ①② in the SVG point to the [Notes](#notes).

The dependency data here is taken from `cargo metadata` (authoritative), not
hand-maintained.

## At a glance

- **45 Rust crates** in the main workspace (`crates/*`).
- **3 WASM-only crates outside the workspace** — `wasm-quarto-hub-client`,
  `wasm-qmd-parser`, and a `tree-sitter-language` shim — excluded because they
  build to `wasm32`/`cdylib` with a separate toolchain (see Note ①).
- **TypeScript** lives in npm workspaces: `ts-packages/*` (9 packages) plus the
  apps `hub-client`, `q2-preview-spa`, `trace-viewer`, and `q2-demos/*`.

## Subsystems (the bands)

Ordered consumers → foundation. ★ marks the highest-fan-in "hub" crates.

| Subsystem | Crates | Role |
|---|---|---|
| **Binaries** (native entry points) | `quarto` (the `q2` bin) ★, `hub`, `pampa` (bin), `qmd-syntax-helper`, `validate-yaml`, `perf-harness`, `reconcile-viewer`, `xtask` | user-facing commands & dev tools |
| **CLI features & LSP** | `quarto-preview`, `quarto-publish`, `quarto-test`, `quarto-trace-server`, `quarto-project-create`, `quarto-hub` (lib), `quarto-lsp`, `quarto-lsp-core` | per-command feature crates; language server |
| **Engine & orchestration** | `quarto-core` ★, `pampa` ★ | `pampa` = qmd parser + writers + filters; `quarto-core` = pipeline orchestrator (see [diagram 1](./01-pipeline.md)) |
| **Document features** | `quarto-doctemplate`, `quarto-citeproc`, `quarto-csl`, `quarto-highlight`, `quarto-sass`, `quarto-config`, `quarto-navigation`, `quarto-analysis` | domain libraries the engine composes |
| **AST & types** | `quarto-pandoc-types` ★, `quarto-ast-reconcile`, `comrak-to-pandoc` | the Pandoc AST definition + AST diff/convert |
| **Parsing & syntax** | `tree-sitter-qmd`, `tree-sitter-doctemplate`, `quarto-treesitter-ast`, `quarto-yaml`, `quarto-yaml-validation`, `quarto-parse-errors`, `quarto-xml` | grammars, tokenization, structured parse errors |
| **Foundation** | `quarto-source-map` ★, `quarto-error-reporting`, `quarto-util`, `quarto-trace`, `quarto-system-runtime`, `quarto-highlight-encoding`, `quarto-brand`, `quarto-error-message-macros` | shared infra; roots of the DAG |
| **WASM build** *(outside `[workspace]`)* | `wasm-quarto-hub-client` (cdylib), `wasm-qmd-parser` (cdylib+rlib), `wasm-printf-fmt`, `tree-sitter-language` (shim), `lua-src` (wasm), `wasm-bindgen-futures` (patch) | recompile the engine to `wasm32` for the browser |

**The hubs.** `quarto-source-map` is the universal foundation — almost every
crate depends on it. `pampa` is the parser hub (largest fan-out: 17 workspace
deps) consumed by `quarto-core`, `quarto-lsp-core`, both WASM crates,
`qmd-syntax-helper`, `perf-harness`, `reconcile-viewer`. `quarto-core` is the
orchestration hub, consumed by `quarto`, `quarto-preview`, `quarto-publish`,
`quarto-test`, `quarto-lsp-core`, `perf-harness`, and `wasm-quarto-hub-client`.
`quarto-system-runtime` is the **native/WASM I/O seam** (filesystem vs. VFS) —
see [diagram 4](./04-q2-preview-wasm.md).

## Full workspace adjacency (the source map)

`crate [targets] → intra-workspace dependencies`. Each crate lives at
`crates/<name>/`.

```
comrak-to-pandoc      [lib]      → pampa, quarto-pandoc-types, quarto-source-map
pampa                 [bin,lib]  → comrak-to-pandoc, quarto-ast-reconcile, quarto-citeproc,
                                   quarto-config, quarto-csl, quarto-doctemplate,
                                   quarto-error-message-macros, quarto-error-reporting,
                                   quarto-highlight-encoding, quarto-pandoc-types,
                                   quarto-parse-errors, quarto-source-map,
                                   quarto-system-runtime, quarto-treesitter-ast,
                                   quarto-util, quarto-yaml, tree-sitter-qmd
perf-harness          [bin]      → pampa, quarto-core, quarto-system-runtime
qmd-syntax-helper     [bin,lib]  → pampa, quarto-error-reporting
quarto (q2)           [bin]      → pampa, quarto-core, quarto-doctemplate, quarto-error-reporting,
                                   quarto-hub, quarto-lsp, quarto-preview, quarto-publish,
                                   quarto-sass, quarto-source-map, quarto-system-runtime,
                                   quarto-test, quarto-trace, quarto-trace-server, quarto-util
quarto-analysis       [lib]      → quarto-error-reporting, quarto-pandoc-types, quarto-source-map
quarto-ast-reconcile  [lib]      → quarto-pandoc-types, quarto-source-map
quarto-brand          [lib]      → (none)
quarto-citeproc       [bin,lib]  → quarto-csl, quarto-error-reporting, quarto-pandoc-types,
                                   quarto-source-map, quarto-xml
quarto-config         [lib]      → quarto-error-reporting, quarto-pandoc-types,
                                   quarto-source-map, quarto-yaml
quarto-core           [lib]      → pampa, quarto-analysis, quarto-ast-reconcile, quarto-config,
                                   quarto-doctemplate, quarto-error-reporting, quarto-highlight,
                                   quarto-navigation, quarto-pandoc-types, quarto-sass,
                                   quarto-source-map, quarto-system-runtime, quarto-trace,
                                   quarto-util, quarto-yaml
quarto-csl            [lib]      → quarto-error-reporting, quarto-source-map, quarto-xml
quarto-doctemplate    [lib]      → quarto-error-reporting, quarto-parse-errors, quarto-source-map,
                                   quarto-treesitter-ast, tree-sitter-doctemplate
quarto-error-reporting[lib]      → quarto-source-map
quarto-highlight      [lib]      → quarto-highlight-encoding, quarto-pandoc-types, quarto-source-map
quarto-hub            [bin,lib]  → quarto-util
quarto-lsp            [lib]      → quarto-lsp-core
quarto-lsp-core       [lib]      → pampa, quarto-analysis, quarto-core, quarto-error-reporting,
                                   quarto-pandoc-types, quarto-source-map,
                                   quarto-system-runtime, quarto-yaml
quarto-navigation     [lib]      → quarto-config, quarto-pandoc-types, quarto-source-map
quarto-pandoc-types   [lib]      → quarto-source-map
quarto-parse-errors   [lib]      → quarto-error-message-macros, quarto-error-reporting, quarto-source-map
quarto-preview        [lib]      → pampa, quarto-core, quarto-error-reporting, quarto-hub,
                                   quarto-pandoc-types, quarto-source-map, quarto-system-runtime,
                                   quarto-trace
quarto-project-create [lib]      → quarto-system-runtime
quarto-publish        [lib]      → quarto-config, quarto-core, quarto-error-reporting,
                                   quarto-pandoc-types, quarto-source-map,
                                   quarto-system-runtime, quarto-util
quarto-sass           [lib]      → quarto-brand, quarto-pandoc-types, quarto-source-map,
                                   quarto-system-runtime
quarto-source-map     [lib]      → (none)
quarto-system-runtime [lib]      → wasm-bindgen-futures
quarto-test           [lib]      → quarto-core, quarto-error-reporting, quarto-system-runtime
quarto-trace          [lib]      → (none)
quarto-trace-server   [lib]      → quarto-trace
quarto-treesitter-ast [lib]      → (none)
quarto-util           [lib]      → (none)
quarto-xml            [lib]      → quarto-error-reporting, quarto-source-map
quarto-yaml           [lib]      → quarto-source-map
quarto-yaml-validation[lib]      → quarto-error-reporting, quarto-source-map, quarto-yaml
reconcile-viewer      [bin]      → pampa, quarto-ast-reconcile, quarto-pandoc-types, quarto-source-map
tree-sitter-doctemplate[lib]     → (none)
tree-sitter-qmd       [bin,lib]  → (none)
validate-yaml         [bin]      → quarto-error-reporting, quarto-source-map, quarto-yaml,
                                   quarto-yaml-validation
xtask                 [bin]      → (none)

# outside the main workspace (WASM build):
wasm-quarto-hub-client[cdylib]   → pampa, quarto-ast-reconcile, quarto-core, quarto-error-reporting,
                                   quarto-highlight, quarto-lsp-core, quarto-pandoc-types,
                                   quarto-project-create, quarto-sass, quarto-source-map,
                                   quarto-system-runtime, quarto-trace, wasm-printf-fmt
wasm-qmd-parser       [cdylib,rlib] → pampa
```

## TypeScript / web (npm workspaces)

Packages in `ts-packages/*`; `name → @quarto/* deps`:

```
@quarto/pandoc-types          → (none)            # AST types, mirror of the Rust types
@quarto/mapped-string         → (none)            # source-mapped strings
@quarto/annotated-qmd         → mapped-string, pandoc-types
@quarto/quarto-automerge-schema → (none)          # the project-as-CRDT schema (see diagram 3)
@quarto/quarto-sync-client    → quarto-automerge-schema
@quarto/preview-renderer      → preview-runtime, quarto-automerge-schema
@quarto/preview-runtime       → pandoc-types, preview-renderer, quarto-automerge-schema, quarto-sync-client
@quarto/hub-mcp               → quarto-automerge-schema, quarto-sync-client
@quarto/sync-test-harness     → quarto-automerge-schema, quarto-sync-client
@quarto/wasm-js-bridge        → (none)
```

Apps (`name → key deps`):

```
hub-client     → @automerge/automerge-repo(+network-websocket,+react-hooks,+storage-indexeddb),
                 @quarto/quarto-automerge-schema, @quarto/quarto-sync-client      # collaborative editor
q2-preview-spa → @quarto/preview-renderer, @quarto/preview-runtime                # embedded in the q2 binary
trace-viewer   → (standalone trace visualizer)
```

**The Rust↔TS bridge.** The compiled `wasm-quarto-hub-client` `.wasm` is loaded
by `@quarto/preview-runtime` (`wasmRenderer.ts`) and directly by `hub-client`\'s
services. That is the seam where the Rust engine enters the browser; both the
collaborative editor and the embedded `q2 preview` SPA render through it. See
[diagram 3](./03-hub-client-automerge.md) (Automerge + WASM preview) and
[diagram 4](./04-q2-preview-wasm.md) (build chain & embedding).

---

## Notes

### ① Three WASM crates live outside the main workspace — *amber*

`wasm-quarto-hub-client`, `wasm-qmd-parser`, and the `tree-sitter-language`
shim are **not** members of the root `[workspace]` (confirmed via
`cargo metadata`: 45 members, none of these three). They build to
`wasm32-unknown-unknown` as `cdylib` with a separate toolchain/flags and pull
in patched dependencies (`lua-src` for wasm, a `wasm-bindgen-futures` patch),
which is incompatible with being ordinary workspace members. The diagram shows
them in a distinct "WASM build" band to make the boundary explicit.
→ `crates/wasm-quarto-hub-client/`, `crates/wasm-qmd-parser/`, root `Cargo.toml`.

### ② `preview-renderer` ⇄ `preview-runtime` is a mutual dependency — *detail*

`@quarto/preview-renderer` depends on `@quarto/preview-runtime` **and**
vice-versa. They are split by concern (React rendering vs. WASM/sync runtime)
but co-evolve; treat them as one unit when reasoning about the preview layer.
→ `ts-packages/preview-renderer/package.json`,
`ts-packages/preview-runtime/package.json`.
