# Plan 2B — q2-preview Pandoc base + framework + asset manifest (Session A)

**Date:** 2026-05-04 (revised 2026-05-07, 2026-05-09; split into 2B+2C on 2026-05-09)
**Branch:** feature/q2-preview
**Status:** Implementation plan
**Milestone:** **M2** — q2-preview reaches visual parity with the HTML format for documents that use **Pandoc base types** (Para, Header, lists, images, links, code, tables, math, footnotes, …). Quarto-specific custom-node renderers (callouts, theorems, proofs, cross-refs, figures-as-FloatRefTarget, equations) ship in **Plan 2C (M2.5)**.

## Goal

Fill q2-preview's empty registry (created by Plan 2A) with real-HTML leaf components for every Pandoc Block / Inline, plus the framework-level plumbing those leaves depend on, plus the asset-manifest pipe that delivers image bytes from VFS to the iframe. Visual + structural parity is the target: q2-preview should render the same Bootstrap-styled HTML (same elements, same classes, same nesting where possible) as the HTML pipeline, so the compiled theme CSS produces visually-identical output. See §Design decisions "Visual + structural parity target" below for the contract.

- **q2-preview's built-in registry of Pandoc primitives** — every Pandoc base type rendered as real HTML (Para → `<p>`, Header → `<h1>`-`<h6>`, BulletList → `<ul>`, **Image → `<img>`**, **Figure → `<figure>` + `<figcaption>`**, etc.). Includes Pandoc gap fills (LineBlock, DefinitionList, Table family, Underline, Strikeout, Superscript, Subscript, SmallCaps, Cite, RawInline, Note).
- **Framework: atomic-aware gate** — framework's `Node` component (in `framework/dispatch.tsx`) gains a gate that no-ops `setLocalAst` for atomic content (Derived source_info, atomic Synthetic kinds, atomic CustomNode types). Located at the single recursion chokepoint, before each format's `Block`/`Inline` dispatcher receives `args`. Benefits both q2-debug and q2-preview automatically — neither format's dispatcher needs modification.
- **Framework: unwrap / rewrap walks** — `framework/customNode.ts` translates between wire-format wrapper Divs/Spans and JS-native `CustomBlockNode` / `CustomInlineNode` shapes. Both formats can consume. Plan 2C's CustomNode components consume the JS-native shapes; 2B ships the plumbing without per-type renderers, so CustomNode wrappers in the AST fall through to dispatchers.tsx's "(not yet implemented)" muted-gray placeholder until 2C lands.
- **Asset manifest plumbing** — parent-side walker collects Image paths, reads VFS bytes, mints blob URLs, and ships a manifest of `{ origPath → blobUrl }` to the iframe alongside each `UPDATE_AST`. Iframe-side `<Image>` is a pure manifest consumer.
- **Pipeline inclusions** — q2-preview pipeline includes `FootnotesTransform` and `AppendixStructureTransform` (two-line `pipeline.rs` change). Both produce Pandoc primitives that 2B's leaves render natively. `TitleBlockTransform` stays excluded; deferred to a follow-up plan.
- **Stub class-name constants** — `quartoClasses.ts` ships with `SECTION` / `SECTION_LEVEL_PREFIX` / footnotes / appendix constants only. Plan 2C extends with the callout / theorem / proof / crossref taxonomy.

q2-preview's leaf components ship as part of the **built-in registry**, not as drafts pasted into demos. The render-components override mechanism (Plan 2A item 13) still works for users who want to override q2-preview leaves; the built-ins are simply the default registry.

Elliot's existing `~/docs/demo-playground/elliot/html.tsx` is the seed for q2-preview's built-in real-HTML leaves. 2B's work is to fill its base-type gaps, port it from `__REACT_AST_DEBUG_RENDERER__` to `__Q2_PREVIEW_RENDERER__`, and ship it as `q2-preview/blocks/` and `q2-preview/inlines/` rather than as a single user file. Plan 2C handles the demo fork.

## Checklist

Phases mirror the §"Estimated scope" Session A / Session B partitioning. Each item is a coherent commit-or-small-cluster of work; expand into sub-items if scope grows during implementation (per CLAUDE.md "Add new items if you discover additional work"). Cross-references point at the design-detail sections under §Scope below.

### Phase 1 — Framework changes (`framework/`)

- [x] **1.1** `framework/customNode.ts` (NEW) + `framework/Ast.tsx` co-edits — implement `unwrapCustomNodes` / `rewrapCustomNodes` walks per §"`framework/customNode.ts` — unwrap / rewrap walks" (structural JSON traversal; full block/inline wire-format asymmetry; mirrors `crates/pampa/src/{writers,readers}/json.rs::{write,read}_custom_{block,inline}`). Single commit also extends `framework/Ast.tsx`: (a) call `unwrapCustomNodes(ast)` after `JSON.parse`; (b) extract `astContext.sourceInfoPool` onto the `RegistryContext.Provider` value (Plan 2A item 4 typed the field but didn't fill it); (c) add a discriminated input — `Ast` accepts either `astJson: string` *or* `ast: PandocAST`, skipping the internal `JSON.parse` when `ast` is provided. The discriminated input is consumed by 2B's Note-numbering walk in `PreviewRoot` (avoids a double-parse for note numbering — see §"`Note.tsx` becomes a JS-side number-with-tooltip-body fallback"). Includes round-trip property tests in `framework/customNode.test.ts` (node env): six concrete `type_name`s (`Callout`, `Theorem`, `Proof`, `FloatRefTarget`, `Equation`, `CrossrefResolvedRef`) plus `IncludeExpansion` placeholder; both `unwrap(rewrap(x)) ≡ x` and `rewrap(unwrap(wireDiv)) ≡ wireDiv` directions; explicit inline-CustomNode case to exercise the no-Plain-wrapper-on-inline-side asymmetry.
- [x] **1.2** `framework/types.ts` — add concrete `CustomBlockNode` / `CustomInlineNode` / `Slot` / `CustomNodeBase` shapes per §"`framework/types.ts` — concrete CustomNode shapes" (Plan 2pre added `MathInline` but did not stage placeholders for these — they are new).
- [x] **1.3** `framework/dispatch.tsx` — atomic-aware gate inside `Node` + `CustomBlock` / `CustomInline` entries in `renderChildrenRegistry` (consumed by `Fallback`, not by per-type components — see §"CustomBlock / CustomInline traversal") + extend `blockTypes` from 11 to **19 entries** (additions: `LineBlock`, `DefinitionList`, `Table`, `CustomBlock`, `BlockMetadata`, `NoteDefinitionPara`, `NoteDefinitionFencedBlock`, `CaptionBlock`). Per §"atomic-aware gate inside `Node`" and §"CustomBlock / CustomInline traversal" → "`blockTypes` extension". Includes atomic-detection tests covering all three convergence paths (Derived source_info, atomic Synthetic kinds, atomic CustomNode types).

### Phase 2 — Asset manifest plumbing

*Plan 2A is fully landed (commits `fe40973b` + theme follow-ups). Items 2.2 and 2.3 extend the existing `Q2PreviewIframe.tsx` and `entry.tsx`; no cross-plan ordering risk.*

- [x] **2.1** `hub-client/src/utils/vfsPaths.ts` (NEW) — extract `resolveRelativePath`, `normalizePath`, `guessMimeType` from three existing private copies (`iframePostProcessor.ts:329, :343, :356`; `iframeLinkHandlers.ts:116, :123` — `resolveRelativePath` + `normalizePath` only, no `guessMimeType`; `ReactAstSlideRenderer.tsx:886, :900, :913` — all three); migrate the call sites to import from the new module. Then implement `q2-preview/assetWalker.ts::buildAssetManifest` per §"Parent-side asset walker" (collect Image paths, resolve via `vfsPaths.ts`, VFS read, base64-string-keyed cache, mint/reuse blob URLs, return revocations). Two commits: helper extraction first (pure refactor), walker second. Includes `assetWalker.test.ts` covering cache hit, content-change eviction with revocation, image removal, external-URL skipping, VFS-read failure, and an N=100 stress case.
- [x] **2.2** `q2-preview/AssetManifestContext.tsx` (new) + `Q2PreviewIframe.tsx` walker integration — cache ref, `useMemo` keyed on `astJson + currentFilePath`, extend `UPDATE_AST` payload to `{ astJson, currentFilePath, assetManifest }`, unmount cleanup. Includes `Q2PreviewIframe` integration test asserting payload contents and unmount revocation. Per §"Manifest distribution: rides on `UPDATE_AST`" and §"`q2-preview/AssetManifestContext.tsx`".
- [x] **2.3** `q2-preview/entry.tsx` — destructure `assetManifest` from `UPDATE_AST` payload and forward as a `PreviewRootProps` field; `PreviewRoot` wraps `<Ast>` with `<AssetManifestContext.Provider>` alongside the existing `<PreviewContext.Provider>`. Call `rewrapCustomNodes(newAst)` inside the `setAst` callback before `postMessage({ type: 'SET_AST' })`. (Forward unwrap lives in `framework/Ast.tsx` per item 1.1, not here.) Per §"Update q2-preview entry to call rewrap before `SET_AST`".

### Phase 3 — q2-preview leaf components

- [ ] **3.1** `q2-preview/utils.ts` — `lookupAssetUrl`, `inlinesToPlainText`, `blocksToPlainText` per §"`q2-preview/utils.ts` — shared component utilities". Required by Image (lookupAssetUrl), alt-text Stringify (inlinesToPlainText), and Note.tsx tooltip-body (blocksToPlainText). Plan 2C extends with `formatRefLabel`, `composeAttr`, `renderSlot` for its CustomNode components.
- [ ] **3.2** `q2-preview/blocks/*.tsx` — 14 Pandoc Block components (11 existing-pattern from Elliot's `html.tsx` + 3 gap fills: LineBlock, DefinitionList, Table family). Includes `Figure.tsx` per §"`q2-preview/blocks/Figure.tsx` — `<figure>` + `<figcaption>`" (renders body via `<Block />`, reads `c[1][1]` directly for caption). Plus `q2-preview/blocks/index.ts` barrel.
- [ ] **3.3** `q2-preview/inlines/*.tsx` — 20 Pandoc Inline components (12 existing-pattern + 8 gap fills: Underline, Strikeout, Superscript, Subscript, SmallCaps, RawInline, Cite, Note). Includes `Image.tsx` per §"`q2-preview/inlines/Image.tsx` — full Pandoc semantics" (manifest consumer via `useContext(AssetManifestContext)`, no `vfsReadBinaryFile` in iframe) and `Math.tsx` per §"`q2-preview/inlines/Math.tsx` — KaTeX leaf" (near-verbatim port of Elliot's `html.tsx:259–279` with two divergences). Plus `q2-preview/inlines/index.ts` barrel.

### Phase 4 — (moved to Plan 2C)

CustomNode components, full `quartoClasses.ts` taxonomy, registry assembly with `CustomNodeRegistryContext`, and the demo fork are all in Plan 2C. After 2B lands, CustomNode wrappers in the AST (Callout, Theorem, etc.) fall through to `dispatchers.tsx`'s "(not yet implemented)" muted-gray placeholder — same UX as the current Plan-2A state for those nodes. Pandoc base types render correctly.

### Phase 5 — Verification (Session A scope only)

- [ ] **5.1** Pandoc base-type integration tests — vitest under `q2-preview.integration.test.tsx` (jsdom). One test per new gap-fill component; plus Figure body recursion, Image edge cases (manifest hit / external pass-through / `data:` pass-through / manifest miss / width-height kvs / id-classes-title / alt-text via Stringify), atomic CustomNode read-only (without per-type components — assert the gate fires for `CrossrefResolvedRef` even though it falls through to the muted-gray placeholder), Derived inline read-only, recursion-contract bypass (see §"Recursion contract for the atomic gate"), reference-preservation property test for `unwrapCustomNodes`, class-compatibility for the stub constants. Per §"Vitest integration tests" and §"Pandoc base-type gap-fill tests".

  **q2-debug atomic-gate parity test (mandatory).** The atomic gate sits in framework's `Node` and benefits q2-debug "for free" — but `q2-debug.integration.test.tsx` today (8 tests, verified) has zero atomic-gate coverage. Add at least one assertion to `q2-debug.integration.test.tsx` that mounts a Para containing a `CrossrefResolvedRef` CustomInline and asserts the child's `setLocalAst` is the no-op sentinel (or that an attempted edit does not call the spy). This locks the framework-level guarantee at the q2-debug surface so a future regression in the gate (or a future reorganization that accidentally moves the gate downstream of one format's dispatcher) fails on both formats' integration tests, not just q2-preview's. ~20 LOC.

  **Test replacement note.** `q2-preview.integration.test.tsx` already exists (Plan 2A, ~80 LOC) and locks the *empty-registry* placeholder contract. Four of its existing tests assert on Plan-2A-shape behavior that 2B replaces:
  - "renders a top-level block as a muted-gray placeholder" — Para now renders as real `<p>`. **Replace** with a real-render assertion.
  - "recurses into children so nested inlines also surface placeholders" — Str now renders as text. **Replace** with an inline-render assertion.
  - "uses the muted-gray aesthetic on the placeholder DOM" — still applies for unregistered CustomBlock/CustomInline (Plan 2C populates the registry). **Narrow** the test to assert the muted-gray aesthetic specifically for an unregistered CustomNode type (e.g. `Callout`).
  - "renders registry containing only {Ast, Block, Inline}" — registry grows to ~30 keys after 2B. **Replace** with an assertion on registry shape (registry contains the expected Pandoc base tags; `CustomBlock`/`CustomInline` keys are NOT yet present — those are 2C).
  Append the new component tests after the replacements; do not let stale Plan-2A assertions linger as commented-out code or skip-it blocks.
- [ ] **5.2** Smoke-all q2-preview asset-manifest fixture — `crates/quarto/tests/smoke-all/q2-preview/image-with-attrs.qmd` (single-doc) + a real PNG asset committed as a sibling file. Single Image with `![alt](hero.png){width=400}`. Asserts `<img>` rendered with `src^="blob:"` (substring match for the blob-URL prefix produced by the parent walker — Playwright's `toHaveAttribute` with a regex), `width="400"`, and the alt-text content. Frontmatter shape (`_quarto.tests.run.requires_js: true`, `_quarto.tests.q2-preview.ensureHtmlElements`) per §"Smoke-all q2-preview fixture". This is the smoke-level safety net for the asset-manifest end-to-end path; multi-element fixtures (callout / theorem / project-mode multi-element / with-render-components override) move to Plan 2C since they exercise CustomNode rendering. (The `PreviewIframeKind = 'q2-preview'` smoke-all infrastructure landed in Plan 2A item 12 — `previewExtraction.ts:23`.)
- [ ] **5.3** WASM integration test (project-mode safety net) — `assetManifestProject.wasm.test.ts` per §"WASM integration tests" in the test plan. Mirrors `themeFingerprint.wasm.test.ts` (Plan 2A) at the WASM-bridge layer; renders a `_quarto.yml`-rooted project doc with a `![](hero.png)` Image and asserts the parent walker resolves the path correctly against the project's `currentFilePath`. Catches default-project `currentFilePath` resolution bugs analogous to Plan 2A's theme path mismatch. `themeFingerprint.wasm.test.ts` itself must be preserved when `pass2_renderer.rs` is touched. (`customNodeWireFormatProject.wasm.test.ts` moves to Plan 2C — the wire-format unwrap path only matters once 2C's CustomNode renderers consume it.)
- [ ] **5.4** Run **`cargo xtask verify --e2e`** before declaring 2B complete. Default `cargo xtask verify` skips the Playwright runner (`--e2e` is opt-in per project CLAUDE.md), so the `image-with-attrs.qmd` fixture landed in 5.2 is *not* exercised by the standard verify flow. Without this step the iframe boot path, blob-URL minting through the real VFS, and the manifest postMessage round-trip all go untested at the integration layer. Also do a manual browser session against a running hub for sanity (per project CLAUDE.md "End-to-end verification before declaring success"); record the invocation and an inspected-output snippet in the implementation transcript or the plan's checklist comments. **Visual verification by the user is the M2 acceptance gate** — the user reviews the rendered output of a multi-paragraph document with images, footnotes, and an appendix license metadata before signing off.

## Scope

### In scope

#### Framework changes (apply to both formats)

##### `framework/customNode.ts` — unwrap / rewrap walks

Pure functions, no React, no context:

```ts
export function unwrapCustomNodes(ast: PandocAST): PandocAST;
export function rewrapCustomNodes(ast: PandocAST): PandocAST;
```

**Walk strategy: structural JSON-level traversal.** Both walks recursively visit every JSON object/array in the AST. The wrapper-detection criterion is purely structural (`t === 'Div' | 'Span'` AND classes contains `'__quarto_custom_node'`), so the walks do not need an AST-shape dispatch table — `Para.c`, `Header.c[2]`, `BulletList.c[i][j]`, slot contents, etc. are all reached by descending into every `c` field encountered. This avoids duplicating the per-tag knowledge already in `framework/dispatch.tsx`'s `renderChildrenRegistry`.

**Walker scope: only `c` fields.** The walker descends into `c` fields exclusively. It does *not* recurse into `plain_data` (which is a JSON-stringified value inside the `data-custom-data` kv at unwrap time, and a parsed JS value attached to the JS-native CustomNode shape afterward). All current `plain_data` producers emit flat shapes — primitives, arrays of primitives, plain objects whose values are primitives or `{ section: usize[], order: usize }`-style records (verified against `callout.rs:210`, `theorem.rs:282-285`, `proof.rs:145`, `float_ref_target.rs:292,323`, `equation_label.rs:215-217`, `crossref/index.rs::Order`, `crossref_resolve.rs:294-314`). No producer stores AST-shaped sub-objects. If a future producer wants to embed AST in `plain_data`, that producer is responsible for documenting that the embedded AST will *not* be unwrapped (since walking `plain_data` would also break round-trip rewrap). This invariant is checked by the inline-CustomNode round-trip property test — if it ever needs adjustment, the failure mode will be visible in the test rather than silent.

###### Walker purity contract (load-bearing for Note WeakMap)

`unwrapCustomNodes` is a pure function with one critical implementation invariant: **subtrees that contain no `__quarto_custom_node` wrappers anywhere underneath are returned by reference, not deep-copied.** Only objects on the path from the AST root to a transformed wrapper get rebuilt; everything off that path is the same JS object the parser produced.

Concretely:

- A leaf node (`{t: 'Str', c: 'foo'}`) is returned by reference.
- A `Para` whose `c` array contains no CustomNode wrappers is returned by reference (and so is its `c` array).
- A `Para` whose `c` array contains one `Span(__quarto_custom_node)` is rebuilt: a new `Para` object with a new `c` array, where the wrapper is replaced by a fresh `CustomInlineNode` and the *other* siblings are still the original references.
- A `Note` inline anywhere under a non-wrapper ancestor is returned by reference, regardless of how deep.

This invariant is what makes the Note-numbering `WeakMap<NoteInline, number>` work: PreviewRoot's pre-unwrap walk (see §"`Note.tsx` becomes a JS-side number-with-tooltip-body fallback") records `Note` references that the eventual `<Ast>` render-time lookup must still find by identity. A naïve "walk and rebuild every container" implementation would break the lookup silently — every `Note` would be a fresh object post-unwrap and the WeakMap would miss everywhere.

**Implementation pattern**: standard structural-sharing recursion. Walk; for each node decide if it (or any descendant) is a wrapper; if neither, return the node by reference; otherwise build a new node with new children, recursing per-child and reusing references for non-rewriting branches. The "any descendant" check is implicit in the recursive return value — if every recursive call returns its input by reference, the parent can return its input too.

```ts
// Sketch (not the full algorithm):
function unwrapNode(node: any): any {
  if (isWrapper(node)) return decodeWrapper(node);  // always fresh
  if (!hasArrayChildren(node)) return node;          // leaf — by reference
  const original = node.c;
  let next = original;
  for (let i = 0; i < original.length; i++) {
    const child = original[i];
    const unwrappedChild = unwrapNode(child);
    if (unwrappedChild !== child) {
      if (next === original) next = original.slice();  // copy-on-write
      next[i] = unwrappedChild;
    }
  }
  return next === original ? node : { ...node, c: next };
}
```

The copy-on-write pattern keeps the invariant robust: as soon as any child changed, we clone the array; otherwise the array (and the parent node) are returned unchanged. Nested arrays (`BulletList.c[i][j]`, `Table` cell matrix) follow the same pattern recursively.

**Round-trip property test extension**: in addition to the existing `unwrap(rewrap(x)) ≡ x` and `rewrap(unwrap(wireDiv)) ≡ wireDiv` assertions, add a **reference-preservation test**: build a wire AST whose only CustomNode is at depth 3 inside one branch, capture references to leaves in unrelated branches, run unwrap, and assert the unrelated-branch leaves are `===` to the captured references. Locks the contract as observable behavior.

###### Forward path (wire → JS-native)

`unwrapCustomNodes` is called inside `framework/Ast.tsx` immediately after `JSON.parse(astJson)` and before the registry dispatches (see §"`framework/Ast.tsx` — co-edits" below). It walks the AST once and replaces every wrapper Div / Span with a `CustomBlockNode` / `CustomInlineNode` shape. After the walk, the AST contains zero `__quarto_custom_node` references — the registry's `Div` / `Span` entries only ever see real Divs / Spans.

**Algorithm** (mirror of `crates/pampa/src/readers/json.rs::read_custom_block_from_div:2220` and `read_custom_inline_from_span:2358`):

1. **Detection**: a node is a custom-node wrapper iff `node.t === 'Div' || node.t === 'Span'` AND `node.c[0][1]` (the classes array) contains `'__quarto_custom_node'`.
2. **Metadata extraction** from `node.c[0][2]` (the kvs map):
   - `type_name`: `kvs['data-custom-type']`, default `'Unknown'`.
   - `slot_meta`: `JSON.parse(kvs['data-custom-slots'] ?? '{}')`. Maps slot name → kind string (`"Block" | "Inline" | "Blocks" | "Inlines"`).
   - `plain_data`: `JSON.parse(kvs['data-custom-data'] ?? 'null')`.
3. **Attr stripping**: build `attr` from the wrapper's `c[0]` minus the custom-node leakage:
   - `id` stays.
   - Filter `'__quarto_custom_node'` out of classes (preserves order of remaining classes).
   - Strip `data-custom-type`, `data-custom-slots`, `data-custom-data` from kvs (preserves order of remaining entries).
4. **Slot iteration** — walk `node.c[1]` (the wrapper's children). Each is a `Div` (block-wrapper) or `Span` (inline-wrapper) carrying `data-slot-name` in its kvs. Defensively `continue` on (mirroring reader at `:2278–2292` and `:2415–2430`):
   - Wrong tag (non-`Div` in a block wrapper, non-`Span` in an inline wrapper).
   - Missing `data-slot-name`.
   - Malformed `c` shape.
5. **Per-slot decoding** by kind. Look up the kind in `slot_meta`; default to `'Blocks'` for block wrappers and `'Inlines'` for inline wrappers (mirrors `:2298` and `:2436`):
   - **Block wrapper, `Block` slot**: `slotContent[0]` (single block).
   - **Block wrapper, `Inline` slot**: `slotContent[0]` is a `Plain` block; take `slotContent[0].c[0]`. The `Plain` wrapper exists because `writers/json.rs:1340` emits `[{t:'Plain', c:[inline]}]` to keep slot content typed as blocks on the wire.
   - **Block wrapper, `Blocks` slot**: `slotContent` (the array as-is).
   - **Block wrapper, `Inlines` slot**: `slotContent[0]` is a `Plain`; take `slotContent[0].c`.
   - **Inline wrapper, `Inline` slot**: `slotContent[0]` (single inline). **No Plain wrapper to strip** — `writers/json.rs:1422` writes the inline directly into the slot `Span`'s content.
   - **Inline wrapper, `Inlines` slot**: `slotContent` (the array as-is). Again, no Plain wrapper.
   - **Inline wrapper, `Block` / `Blocks` slot**: degenerate case (Q-3-39 — block slot in inline custom node). Rust writer at `:1428` emits a placeholder `Str{c:'[block content]'}` and a diagnostic; round-trip is intentionally lossy. v1 unwrap mirrors the reader at `:2453` and treats the slot as `Inlines`.
6. **Recursion**: after decoding a slot's contents, recurse into them so nested CustomNodes (Plan 8 case) are unwrapped too. The structural JSON walk handles this automatically — the slot-decoded value is just another subtree the walker keeps descending.
7. **Output shape**:
   ```ts
   {
     t: 'CustomBlock' | 'CustomInline',
     type_name, slots, plain_data, attr,
     s: <wrapper's source-info index, preserved>,
   }
   ```
   The wrapper's `s` field (if present) carries the CustomNode's source_info — set at `writers/json.rs:1373`. Inner slot wrapper Divs / Spans don't have `s`; they're synthetic and disappear at unwrap.

###### Reverse path (JS-native → wire)

`rewrapCustomNodes` is called in `q2-preview/entry.tsx`'s `setAst` callback (and in q2-debug's, if it ever grows interactive editing) just before `postMessage({ type: 'SET_AST' })`. It walks the JS-native AST and rewrites every `CustomBlockNode` / `CustomInlineNode` back to a wire-format Div / Span.

**Walker scope: descend both `c` fields and `slots` values.** After unwrap, the AST is a mixed shape: wire-format Pandoc nodes (`Para`, `Header`, etc.) carry their children in a `c` field; JS-native CustomNodes (`CustomBlock`, `CustomInline`) carry their children in `slots[name].value`. Rewrap must descend into both — `c` fields to find CustomNodes nested under wire-format ancestors, and `slots` values to find CustomNodes nested under other CustomNodes (Plan 8 case). The walker also does **not** recurse into `plain_data` (same rationale as unwrap — round-trippability requires plain_data to be opaque to walks).

**Walker purity contract: same as unwrap.** Subtrees containing no `CustomBlock`/`CustomInline` anywhere underneath are returned by reference. Only the path from root to a transformed JS-native CustomNode gets rebuilt. This isn't load-bearing for any existing consumer (rewrap output goes straight to `JSON.stringify` for postMessage), but it's cheap to maintain and lets future consumers that need referential stability rely on it.

**Algorithm** (mirror of `writers/json.rs::write_custom_block:1297` and `write_custom_inline:1381`):

1. **Detection**: `node.t === 'CustomBlock' || node.t === 'CustomInline'`.
2. **Slot metadata**: build `slot_meta` by mapping each slot's `kind` to its capitalised name (`'block' → 'Block'`, etc.).
3. **Wrapper attr**: clone `node.attr`, then:
   - Insert `'__quarto_custom_node'` at index 0 of classes (matches `:1329` and `:1413`).
   - Append to kvs (preserving insertion order — user kvs first, custom data-* last):
     - `'data-custom-type': type_name`
     - `'data-custom-slots': JSON.stringify(slot_meta)`
     - `'data-custom-data': JSON.stringify(plain_data)` — **only emitted when** `plain_data !== null && plain_data !== undefined`, mirroring Rust's `!is_null()` guard at `:1320`.
4. **Slot encoding** — for each `(name, slot)` in `node.slots`, emit a slot wrapper. The slot wrapper's attr is `('', [], { 'data-slot-name': name })` — empty id, empty classes, single kv:
   - **CustomBlock**, slot wrapper is a `Div`:
     - `Block` slot: `c: [wrapperAttr, [slot.value]]`.
     - `Inline` slot: `c: [wrapperAttr, [{ t:'Plain', c:[slot.value] }]]`.
     - `Blocks` slot: `c: [wrapperAttr, slot.value]`.
     - `Inlines` slot: `c: [wrapperAttr, [{ t:'Plain', c:slot.value }]]`.
   - **CustomInline**, slot wrapper is a `Span`:
     - `Inline` slot: `c: [wrapperAttr, [slot.value]]`.
     - `Inlines` slot: `c: [wrapperAttr, slot.value]`.
     - `Block` / `Blocks` slot: not round-trippable; v1 emits a placeholder `Str{c:'[block content]'}` (mirrors `:1438`).
5. **Outer wrapper**: emit `{ t: 'Div' | 'Span', c: [wrapper_attr, slot_wrappers], s: node.s }`.
6. **Recursion**: recurse into slot contents before encoding so nested JS-native CustomNodes are rewrapped first.

###### Order preservation

- **Classes**: `'__quarto_custom_node'` is always at index 0 of the wire wrapper's classes; user classes follow in their original order.
- **Kvs**: user kvs first (in their original order), then `data-custom-type`, `data-custom-slots`, `data-custom-data` appended last in that order. Plain JS objects with string keys preserve insertion order in modern engines (V8, JSC, SpiderMonkey), so `Record<string, string>` is sufficient — no `Map` needed.
- **Slots**: slot iteration order is preserved across unwrap → rewrap so `slot_meta` JSON and the slot wrapper sequence match the original wire format.

###### Round-trip property

For every CustomNode shape that appears in q2-preview's pipeline output:

- `unwrap(rewrap(jsNative)) ≡ jsNative` — JS-native fixed point.
- `rewrap(unwrap(wireDiv)) ≡ wireDiv` — wire-format fixed point (structural equality, not byte-identical: `JSON.stringify` may differ in whitespace from Rust's serializer, but the parsed shape is identical).

Rust-side anchors at `crates/pampa/src/writers/json.rs:3893` (block — Callout with Inlines+Blocks slots, plain_data object), `:3960` (inline — Tooltip with Inlines slot), `:4023` (preserves user attr — id, classes, kvs). JS-side property tests cover the same three plus the cases the Rust tests don't exercise: single-`Block` slot, single-`Inline` slot in a CustomBlock, empty slots, nested custom-in-custom (Plan 8 case).

###### `framework/Ast.tsx` — co-edits

Three minimal changes inside `Ast.tsx` are folded into the customNode.ts commit:

1. **Accept a discriminated input.** Today `Ast.tsx:35` does `ast = JSON.parse(astJson)` unconditionally. Change the props type to `{ astJson: string } | { ast: PandocAST }` (plus the existing `currentFilePath`, `onNavigateToDocument`, `setAst`, `registry` fields). Acquire the parsed AST as: `const parsed = 'ast' in props ? props.ast : JSON.parse(props.astJson)`. The error-pane try/catch stays around the `JSON.parse` branch only — the `ast` branch trusts the caller (which itself caught parse errors and fell back to the string path; see §"`Note.tsx`" below for the PreviewRoot consumer).
2. **Unwrap runs after parse acquisition, on both branches.** Immediately after the discriminated parse step, call `ast = unwrapCustomNodes(parsed)`. **The unwrap call is unconditional** — it runs on both the string-input and pre-parsed-input paths. PreviewRoot's pre-unwrap Note walk (which depends on the parsed-but-not-yet-unwrapped shape) hands `<Ast>` the still-wire-format object; `<Ast>` then unwraps it and the WeakMap-by-reference contract from §"Walker purity contract" preserves the Note references across the unwrap.
3. **Extract `astContext.sourceInfoPool` onto the `RegistryContext.Provider` value.** Today `Ast.tsx:47` provides `{ registry }` only; the `sourceInfoPool?` field on `RegistryContext` (typed by Plan 2A item 4 at `RegistryContext.tsx:21` but not yet filled — verified) needs to come from `(parsed as any).astContext?.sourceInfoPool`. Use the *parsed* AST for this — `astContext` lives on the wrapper object that `JSON.parse` produces and that PreviewRoot also passes through. The atomic gate in `Node` reads it via `useContext(RegistryContext).sourceInfoPool` — see §"atomic-aware gate inside `Node`" below.

All three edits live in the same commit so the discriminated-input plumbing, the unwrap call, and the pool wiring don't appear in inconsistent intermediate states. Estimated total: ~15 LOC.

**Order of operations in `Ast.tsx`** (single render path):

```
1. parsed   = 'ast' in props ? props.ast : JSON.parse(props.astJson)
2. pool     = parsed.astContext?.sourceInfoPool
3. unwrapped = unwrapCustomNodes(parsed)
4. <RegistryContext.Provider value={{ registry, sourceInfoPool: pool }}>
5.   <Node node={unwrapped.blocks ...} ... />
6. </RegistryContext.Provider>
```

Step 2 reads the pool *before* unwrap; the wrapper's `astContext` field is on the AST root and untouched by unwrap (which only rewrites `c`-field descendants), so reading either before or after is equivalent — the pre-unwrap read keeps the cost off the unwrapped tree's first descent. q2-debug stays on the string path (it never passes a pre-parsed `ast`); the discriminated input is opt-in.

###### q2-debug input assumption

The unwrap walk runs unconditionally inside framework's `Ast` — both formats see it. q2-debug today renders the **raw, pre-pipeline AST**, which never contains `__quarto_custom_node` wrappers (CustomNodes are produced by transforms in q2-preview's pipeline, not q2-debug's). Under that assumption, unwrap is a no-op for q2-debug and the unconditional placement is safe. **If that assumption ever changes** — e.g. q2-debug is ever pointed at post-pipeline AST — q2-debug's bordered-Div rendering would become bordered "Not registered: CustomBlock" instead, since q2-debug doesn't register CustomBlock/CustomInline. The fix at that point is to gate the unwrap call on format (move it from framework's `Ast` into each format's `'Ast'` registry component, where the format opts in). Documented here so the assumption is recoverable from the plan rather than buried in code.

###### setLocalAst → setAst handler

In q2-preview's entry (and q2-debug's, if/when it grows interactive editing):

```ts
setAst={(newAst) => {
  window.parent.postMessage({
    type: 'SET_AST',
    ast: rewrapCustomNodes(newAst),
  }, '*');
}}
```

##### `framework/types.ts` — concrete CustomNode shapes + missing Cite type

Add concrete `CustomBlockNode` / `CustomInlineNode` / `Slot` / `CustomNodeBase` shapes to `framework/types.ts` (Plan 2pre added `MathInline` to the inline union but did not stage placeholders for these — they are new). Also add `CiteInline` — verified absent from `framework/types.ts` today, even though Pandoc emits `Cite` whenever the source uses `@…` citation syntax:

```ts
// Pandoc shape: Cite [Citation] [Inline]
// c[0] is the Citations array (Pandoc metadata used for bibliography);
// c[1] is the visible inlines Pandoc fills in for the link text.
// v1 q2-preview renders c[1] only; c[0] is unstructured `unknown` because
// the Citation shape is not consumed today.
export interface CiteInline {
  t: 'Cite';
  c: [unknown[], InlineNode[]];
  s?: number;
}
```

Add `CiteInline` to the `InlineNode` union alongside the existing variants. The full Citation shape (id / prefix / suffix / mode / hash / etc.) is deferred until bibliography rendering ships; typing `c[0]` as `unknown[]` keeps the surface minimal without committing to a partial Citation type.

```ts
import type { Attr, BlockNode, InlineNode } from './types';

export type Slot =
  | { kind: 'block';   value: BlockNode }
  | { kind: 'inline';  value: InlineNode }
  | { kind: 'blocks';  value: BlockNode[] }
  | { kind: 'inlines'; value: InlineNode[] };

interface CustomNodeBase {
  type_name: string;
  slots: Record<string, Slot>;
  plain_data: unknown;
  attr: Attr;
  s?: number;
}

export interface CustomBlockNode extends CustomNodeBase { t: 'CustomBlock' }
export interface CustomInlineNode extends CustomNodeBase { t: 'CustomInline' }
export type CustomNode = CustomBlockNode | CustomInlineNode;
```

The `'CustomBlock'` / `'CustomInline'` discriminator (not a single `'Custom'` + `variant` field) is chosen because (a) the framework's `Node` dispatcher uses a hardcoded `blockTypes` array, and adding two distinct `t` values fits with a one-line addition; (b) block-vs-inline becomes a static type property; (c) round-trip is unambiguous.

##### `framework/dispatch.tsx` — atomic-aware gate inside `Node`

Plan 2pre's refined architecture moves `Block`/`Inline` dispatchers out of framework into format-owned files (`q2-debug/dispatchers.tsx`, `q2-preview/dispatchers.tsx`). Putting the atomic gate in either format's dispatcher would either duplicate the code or only protect one format. The cleaner home is framework's `Node` component (in `framework/dispatch.tsx`) — the single recursion chokepoint that runs *before* either format's dispatcher receives `args`.

`Node`'s body gains the gate:

```tsx
const NOOP = () => {};

const Node = ({ node, setLocalAst, onNavigateToDocument }: NodeProps) => {
  const ctx = useContext(RegistryContext);
  const pool = ctx.sourceInfoPool;

  const isAtomic = isAtomicSourceInfo(node, pool, ATOMIC_SYNTHETIC_KINDS)
                || ((node.t === 'CustomBlock' || node.t === 'CustomInline')
                    && isAtomicCustomNode(node.type_name));

  const effectiveSetLocalAst = isAtomic ? NOOP : setLocalAst;

  const isBlock = blockTypes.includes(node.t);
  const Dispatcher = ctx.registry[isBlock ? 'Block' : 'Inline'];
  if (!Dispatcher) {
    // Programmer error: format shipped a registry without 'Block'/'Inline'.
    // Both shipped formats register them; this branch never fires in normal flow.
    return <>{`Dispatcher not registered: ${isBlock ? 'Block' : 'Inline'}`}</>;
  }
  return <Dispatcher node={node} setLocalAst={effectiveSetLocalAst} onNavigateToDocument={onNavigateToDocument} />;
};
```

The format's `Block` / `Inline` dispatcher receives already-gated `args` and continues with its own `registry[node.t]` lookup unchanged — q2-debug's bordered-box leaves and q2-preview's real-HTML leaves both see a no-op `setLocalAst` for atomic content without any per-format awareness.

Three atomic detection paths converge into one gate:

1. **Derived source_info** (Plan 6's shortcode resolutions) — via `isAtomicSourceInfo`'s `isDerived` arm.
2. **Atomic Synthetic source_info** (Plan 4's `By::is_atomic_synthesizer()`) — via `ATOMIC_SYNTHETIC_KINDS`.
3. **Atomic CustomNode types** (`CrossrefResolvedRef` today; `IncludeExpansion` post-Plan-8) — via `isAtomicCustomNode`.

The gate is correctness-level: atomic content's source AST and rendered output diverge (e.g. `@fig-1` source vs. "Figure 1" rendered), so editing into rendered atomic content would corrupt the source. Both formats benefit automatically; q2-debug picks it up "for free" if it ever grows editing affordances, without modifying its dispatcher.

##### `framework/dispatch.tsx` — CustomBlock / CustomInline traversal

Add entries to `renderChildrenRegistry` (which lives in `framework/dispatch.tsx` after Plan 2pre's collapse, framework-internal) for `CustomBlock` and `CustomInline` so child traversal works for slot contents. The gate above ensures atomic children don't get a usable `setLocalAst`.

These two new entries are *generic* — they iterate slots without per-type knowledge. **The standard render path does not call them**: per-type components (`Callout`, `Theorem`, …) drive their own slot rendering via `renderSlot` from `q2-preview/utils.ts`, which builds `<Node>` instances directly. **The actual consumer is `Fallback`** (the `'__fallback__'` entry in `customNodeRegistry`) — it has no per-type slot knowledge and delegates to `renderChildren({ node: customNode })` for generic walk-and-render, which routes through these registry entries. Any future generic walker (debug introspection, AST-shape diagnostics) that calls `renderChildren` on a CustomNode will hit the same path.

**Pseudocode for the two entries** (matches `dispatch.tsx:39-43`'s existing entry signature `(args: {node, setLocalAst, onNavigateToDocument?}) => React.ReactNode`):

```ts
// CustomBlock and CustomInline share the same shape — both have a `slots`
// map keyed by slot name. Iteration order follows the JS object's insertion
// order, which is preserved across unwrap (per §"Order preservation").
const renderCustomNodeChildren = (args) => {
  const { node, setLocalAst, onNavigateToDocument } = args;
  const slotEntries = Object.entries(node.slots) as Array<[string, Slot]>;
  return slotEntries.flatMap(([name, slot]) => {
    const setSlot = (next: Slot) =>
      setLocalAst({ ...node, slots: { ...node.slots, [name]: next } });
    switch (slot.kind) {
      case 'block':
        return [<Node key={name} node={slot.value}
          setLocalAst={(n) => setSlot({ kind: 'block', value: n })}
          onNavigateToDocument={onNavigateToDocument}
        />];
      case 'inline':
        return [<Node key={name} node={slot.value}
          setLocalAst={(n) => setSlot({ kind: 'inline', value: n })}
          onNavigateToDocument={onNavigateToDocument}
        />];
      case 'blocks':
        return slot.value.map((b, i) => <Node key={`${name}-${i}`} node={b}
          setLocalAst={(n) => {
            const next = slot.value.slice();
            next[i] = n;
            setSlot({ kind: 'blocks', value: next });
          }}
          onNavigateToDocument={onNavigateToDocument}
        />);
      case 'inlines':
        return slot.value.map((inl, i) => <Node key={`${name}-${i}`} node={inl}
          setLocalAst={(n) => {
            const next = slot.value.slice();
            next[i] = n;
            setSlot({ kind: 'inlines', value: next });
          }}
          onNavigateToDocument={onNavigateToDocument}
        />);
    }
  });
};

renderChildrenRegistry['CustomBlock']  = renderCustomNodeChildren;
renderChildrenRegistry['CustomInline'] = renderCustomNodeChildren;
```

This is the same body shape as `q2-preview/utils.ts::renderSlot` — both build `<Node>` per slot value with a copy-on-write `setLocalAst`. The duplication is intentional: `renderSlot` is the per-component slot-render helper (called by named slots: `renderSlot(slots.title, ...)`); `renderChildrenRegistry['CustomBlock'|'CustomInline']` is the generic-walk fallback that doesn't know which slot is which. Keeping them separate avoids coupling Fallback's walk to the per-component naming conventions (Callout's `title`/`content` vs FloatRefTarget's `caption_long`/`caption_short` etc.).

**No JSX wrapper element.** The entries return a flat list of children; the surrounding component (Fallback, or any future generic walker) decides how to wrap. This matches the existing `renderChildrenRegistry` entries (`Para`, `Header`, etc.), which also return raw child arrays and let the per-tag component supply the `<p>` / `<h1>` / etc.

**`renderChildrenRegistry` does not grow per custom-node type.** A new custom-node type adds *one* entry in `customNodeRegistry` (per-format, keyed by `type_name`) and *zero* entries in `renderChildrenRegistry`. The framework table has Pandoc-base-type entries plus exactly two abstract-category entries (`'CustomBlock'`, `'CustomInline'`) — no further entries are anticipated. See 2pre §"`renderChildrenRegistry` is framework-internal" for the contract this preserves.

###### `blockTypes` extension

Extend `blockTypes` from its current 11 entries to **19 entries** total. Today's array (`framework/dispatch.tsx:29`) is:

```ts
['Para', 'Plain', 'Header', 'CodeBlock', 'BulletList', 'OrderedList', 'BlockQuote', 'Div', 'HorizontalRule', 'RawBlock', 'Figure']
```

Add 8 entries:

- **In-scope new leaves** (rendered by 2B's `q2-preview/blocks/`): `'LineBlock'`, `'DefinitionList'`, `'Table'`.
- **Post-unwrap discriminator**: `'CustomBlock'`. After `unwrapCustomNodes`, custom-block wrapper Divs become `t: 'CustomBlock'`; without this entry `Node` would misroute them to the `Inline` dispatcher.
- **Out-of-scope but defensively-routed-as-block**: `'BlockMetadata'`, `'NoteDefinitionPara'`, `'NoteDefinitionFencedBlock'`, `'CaptionBlock'`. These tags can appear in the AST (per `crates/pampa/src/writers/json.rs:1242, :1251, :1257, :1263`); without `blockTypes` membership they'd be misclassified as inlines and the format's `Inline` dispatcher would render the "(not yet implemented)" placeholder. With `blockTypes` membership the format's `Block` dispatcher renders the placeholder correctly. The §Defensive variants list below stays unchanged — these tags continue to fall through to the muted-gray placeholder in v1.

`Node`'s `isBlock` test already routes via this array, so the addition is a single 8-element extension.

#### Asset manifest plumbing (parent walker + iframe distribution)

Per Plan 2A §"Provided: blob-URL asset contract," image bytes flow as blob URLs minted in the parent. 2B implements the contract end-to-end: a parent-side walker that produces a manifest of `{ origPath → blobUrl }`, and an iframe-side context that distributes the manifest to `Image` components.

##### Parent-side asset walker

**Location: `Q2PreviewIframe.tsx`** (q2-preview-specific; parent already has WASM access for `vfsReadBinaryFile`; close to the postMessage send site). Implemented as a pure function `buildAssetManifest(...)` in a new module:

```ts
// hub-client/src/components/render/q2-preview/assetWalker.ts
import { resolveRelativePath, guessMimeType } from '../../../utils/vfsPaths';

export interface ManifestCacheEntry {
  url: string;
  contentB64: string;  // the base64 string used as the cache identity (see "Cache key" below)
}

export interface AssetManifestResult {
  manifest: Record<string, string>;     // origPath → blobUrl
  revoked: string[];                     // URLs that fell out of the manifest
}

/**
 * Walks the AST for Image nodes, resolves paths, reads VFS bytes,
 * mints (or reuses cached) blob URLs, and returns a manifest plus
 * the list of URLs that should be revoked.
 *
 * The cache (caller-owned) memoizes by `(resolvedPath, contentB64)`
 * so unchanged image content keeps the same blob URL across re-renders
 * — browsers cache fetched blob URLs internally, so reuse is free.
 */
export function buildAssetManifest(
  astJson: string,
  currentFilePath: string,
  cache: Map<string, ManifestCacheEntry>,
): AssetManifestResult {
  const ast = JSON.parse(astJson);
  const imagePaths = collectImagePaths(ast);  // walks Image nodes, skips externals
  const manifest: Record<string, string> = {};
  const seenKeys = new Set<string>();

  for (const origPath of imagePaths) {
    const resolved = stripLeadingSlash(resolveRelativePath(currentFilePath, origPath));
    const result = vfsReadBinaryFile(resolved);
    if (!result.success || !result.content) continue;

    // Cache key: base64 content is itself the identity. vfsReadBinaryFile
    // returns base64-encoded bytes (per iframePostProcessor.ts:206), and
    // identical bytes always produce identical base64 — so the base64
    // string is a collision-free, synchronous content fingerprint.
    // Cost: ~133KB per 100KB image held in cache; ~6.6MB for a 50-image doc.
    const cacheKey = `${resolved}\0${result.content}`;
    seenKeys.add(cacheKey);

    let entry = cache.get(cacheKey);
    if (!entry) {
      const blob = base64ToBlob(result.content, guessMimeType(resolved));
      entry = { url: URL.createObjectURL(blob), contentB64: result.content };
      cache.set(cacheKey, entry);
    }
    manifest[origPath] = entry.url;
  }

  // Revoke and evict cache entries no longer referenced.
  const revoked: string[] = [];
  for (const [key, entry] of cache) {
    if (!seenKeys.has(key)) {
      URL.revokeObjectURL(entry.url);
      revoked.push(entry.url);
      cache.delete(key);
    }
  }

  return { manifest, revoked };
}

// ~5-line helper, local to assetWalker.ts:
function base64ToBlob(b64: string, mime: string): Blob {
  const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0));
  return new Blob([bytes], { type: mime });
}
```

`Q2PreviewIframe` owns the cache as a ref, calls `buildAssetManifest` once per `UPDATE_AST` cycle (via `useMemo` keyed on `astJson + currentFilePath`), and revokes everything on iframe unmount.

**Cache key rationale.** The cache key is `${resolvedPath}\0${base64}` — path-prefixed for clarity (so cache entries are easy to debug by inspection) and because two documents in a project may resolve different paths to identical bytes. The *content identity* is the base64 string itself — a deterministic 1-to-1 encoding of the underlying bytes, requiring no hash. SHA-256 via `crypto.subtle.digest` would force the walker async (breaking the `useMemo` pattern Plan 2A item 6 specifies); FNV-1a or Murmur3 in pure JS would collide marginally and cost ~10 LOC for no benefit when the base64 is already in hand. If memory pressure ever becomes a problem (say the user uploads a 50MB image), revisit then.

External URLs (`http://`, `https://`, `data:`, `//`) are skipped during the walk — `collectImagePaths` filters them out so the manifest only contains project-relative paths.

**`stripLeadingSlash` helper**: `resolveRelativePath` returns paths starting with `/` (absolute-from-project-root convention), but the VFS stores paths without the leading slash (per `iframePostProcessor.ts:201`). One-line strip kept local to `assetWalker.ts` rather than baked into `vfsPaths.ts` because other consumers of the helper want the leading slash.

##### `hub-client/src/utils/vfsPaths.ts` — extract shared helpers

`resolveRelativePath`, `guessMimeType`, and `normalizePath` are duplicated three times today as private functions in `iframePostProcessor.ts:329, 343, 356` and partially in `iframeLinkHandlers.ts:116, 123` (`resolveRelativePath` + `normalizePath`; no `guessMimeType` since link handlers don't deal with MIME types) and `ReactAstSlideRenderer.tsx:886, 900, 913`. Plan 2B's asset walker would be the fourth consumer; instead, extract to a new module and migrate the three existing call sites.

**Surface**: three pure functions:

```ts
// hub-client/src/utils/vfsPaths.ts

/** Resolve a relative path against the current file's directory. */
export function resolveRelativePath(currentFile: string, relativePath: string): string;

/** Normalize a path: collapse `.`, `..`, leading slash. */
export function normalizePath(path: string): string;

/** Guess a MIME type from file extension. Returns 'application/octet-stream' for unknown. */
export function guessMimeType(path: string): string;
```

**Migration**: this is the **first commit of Plan 2B's asset-manifest cluster** (Phase 2 in the checklist). Land it before introducing `assetWalker.ts` so the new consumer reads from the canonical source. Three existing files lose their private copies and import from the new module:

- `hub-client/src/utils/iframePostProcessor.ts` (lines 329, 343, 356).
- `hub-client/src/utils/iframeLinkHandlers.ts` (lines 116 and 123 — `resolveRelativePath` and `normalizePath`).
- `hub-client/src/components/render/ReactAstSlideRenderer.tsx` (lines 886, 900, 913).

No behavioral change; pure refactor. ~50 LOC in the new file, ~40 LOC removed across the three existing files. Net ~+10 LOC.

##### Manifest distribution: rides on `UPDATE_AST`

The manifest piggybacks on the existing `UPDATE_AST` payload rather than getting its own message type — manifest and AST are tightly coupled (manifest is derived from AST contents), and shipping them together guarantees ordering. The payload grows from `{ astJson, currentFilePath }` to `{ astJson, currentFilePath, assetManifest }`:

```ts
iframeRef.current.contentWindow.postMessage(
  {
    type: 'UPDATE_AST',
    payload: { astJson, currentFilePath, assetManifest },
  },
  '*'
);
```

Manifest size is bounded by the number of unique image paths × ~50 bytes per blob URL string. A 50-image doc → ~3KB. Negligible vs. the AST itself.

##### `q2-preview/AssetManifestContext.tsx`

New file. Iframe-side React context that distributes the manifest to `Image` components:

```tsx
import { createContext } from 'react';

export const AssetManifestContext = createContext<Record<string, string>>({});
```

2B extends Plan 2A's top-level `updateAst(payload)` callback in `q2-preview/entry.tsx` to destructure `assetManifest` from each `UPDATE_AST` payload and forward it as a prop to `<PreviewRoot>`. `PreviewRoot`'s props (the `PreviewRootProps` interface defined in 2A item 9) grow by one field: `assetManifest: Record<string, string>`. PreviewRoot wraps the `<Ast>` mount with `<AssetManifestContext.Provider value={props.assetManifest}>` (alongside its existing `<PreviewContext.Provider>`). Image components consume via `useContext(AssetManifestContext)`.

##### `q2-preview/CustomNodeRegistryContext.tsx` — moved to Plan 2C

Plan 2C adds `CustomNodeRegistryContext.tsx` as a sibling of `AssetManifestContext` to distribute the merged customNodeRegistry (built-ins + user-TSX overrides) to the CustomBlock/CustomInline dispatchers. Plan 2B's PreviewRoot wires only the `AssetManifestContext.Provider` and 2A's `mergedRegistry` (Pandoc-tag overrides); the second context provider lands in 2C alongside its `customNodeRegistry` and `Fallback` consumers.

##### Manifest miss handling

`lookupAssetUrl(manifest, url)` falls back to the original URL on manifest miss. The resulting `<img src="hero.png">` will fail to load in the iframe (no fetcher for project-relative paths), producing a visible broken-image affordance. This is intentional — silently swallowing missing images would hide bugs in the walker or the upload pipeline. Future enhancement: render an explicit placeholder component, but v1 lets the browser's default broken-image icon surface the issue.

#### q2-preview leaf components

##### `q2-preview/blocks/`

Real-HTML implementations of every Pandoc Block variant. Seeded from Elliot's `html.tsx`; gap fills from cross-referencing `crates/quarto-pandoc-types/src/block.rs`:

| File | Pandoc node | Renders as |
|---|---|---|
| `q2-preview/blocks/Para.tsx` | `Para` | `<p>` |
| `q2-preview/blocks/Plain.tsx` | `Plain` | Fragment (no wrapper) |
| `q2-preview/blocks/Header.tsx` | `Header` | `<h1>`-`<h6>` with id, classes, data-* attrs |
| `q2-preview/blocks/CodeBlock.tsx` | `CodeBlock` | `<pre><code>` with id, classes, attrs |
| `q2-preview/blocks/BulletList.tsx` | `BulletList` | `<ul>` |
| `q2-preview/blocks/OrderedList.tsx` | `OrderedList` | `<ol start={N}>` |
| `q2-preview/blocks/BlockQuote.tsx` | `BlockQuote` | `<blockquote>` |
| `q2-preview/blocks/Div.tsx` | `Div` | `<div>` with id, classes, data-* attrs |
| `q2-preview/blocks/HorizontalRule.tsx` | `HorizontalRule` | `<hr>` |
| `q2-preview/blocks/RawBlock.tsx` | `RawBlock` | If `format === 'html'`, `dangerouslySetInnerHTML`; else `<pre>` |
| `q2-preview/blocks/Figure.tsx` | `Figure` | `<figure>` + body blocks + `<figcaption>` |
| `q2-preview/blocks/LineBlock.tsx` | `LineBlock` (gap) | `<div class="line-block">` with each line as a `<div>` of inlines |
| `q2-preview/blocks/DefinitionList.tsx` | `DefinitionList` (gap) | `<dl><dt>...</dt><dd>...</dd></dl>` |
| `q2-preview/blocks/Table.tsx` | `Table` (gap) | `<table>` + `<caption>` + `<thead>` + `<tbody>` + `<tfoot>` |

##### `q2-preview/inlines/`

Real-HTML implementations of every Pandoc Inline variant:

| File | Pandoc node | Renders as |
|---|---|---|
| `q2-preview/inlines/Str.tsx` | `Str` | text node |
| `q2-preview/inlines/Space.tsx` | `Space` | `' '` |
| `q2-preview/inlines/SoftBreak.tsx` | `SoftBreak` | `'\n'` |
| `q2-preview/inlines/LineBreak.tsx` | `LineBreak` | `<br>` |
| `q2-preview/inlines/Emph.tsx` | `Emph` | `<em>` |
| `q2-preview/inlines/Strong.tsx` | `Strong` | `<strong>` |
| `q2-preview/inlines/Code.tsx` | `Code` | `<code>` with id, classes, attrs |
| `q2-preview/inlines/Link.tsx` | `Link` | `<a href title>` |
| **`q2-preview/inlines/Image.tsx`** | `Image` | `<img>` with full Pandoc semantics (see below) |
| `q2-preview/inlines/Span.tsx` | `Span` | `<span>` with id, classes, attrs |
| `q2-preview/inlines/Quoted.tsx` | `Quoted` | `'…'` or `"…"` characters around children |
| `q2-preview/inlines/Math.tsx` | `Math` | KaTeX-rendered `<span>` (DisplayMath / InlineMath) |
| `q2-preview/inlines/Underline.tsx` | `Underline` (gap) | `<u>` |
| `q2-preview/inlines/Strikeout.tsx` | `Strikeout` (gap) | `<s>` |
| `q2-preview/inlines/Superscript.tsx` | `Superscript` (gap) | `<sup>` |
| `q2-preview/inlines/Subscript.tsx` | `Subscript` (gap) | `<sub>` |
| `q2-preview/inlines/SmallCaps.tsx` | `SmallCaps` (gap) | `<span style="font-variant: small-caps">` |
| `q2-preview/inlines/RawInline.tsx` | `RawInline` (gap) | If `format === 'html'`, `dangerouslySetInnerHTML`; else `<code>` |
| `q2-preview/inlines/Cite.tsx` | `Cite` (gap) | Visible inlines (second-position content); citations array provides metadata. **Note**: `Cite` is also missing from `framework/types.ts` (verified — no `CiteInline` type today, even though `Cite` *can* appear in any post-pipeline AST). 2B's framework/types.ts edit adds the type alongside the new CustomNode shapes. Pseudocode: `const [, inlines] = node.c; return <>{inlines.map((inl, i) => <Node key={i} node={inl} setLocalAst={...} />)}</>` — single Fragment, no wrapper element. The `c[0]` citations array (Pandoc's `[Citation]`) is metadata for future bibliography rendering and is ignored in v1. |
| `q2-preview/inlines/Note.tsx` | `Note` (gap, defensive) | Number-with-tooltip-body fallback for `reference-location: block`/`section` and any other config that leaves raw `Note` inlines in the AST. See §"FootnotesTransform inclusion" below for the full design and the pointer to bd-1kly (the upstream fix). Default `document` location replaces all `Note` inlines with `Span(Sup(Link))` upstream of q2-preview's renderer, so this component fires only on non-default configs. |

##### `q2-preview/inlines/Image.tsx` — full Pandoc semantics

```tsx
import { useContext } from 'react';
import { AssetManifestContext } from '../AssetManifestContext';
import { lookupAssetUrl, inlinesToPlainText } from '../utils';
import type { ImageInline } from '../../framework/types';

export function Image({ node }: { node: ImageInline }) {
  const [[id, classes, kvs], altInlines, [url, title]] = node.c;
  const manifest = useContext(AssetManifestContext);

  const src = lookupAssetUrl(manifest, url);
  const alt = inlinesToPlainText(altInlines);
  const kvMap = Object.fromEntries(kvs);

  return (
    <img
      src={src}
      alt={alt}
      {...(title ? { title } : {})}
      {...(id ? { id } : {})}
      {...(classes.length ? { className: classes.join(' ') } : {})}
      {...(kvMap.width ? { width: kvMap.width } : {})}
      {...(kvMap.height ? { height: kvMap.height } : {})}
    />
  );
}
```

`lookupAssetUrl(manifest, url)` (in `q2-preview/utils.ts`) checks for external patterns (`https?:`, `data:`, `//`) and passes them through; otherwise looks up `url` in the manifest and returns the blob URL, falling back to the original URL on miss (the resulting broken `<img>` is a deliberate signal that resolution failed). `inlinesToPlainText` recursively walks inlines (`Str`, `Space`, `Code`, `SoftBreak`, `LineBreak`, etc.) into a plain string for the `alt` attribute.

`PreviewContext` is no longer imported by `Image.tsx`. Path resolution against `currentFilePath` happens in the parent-side asset walker (see §"Asset manifest plumbing" below). `PreviewContext` continues to carry `currentFilePath` for link handlers (Plan 2A item 10).

The legacy `/.quarto/...` branch from `iframePostProcessor.ts:177-210` is **not** ported — q2-preview's body AST never carries `/.quarto/...` image paths (per Plan 2A §"Provided: blob-URL asset contract").

##### `q2-preview/inlines/Math.tsx` — KaTeX leaf

Near-verbatim port of Elliot's Math from `~/docs/demo-playground/elliot/html.tsx:259–279`. The entry already imports `katex` statically (per `q2-preview/entry.tsx:30`) and bundles `katex/dist/katex.min.css`, so the built-in component imports `katex` directly via ESM rather than reading `window.katex`:

```tsx
import { memo } from 'react';
import katex from 'katex';
import type { MathInline } from '../../framework/types';

export const Math = memo(({ node }: { node: MathInline }) => {
  const [{ t: mathType }, latex] = node.c;
  const isDisplayMath = mathType === 'DisplayMath';
  try {
    const html = katex.renderToString(latex, {
      displayMode: isDisplayMath,
      throwOnError: false,
      output: 'html',
    });
    return <span dangerouslySetInnerHTML={{ __html: html }} />;
  } catch {
    // Surface the raw LaTeX so failed parses don't vanish silently.
    return <span>{latex}</span>;
  }
});
```

Three divergences from Elliot's pattern:

- **Direct ESM import of `katex`** instead of reading `window.katex`. `window.katex` is set inside the entry's `loadCustomComponents` (line 126) only when `LOAD_CUSTOM_COMPONENTS` arrives; if a document has no user-TSX overrides, `window.katex` is `undefined` and Elliot's pattern would crash. The static import works in every render path. (`window.katex` is still set for user-TSX components that expect it, including users who paste Elliot's `html.tsx` to override built-ins.)

  **Sandbox safety.** The iframe is sandboxed with `sandbox="allow-scripts allow-same-origin"` (per `Q2PreviewIframe.tsx:168`); the `allow-same-origin` token combined with same-origin Vite bundling means the iframe can run module scripts that import from the same origin's bundle. The static `import katex from 'katex'` resolves at iframe load time against the bundled chunk that Vite emits alongside `q2-preview.html`; no cross-origin fetch, no eval, no `data:` URI for the import. KaTeX itself runs entirely client-side without network access. Same-origin import is what makes "use the bundle, not `window.katex`" safe — without `allow-same-origin` the import would fail and we'd be forced back to the global-via-postMessage pattern.
- Drop `console.log('katex', katex)` and `console.error` calls — let errors surface in dev tools naturally rather than as console spam.
- Explicit `<span>{latex}</span>` fallback on the catch path. Elliot's version returns `undefined` from the `catch`, which renders as nothing — a KaTeX failure makes the equation vanish silently. Showing the raw LaTeX makes the failure visible.

Used as a registry leaf for plain `Math` inlines outside any Equation wrapper. Plan 2C's `Equation.tsx` also targets this component as its slot-render target after appending `\tag{N}` to the LaTeX (see Plan 2C §"`Equation.tsx`").

##### Pipeline change: include `FootnotesTransform` in q2-preview

Per the 2026-05-09 audit, **`"footnotes"` is removed from `Q2_PREVIEW_TRANSFORM_EXCLUDED`** at `crates/quarto-core/src/pipeline.rs:1049-1062`. This is a pipeline.rs change, not a hub-client change — listed here because q2-preview's Note.tsx behavior depends on it.

**Rationale.** `FootnotesTransform` (`crates/quarto-core/src/transforms/footnotes.rs:67`) does two things:

1. **Numbers footnotes** in document order and replaces each `Inline::Note` with `Span(id="fnrefN")(Sup(Link(href="#fnN", class="footnote-ref")(Str(N))))` per `:440-460`.
2. **Builds a `Div(class="footnotes")` at the end of the document** containing an OrderedList of footnote bodies, each with a backlink to the inline reference.

Both outputs use Pandoc primitives (Span, Sup, Link, Div, OrderedList) that q2-preview's blocks/inlines components already render. The transform is HTML-output-shaped only in its class names (`footnote-ref`, `footnote-back`, `footnotes`); the structural shape is generic.

**Why it was originally excluded** (per `pipeline.rs:1024` comment): "synthesize-with-no-preimage." The footnotes section, the backlink arrows, and the wrapper `Span(id="fnrefN")` are constructed without source positions — future edit affordances would need atomicity to prevent corrupting source.

**Why it's safe for v1 q2-preview**: q2-preview is structural rendering only ("Edit affordances ... v1 is structural-only rendering" — §Out of scope). The actual *Note content* (the user-typed footnote body) keeps its source_info; only the chrome (section wrapper, backlink) is synthesized. When edit UI ships in a later plan, the proper fix is atomicity markers on the synthesized chrome (Plan 4/6/7's territory), not excluding the transform.

**Effects**:

- **Note marker numbering is automatic.** No JS pre-walk needed; the transform's counter does the work upstream.
- **`NoteDefinitionPara` and `NoteDefinitionFencedBlock` are transformed away.** §Out of scope's defensive-rendering for these tags becomes inert — they no longer reach the iframe's AST in the default (`reference-location: document`) configuration.
- **Three new classes added to the taxonomy** (in `q2-preview/quartoClasses.ts`):
  ```ts
  export const FOOTNOTES = 'footnotes';
  export const FOOTNOTE_REF = 'footnote-ref';
  export const FOOTNOTE_BACK = 'footnote-back';
  ```
- **`Note.tsx` becomes a JS-side number-with-tooltip-body fallback.** With `reference-location: document` (default) or `margin`, raw `Note` inlines never reach the iframe — they're transformed upstream. With `reference-location: block` or `section`, `FootnotesTransform` no-ops at `footnotes.rs:99-105` and raw `Note(Block[])` inlines survive into the AST.

  **The "Pandoc handles this" comment at `footnotes.rs:99` is stale** — it's from when the architecture assumed real Pandoc would be the eventual writer. pampa's HTML writer at `crates/pampa/src/writers/html.rs:806-817` also doesn't handle these configs correctly: it emits `<sup class="footnote-ref"><a href="#fn{N}">[{N}]</a></sup>` where `{N}` is the *length* of the note's content array, not a sequential number. There's a TODO at `:815-816` acknowledging the gap. Neither layer numbers Notes correctly for block/section configs — the work is missing in both places.

  The proper fix is upstream — extend `FootnotesTransform` to handle block/section by numbering Notes in document order and emitting per-block / per-section footnote sections at the right boundary. **Tracked as bd-1kly.** When that lands, `Note.tsx` becomes inert (raw Notes never reach the iframe under any config) and can be deleted.

  Until bd-1kly lands, q2-preview's v1 fallback is JS-side and intentionally degraded.

  **Where the walk runs.** In `PreviewRoot` (`q2-preview/entry.tsx`), inside a `useMemo` keyed on `astJson`. The memo parses the JSON, walks for Notes, and returns `{ ast, noteNumbers }`:

  ```ts
  // PreviewRoot
  const { ast, noteNumbers } = useMemo<{
    ast: PandocAST | null;
    noteNumbers: WeakMap<NoteInline, number>;
  }>(() => {
    try {
      const parsed = JSON.parse(astJson) as PandocAST;
      return { ast: parsed, noteNumbers: walkForNoteNumbers(parsed) };
    } catch {
      return { ast: null, noteNumbers: new WeakMap() };
    }
  }, [astJson]);

  // Hand the parsed AST to <Ast> via the discriminated input. On parse
  // failure (ast === null), fall back to the string path so framework's
  // existing try/catch renders the red error pane.
  return ast
    ? <Ast ast={ast} {...rest} />
    : <Ast astJson={astJson} {...rest} />;
  ```

  **Parse-error handling.** PreviewRoot's `useMemo` swallows parse errors and falls back to the `astJson` (string) path, which routes through framework's existing error handler in `Ast.tsx`. This keeps a single error-display surface for malformed JSON regardless of which input path the caller used.

  Two parses on the *happy* path is what the discriminated input avoids: when the parse succeeds, PreviewRoot passes the parsed AST and `<Ast>` skips its internal `JSON.parse`. Memoization on `astJson` keeps the walk from re-running across unrelated re-renders.

  **Walk runs unconditionally.** The walk fires on every `UPDATE_AST` regardless of `reference-location` metadata. Producing an empty `WeakMap` for documents with zero Notes (the common case) is cheap — a single AST traversal at JSON.parse cost. Gating on a metadata read up front would add complexity (config plumbing into PreviewRoot) and save microseconds; not worth it.

  **Walk strategy.** Same structural-JSON-traversal pattern as `unwrapCustomNodes` (see §"Walk strategy" above). Descend every `c` field encountered. When the walker hits `node.t === 'Note'`, increment a counter and store `(noteRef, counter)` in the WeakMap. The walker descends into wire-format CustomNode wrappers via the slot wrapper Div/Span children at `c[1][i].c[1]` — so footnotes inside a callout body or theorem proof are reached. The walker can run *pre-unwrap* (over the JSON.parse output): unwrap creates new outer `CustomBlock`/`CustomInline` shapes but passes inner content through unchanged, so a `Note` inline reference is identical pre- and post-unwrap. The WeakMap keyed by object identity therefore works regardless of which side of the unwrap the lookup happens on.

  **Map key: `WeakMap<NoteInline, number>`.** Object identity, not source-info pool index. `node.s` is unreliable here (filter provenance can share source info across distinct Notes); position-in-traversal can't be recovered by `Note.tsx` without re-walking. Object identity is preserved across renders by the `useMemo` (which keeps the same parsed AST reference until `astJson` changes), and the JS-native AST that `<Ast>` constructs internally re-parses the same `astJson` on every mount — but the WeakMap lookup is from `Note.tsx`'s perspective on the *PreviewRoot-side parse*. Since `<Ast>` parses *its own* copy independently, the WeakMap lookup would *fail* if keyed by reference from a different parse.

  **Resolution: pass the parsed AST through, don't re-parse.** PreviewRoot's `useMemo` outputs `{ ast, noteNumbers }`. Modify `framework/Ast.tsx` to accept either `astJson: string` *or* `ast: PandocAST` (one or the other). When `ast` is provided, skip the internal `JSON.parse` and use the passed object directly. Both PreviewRoot (q2-preview) and any future format that wants a single-parse pattern can opt in. q2-debug keeps its current `astJson`-only path.

  This is a small `framework/Ast.tsx` change — `~10 LOC` to add the discriminated input — and lives in the existing customNode.ts commit (where Ast.tsx already gets co-edits for the unwrap call and `sourceInfoPool` extraction). The cost of the alternative (mutating Notes with a `__q2p_number` field) is "we mutate the parsed AST at walk time" plus "the field leaks into `setAst` postMessage payloads if we don't strip it"; the discriminated-input approach avoids both.

  - **`Note.tsx` render**: `<sup class="footnote-ref" title="{stringified body}">{number}</sup>`. The `title=` attribute carries the footnote body via `blocksToPlainText(node.c)` (companion to `inlinesToPlainText`; see §"`q2-preview/utils.ts`"), capped at 250 characters with a trailing `'…'` when truncated. The cap is per §"Resolved → Note tooltip body length" and exists so long footnotes don't render awkwardly in browser tooltip UI. Body is reachable on hover; placement is incorrect (no per-block / per-section footnote section) but every word up to the cap is still visible. If the WeakMap lookup misses (defensive — shouldn't happen), render `<sup class="footnote-ref">?</sup>` so the unhandled case is visible.
  - **Class taxonomy**: emits the same `footnote-ref` class as the document-mode transform, so the eventual tippy.js popup integration (out of scope for 2B; see §"Reference popups note" below) can target both paths uniformly when it lands.
  - **No body section appended** to the document — the body is in the `title=` attribute, not a separate block. Position-correct rendering is bd-1kly's job, not q2-preview's.
  - **Scope cap**: ~30 LOC for the walk, `NoteNumberingContext`, `Note.tsx`, and the `framework/Ast.tsx` discriminated-input change. The implementor should resist the temptation to also implement the position-correct fallback (~80 LOC for block/section boundary tracking + per-boundary list emission) — it duplicates bd-1kly's work and creates two implementations of the same concept that have to be kept in sync.
  - **User overrides**: `Note.tsx` can be overridden via `render-components: [...]` like any other inline. `NoteNumberingContext` always provides numbers (when block/section is in effect); user overrides can read or ignore them via `useContext(NoteNumberingContext)`.

  **Reference popups note (informational, not in scope).** TS Quarto's HTML output gets hover-popup footnotes via tippy.js layered on top of the standard `<sup class="footnote-ref">` markup — the AST shape doesn't change, the popup is purely a presentation-layer enhancement. Once `FootnotesTransform` handles all four reference-location configs uniformly (post-bd-1kly), q2-preview can include tippy.js in its iframe bundle and get popups for free. Not 2B work; flagged so the plumbing isn't accidentally designed to preclude it.

##### Pipeline change: include `AppendixStructureTransform` in q2-preview

Per the 2026-05-09 audit, **`"appendix-structure"` is removed from `Q2_PREVIEW_TRANSFORM_EXCLUDED`** at `crates/quarto-core/src/pipeline.rs:1049-1062`. Same reasoning as `FootnotesTransform`: it produces output that q2-preview's components already render, and the "synthesize-with-no-preimage" exclusion rationale doesn't bite for v1 structural rendering.

**What it does** (`crates/quarto-core/src/transforms/appendix.rs:9-35`): consolidates appendix-related content into a single `Div(id="quarto-appendix", class="default")` at the end of the document. Pulls in:

- User-defined appendix sections (`:::{.appendix} ...`).
- Footnotes section (from `FootnotesTransform`, now also included).
- Bibliography section (from `CiteprocTransform` — currently not in the pipeline; this branch is inert for v1).
- License / copyright / citation sections from `license:`, `copyright:`, `citation:` YAML metadata (each becomes a small `Div(class="section")` with an h2 header).

**Why it's safe**: every output is pure Pandoc primitives (`Block::Div`, `Block::Header`, `Block::Paragraph`, `Inline::Str`, `Inline::Link`) — no `RawBlock` HTML strings. q2-preview's `Div`/`Header`/`Paragraph` components render the appendix container exactly as the HTML pipeline would, with the same `quarto-appendix` / `quarto-bibliography` / `quarto-reuse` / `quarto-copyright` / `quarto-citation` class names so theme CSS targeting matches.

**Effects**:

- The footnotes section moves from "loose at document end" to "nested inside `<div id="quarto-appendix">`." Visually identical to the HTML pipeline.
- License/copyright/citation YAML metadata renders automatically without 2B touching `PreviewDocument.tsx` or the metadata pipeline.
- New classes added to `quartoClasses.ts`:
  ```ts
  export const QUARTO_APPENDIX = 'quarto-appendix';      // outer container
  export const QUARTO_BIBLIOGRAPHY = 'quarto-bibliography';
  export const QUARTO_REUSE = 'quarto-reuse';            // license section
  export const QUARTO_COPYRIGHT = 'quarto-copyright';
  export const QUARTO_CITATION = 'quarto-citation';
  ```

**Bibliography note**: `AppendixStructureTransform` looks for a `<div id="refs">` (citeproc output) to fold into the appendix. `CiteprocTransform` is not in the q2-preview pipeline today (and not in the HTML pipeline either, per the audit — defer-to-Pandoc), so the bibliography branch finds nothing and skips. If/when Citeproc lands, the appendix transform automatically picks up the `<div id="refs">` it produces; no further q2-preview change needed.

**Footnotes-branch-under-non-default-reference-location note**: same inert pattern. `AppendixStructureTransform`'s footnotes branch (`appendix.rs:140-145`) calls `extract_footnotes`, which looks for a `Div(id="footnotes")` in the AST. With `reference-location: block` or `section`, `FootnotesTransform` no-ops upstream and *no* footnotes div is produced — `extract_footnotes` returns `None` and the appendix's footnotes branch silently does nothing. The user-defined appendix sections, license, copyright, and citation branches still work normally. When bd-1kly lands and `FootnotesTransform` handles all reference-location values uniformly, the appendix's footnotes branch picks up the produced section automatically; no further q2-preview change needed.

##### Pipeline change: `TitleBlockTransform` is **not** included (deferred)

Reviewed in the 2026-05-09 audit. `TitleBlockTransform` (`crates/quarto-core/src/transforms/title_block.rs:67-74`) gates on `ctx.format.is_html()`: in HTML mode it does nothing (the HTML template generates `<header id="title-block-header">` from metadata variables); in non-HTML modes it prepends an h1 from the title metadata.

q2-preview's format maps to HTML (`crates/quarto-core/src/format.rs:119`: `"q2-preview" => Some(("html", Some("preview")))`), so `should_add_h1` returns false unless the user opts into minimal-template mode. **But q2-preview bypasses the HTML template entirely** (`ApplyTemplateStage` is excluded — see `pipeline.rs` exclusion comment), so neither branch of the gate produces a title block for q2-preview: the template-generated header isn't there because there's no template, and the AST h1-injection path isn't taken because q2-preview reports `is_html()`.

**Consequence**: a user document with `title: My Document` in YAML and no body `# heading` renders with no visible title in q2-preview today. (Plan 2A item 11 surfaces the title for the browser tab via metadata extraction; the body is empty.) That's a pre-existing 2A gap, not a 2B regression.

**Why not just include the transform**: making `should_add_h1` return true for q2-preview requires either (a) a Rust change to gate on something other than `is_html()` — e.g. a new `template_will_run` accessor on `ctx.format` — or (b) treating q2-preview as forced-`minimal: true`, which has surface-area implications elsewhere. Either is a real Rust change with downstream effects, not a one-line deny-list edit.

**Decision**: defer to a follow-up plan. Plan 2B's scope is the renderer; the title-rendering decision is upstream and worth its own plan to consider rendering options (Rust transform vs. JS-side title block in `PreviewDocument.tsx`) holistically.

##### `q2-preview/blocks/Figure.tsx` — `<figure>` + `<figcaption>`

```tsx
export function Figure({ node }: { node: FigureBlock }) {
  const [[id, classes, _kvs], [_short, captionBlocks], bodyBlocks] = node.c;
  return (
    <figure
      {...(id ? { id } : {})}
      {...(classes.length ? { className: classes.join(' ') } : {})}
    >
      {bodyBlocks.map((b, i) => <Block key={i} node={b} />)}
      {captionBlocks.length > 0 && (
        <figcaption>
          {captionBlocks.map((b, i) => <Block key={i} node={b} />)}
        </figcaption>
      )}
    </figure>
  );
}
```

Crossref-numbered captions (`Figure 1: …`) are already baked into the caption blocks by `CrossrefResolveTransform` (in q2-preview's pipeline at `pipeline.rs:881, :977`); q2-preview gets that for free.

This component renders body blocks via `<Block />` and reads `c[1][1]` directly for the caption — it does **not** call `renderChildren(args)` for the figure as a whole. That avoids any interaction with `renderChildrenRegistry.Figure`, which was rewritten in 2pre to render only `c[2]` (the main body) and to drop the buggy short-caption / `// TODO:` interleaving. Either pattern is correct after 2pre; this component happens to slot blocks individually for the caption hairsplit.

##### `q2-preview/custom/` — moved to Plan 2C

Plan 2C ships the seven type-specific CustomNode components (`Callout`, `Theorem`, `Proof`, `FloatRefTarget`, `Equation`, `CrossrefResolvedRef`, `IncludeExpansion`) plus `Fallback.tsx` plus `theoremEnvs.ts`. Plan 2B's framework changes (Phase 1) make the unwrap path land JS-native CustomNode shapes in the AST; Plan 2B's registry stops there, so CustomNode wrappers fall through to `dispatchers.tsx`'s "(not yet implemented)" muted-gray placeholder until 2C populates the registry.

#### `q2-preview/registry.ts` assembly (2B scope: Pandoc-base only)

```ts
import type { FormatRegistry } from '../framework/types';
import * as Blocks from './blocks';
import * as Inlines from './inlines';
import { Block, Inline } from './dispatchers';  // q2-preview's own; created in Plan 2A
import { PreviewDocument } from './PreviewDocument';

export const previewRegistry: FormatRegistry = {
  ...Blocks,
  ...Inlines,
  Block,
  Inline,
  Ast: PreviewDocument,  // q2-preview's root wrapper, registered under the 'Ast' key (no debug styling)
};
```

`previewRegistry` keeps `FormatRegistry` (the typed shape from `framework/types.ts:89` that enforces `Ast`/`Block`/`Inline` keys). 2B does **not** register `CustomBlock`/`CustomInline` keys — Plan 2C extends this assembly with the dispatcher entries plus `customNodeRegistry` plus `CustomNodeRegistryContext`.

Plan 2A's `q2-preview/dispatchers.tsx` ships `Block` and `Inline` with the muted-gray "(not yet implemented)" miss path. 2B's leaves under `Blocks` / `Inlines` populate the registry so the miss path stops firing for Pandoc base types. CustomNode wrappers (`{t: 'CustomBlock' | 'CustomInline'}` shapes produced post-unwrap) hit the same miss path — the registry has no entry for those keys yet — so they render as muted-gray placeholders until 2C lands.

#### `q2-preview/utils.ts` — shared component utilities

- `lookupAssetUrl(manifest, url): string` — checks for external URL patterns (`https?:`, `data:`, `//`) and passes them through; otherwise looks up in the asset manifest, falling back to the original URL on miss. ~12 LOC.
- `inlinesToPlainText(inlines: InlineNode[]): string` — Stringify pass for alt text and other plain-text contexts. Walks `Str`, `Space`, `SoftBreak`, `LineBreak`, `Code`, `Emph`, `Strong`, `Link`, `Quoted`, `Underline`, `Strikeout`, `Superscript`, `Subscript`, `SmallCaps`, `Span`, `Cite` (recurses into `c[1]`), `Math` (returns the LaTeX source), `Image` (returns alt-text recursion). Unknown inlines contribute empty string.
- `blocksToPlainText(blocks: BlockNode[]): string` — companion helper used by `Note.tsx`'s `title=` attribute. Walks `Para`, `Plain`, `Header`, `BlockQuote`, `Div`, `Figure`, `BulletList`, `OrderedList`, `LineBlock`, `DefinitionList`, `Table` (caption + cells), `CodeBlock`/`RawBlock` (returns the raw text), `HorizontalRule` (empty). For each block whose body is `Inlines`, delegates to `inlinesToPlainText`; for nested-block containers (BlockQuote, Div, Figure, list items, table cells), recurses. Block boundaries join with a single space (the consumer is a hover-tooltip `title=` attribute, not a typeset document — no need to preserve paragraph breaks). ~30 LOC; lives in the same file as `inlinesToPlainText` so the two share the inline-walking switch.

Plan 2C extends `utils.ts` with `formatRefLabel`, `composeAttr`, and `renderSlot` (the per-component slot dispatcher consumed by Callout / Theorem / etc.). 2B does not need those helpers — no Pandoc-base component renders CustomNode slots.

#### `q2-preview/quartoClasses.ts` — class-name constants (Plan 2B stub)

Plan 2B ships a stub of `quartoClasses.ts` containing only the constants needed for Pandoc-base + footnotes/appendix rendering. The Quarto-feature taxonomy (callout, theorem, proof, quarto-xref) lands in Plan 2C alongside its CustomNode components.

**Drift-detection caveat.** No test in this plan is a true compile-time Rust↔JS drift detector — JS constants and Rust strings live in different toolchains. Two coverage layers reduce drift risk:

1. The **vitest "Class-compatibility test"** (§Test plan) asserts at runtime that each q2-preview component emits the documented class names. If a JS constant is changed in `quartoClasses.ts` without updating the consumer, the test fails. If the *Rust* class name drifts without updating `quartoClasses.ts`, this test still passes — only the smoke-all `image-with-attrs.qmd` fixture (2B) and Plan 2C's `multi-element-doc.qmd` fixture catch that, and only because they run the full Rust→React pipeline against a real document.
2. The **Rust-side `pipeline.rs` validation tests** at `crates/quarto-core/src/pipeline.rs:1987` (`q2_preview_transform_excluded_names_exist_in_html_pipeline`) and `:2053` (the stage-list counterpart) already prevent typos in `Q2_PREVIEW_TRANSFORM_EXCLUDED` / `Q2_PREVIEW_STAGE_EXCLUDED` from drifting silently. They validate name-spelling against the actual transform list, but do **not** assert membership. 2B's `pipeline.rs` change — removing `"footnotes"` and `"appendix-structure"` from `Q2_PREVIEW_TRANSFORM_EXCLUDED` — is covered by these tests for spelling but membership is exercised only via Plan 2C's `multi-element-doc.qmd` and `multi-element-project/` fixtures (which include a footnote / appendix and assert the rendered output structure).

Re-verify class-name constants on any major Rust transform refactor; the runtime + smoke-all coverage above is the safety net.

```ts
// Section / level — crates/pampa/src/transforms/sectionize.rs:114
// (Rendered by q2-preview's Div.tsx; not a CustomNode, but worth pinning
// for class-compatibility tests.)
export const SECTION = 'section';
export const SECTION_LEVEL_PREFIX = 'level';  // level1, level2, ..., level6

// Footnotes — emitted by FootnotesTransform (now included in q2-preview's
// pipeline; see §"Pipeline change: include FootnotesTransform").
// Source: crates/quarto-core/src/transforms/footnotes.rs:26-35,440-460
export const FOOTNOTES = 'footnotes';            // outer Div(class="footnotes")
export const FOOTNOTE_REF = 'footnote-ref';      // <a> inside the inline <sup>
export const FOOTNOTE_BACK = 'footnote-back';    // backlink <a> inside each <li>

// Appendix container — emitted by AppendixStructureTransform (now included
// in q2-preview's pipeline; see §"Pipeline change: include AppendixStructureTransform").
// Source: crates/quarto-core/src/transforms/appendix.rs:244-365
export const QUARTO_APPENDIX = 'quarto-appendix';        // outer container Div
export const QUARTO_BIBLIOGRAPHY = 'quarto-bibliography'; // bib section (inert until Citeproc)
export const QUARTO_REUSE = 'quarto-reuse';              // license section
export const QUARTO_COPYRIGHT = 'quarto-copyright';      // copyright section
export const QUARTO_CITATION = 'quarto-citation';        // how-to-cite section
```

**The first commit of Phase 3** (after the Pandoc-base components have been written) ports the stub constants above into the file. 2B's components reference them (e.g. Section / Header rendering uses `SECTION` and `SECTION_LEVEL_PREFIX`; the FootnotesTransform output's wrapper Div picks up `FOOTNOTES` via `Div.tsx`'s class passthrough). Plan 2C extends the file with callout/theorem/proof/quarto-xref/etc. before consumers in Phase 4 reference them.

#### Update q2-preview entry to call rewrap before `SET_AST`

`q2-preview/entry.tsx` (created by Plan 2A) is updated to call `rewrapCustomNodes(newAst)` inside the `setAst` callback before `postMessage({ type: 'SET_AST', ast: ... })`. The forward (unwrap) direction lives in `framework/Ast.tsx` (see §"`framework/customNode.ts`" → "`framework/Ast.tsx` co-edits"); only the rewrap lives in entry.tsx.

#### Fork Elliot's demos to `gordon/render-components` — moved to Plan 2C

The demo fork depends on 2C's CustomNode components having shipped (so the "prune now-built-in" pruning is meaningful — most of Elliot's `html.tsx` overlaps with Pandoc base which 2B ships, but several CustomNode patterns don't become redundant until 2C lands). Plan 2C handles the fork.

### Out of scope

- Layout / chrome components (TOC sidebar, navbar, footer, page-nav strip rendering as page chrome). Deferred per Plan 2A.
- Edit affordances (theorem-rename UI, callout-type changer, etc.). v1 is structural-only rendering.
- Drift-detection contract test (Rust HTML output ↔ React render). Useful long-term; defer.
- Body-classes derivation, navbar brand-fallback. Deferred per Plan 2A.
- Quarto-specific Image extensions: `fig-align`, `fig-link`, `fig-alt`, `lightbox`, subfigures, `fig-cap-location`. Tier 3 — defer to a follow-up plan parallel to "q2-preview layout chrome."
- `BlockMetadata` (Quarto extension, structured config blocks — not user-visible content), rendered as fallback in v1.
- `NoteDefinitionPara`, `NoteDefinitionFencedBlock` (Quarto reference-style note definitions): with `FootnotesTransform` now included in q2-preview's pipeline, these are transformed away upstream in the default `reference-location: document` configuration. The `block`/`section` cases (where the upstream transform currently no-ops — see bd-1kly) leave them in the AST; covered by `Note.tsx`'s number-with-tooltip-body fallback.

### Defensive variants

- **Out-of-band**: `Shortcode` (desugared by `ShortcodeResolveTransform`), `NoteReference` / `InlineAttr` / `CaptionBlock` (defensive errors Q-3-21 / Q-3-31 / Q-3-32). If they appear, it's a bug elsewhere; v1 renders fallback.
- **Critic markup**: `Insert` / `Delete` / `Highlight` / `EditComment` are defensively serialized as `<span class="critic-{type}">` in the AST and pass through the existing `Span` component.

## Design decisions

- **Real-HTML leaves as q2-preview's built-in registry** — not "drafts pasted into demos." Pasted-demo overrides via `render-components: [...]` (Plan 2A item 13) still work; they layer on top of the built-ins instead of replacing missing defaults.
- **q2-preview/blocks/, q2-preview/inlines/, q2-preview/custom/ as a directory tree of one component per file**. Easier to navigate, override, and test than a single `html.tsx`. Barrel files (`q2-preview/blocks/index.ts` etc.) provide name-keyed re-exports for the registry.
- **Atomic-aware gate in framework's `Node`, not in either format's `Block`/`Inline`.** Plan 2pre moves the dispatchers out of framework into format-owned files; `Node` is the only remaining cross-format chokepoint where the gate can sit once. Correctness-level concern; benefits both formats. q2-debug picks up the gate "for free."
- **Two registries**: `componentRegistry` keyed by `node.t`, `customNodeRegistry` keyed by `type_name`. User overrides target one or the other explicitly.
- **User overrides win — for Pandoc tags (2B) and CustomNode types (2C).** Plan 2A already plumbs Pandoc-tag overrides via `mergedRegistry = { ...previewRegistry, ...customRegistry }` at `entry.tsx:179-182`. Plan 2C extends the same model to CustomNode `type_name`s via `CustomNodeRegistryContext` and a parallel `mergedCustomNodeRegistry`. The two namespaces (Pandoc tags vs CustomNode type_names) are disjoint, so a single user-TSX export bag feeds both maps unambiguously. 2B does not change the override mechanism; only ships the Pandoc-base built-ins that 2A's merge already overrides.
- **Recursion contract for the atomic gate.** The atomic-aware gate sits in framework's `Node` (in `framework/dispatch.tsx`) and only fires when a child enters via `<Node>`. Built-in components and the Plan-2A registries satisfy this transitively because every recursion path uses `renderChildren(args)` (which constructs `<Node>` for each child) or `renderSlot(slot, setSlot, ctx)` (which also builds `<Node>` per slot value — see §"`q2-preview/utils.ts`"). User-TSX overrides registered via `render-components: [...]` MUST follow the same rule: **recurse via `<Node>` or `renderChildren`, never iterate `node.c` and emit child JSX directly.**

  A user component that walks `node.c` itself bypasses the gate for its descendants — atomic content beneath a non-atomic ancestor (e.g. a shortcode-resolved Span inside a user-overridden Para) silently loses its read-only protection. Today this only matters once edit affordances ship (post-Plan-7), but the contract is load-bearing the moment they do, and the failure mode is invisible at v1: user overrides that bypass `<Node>` look correct in v1's structural-rendering world and start corrupting source the day editing turns on.

  Verified that all four files in `~/docs/demo-playground/elliot/` (`html.tsx`, `kanban.tsx`, `comment.tsx`, `simple.tsx`) follow this rule today — every child-rendering path goes through `renderChildren` or delegates to the framework's `Block`/`Inline` dispatcher (`<B node={...} ...>`). Direct `node.c` reads in those files are for *attribute* extraction (header level, list start, image url) or for *filtering* before delegating to a framework dispatcher, never for hand-rolled child-rendering JSX.

  The Phase 5.1 vitest harness includes a regression fixture that mounts a deliberately-bypassing user-override component over an atomic CustomNode child and asserts the gate did *not* protect it — so the day someone tries to harden user-extension safety, they have a known-bad fixture rather than discovering it in production. This is a **negative** test — its purpose is to lock the documented behavior, not to celebrate it.
- **CustomBlock / CustomInline dispatch — registry stub in 2B, populated in 2C.** The framework's `Node` dispatcher gets `'CustomBlock'` added to `blockTypes` in 2B (so post-unwrap CustomBlock nodes route correctly). 2B does **not** register `CustomBlock`/`CustomInline` keys in `previewRegistry`; the muted-gray placeholder fires for those node types until 2C adds the dispatcher entries that look up `customNodeRegistry[node.type_name]`.
- **`html.tsx` and `custom.tsx` paste-in pattern still works** for users who want to override q2-preview's defaults. The 2B build-out makes the registry no longer require pasting to be useful.
- **The `'Ast'` registry entry in q2-preview is minimal**: just calls `renderChildren({ node: ast, setLocalAst: setAst, ... })` with no debug wrapper. The format-specific outer wrapper (PreviewContext provider, etc.) is in `q2-preview/entry.tsx` (`PreviewRoot`), not in the registry. (The registry key `'Ast'` is shared with q2-debug — see 2pre §"What stays exactly the same"; only the registered component differs per format.)
- **Image alt-text via Stringify**, not just `Str` filtering. Elliot's `html.tsx` had a `Str`-only filter; a real Pandoc Stringify pass handles `Emph` / `Code` / `SoftBreak` / etc. inside alt text correctly.
- **Visual + structural parity target.** q2-preview targets **Bootstrap-flavored HTML output structurally identical to the HTML pipeline's writer output** — same elements, same classes, same nesting — so Quarto's compiled theme CSS (which is Bootstrap-derived; see Plan 2A item 11's `theme.css` plumbing) produces visually-matching output without per-format CSS forks. Concretely:

  - **Element parity**: `Para → <p>`, `Header → <h1>..<h6>`, `BulletList → <ul>`, `OrderedList → <ol>`, `BlockQuote → <blockquote>`, `Figure → <figure>` + `<figcaption>`, `Image → <img>`, `Code → <code>`, `CodeBlock → <pre><code>`, `Emph → <em>`, `Strong → <strong>`, `Link → <a>`, etc. — match Pandoc's HTML writer choices, not invent new ones.
  - **Class parity**: see §"`q2-preview/quartoClasses.ts`" for 2B's stub constants (`section`, `level1`–`level6`, `footnote-ref`, `footnotes`, `footnote-back`, `quarto-appendix`, etc.) and Plan 2C §"`q2-preview/quartoClasses.ts` — class-name extensions" for the Quarto-feature taxonomy (`callout`, `callout-note`, `theorem`, `theorem-title`, `quarto-xref`, …). Both files cross-reference Rust source line numbers so drift is caught.
  - **Where divergence is allowed**: when Pandoc's writer choice conflicts with React's children-as-array model (e.g. some writers use string concatenation for inline content where React composes nodes), or when Pandoc's writer relies on document-final post-processing q2-preview can't reproduce (e.g. table column-width fixup applied after HTML serialization). In those cases, prefer the smallest deviation that preserves CSS selector targets.
  - **Where divergence is forbidden**: anywhere theme CSS targets a specific element-or-class combination. If a Bootstrap rule says `.callout > .callout-header { ... }`, the Callout component must emit a child element with class `callout-header` directly under `.callout` — not a Span nested inside another Div. Class-without-structure parity wouldn't render correctly.

  **Rationale.** The previous "class-compatible; DOM may diverge" framing was permissive enough to allow divergence the theme CSS couldn't tolerate (e.g. a `<section class="callout">` instead of `<div class="callout">`, where Bootstrap's `.callout { ... }` rule still matches but child-selector rules might not). Tightening to element-and-structure parity removes that risk.

## Encode / decode / unwrap / rewrap (terminology)

The CustomNode lifecycle has four operations across the system:

- **Wrap (Rust → wire)**: `pampa/src/writers/json.rs::write_custom_block` / `write_custom_inline`. Rust CustomNode → wire-format Div / Span with `__quarto_custom_node` class.
- **Decode (Rust read)**: `pampa/src/readers/json.rs::read_custom_block_from_div` / `read_custom_inline_from_span`. Wire-format → Rust CustomNode.
- **Unwrap (JS, in iframe)**: NEW in 2B. Wire-format → JS-native `CustomBlockNode` / `CustomInlineNode`. Mirrors Rust's decode. Lives in `framework/customNode.ts`.
- **Rewrap (JS, in iframe before postMessage)**: NEW in 2B. JS-native CustomNode → wire-format Div / Span. Mirrors Rust's wrap. Lives in `framework/customNode.ts`.

Round-trip property: `unwrap(rewrap(x)) === x` and `wrap(unwrap(wireDiv)) === wireDiv`.

## Soft activation dependencies

- **Plan 4** introduces `Synthetic { by: By }` and `Derived { from, by }` SourceInfo variants. Until Plan 4 lands, no inline can have Derived source_info.
- **Plan 6** populates Derived source_info on shortcode resolutions. After Plan 6, the dispatcher's atomic detection activates for shortcode-resolved inlines.
- **Plan 8** introduces `IncludeExpansion` CustomNode and amends `atomicCustomNodes.ts` to add it. 2B's `IncludeExpansion` component is registered from the start.

## Multi-plan contracts

### Consumed: Plans 2pre and 2A foundation

Plan 2A is **fully landed** as of commits `fe40973b` (the foundation) plus follow-ups `81e48f10` and `e6381abd` (theme-fingerprint and theme-CSS-path fixes for default-project mode) and `0887a3fa` (refactored q2-preview pipeline construction to deny-list form — `Q2_PREVIEW_TRANSFORM_EXCLUDED` and `Q2_PREVIEW_STAGE_EXCLUDED`). The set of excluded transforms is unchanged from the original allow-list (still excludes `crossref-render`, `callout-resolve`, etc.); the refactor is structural only.

- `framework/types.ts` — `BlockNode`, `InlineNode`, `PandocAST`, `Attr`, `MathInline` (added by 2pre); 2B adds `Slot`, `CustomNodeBase`, `CustomBlockNode`, `CustomInlineNode` (new — Plan 2pre did not stage placeholders).
- `framework/RegistryContext.tsx` — exported context with `sourceInfoPool?` typed by 2A item 4 but **not yet filled**; 2B's Ast.tsx co-edit wires it up.
- `framework/Ast.tsx` — extended by 2B with three additions: (a) call `unwrapCustomNodes(ast)` after `JSON.parse(astJson)`, (b) extract `astContext.sourceInfoPool` onto the `RegistryContext.Provider` value, (c) accept a discriminated input (`{ astJson: string } | { ast: PandocAST }`) so q2-preview's PreviewRoot can pass an already-parsed AST and avoid a double-parse for the Note-numbering walk. All three edits live in the customNode.ts commit.
- `framework/dispatch.tsx` (the consolidated recursion-and-render module from 2pre — houses `Node`, `renderChildren`, `renderNode`, `blockTypes`, and the framework-internal `renderChildrenRegistry`) — 2B modifies `Node` (atomic gate), adds `CustomBlock`/`CustomInline` entries to `renderChildrenRegistry`, and extends `blockTypes` from 11 to 19 entries (8 additions — see §"`blockTypes` extension" above). The mutations to `renderChildrenRegistry` are framework-evolves-itself changes — the structure is not exposed via `framework/index.ts` or any format global. See 2pre §"`renderChildrenRegistry` is framework-internal" for the contract.
- `q2-preview/PreviewContext.tsx`, `q2-preview/dispatchers.tsx`, `q2-preview/PreviewDocument.tsx`, `q2-preview/registry.ts` skeleton — landed by Plan 2A items 7 & 8; 2B fills in the leaf and CustomNode components and rewires the registry assembly.
- `q2-preview/Q2PreviewIframe.tsx` — landed by Plan 2A item 6. 2B's item 2.2 extends it by adding the asset-walker call (cache ref, `useMemo`, payload extension to `{ astJson, currentFilePath, assetManifest }`, unmount cleanup).  Today's `Q2PreviewIframe.tsx:115-125` posts `UPDATE_AST` with payload `{ astJson, currentFilePath }` (no manifest yet).
- `q2-preview/entry.tsx` — landed by Plan 2A item 9. Sets `__Q2_PREVIEW_RENDERER__` at module top, registers a module-top message handler covering `LOAD_CUSTOM_COMPONENTS` / `UPDATE_AST` / `UPDATE_THEME`, and dispatches via a `PreviewRoot` component with `PreviewRootProps = { astJson, currentFilePath, onNavigateToDocument?, setAst }`. Imports `katex` statically, bundles `katex/dist/katex.min.css`, and sets `window.katex` inside `loadCustomComponents` for user TSX. **Critically**, the `setAst` callback at `entry.tsx:221-226` posts the AST to the parent verbatim — 2B's item 2.3 inserts the `rewrapCustomNodes(newAst)` call there. 2B also extends `UpdateAstPayload` (line 63-66) to include `assetManifest` and forwards it via `<AssetManifestContext.Provider>` inside `PreviewRoot`. The `mergedRegistry` spread at `entry.tsx:179-182` (`{ ...previewRegistry, ...customRegistry }`) is the encoded "user overrides win" rule — see §Design decisions.
- `hub-client/public/q2-preview.html` — unchanged.
- `hub-client/src/types/sourceInfo.ts`, `hub-client/src/utils/sourceInfo.ts`, `hub-client/src/utils/atomicCustomNodes.ts` — read by the framework dispatcher gate.
- `hub-client/src/utils/iframeLinkHandlers.ts` — installed by 2A; unchanged.
- `hub-client/src/utils/vfsPaths.ts` — **new** in 2B (extracted from three existing private copies of `resolveRelativePath` / `guessMimeType` / `normalizePath`). See §"vfsPaths.ts — extract shared helpers" above.

### Consumed: Plan 1's page-scoped image artifacts (via Plan 2A's blob-URL asset contract)

q2-preview's AST keeps `<img src>` as the user wrote it. Plan 2A's §"Provided: blob-URL asset contract" specifies the parent-mints-URL / iframe-consumes-URL pattern; 2B applies it to image bytes via the asset manifest plumbing described above (§"Asset manifest plumbing").

The renderer does not contribute image bytes (per Plan 2A's contract — bd-3gtn note). Bytes come from the user's original VFS upload (`automergeSync` → `vfsAddBinaryFile`). The parent-side walker (in `Q2PreviewIframe.tsx`) reads via `vfsReadBinaryFile`, mints blob URLs, and posts the manifest in `UPDATE_AST`. `Image.tsx` is a pure manifest consumer — no VFS access in the iframe.

### Provided: Pandoc base + framework + asset manifest (consumed by Plan 2C)

After 2B lands, documents using **Pandoc base types** (Para, Header, lists, images, links, code, tables, math, footnotes via the now-included `FootnotesTransform`, appendix sections via the now-included `AppendixStructureTransform`) render with visual fidelity matching the HTML format. **Documents using Quarto custom-node features** (callouts, theorems, proofs, figures-as-FloatRefTarget, equations as Equation CustomNodes, cross-references) show muted-gray placeholders for those nodes — Plan 2C populates the registry to fix that.

Plan 2C consumes:
- Framework changes (`framework/customNode.ts` unwrap/rewrap, `framework/types.ts` CustomNode shapes, `framework/dispatch.tsx` atomic gate + `blockTypes` extension + `renderChildrenRegistry` entries, `framework/Ast.tsx` co-edits).
- Pandoc base-type leaves (every CustomNode component's slot recursion goes through `<Node>` and lands in 2B's blocks/inlines).
- Asset manifest plumbing (Image rendering inside Callout / Theorem / FloatRefTarget bodies uses 2B's `AssetManifestContext`).
- Stub `quartoClasses.ts` (2C extends with the Quarto-feature taxonomy).
- `q2-preview/utils.ts` partial (2C adds `formatRefLabel`, `composeAttr`, `renderSlot`).
- Pipeline exclusion list (2C relies on `crossref-render` and `callout-resolve` continuing to be excluded so the CustomNode wrappers survive into the iframe).

Plans 4 / 6 / 7 / 8 add to this incrementally without 2B or 2C needing amendment.

## Open questions for implementation

(All open questions from prior revisions have been resolved during 2026-05-09 audits — see "Resolved" subsections below. New open questions should be added here as they arise.)

### Resolved during 2026-05-09 implementor-review pass (latest)

- **React StrictMode + WeakMap reuse — non-issue.** Verified: q2-preview's iframe entry (`entry.tsx:209-227`) calls `createRoot().render(<PreviewRoot />)` *without* a `<StrictMode>` wrapper. The hub-client app shell uses StrictMode at `main.tsx`, but the iframe is a separate React tree mounted from `q2-preview.html`'s `<script type="module">` and inherits no parent React context. So the StrictMode double-render cycle — which would have invalidated the `useMemo`-keyed WeakMap on first mount — does not apply at this layer. If a future change wraps the iframe entry in StrictMode (e.g. for development-only assertions), the Note-numbering useMemo will need migration to a `useRef`-backed cache; flagged here so the relationship is recoverable.
- **KaTeX bundle weight — accepted for v1, flagged for follow-up.** Verified: `q2-preview/entry.tsx:30` imports katex statically; no chunking is configured in `vite.config.ts`. Every q2-preview document ships katex (~280KB minified). The alternative (lazy `import('katex')` inside `Math.tsx`) trades initial bundle size for a paint flash on first equation render and complicates the catch-path (raw-LaTeX fallback would need to be lazy-rendered too). v1 keeps the static import. If bundle size becomes a real complaint, the route is to chunk-split katex via Vite's `manualChunks` config rather than lazy-importing per component — that keeps the synchronous render-path semantics. Out of scope for 2B.
- **`IncludeExpansion` shadowing user exports — accepted.** A user-authored TSX export named `IncludeExpansion` would shadow 2B's dormant placeholder via `mergedCustomNodeRegistry`. Until Plan 8 ships `IncludeExpansion` CustomNodes, the shadowed entry never fires (no AST node ever has `type_name === 'IncludeExpansion'`), so the shadow is inert. Once Plan 8 lands, the user's component takes precedence — same "User overrides win" rule as for every other CustomNode type. Documented here so the placeholder's "always registered, can be overridden" semantics aren't surprising.
- **Note tooltip body length — capped at 250 chars + ellipsis.** HTML `title=` attributes are technically unbounded but browsers truncate display variably (Safari ~300 chars, Chrome shows full but wraps awkwardly). For a hover-affordance fallback this is informational, not authoritative; cap output of `blocksToPlainText` at 250 characters with a trailing `'…'` when truncated. Long-form footnote viewing is bd-1kly's territory (the proper per-block / per-section section); the cap exists so v1 doesn't degrade gracelessly on long footnotes.

### Resolved during 2026-05-09 audits

- **Equation `\tag{N}` source — resolved.** q2-preview's pipeline excludes `crossref-render` (listed in `Q2_PREVIEW_TRANSFORM_EXCLUDED` at `crates/quarto-core/src/pipeline.rs:1049`, entry at `:1061`), so the Equation CustomNode arrives in the iframe with the **un-tagged** DisplayMath inline in `slots["content"]`. The JS `Equation.tsx` component appends `\tag{N}` itself by reading `plain_data.order.order` — a JS-side port of `crates/quarto-core/src/transforms/crossref_render.rs::render_equation:601`. KaTeX renders `\tag{}` natively. See §"`q2-preview/custom/`" Equation entry.
- **`Math` component placement — resolved.** Lives in `q2-preview/inlines/Math.tsx` (Pandoc base inline; near-verbatim port of Elliot's pattern). Used both as a registry leaf for plain `Math` inlines and as the slot-render target for `Equation.tsx`.
- **Note marker numbering — resolved by including `FootnotesTransform`.** The transform is included in q2-preview's pipeline (removed from `Q2_PREVIEW_TRANSFORM_EXCLUDED`); it numbers footnotes in document order and replaces each `Note` with `Span(Sup(Link))`. No JS pre-walk needed. See §"Pipeline change: include `FootnotesTransform`".
- **Cite rendering — resolved.** Render `c[1]` (the visible inlines that Pandoc fills in for the link text) via `renderChildren`; ignore `c[0]` (the citations array — metadata that future bibliography rendering can consume). One-line component. Bibliography itself is layout chrome, deferred.
- **Quarto Image extensions (`fig-align`, `fig-link`, `fig-alt`, `lightbox`, subfigures, `fig-cap-location`) — resolved.** Silently ignore unknown Image kvs in v1 (current `Image.tsx` already passes only `width`/`height`). Document in user-facing docs that these extensions are not yet implemented in q2-preview. The `<img>` still renders correctly without them; missing fig-align is a degradation, not a failure. Future plan parallel to "q2-preview layout chrome" picks them up.

## References

### Rust side (read during implementation; not modified by 2B)

- `crates/quarto-pandoc-types/src/{block,inline,custom}.rs` — canonical Block / Inline / CustomNode / Slot enums.
- `crates/pampa/src/writers/json.rs::write_custom_block` (line 1297), `write_custom_inline` (line 1381) — wire format for unwrap to mirror.
- `crates/pampa/src/readers/json.rs::read_custom_block_from_div` (line 2220), `read_custom_inline_from_span` (line 2358) — Rust-side decode to mirror in JS unwrap.
- `crates/quarto-core/src/transforms/callout_resolve.rs` — Callout HTML structure source.
- `crates/quarto-core/src/transforms/crossref_render.rs` — Theorem/Proof/FloatRefTarget/Equation/CrossrefResolvedRef HTML rendering.
- `crates/pampa/src/transforms/sectionize.rs` — Section / levelN classes.
- `crates/quarto-core/src/transforms/footnotes.rs` — Note → `Span(Sup(Link))` + `Div.footnotes` section. Now included in q2-preview's pipeline.
- `crates/quarto-core/src/transforms/appendix.rs` — `<div id="quarto-appendix">` consolidation, license/copyright/citation sections. Now included in q2-preview's pipeline.
- `crates/quarto-core/src/transforms/title_block.rs` — title-block injection (HTML-template-aware). **Not** included in q2-preview's pipeline; see §"Pipeline change: `TitleBlockTransform` is not included".

### hub-client side (modified by 2B)

- `hub-client/src/components/render/framework/types.ts` — add concrete `CustomBlockNode` / `CustomInlineNode` / `Slot` / `CustomNodeBase` shapes; also add `CiteInline` (verified absent today even though Cite can appear in any post-pipeline AST).
- `hub-client/src/components/render/framework/Ast.tsx` — call `unwrapCustomNodes` after parsing `astJson`; extract `astContext.sourceInfoPool` onto the `RegistryContext.Provider` value; accept a discriminated input (`{astJson: string} | {ast: PandocAST}`) for callers that have already parsed the AST.
- `hub-client/src/components/render/framework/dispatch.tsx` — atomic-aware gate inside `Node`; add CustomBlock / CustomInline traversal entries to `renderChildrenRegistry`; extend `blockTypes` from 11 to 19 entries.
- `hub-client/src/components/render/framework/customNode.ts` (NEW) — unwrap / rewrap walks.
- `hub-client/src/utils/vfsPaths.ts` (NEW) — extract `resolveRelativePath`, `normalizePath`, `guessMimeType` from three existing private copies.
- `hub-client/src/utils/iframePostProcessor.ts` — drop private copies of helpers; import from `vfsPaths.ts`.
- `hub-client/src/utils/iframeLinkHandlers.ts` — drop private copies of `resolveRelativePath` and `normalizePath`; import from `vfsPaths.ts`.
- `hub-client/src/components/render/ReactAstSlideRenderer.tsx` — drop private copies of helpers; import from `vfsPaths.ts`.
- `hub-client/src/components/render/q2-preview/blocks/*.tsx` (NEW) — every Pandoc Block.
- `hub-client/src/components/render/q2-preview/inlines/*.tsx` (NEW) — every Pandoc Inline (incl. Image, Math, Note, Cite).
- `hub-client/src/components/render/q2-preview/registry.ts` — populate with Pandoc base-type entries (no CustomBlock/CustomInline keys; those land in 2C).
- `hub-client/src/components/render/q2-preview/utils.ts` (NEW) — `lookupAssetUrl`, `inlinesToPlainText`, `blocksToPlainText` (2B subset; `formatRefLabel`/`composeAttr`/`renderSlot` ship in 2C).
- `hub-client/src/components/render/q2-preview/quartoClasses.ts` (NEW, stub) — `SECTION`, `SECTION_LEVEL_PREFIX`, `FOOTNOTES`, `FOOTNOTE_REF`, `FOOTNOTE_BACK`, `QUARTO_APPENDIX`, `QUARTO_BIBLIOGRAPHY`, `QUARTO_REUSE`, `QUARTO_COPYRIGHT`, `QUARTO_CITATION`. 2C extends with callout/theorem/proof/quarto-xref constants.
- `hub-client/src/components/render/q2-preview/NoteNumberingContext.tsx` (NEW) — context that distributes the JS-side note-numbering map to `Note.tsx`. Used only when `FootnotesTransform` no-ops (block/section configs); inert otherwise. Removed when bd-1kly lands.
- `hub-client/src/components/render/q2-preview/assetWalker.ts` (NEW) — `buildAssetManifest` parent-side walker.
- `hub-client/src/components/render/q2-preview/AssetManifestContext.tsx` (NEW) — iframe-side context for manifest distribution.
- `hub-client/src/components/render/q2-preview/Q2PreviewIframe.tsx` — extends Plan 2A item 6 with asset walker, URL cache management, manifest in `UPDATE_AST` payload.
- `hub-client/src/components/render/q2-preview/entry.tsx` — extends Plan 2A item 9 with rewrap before `SET_AST`, manifest extraction from `UPDATE_AST` payload, `AssetManifestContext.Provider`, and PreviewRoot's Note-numbering `useMemo` walk. Plan 2C adds the second context provider (`CustomNodeRegistryContext`) and the `mergedCustomNodeRegistry` computation.
- `crates/quarto-core/src/pipeline.rs` — remove `"footnotes"` and `"appendix-structure"` from `Q2_PREVIEW_TRANSFORM_EXCLUDED:1049-1062`. Two-line change. See §"Pipeline change: include `FootnotesTransform`" and §"Pipeline change: include `AppendixStructureTransform`". `TitleBlockTransform` (`"title-block"`) stays excluded — see §"Pipeline change: `TitleBlockTransform` is not included".

  **Stage exclusion list (`Q2_PREVIEW_STAGE_EXCLUDED` at `pipeline.rs:334`) is unchanged by 2B.** Verified contents: `["code-highlight", "math-js", "render-html-body", "apply-template"]`. None of these affect 2B's rendering; `apply-template` is the one that bypasses the HTML wrapper template — keeping it excluded is what allows q2-preview to surface raw body AST to the iframe. The two existing validation tests at `pipeline.rs:1987` (transforms) and `:2053` (stages) lock spelling against the actual transform/stage lists; removing `"footnotes"` and `"appendix-structure"` from the transform exclusion list passes both validators automatically.

### Files modified by Plan 2C (not 2B)

- `hub-client/src/components/render/q2-preview/custom/*.tsx` (NEW) — type-specific CustomNode components + Fallback.
- `hub-client/src/components/render/q2-preview/theoremEnvs.ts` (NEW) — `theoremEnvFor(refType)` port of `theorem_env_for`.
- `hub-client/src/components/render/q2-preview/CustomNodeRegistryContext.tsx` (NEW) — iframe-side context for the merged customNodeRegistry.
- `hub-client/src/components/render/q2-preview/quartoClasses.ts` — extend stub with callout/theorem/proof/quarto-xref taxonomy.
- `hub-client/src/components/render/q2-preview/utils.ts` — extend with `formatRefLabel`, `composeAttr`, `renderSlot`.
- `hub-client/src/components/render/q2-preview/registry.ts` — extend with `CustomBlock`/`CustomInline` dispatcher entries + `customNodeRegistry`.
- `hub-client/src/components/render/q2-preview/entry.tsx` — extend PreviewRoot with `CustomNodeRegistryContext.Provider` and `mergedCustomNodeRegistry`.

### Demo files

- Elliot's existing `~/docs/demo-playground/elliot/html.tsx` is the seed for the q2-preview blocks/inlines registry. Files in `q2-preview/blocks/` and `q2-preview/inlines/` adopt his approach with the gap fills enumerated above and the alt-text-via-Stringify improvement.
- `q2-preview/inlines/Math.tsx` is a near-verbatim port of Elliot's Math at `html.tsx:259–279`. See §"`q2-preview/inlines/Math.tsx`" for the two divergences (drop console statements; explicit raw-LaTeX fallback on KaTeX error).

## Test plan

### Test-tier conventions

Mirroring the pattern Plan 2pre established for q2-debug:

- **vitest unit** (`*.test.ts(x)`, `node` env, no DOM) — pure logic / data tests.
- **vitest integration** (`*.integration.test.tsx`, jsdom env) — React mounting against real `<Ast registry={q2PreviewRegistry}>`. The bulk of "component renders correctly" lives here. ~100 tests/file at sub-second runtime.
- **smoke-all WASM** (`crates/quarto/tests/smoke-all/q2-preview/*.qmd` + `.tsx`) — declarative fixtures with `_quarto.tests.q2-preview.ensureHtmlElements`. Run by both the CLI runner (HTML output only) and the Playwright runner (iframe DOM). q2-preview fixtures use `requires_js: true` so the CLI runner skips them; the Playwright runner picks them up. Adds the third `PreviewIframeKind` (see §"Smoke-all q2-preview infrastructure" below).
- **Playwright e2e** (`hub-client/e2e/*.spec.ts`) — imperative interaction tests for things smoke-all's declarative DSL can't fit cleanly. Used sparingly per project policy.

#### Project-context coverage rule

**Every WASM-path-significant feature must have at least one test covering single-doc, default-project (with `_quarto.yml`), and — where applicable — website-project (`type: website`).** This is a hard rule, not a "nice to have."

The single-doc-only blind spot bit Plan 2A twice in real-browser testing despite extensive unit + integration + WASM coverage:

- `81e48f10` — `theme_fingerprint` was missing from the `RenderResponse` JSON in default-project mode. The orchestrator drained Project-scoped artifacts via `flush_site_libs` for non-website projects, never merging into `project_artifacts`; my single-doc-only WASM regression test didn't exercise the orchestrator path at all.
- `e6381abd` — q2-preview theme CSS lands at `/.quarto/project-artifacts/styles.css` for single-doc but at `quarto/quarto-theme-<fp>.css` for project mode; the iframe wrapper read the single-doc path unconditionally. Single-doc tests passed; project-mode renders silently lost the theme `<link>`.

The lesson: q2-preview has **two** WASM render entry points (`render_qmd_to_response` single-doc, `render_project_active_page_to_response` project) and the project entry has an **internal default-vs-website branch** (driven by `lib_dir`/`site_libs` and the project type). The two Plan 2A bugs both crossed the single-doc-vs-project boundary, not the default-vs-website one — so for 2B "single-doc + project (one of either)" is usually sufficient coverage. Add a website-project case only when a feature plausibly behaves differently under the website pipeline (e.g. `LinkRewriteTransform` interaction). 2B's asset manifest, CustomNode unwrapping, atomic-gate detection, and render-components override path all branch on project context the same way; explicit per-branch coverage prevents the same class of bug here.

When adding tests, ask: "if this feature broke only in default-project mode, would my tests catch it?" If the answer requires manual browser verification, add a fixture.

The component-mount tests in the next subsection live under `hub-client/src/components/render/q2-preview/q2-preview.integration.test.tsx`, mirroring `q2-debug.integration.test.tsx`.

### Vitest integration tests (jsdom, mounting `<Ast registry={q2PreviewRegistry}>`)

- **Unwrap / rewrap round-trip property**: lives in `framework/customNode.test.ts` (pure logic; node env). Cover the six in-tree CustomNode `type_name`s explicitly — `"Callout"`, `"Theorem"`, `"Proof"`, `"FloatRefTarget"`, `"Equation"`, `"CrossrefResolvedRef"` — plus a v1 placeholder shape for `"IncludeExpansion"` (Plan 8). For each, assert `unwrap(rewrap(jsNative)) ≡ jsNative` and `rewrap(unwrap(wireDiv)) ≡ wireDiv`. Also include cases the Rust round-trip tests don't exercise: single-`Block` slot, single-`Inline` slot in a CustomBlock, empty slots, CustomNode-in-slot-of-CustomNode (Plan 8 shape).
- **Inline-CustomNode-specific round-trip case**: explicitly fixture an inline CustomNode (`CrossrefResolvedRef` is the obvious choice) so the no-Plain-wrapper-on-inline-side asymmetry is exercised. The block-CustomNode round-trip won't catch it.
- **Rust → JS → Rust round-trip**: build a CustomNode in Rust, wrap to JSON, ship to JS, unwrap, rewrap, ship back, decode in Rust, assert structural equality. (Cross-language; cargo nextest test on the Rust side. Anchors the JS algorithm against `crates/pampa/src/writers/json.rs:3893`, `:3960`, `:4023`.)
- **Image renderer component tests**: mount `<Image>` wrapped in `<AssetManifestContext.Provider value={manifest}>` with fixtures pointing at:
  - Project-relative path (`hero.png`) with `manifest = { 'hero.png': 'blob:abc' }` — assert `<img src="blob:abc">`.
  - External URL (`https://...`) — assert pass-through; manifest is not consulted.
  - `data:` URI — assert pass-through.
  - Project-relative path with empty manifest (manifest miss, simulates failed VFS resolution) — assert `<img src="hero.png">` (fallback to original URL — broken-image affordance).
  - Image with `width` / `height` kvs — assert attrs on `<img>`.
  - Image with id, classes, title — assert all attributes on `<img>`.
  - Image with non-`Str` alt inlines (`Emph`, `Code`) — assert alt text contains the expanded plain text.

  No mocking of `vfsReadBinaryFile` — it's not called from `Image.tsx` under Design B. The walker tests below cover the VFS-read path in isolation.

- **Asset walker tests** (vitest, `assetWalker.test.ts`): mock `vfsReadBinaryFile` and `URL.createObjectURL`/`revokeObjectURL`. Drive `buildAssetManifest` with various AST shapes:
  - AST with one Image, valid VFS bytes → assert one `createObjectURL` call, manifest contains `{ origPath: <minted URL> }`, no revocations.
  - Same AST re-walked with same content (cache hit) → assert no new mint, no revoke; manifest URL identical.
  - Same AST re-walked with changed bytes (different content hash) → assert old URL revoked, new URL minted, manifest updated.
  - AST with image removed → assert old URL revoked, manifest empty.
  - AST with multiple images, some external (`https://...`) — assert externals are skipped (not in manifest).
  - AST with image whose VFS read fails — assert path is omitted from manifest (no entry, no mint).
  - Stress: AST with N=100 images → assert exactly N mints on first run, 0 on second run with same content.
- **`Q2PreviewIframe` integration test (vitest)**: mount `<Q2PreviewIframe>` with a mock iframe and an AST containing images; assert the `UPDATE_AST` postMessage payload contains `assetManifest` matching the walker output. On unmount, assert all outstanding URLs revoke.
- **Figure renderer**: mount `<Figure>` with fixture containing body Image and caption blocks; assert `<figure>` + `<figcaption>` structure with body recursion.
- **Component snapshot tests**: render each base-type component (Pandoc Block + Inline) with a fixed input; snapshot the rendered DOM. CustomNode-component snapshots move to Plan 2C.
- **Class-compatibility test (2B stub scope)**: for each Pandoc-base component that consumes a `quartoClasses.ts` constant (Section/levelN, FootnotesTransform output's wrapper Div, AppendixStructureTransform output's container Divs), assert the rendered classes match. CustomNode class-compatibility (`callout-*`, `theorem`, `quarto-xref`, etc.) moves to Plan 2C.
- **Atomic CustomNode read-only test**: render a `CrossrefResolvedRef` CustomInline (post-unwrap shape) inside a Para; assert children don't receive a usable `setLocalAst`. **Note**: 2B's registry has no per-type renderer for `CrossrefResolvedRef`, so the assertion runs through the muted-gray placeholder path — but the framework's atomic gate (Phase 1.3) fires before the dispatcher does, so the gate's no-op `setLocalAst` is observable regardless of whether a per-type renderer exists.
- **Reference-preservation property test for `unwrapCustomNodes`**: build a wire AST whose only CustomNode is at depth 3 inside one branch, capture references to leaves in unrelated branches, run unwrap, assert the unrelated-branch leaves are `===` to the captured references. Locks the structural-sharing invariant from §"Walker purity contract" — load-bearing for the Note WeakMap lookup.
- **Derived inline read-only test**: render a Para containing inlines with `Derived` source_info (a shortcode-resolved title); confirm setLocalAst is no-op (shortcode populating a Derived entry — until Plan 6, this test uses hand-constructed pool entries).
- **Recursion-contract bypass test (negative regression guard)**: locks the documented behavior that user-TSX components which iterate `node.c` directly into hand-rolled JSX *bypass* the framework's atomic gate. Per §Design decisions "Recursion contract for the atomic gate." Concrete shape:

  ```ts
  it('user override that iterates node.c directly disables the atomic gate (negative regression guard)', () => {
    const setAstSpy = vi.fn();
    const ast: PandocAST = {
      'pandoc-api-version': [1, 23, 1],
      meta: {},
      blocks: [{
        t: 'Para',
        c: [
          { t: 'Str', c: 'see ' },
          // Atomic inline CustomNode (post-unwrap shape).
          {
            t: 'CustomInline',
            type_name: 'CrossrefResolvedRef',
            slots: { suffix: { kind: 'inlines', value: [] } },
            plain_data: {
              identifier: 'fig-1', kind: 'Figure', ref_type: 'fig',
              resolved: true, kind_source: 'builtin',
              order: { section: [], order: 1 },
            },
            attr: ['', [], {}],
          },
        ],
      }],
    };

    // User override: iterates node.c with hand-rolled JSX. This is the
    // failure pattern the recursion contract forbids — children never
    // re-enter <Node>, so the atomic gate never fires for them.
    const BypassingPara = ({ node, setLocalAst }: NodeArgs<ParaBlock>) => (
      <p data-testid="bypassing-para">
        {node.c.map((child, i) => (
          <button
            key={i}
            data-testid={`child-${i}`}
            onClick={() => setLocalAst({
              t: 'Para',
              c: [...node.c.slice(0, i),
                  { t: 'Str', c: 'EDITED' },
                  ...node.c.slice(i + 1)],
            })}
          />
        ))}
      </p>
    );

    const customRegistry: Record<string, any> = { Para: BypassingPara };
    const merged: FormatRegistry = { ...previewRegistry, ...customRegistry } as FormatRegistry;

    const { getByTestId } = render(
      <Ast
        astJson={JSON.stringify(ast)}
        currentFilePath="/project/test.qmd"
        onNavigateToDocument={() => {}}
        setAst={setAstSpy}
        registry={merged}
      />,
    );

    // Click child-1 — the atomic CrossrefResolvedRef. If the gate had
    // fired, setLocalAst would be NOOP at the atomic level and the
    // edit would never reach setAst. Because BypassingPara constructs
    // its own setLocalAst closure outside <Node>, the gate is bypassed
    // and the edit propagates.
    fireEvent.click(getByTestId('child-1'));

    // Negative assertion: the spy WAS called. This is the failure mode
    // that the recursion contract documents — locking it as a regression
    // fixture means a future implementor who tries to harden the gate by
    // making it propagate downward must explicitly update this test.
    expect(setAstSpy).toHaveBeenCalledTimes(1);
  });
  ```

  The test is *negative* — it asserts the gate failed open. A future hardening pass that makes the gate propagate to descendants (e.g. by passing an "I'm under an atomic ancestor" flag through React context, or wrapping `setLocalAst` at every `<Node>` level) would cause this test to fail, prompting the implementor to document the new behavior and replace the test.

### WASM integration tests (project-mode safety net) — 2B scope

These tests live in `hub-client/src/services/*.wasm.test.ts` and drive `wasm.render_page_in_project` directly. They isolate "is the Rust→WASM bridge correct" from "are the iframe-side mocks set up right." Pattern follows `themeFingerprint.wasm.test.ts` (commits `81e48f10` + `e6381abd`).

- **`assetManifestProject.wasm.test.ts`** (Plan 2B item 5.3): render a `_quarto.yml`-rooted project doc with `![](hero.png)`, real PNG bytes via `vfs_add_file('/project/hero.png', ...)`. Assert the response's `ast_json` contains an `Image` node with `target.0 === "hero.png"` (paths preserved unchanged through the q2-preview pipeline). Then exercise the parent walker (`buildAssetManifest`) against the parsed AST + `currentFilePath="/project/index.qmd"`. Assert the manifest contains `{ "hero.png": "blob:..." }`. Catches default-project `currentFilePath` resolution bugs analogous to Plan 2A's theme path mismatch.
- **`themeFingerprint.wasm.test.ts`** (already exists, **must remain**): locks Plan 2A's `theme_fingerprint` field on `RenderResponse` and the dual-write of theme CSS to `styles.css` for both single-doc and project modes. When 2B's asset-manifest plumbing modifies `pass2_renderer.rs` for `WasmPassTwoOutput` field additions or new artifact handling, do not delete or weaken this test.
- **`customNodeWireFormatProject.wasm.test.ts`** moves to Plan 2C — the wire-format unwrap path's correctness only matters once 2C's CustomNode renderers consume it.

### Pandoc base-type gap-fill tests (vitest integration)

- One per new component (LineBlock, DefinitionList, Table family, Underline, Strikeout, Superscript, Subscript, SmallCaps, RawInline, Cite, Note). Render representative AST node, snapshot DOM.
- **Table family integration**: render a real markdown pipe table through q2-preview pipeline, assert `<table>` / `<thead>` / `<tbody>` structure with correct cell alignment classes.

### Smoke-all q2-preview infrastructure (landed in Plan 2A item 12)

The `PreviewIframeKind = 'html' | 'q2-debug' | 'q2-preview'` extension landed as part of Plan 2A item 12 (commit `fe40973b`). `hub-client/e2e/helpers/previewExtraction.ts:23` and the dispatch in `smoke-all.spec.ts` are in place. Plan 2B's only remaining smoke-all work is the asset-manifest fixture (next subsection); Plan 2C adds the multi-element fixtures.

### Smoke-all q2-preview fixture (2B scope: asset manifest only)

Under `crates/quarto/tests/smoke-all/q2-preview/`. **The directory does not yet exist** — Plan 2B item 5.2 creates it; existing smoke-all fixtures live at `crates/quarto/tests/smoke-all/{filters,q2-debug,...}/`. Plan 2C adds the multi-element fixtures (`multi-element-doc.qmd`, `multi-element-project/`, `with-render-components/`); 2B ships only the image-with-attrs fixture.

#### Frontmatter shape (verified)

`requires_js` and `ensureHtmlElements` live under `_quarto.tests` (verified against `crates/quarto/tests/smoke-all/q2-debug/render-components-reactji.qmd:6-15`):

```yaml
---
title: Image with attributes
format: q2-preview
_quarto:
  tests:
    run:
      requires_js: true
    q2-preview:
      ensureHtmlElements:
        - ['img[src^="blob:"]']
        - ['img[width="400"]']
        - ['img[alt="alt"]']
---
```

`ensureHtmlElements` is an **array of arrays of CSS selectors** — each inner array is a list of selectors that must all be present in the iframe DOM for that assertion line to pass. Use one inner-array entry per assertion; multi-selector inner arrays only when several selectors must co-exist.

#### Fixture (2B)

- **`q2-preview/image-with-attrs.qmd`** (single-doc) + a real PNG asset committed as a sibling file. Single Image with `![alt](hero.png){width=400}`. Asserts `<img>` rendered with `src^="blob:"` (substring match for the blob-URL prefix produced by the parent walker — Playwright's `toHaveAttribute` with a regex), `width="400"`, and the alt-text content.

The fixture covers the iframe boot path for q2-preview, blob-URL minting through the real VFS, manifest distribution to the iframe, and the end-to-end browser path (parent walker → manifest postMessage → context provider → `Image` lookup → `<img src="blob:...">`). It uses `_quarto.tests.run.requires_js: true` so the CLI smoke-all runner skips it and the Playwright runner picks it up. No imperative Playwright spec file needed; the existing `smoke-all.spec.ts` runner discovers it automatically.

## Dependencies

### Hard dependencies (all landed)

- **Plan 2pre** ✅ — directory restructure. 2B's framework changes (atomic-aware gate inside `Node`, customNode.ts, types.ts CustomNode shapes, CustomBlock entries in `renderChildrenRegistry`, all in `framework/dispatch.tsx`) reference paths and structures Plan 2pre establishes.
- **Plan 2A** ✅ — q2-preview surface scaffolding (commit `fe40973b` plus theme follow-ups `81e48f10`, `e6381abd`, and pipeline-construction refactor `0887a3fa`). 2B fills the registry skeleton 2A creates; consumes PreviewContext, registry barrel, entry.tsx, Q2PreviewIframe.tsx.
- **Plan 1** ✅ — pipeline + format detection.

### Soft / activation dependencies

(See §"Soft activation dependencies" above.) Plans 4, 6, 7, 8 add to the AST shape 2B watches for; until they land, the relevant detection arms stay dormant.

### Blocks

Nothing structurally. Plans 4 / 5 / 6 / 7 / 8 can land in parallel with 2B; they decorate the AST that 2B's components render.

## Risk areas

- **Round-trip correctness in unwrap / rewrap.** The two functions must be exact mirrors of each other and of Rust's `write_custom_block` / `read_custom_block_from_div`. Property tests at `customNode.test.ts` catch drift; Rust-side anchor tests at `writers/json.rs:3893, :3960, :4023` lock the wire format.
- **Wire-format Plain-wrapper asymmetry between block and inline CustomNodes.** The block wrapper (`Div`) wraps `Inline` / `Inlines` slots in `Plain` blocks (writer at `:1340, :1351`); the inline wrapper (`Span`) does **not** (writer at `:1422, :1425`). Unwrap and rewrap have separate code paths for the two wrappers; a single "always strip Plain" or "always wrap in Plain" implementation would be wrong for one side. Test fixtures must explicitly exercise inline CustomNodes (`CrossrefResolvedRef`) to catch this — pure block-CustomNode round-trips won't.
- **Walker purity invariant for `unwrapCustomNodes`.** The Note-numbering `WeakMap<NoteInline, number>` lookup in `Note.tsx` depends on unwrap returning unmodified subtrees by reference. A naïve "rebuild every container" implementation would silently break the WeakMap. Mitigation: §"Walker purity contract" pins the contract; the reference-preservation property test in Phase 5.1 locks it.
- **`__quarto_custom_node` class polluting rendered DOM after user override**. Resolved by design: unwrap is the single forward-path conversion, runs before any registry dispatch. The `Div` registry slot only sees real Divs.
- **Image alt-text edge cases**: Stringify pass must handle every Pandoc inline that can appear in alt context; missing one degrades alt to empty. Test coverage explicitly walks the inline taxonomy.
- **Blob URL revocation timing.** Walker revokes prior URLs on cache eviction (content hash changed, or path no longer in AST). The cache-keyed-by-content-hash design avoids the revoke-then-fetch race because content-stable images keep the same URL across re-renders. Wholesale revocation only happens on iframe unmount (when the iframe is being torn down anyway). Risk: walker bug causes premature eviction → `<img>` 404s. Mitigated by the asset-walker tests above asserting the cache-hit path does not revoke.
- **Manifest-AST atomicity.** Manifest must arrive *with* the AST it describes — an `Image` rendered before its manifest entry arrives produces a broken image. The current postMessage model guarantees atomicity (manifest and AST share the `UPDATE_AST` payload). If a future plan splits them into separate messages, this contract breaks; flag clearly in the manifest plumbing section.
- **Manifest miss masquerading as success.** Manifest-miss fallback returns the original URL (e.g. `hero.png`), which the iframe's browser will fail to load. Looks like a broken image. Distinguishing "walker bug" from "user typo" requires looking at the manifest in DevTools. v1 accepts this; future enhancement: render an explicit "asset not found: <path>" placeholder.
- **Recursion-contract bypass in user overrides.** Documented in §Design decisions "Recursion contract for the atomic gate." The atomic gate fires only when nodes enter via framework's `<Node>`; user TSX components are free to ignore the contract by iterating `node.c` directly into hand-rolled JSX, silently disabling atomicity for their descendants. v1 has no edit affordances so the failure is latent, but it becomes a real corruption vector once editing ships. Mitigation: a vitest fixture in `q2-preview.integration.test.tsx` that mounts a deliberately-bypassing override over an atomic CustomNode child, asserts the child reaches a non-NOOP `setLocalAst`, and snapshots the result. Locks the contract as observable behavior so the day someone wants to harden it, the regression fixture is already in tree and the contract docs are pre-written.

(Element-and-structure drift, class-taxonomy enumeration completeness, and Equation `\tag{N}` JS-port risks all apply to Plan 2C's CustomNode renderers, not 2B.)

## Estimated scope

| Component | Lines (rough) |
|---|---|
| Framework: `customNode.ts` (unwrap + rewrap, structural JSON walk, block/inline asymmetry, walker purity) | ~220 |
| Framework: `types.ts` CustomNode shapes + `CiteInline` | ~70 |
| Framework: `Ast.tsx` co-edits (discriminated input, unconditional unwrap, sourceInfoPool extraction) | ~15 |
| Framework: atomic gate inside `Node` (`framework/dispatch.tsx`) + tests | ~50 |
| Framework: CustomBlock/CustomInline entries in `renderChildrenRegistry` + `blockTypes` extension (8 entries) | ~50 |
| `hub-client/src/utils/vfsPaths.ts` (NEW; extract `resolveRelativePath`/`normalizePath`/`guessMimeType`) + 3 call-site migrations | ~+10 net (~50 new, ~40 deleted) |
| q2-preview/blocks/*.tsx (14 files; 11 existing-pattern + 3 gap fills) | ~250 |
| q2-preview/inlines/*.tsx (20 files; 12 existing-pattern + 8 gap fills + Math + Note + Cite) | ~220 |
| q2-preview/utils.ts (lookupAssetUrl, inlinesToPlainText, blocksToPlainText) | ~70 |
| q2-preview/quartoClasses.ts (stub: section/levelN, footnotes, appendix) | ~30 |
| q2-preview/NoteNumberingContext.tsx + JS-side numbering walk + Note.tsx tooltip-body fallback (temporary until bd-1kly) | ~30 |
| q2-preview/registry.ts assembly (Block/Inline + Pandoc base entries; no CustomBlock/CustomInline) | ~30 |
| q2-preview/entry.tsx rewrap (in `setAst` callback) + assetManifest extraction + AssetManifestContext provider + Note-numbering useMemo | ~25 |
| q2-preview/assetWalker.ts (parent-side walker, cache via base64 string, revocation) | ~110 |
| q2-preview/AssetManifestContext.tsx | ~15 |
| Q2PreviewIframe.tsx walker integration (cache ref, useMemo, payload extension, unmount cleanup) | ~30 |
| `crates/quarto-core/src/pipeline.rs` — remove `"footnotes"` and `"appendix-structure"` from `Q2_PREVIEW_TRANSFORM_EXCLUDED` | ~2 |
| Tests (round-trip, base-component snapshots, atomic, Derived, Image edge cases, asset walker, Q2PreviewIframe integration, reference-preservation, recursion-contract bypass, q2-debug atomic-gate parity) | ~310 |
| WASM integration test (assetManifestProject.wasm.test.ts) | ~50 |
| Smoke-all q2-preview infrastructure (PreviewIframeKind extension — bundled with Plan 2A item 12) | ~10 |
| Smoke-all q2-preview fixture (image-with-attrs.qmd + PNG asset) | ~30 |
| **Total** | **~1625** |

Reasonable for one focused session.

**Sub-ordering**:
- customNode.ts and the Ast.tsx co-edits land in a single commit (the unwrap call and `sourceInfoPool` wiring are tightly coupled — a half-state would route un-unwrapped Divs to the format dispatcher).
- vfsPaths.ts extraction is the first commit of the asset-manifest cluster, so assetWalker.ts can import the canonical helpers. Pure refactor, no behavior change; reviews fast.
- Pipeline.rs change can land any time during Phase 3 — the smoke-all `image-with-attrs.qmd` fixture doesn't exercise the FootnotesTransform inclusion, but Note.tsx's behavior depends on it.
- `quartoClasses.ts` stub lands before Phase 3 components reference it.
  - **Sub-ordering**: quartoClasses.ts (the enumeration commit) lands first; per §"`q2-preview/quartoClasses.ts`", the class taxonomy must be pinned before consuming components are written.

Risk: Table family is the highest-effort single component (~80 LOC). Budget extra time. Asset walker has a lot of test surface (cache hit/miss, revocation, externals filtering); budget time for those tests in Session A. Equation's JS-side `\tag{N}` port is also non-trivial (~30 LOC mirroring `crossref_render.rs:601`).

**Cross-plan ordering**: Plan 2A is fully landed (`fe40973b` + follow-ups `81e48f10` + `e6381abd` + `0887a3fa`). Plan 2B can start immediately. The smoke-all `PreviewIframeKind = 'q2-preview'` extension that 5.2 depends on landed in 2A item 12; verified at `hub-client/e2e/helpers/previewExtraction.ts`.

## Related beads issues

Tracked work *outside* 2B's scope that 2B's design assumes or that 2B's temporary measures hand off to:

- **bd-1kly** — *Complete `FootnotesTransform` for `reference-location: block`/`section`.* Upstream Rust fix for the gap that 2B's `Note.tsx` tooltip-body fallback works around. When closed, q2-preview's `Note.tsx`, `NoteNumberingContext`, and the JS-side numbering walk all become inert and can be deleted (~30 LOC removed). Also unblocks the tippy.js popup integration noted in §"Reference popups note."

Future plans that decorate the AST 2B renders (Plans 4 / 5 / 6 / 7 / 8) are tracked in §"Soft activation dependencies" rather than here.

## Notes

- This plan was originally a single monolithic Plan 2B (~1781 lines, ~2375 LOC implementation surface) that outgrew the realistic single-session context budget. On 2026-05-09 it was split into Plan 2B (Session A — Pandoc base + framework + asset manifest) and Plan 2C (Session B — Quarto custom-node renderers + verification). Plan 2C is at `claude-notes/plans/2026-05-09-q2-preview-plan-2c-customnode-rendering.md`. Plan 2B does not need to land Plan 2C in the same PR / session; the user gates progress between Session A and Session B with a visual verification pass.
- This plan replaces the original Plan 2B, which framed `html.tsx` and `custom.tsx` as "drafts pasted into demos." The 2026-05-07 review established that q2-preview is a sibling format with its own built-in registry; the paste-in pattern still works for user overrides but is no longer the default delivery mechanism.
- The atomic-aware gate moved from "modify q2-debug's dispatcher" to framework's `Node` (the single recursion chokepoint, in `framework/dispatch.tsx`) — benefits both formats automatically without modifying either format's dispatcher.
- Image and Figure moved into 2B as the natural place for "Pandoc base type leaves with full semantics."
- Following the user's lead: q2-preview is intended to evolve toward a system component (likely a Quarto extension), but the bundling / distribution mechanics are out of scope for 2B.

### Revision history

- **2026-05-09 (Plan 2B/2C split)**: monolithic plan split into two for execution. Session A scope (this plan, 2B): framework changes + asset-manifest plumbing + Pandoc base-type leaves + footnotes/appendix pipeline change + Note.tsx + stub `quartoClasses.ts` + image-with-attrs smoke fixture + `assetManifestProject.wasm.test.ts`. Session B scope (Plan 2C): Quarto-feature CustomNode components + full `quartoClasses.ts` taxonomy + `CustomNodeRegistryContext` + multi-element smoke fixtures + `customNodeWireFormatProject.wasm.test.ts` + demo fork + `cargo xtask verify --e2e`. After the split: 2B Goal narrowed to "M2 — Pandoc base types"; 2C Goal is "M2.5 — Quarto custom nodes." User confirms M2 visually before Session B starts; M2.5 visually after Session B. Estimated scope dropped 2375 → 1625 LOC (Plan 2B) + ~1075 LOC (Plan 2C). Two amendments to 2B's scope from the split: (1) `quartoClasses.ts` ships as a stub (footnote/appendix/section constants only); 2C extends. (2) `assetManifestProject.wasm.test.ts` and a single `image-with-attrs.qmd` smoke fixture pulled into 2B's Phase 5 so 2B is self-locking and 2C doesn't inherit verification debt for 2B's work.



- **2026-05-09**: Asset-transport architecture switched to blob-URL manifests (Plan 2A's Design B applied to images):
  - `Image.tsx` no longer reads VFS bytes — became a pure `AssetManifestContext` consumer that looks up `node.target.0` in the manifest. Removed `PreviewContext` dependency from `Image.tsx`. (`PreviewContext` continues to carry `currentFilePath` for link handlers.)
  - New parent-side asset walker (`q2-preview/assetWalker.ts`) walks the AST for `Image` nodes, resolves paths against `currentFilePath`, reads VFS bytes, mints blob URLs (cache-keyed by content hash for stable URLs across re-renders), and produces a manifest. Cache eviction triggers `URL.revokeObjectURL`.
  - New `AssetManifestContext.tsx` distributes the manifest to iframe components.
  - Manifest piggybacks on the existing `UPDATE_AST` payload — no new message type, manifest-and-AST always arrive together.
  - `q2-preview/utils.ts`: `resolveImageSrc` removed; `lookupAssetUrl(manifest, url)` added.
  - Multi-plan contract section retitled to reference Plan 2A's "blob-URL asset contract" and describe 2B's image manifest as the application of that contract.
  - Test plan: Image renderer tests stop mocking `vfsReadBinaryFile`; mock the `AssetManifestContext` value instead. New asset-walker test suite (cache hit, cache eviction with revocation, external skipping, manifest-AST atomicity via `Q2PreviewIframe` integration test). smoke-all `image-with-attrs.qmd` assertion changed from `src^="data:image/"` to `src^="blob:"`.
  - Risk areas: added blob-URL revocation timing, manifest-AST atomicity, manifest-miss masquerading.
  - Estimated scope: ~1750 → ~1980 LOC (added ~150 LOC for walker, context, integration, and walker tests; saved ~10 LOC by simplifying utils).
  - Rationale: aligns with Plan 2A's unified asset-transport architecture (URL strings on the wire, bytes only on the parent). Maps cleanly onto a future service-worker swap-in: replace the parent walker + manifest with SW request interception, and `<img src>` semantics in the iframe stay unchanged.
- **2026-05-09 (downstream of 2A research follow-ups)**: 2A's `PreviewRoot` was restructured to be narrowly scoped (`PreviewContext.Provider` + link handlers + `<Ast>` mount only — theme handling moved to module-top in `entry.tsx` to fix a postMessage race). 2B's `AssetManifestContext` extension fits the same prop-forwarding pattern: 2A's top-level `updateAst(payload)` callback destructures `assetManifest` from the payload and forwards as a prop. PreviewRoot wraps the `<Ast>` mount with `<AssetManifestContext.Provider>` alongside the existing `<PreviewContext.Provider>`. No structural change to 2B; the §"AssetManifestContext.tsx" prose was tightened to reference 2A's now-explicit `PreviewRootProps` interface instead of the looser "extracts from payload state" phrasing.

- **2026-05-09 (audit pass against current sources)**: comprehensive review of plan claims against `crates/pampa/src/{readers,writers}/json.rs`, `crates/quarto-core/src/{pipeline.rs,crossref/mod.rs,transforms/}`, and the post-2pre `hub-client/src/components/render/` tree. Substantive changes:
  - **§`framework/customNode.ts`** rewritten with full algorithm (~80-line spec, structural JSON-level walk per design decision, block/inline asymmetry made explicit). Key correction: the wire format wraps `Inline` / `Inlines` slots in a `Plain` block **only** for block CustomNodes (`Div` outer); inline CustomNodes (`Span` outer) embed inlines directly with no Plain wrapper. The previous "strips the Plain wrapper from Inline / Inlines slots" blanket rule was wrong for the inline side.
  - **Unwrap location moved out of `entry.tsx` into `framework/Ast.tsx`** to match the plan's "both formats see it" claim. Only `rewrapCustomNodes` lives in entry.tsx now (in the `setAst` callback). `Ast.tsx` gains a 2-line co-edit alongside `sourceInfoPool` extraction (Plan 2A item 4 typed the field but didn't fill it).
  - **`blockTypes` extension is 8 entries, not 1**: gap-fill leaves (`LineBlock`, `DefinitionList`, `Table`), the `CustomBlock` post-unwrap discriminator, and four defensively-routed-as-block tags (`BlockMetadata`, `NoteDefinitionPara`, `NoteDefinitionFencedBlock`, `CaptionBlock`). Without these the format's `Inline` dispatcher would render the placeholder.
  - **Equation `\tag{N}` is JS-side, not Rust-side**: q2-preview's pipeline excludes `CrossrefRenderTransform`, so `Equation.tsx` ports `crossref_render.rs:601` into JS. The original Open Question framing ("confirm `\tag` survives slot dispatch") was mis-framed — `\tag` was never appended on the Rust side for q2-preview.
  - **`Math.tsx`** subsection added — near-verbatim port of Elliot's `html.tsx:259–279` with two divergences (no console statements; explicit raw-LaTeX fallback on KaTeX error).
  - **`hub-client/src/utils/vfsPaths.ts`** extracted as a first commit — `resolveRelativePath`, `normalizePath`, `guessMimeType` had three near-duplicate private copies; consolidate to one canonical module before the asset walker introduces a fourth consumer.
  - **Asset walker cache key** changed from `hashBytes(content)` (algorithm unspecified) to the base64 string itself — `vfsReadBinaryFile` returns base64, identical bytes → identical base64, no hash needed, fully synchronous (compatible with the `useMemo` pattern Plan 2A item 6 specifies).
  - **`Fallback`** name canonicalized — pseudocode alternated between `GenericFallback` and `__fallback__`. Symbol is `Fallback`, file is `Fallback.tsx`, registry key is `'__fallback__'`.
  - **CustomNode `type_name` enumeration**: six concrete strings (`"Callout"`, `"Theorem"`, `"Proof"`, `"FloatRefTarget"`, `"Equation"`, `"CrossrefResolvedRef"`) plus `"IncludeExpansion"` (Plan 8 dormant) — round-trip property test now lists them explicitly.
  - **`renderChildrenRegistry['CustomBlock'|'CustomInline']` consumer pinned**: `Fallback`. Per-type components (Callout, Theorem, …) drive their own slot rendering via `renderSlot`; the framework registry entries are the generic-walk fallback.
  - **`previewRegistry` typed `FormatRegistry`** (matches existing `q2-preview/registry.ts`); `customNodeRegistry` stays `Record<string, ...>` since its keys are dynamic.
  - **Cross-plan ordering** for Plan 2A's unchecked items (6, 9, 12) made explicit in §Multi-plan contracts and §Sequencing.
  - Estimated scope: ~1980 → ~2130 LOC (primarily customNode.ts grows ~60 LOC for the full algorithm, `q2-preview/custom/` grows ~30 LOC for Equation's `\tag{N}` port, vfsPaths.ts adds ~+10 LOC net, blockTypes extension and Ast.tsx co-edits add ~25 LOC, tests grow ~40 LOC for inline-CustomNode and asymmetry coverage).

- **2026-05-09 (post-Plan-2A audit + open-question research)**: Plan 2A landed (`fe40973b` + follow-ups). Re-audit confirmed all 2B assumptions hold against the as-shipped 2A surface. Refinements:
  - **"User overrides win"** pinned in §Design decisions, citing the encoded merge order at `q2-preview/entry.tsx:179–182` (`{ ...previewRegistry, ...customRegistry }` — user TSX shadows built-ins for colliding keys).
  - **Class taxonomy enumerated up front** in §"`q2-preview/quartoClasses.ts`". Pinned constants for Callout (10 names + subtype/appearance prefixes), Theorem (`theorem`, `theorem-title`), Proof (`proof` only — **no `proof-title`**, the label is an inline `<em>`), Equation (no class — preserves user attr), FloatRefTarget (no class — preserves user attr), CrossrefResolvedRef (`quarto-xref` Link), Section (`section`, `level1..6`). Cross-referenced to Rust line numbers in `crates/quarto-core/src/transforms/{callout_resolve,crossref_render,float_ref_target}.rs` and `crates/pampa/src/transforms/sectionize.rs`.
  - **Proof component fixed**: dropped the spurious `proof-title` class. The Proof CustomNode renders as `<div class="proof"><p><em>Proof.</em> ...body...</p></div>` per `crossref_render.rs:566–575`.
  - **`plain_data.order.order` access path verified**: `crossref_index.rs:280–289` writes `plain_data.order = { section: [...], order: n }` for every numbered crossref CustomNode (Theorem, Proof, FloatRefTarget, Equation). The pseudocode in §"`q2-preview/custom/`" Equation entry was already correct — `node.plain_data?.order?.order` reads the integer.
  - **Math.tsx uses direct ESM `import katex`** rather than `window.katex`. The entry imports katex statically (`entry.tsx:30`); reading `window.katex` from a built-in component is unsafe because `window.katex` is set inside `loadCustomComponents` and `undefined` for documents without user TSX overrides. `window.katex` continues to be set for user TSX that expects it (e.g. users who paste Elliot's `html.tsx`).
  - **q2-preview pipeline construction refactor noted** (`0887a3fa`): `build_q2_preview_transform_pipeline` is now `build_transform_pipeline` filtered by `Q2_PREVIEW_TRANSFORM_EXCLUDED` (deny-list), not an explicit allow-list. The set of excluded transforms is unchanged (still excludes `crossref-render`, `callout-resolve`); 2B's CustomNode-survival assumption holds.
  - **Cross-plan ordering risk dropped**: Plan 2A is fully landed, so 2B can start immediately. Phase 2 dependency note relaxed; §Hard dependencies list marked all three (2pre / 2A / 1) as ✅ landed.
  - **Smoke-all `PreviewIframeKind = 'q2-preview'`** verified at `previewExtraction.ts:23`. §"Smoke-all q2-preview infrastructure" subsection collapsed to a one-line note.
  - No estimated-scope change.

- **2026-05-09 (verification-process gap follow-up)**: lessons from Plan 2A's manual-browser-test discovery of two project-mode bugs (`81e48f10` `theme_fingerprint` missing in default-project mode; `e6381abd` theme CSS path mismatch in project mode). Plan 2B had the same single-doc-only blind spot. Substantive changes:
  - **Project-context coverage rule** added to §"Test-tier conventions" — every WASM-path-significant feature must have at least one test covering single-doc, default-project, and (where applicable) website-project. Hard rule, not optional.
  - **WASM integration tests subsection** added to §"Test plan" — `assetManifestProject.wasm.test.ts` (asset manifest in project mode) and `customNodeWireFormatProject.wasm.test.ts` (CustomNode wire format in project mode), pattern follows `themeFingerprint.wasm.test.ts`. Phase 5 grew item 5.3 for these tests.
  - **Two new smoke-all fixtures**: `multi-element-project/` (default-project counterpart of the existing `multi-element-doc.qmd`) and `with-render-components/` (project-mode override fixture replacing the manual confirmation in §"Fork Elliot's demos"). Phase 5 item 5.2 updated.
  - **`themeFingerprint.wasm.test.ts` preservation note**: Plan 2A's regression test must remain when 2B touches `pass2_renderer.rs`. Documented in §"WASM integration tests" and as part of item 5.3.
  - **Manual override confirmation removed**: §"Fork Elliot's demos" no longer asks for manual end-to-end browser verification — that role is now filled by the `with-render-components/` smoke fixture. Manual checks are exactly the kind of verification that hid Plan 2A's bugs.
  - **Stale line refs updated**: `pipeline.rs:1039–1042` (which now points at the deny-list rationale doc-comment) replaced with `Q2_PREVIEW_TRANSFORM_EXCLUDED` at `pipeline.rs:1049`, with `"crossref-render"` entry at `:1061`, in two §Scope locations (Equation entry; Resolved Open Question entry). `iframeLinkHandlers.ts:116` extended to mention `normalizePath` at `:123` (both functions need extraction, not just `resolveRelativePath`).
  - Estimated-scope change: ~2130 → ~2230 LOC (+~80 LOC for two WASM tests, +~70 LOC for two new smoke fixtures, –~50 LOC saved by dropping manual-override verification overhead).
  - Rationale: plan-level encoding of the verification-process lesson so future Plan-N work doesn't repeat the trap. The cost of two extra fixtures and two WASM tests is small; the cost of project-mode bugs slipping past CI to manual browser sessions is high.

- **2026-05-09 (final open-question pass + with-render-components fix)**: closed all surviving open questions and corrected one cascading issue.
  - **`FootnotesTransform` included in q2-preview's pipeline** (one-line `pipeline.rs` change: drop `"footnotes"` from `Q2_PREVIEW_TRANSFORM_EXCLUDED`). v1 q2-preview is structural rendering only, so the transform's "synthesize-with-no-preimage" exclusion rationale doesn't bite yet. Effects: auto-numbered footnotes (no JS pre-walk needed), footnotes section at document end, automatic resolution of `NoteDefinitionPara` / `NoteDefinitionFencedBlock`. New classes (`footnotes`, `footnote-ref`, `footnote-back`) added to `quartoClasses.ts`. `Note.tsx` retitled as defensive fallback for the rare `reference-location: block`/`section` cases.
  - **CustomNode override mechanism added.** Discovered while auditing the agent-added `with-render-components/` smoke fixture: the original merge order (`{ ...previewRegistry, ...customRegistry }`) only merged into the Pandoc-tag registry, so a user export of `Callout` (a CustomNode `type_name`, not a Pandoc tag) never reached the dispatch path. Fix: introduce `CustomNodeRegistryContext` (sibling of `AssetManifestContext`); `PreviewRoot` computes `mergedCustomNodeRegistry = { ...customNodeRegistry, ...customRegistry }` from the same user-TSX export bag (Pandoc tags and CustomNode type_names live in disjoint namespaces, so a single merge is unambiguous). `CustomBlock`/`CustomInline` dispatchers in `registry.ts` become `useContext`-reading wrapper components. User API unchanged (`render-components: [...]` with named exports). `with-render-components/` fixture updated to test both override directions (Para + Callout).
  - **Cite rendering — resolved.** v1 renders `c[1]` (visible inlines) via `renderChildren`; ignores `c[0]` (citations metadata, which future bibliography rendering will consume).
  - **Quarto Image extensions — resolved.** v1 silently ignores unknown Image kvs (current Image.tsx only passes width/height); future plan parallel to layout-chrome picks them up.
  - **"Three WASM render entry points" reworded** to "two entry points with internal default-vs-website branch in the project entry." More accurate; doesn't water down the project-context coverage rule.
  - Estimated scope: ~2230 → ~2255 LOC (+~25 LOC for `CustomNodeRegistryContext.tsx` + PreviewRoot extension; +~10 LOC for registry.ts dispatchers becoming `useContext`-reading wrappers; +~10 LOC tests for customNode override; +1 LOC pipeline.rs change; –~10 LOC saved by dropping JS pre-walk for note numbering).
  - Rationale: closes the "open questions" section by either resolving each item explicitly or punting with a clear deferred-work pointer. Eliminates the with-render-components fixture / dispatch-mechanism mismatch the previous revision left in.

- **2026-05-09 (visual-parity contract + extra transform inclusions + sandbox rationale)**: tightened the visual-fidelity contract from "class-compatible" to "Bootstrap-flavored visual + structural parity," researched two more transforms for inclusion, and documented the iframe-sandbox rationale for direct katex import.
  - **§Goal** rewritten to frame visual + structural parity (not just class parity) and to call out Bootstrap as the explicit target for class names. Goal sentence references the Plan-2A theme.css plumbing that makes the parity load-bearing.
  - **§Design decisions "Visual fidelity tier"** replaced by **"Visual + structural parity target"** — element parity (per-tag table), class parity (cross-referenced to `quartoClasses.ts`), explicit "where divergence is allowed" / "where divergence is forbidden" boundaries with the "child-selector CSS rules require child-element nesting" rationale. Closes the gap the previous "DOM may differ where it doesn't affect CSS" framing left open.
  - **§Risk areas** "Drift between Rust's HTML output and our React rendering" rewritten to call out the two element-and-structure drift classes (wrong element vs. wrong nesting depth) and to point at the test plan's element-structure assertions and smoke-all `multi-element-doc.qmd` selector list.
  - **`AppendixStructureTransform` included in q2-preview's pipeline** (second one-line `pipeline.rs` change: drop `"appendix-structure"` alongside `"footnotes"` from `Q2_PREVIEW_TRANSFORM_EXCLUDED`). Pure Pandoc primitive output (`Div`, `Header`, `Paragraph`, `Link`); structurally identical to the HTML pipeline. Effects: footnotes section nests inside `<div id="quarto-appendix">`; license/copyright/citation YAML metadata renders automatically; bibliography branch is inert until Citeproc lands. Five new classes added to `quartoClasses.ts` (`quarto-appendix`, `quarto-bibliography`, `quarto-reuse`, `quarto-copyright`, `quarto-citation`).
  - **`TitleBlockTransform` deferred to a follow-up plan**, not included. Its `should_add_h1` gates on `is_html()`; q2-preview maps to HTML, so the transform no-ops. But q2-preview also bypasses the HTML template (`ApplyTemplateStage` excluded), so neither branch produces a body title. Making the transform useful for q2-preview requires a Rust change (a new `template_will_run` accessor or treating q2-preview as forced-`minimal: true`) that reaches beyond Plan 2B's scope. Pre-existing 2A gap, not a 2B regression — documented in §"Pipeline change: `TitleBlockTransform` is not included".
  - **Iframe-sandbox + same-origin Vite bundling rationale** added to §"`q2-preview/inlines/Math.tsx`" — the `allow-same-origin` token in `Q2PreviewIframe.tsx:168`'s sandbox attribute combined with same-origin bundle resolution is what makes the static `import katex from 'katex'` safe; without it we'd fall back to the global-via-postMessage pattern. Future readers can follow the rationale without reconstructing it from sandbox-attribute first principles.
  - Estimated scope: ~2255 → ~2255 LOC (no measurable change; transform inclusions are 2 lines in pipeline.rs, plus 5 lines of class constants, plus minor doc edits). The new visual-parity contract is process and test-plan, not LOC.
  - Rationale: the agent-flagged ambiguity on visual fidelity ("we'll wear the same uniform but the body underneath might be shaped differently") is closed. The two transform decisions (include appendix, defer title-block) are both motivated and reversible. The sandbox rationale is the kind of one-line note that prevents a future reader from re-litigating an already-resolved design choice.

- **2026-05-09 (review session: implementor-detail pass)**: comprehensive amend after a research pass against current sources. Settled the implementation-detail gaps that would have surfaced as questions during 2B's implementation phase. Substantive changes:
  - **Recursion contract for the atomic gate** added to §Design decisions and mirrored in §Risk areas. The gate fires only when nodes enter via framework's `<Node>`; user-TSX overrides MUST recurse via `<Node>` / `renderChildren` / `renderSlot`, never iterate `node.c` into hand-rolled JSX. Verified Elliot's existing demos satisfy the contract today (every child-rendering path uses `renderChildren` or delegates to the framework's `Block`/`Inline` dispatcher). Phase 5.1 gains a deliberate-bypass regression fixture so the contract is observable behavior, not folklore.
  - **§"`q2-preview/custom/`" rewritten** from a per-component prose bullet list to per-component subsections with explicit slot lists, `plain_data` field tables (writer file:line + JSON type + reader file:line for every field), and pseudo-output structure. Audited against `crates/quarto-core/src/transforms/{callout,callout_resolve,theorem,proof,float_ref_target,equation_label,crossref_resolve}.rs` and `crates/quarto-core/src/transforms/crossref_render.rs`. Closes the implementor's "what's actually in plain_data" gap.
  - **`q2-preview/theoremEnvs.ts` (NEW, ~15 LOC)** carved out as its own file. Hosts the JS port of `theorem_env_for` (`crossref_render.rs:388-400`). Theorem env classes (`lemma`, `corollary`, `proposition`, `conjecture`, `definition`, `example`, `exercise`) are a closed 8-entry mapping in Rust, not user-config-driven; the prior plan's "config-driven" framing was wrong. `quartoClasses.ts` stays constants-only; `theoremEnvs.ts` is a function.
  - **Callout structure pinned 3-deep** with the icon-conditional `<i class="callout-icon">` inside `.callout-icon-container`, `.callout-title-container.flex-fill`, and `.callout-body-container.callout-body`. The icon is gated on `plain_data.icon === true`; `flex-fill` was missing from the plan and is mandatory for the title to fill horizontal space next to the icon. Default title (when `slots.title` is absent) is the capitalized `type` per `callout_resolve.rs:264`.
  - **FloatRefTarget slots pinned to three** (`content`, `caption_long`, `caption_short`); plan previously implied two. Figure-vs-div discriminator is `plain_data.ref_type === "fig"` per `crossref_render.rs:263-290`.
  - **CrossrefResolvedRef gains a `suffix` slot** documenting the trailing-text fragment Pandoc fills in for citations like `@fig-1 (and onwards)`. Resolved-vs-unresolved text format pinned (`{kind} {order.order}` with NBSP, or `?{identifier}?`) per `crossref_render.rs:704-715`.
  - **Note.tsx fallback rewritten** as a JS-side number-with-tooltip-body fallback. Replaced the prior `<sup>[?]</sup>` placeholder, which was based on the stale `footnotes.rs:99` comment ("Pandoc handles this — no-op") that's left over from when the architecture assumed real Pandoc would be downstream of pampa. Audit found pampa's HTML writer at `html.rs:806-817` also doesn't handle block/section configs correctly (emits `<sup>[N]</sup>` where `N` is the *length* of the note content array, not a sequential number, with a TODO acknowledging the gap). The proper upstream fix is bd-1kly (extend `FootnotesTransform` to handle block/section uniformly); q2-preview's tooltip fallback is explicitly temporary. New `q2-preview/NoteNumberingContext.tsx` distributes the JS-walk numbering map; `Note.tsx` emits `<sup class="footnote-ref" title="{stringified body}">{number}</sup>`. Scope: ~30 LOC; deletable when bd-1kly closes.
  - **Reference-popups note** added (informational, not in scope) — TS Quarto's hover popups are a tippy.js layer over the standard `<sup class="footnote-ref">` markup. Once bd-1kly lands, q2-preview can include tippy.js and get popups for free; flagged so 2B's plumbing isn't designed to preclude it.
  - **`unwrapCustomNodes` walker scope** explicitly pinned: descends only into `c` fields, never into `plain_data`. Verified all current `plain_data` producers emit flat shapes (primitives, arrays of primitives, `{ section: usize[], order: usize }` records). The invariant is checked transitively by the inline-CustomNode round-trip property test.
  - **Phase 5.1 test-replacement note** added: four existing tests in `q2-preview.integration.test.tsx` (Plan 2A's empty-registry placeholder contract) get explicitly replaced — not appended-around or skipped. Without the call-out, an implementor who doesn't know the file pre-existed could leave stale assertions in tree.
  - **Phase 2.1 helper-extraction line refs** corrected — the prior text said "ReactAstSlideRenderer.tsx (lines 886, 913)" but the file actually has three private copies at 886/900/913. The §References list elsewhere was already correct; only the Phase-2.1 checklist line was wrong.
  - **§Cache key rationale** tightened — distinguishes cache-key composition (`${path}\0${base64}` for clarity / debug) from content identity (the base64 string itself, which is the deterministic 1-to-1 byte encoding). The prior framing implied the path was unnecessary, contradicting the actual implementation.
  - **`write_custom_inline` line ref** corrected from `:1380` to `:1381` in §References (matches the in-prose `:1381` reference at the algorithm section).
  - **Beads issue filed**: bd-1kly tracks the upstream `FootnotesTransform` completion. Plan now references it from §"Pipeline change: include `FootnotesTransform`" so the temporary nature of `Note.tsx` is recoverable from the plan.
  - Estimated scope: ~2255 → ~2300 LOC (+15 for `theoremEnvs.ts`, +30 for `NoteNumberingContext.tsx` + numbering walk + `Note.tsx` tooltip fallback). The §"`q2-preview/custom/`" rewrite is documentation churn, not LOC.
  - Rationale: every change in this revision is the result of either (a) an implementor-detail gap surfaced by the research pass, (b) a stale comment in the source we should not perpetuate, or (c) a contract that was load-bearing at edit-time but unwritten. No policy decisions; the plan now contains the details an implementor needs to start without re-investigating Rust source.

- **2026-05-09 (implementor-review clarifications pass)**: tightened the framework-side contracts that were correct in spirit but not airtight in letter, plus closed four open questions that were unwritten in earlier revisions. Substantive changes:
  - **§Walker purity contract** added under `framework/customNode.ts` — pins `unwrapCustomNodes` as a structural-sharing pure function (subtrees containing no wrappers are returned by reference; copy-on-write for the path to a transformed node). Load-bearing for the Note-numbering `WeakMap<NoteInline, number>` contract; without the invariant a naïve "rebuild every container" walk would silently break the WeakMap. New reference-preservation property test.
  - **§`framework/Ast.tsx` co-edits** rewritten to a 3-step ordered list: (1) discriminated input acquisition (`'ast' in props ? props.ast : JSON.parse(props.astJson)`), (2) unconditional `unwrapCustomNodes(parsed)`, (3) `astContext.sourceInfoPool` extraction onto the Provider value. Explicitly states unwrap runs on both branches; q2-debug stays on the string path.
  - **§Rewrap walker scope** added — descend both `c` fields and `slots` values; same purity contract as unwrap. Closes the symmetric-scope gap.
  - **§"CustomBlock / CustomInline traversal" pseudocode** added — concrete `renderChildrenRegistry` entries matching the existing signature `(args: {node, setLocalAst, onNavigateToDocument?}) => ReactNode`. Iterates `Object.entries(node.slots)`, emits `<Node>` per slot value, copy-on-write `setLocalAst`. No JSX wrapper element (caller decides).
  - **§`q2-preview/utils.ts`** now lists `blocksToPlainText(blocks): string` as a companion to `inlinesToPlainText`. Used by `Note.tsx`'s `title=` attribute. Walks all block variants and delegates to `inlinesToPlainText` for inline content. ~30 LOC.
  - **§`Note.tsx` render** now calls `blocksToPlainText(node.c)` capped at 250 characters with `…` truncation. Browser `title=` UI truncates inconsistently around 300 chars; cap matches the lower bound.
  - **§Equation defensive fallback** — three branches for non-canonical `slots["content"]` shapes (empty Inlines / first inline isn't `Math(DisplayMath)` / first is correct but with sibling inlines). All branches render `<span id={id}>` and never crash; the canonical single-Math case is unchanged. ~5 LOC of defensive code per branch.
  - **§`framework/types.ts`** adds `CiteInline` (verified absent today). Cite can appear in any AST that uses `@…` citation syntax; missing the type was a latent gap. `c[0]` typed as `unknown[]` until bibliography rendering ships a real Citation shape.
  - **§Cite component pseudocode** added — `const [, inlines] = node.c; return <>{inlines.map((inl, i) => <Node key={i} node={inl} ... />)}</>`. Single Fragment, no wrapper. `c[0]` ignored in v1.
  - **§Class-compatibility "compile-time drift catch" claim softened** — replaced with accurate two-layer framing (vitest runtime assertion + smoke-all `multi-element-doc.qmd` end-to-end check). References the existing `pipeline.rs:1987` and `:2053` validation tests that already lock exclusion-name spelling against the actual transform/stage lists.
  - **§Smoke-fixture mechanics pinned** — verified frontmatter shape (`_quarto.tests.run.requires_js: true`, `_quarto.tests.q2-preview.ensureHtmlElements: [['selector'], ...]`), verified default-project `_quarto.yml` shape (minimal `project:\n  title: ...`, no `type:` key). `multi-element-doc.qmd` extended to include a footnote and a `license:` metadata key so the two pipeline-inclusion changes (`FootnotesTransform`, `AppendixStructureTransform`) get smoke coverage; previously they had only unit-test coverage.
  - **§q2-debug atomic-gate test parity** added to Phase 5.1 — q2-debug's integration test (verified: 8 tests, zero atomic-gate coverage) gets at least one `CrossrefResolvedRef` no-op assertion. Symmetric to q2-preview's gate test.
  - **§Stage exclusion list documented** — `Q2_PREVIEW_STAGE_EXCLUDED` at `pipeline.rs:334` is `["code-highlight", "math-js", "render-html-body", "apply-template"]`. 2B leaves it unchanged; documenting so it's not re-investigated.
  - **§Open questions resolved**: React StrictMode (verified q2-preview iframe entry has no StrictMode wrapper at `entry.tsx:209-227`; double-render concern doesn't apply); KaTeX bundle weight (accepted for v1, route to `manualChunks` if needed); `IncludeExpansion` shadowing user exports (inert until Plan 8); Note tooltip body length cap.
  - Estimated scope: ~2310 → ~2360 LOC (+15 walker purity test + reference-preservation property test, +20 q2-debug atomic-gate parity test, +10 Cite component + framework/types.ts entry, +5 Equation defensive fallback). The renderChildrenRegistry pseudocode replaces empty prose; no LOC delta there.
  - Rationale: every change is the result of the prior implementor review surfacing a contract that was load-bearing at edit-time but unwritten, or a piece of source state (StrictMode, _quarto.yml shape, `Q2_PREVIEW_STAGE_EXCLUDED` contents, Cite type absence) that an implementor would otherwise re-investigate. No policy decisions; the plan is now self-contained against the as-shipped 2A surface.

- **2026-05-09 (follow-on spec pass: Note walk concretization, regression test pseudocode, appendix-inert clarification)**: closed the residual implementation-detail questions surfaced after the prior pass. No policy decisions; all five items reduced to concrete spec.
  - **Note-numbering walk fully specified**: runs in `PreviewRoot` via `useMemo` keyed on `astJson`; produces a `WeakMap<NoteInline, number>` keyed by object identity; descends `c` fields recursively (handles wire-format CustomNode wrappers via slot wrapper Div/Span children at `c[1][i].c[1]`); pre-unwrap walk works because `unwrapCustomNodes` preserves inner content references through slot decoding.
  - **`framework/Ast.tsx` discriminated input** added — accepts either `astJson: string` or `ast: PandocAST`. PreviewRoot's `useMemo` produces a parsed AST that `<Ast>` consumes directly, avoiding a double-parse. ~10 LOC added to `Ast.tsx` co-edits; opt-in (q2-debug stays on the string path). Lives in the same commit as `customNode.ts` since both touch `Ast.tsx`.
  - **Recursion-contract regression test pseudocode** added to §"Vitest integration tests" — concrete `BypassingPara` user override that iterates `node.c` directly, `vi.fn()` setAst spy, click-to-trigger pattern, negative assertion (`expect(setAstSpy).toHaveBeenCalledTimes(1)`) with explicit "this is the failure mode the contract documents" comment. The test is intentionally negative — a future hardening pass that propagates the gate would cause it to fail and prompt explicit replacement.
  - **AppendixStructureTransform footnotes-branch-inert clarification** added below the existing "Bibliography note" passage — `extract_footnotes` returns `None` when no `Div(id="footnotes")` exists (verified at `appendix.rs:140-145, 211-225`), so the appendix's footnotes branch silently no-ops for `block`/`section` configs, just like the bibliography branch is inert without Citeproc. License/copyright/citation branches still render.
  - **§"`q2-preview/quartoClasses.ts`" verify-disclaimer** tightened — replaced "verify before locking the constants" (true at write-time, stale after the audit) with "Re-verify on any major Rust transform refactor — when a class name changes in Rust without a corresponding constant update here, the §'Class-compatibility test' in §Test plan catches the drift at compile time."
  - **New §"Related beads issues" section** before §Notes — lists bd-1kly explicitly with the deletion plan once the upstream fix lands. Future plans extend this list rather than scattering issue refs through prose.
  - Estimated scope: ~2300 → ~2310 LOC (+10 for `framework/Ast.tsx` discriminated input). The recursion-contract test pseudocode lives within the existing tests budget.
  - Rationale: the items in this pass were all "specifiable, not deciding" — research confirmed implementation paths for each, no remaining policy choices. The plan is now implementor-ready for both Session A and Session B; no Rust-source-reading required during implementation.
