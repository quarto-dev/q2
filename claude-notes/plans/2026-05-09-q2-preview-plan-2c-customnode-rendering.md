# Plan 2C — q2-preview Quarto custom-node rendering + verification

**Date:** 2026-05-09
**Branch:** feature/q2-preview
**Status:** Implementation plan
**Milestone:** M2 completion. After Plan 2B (Session A) ships the framework + Pandoc-base layer, Plan 2C (Session B) lands the Quarto-specific custom-node renderers and end-to-end verification so q2-preview reaches visual parity with the HTML format for documents using callouts, theorems, proofs, figures, equations, and cross-references.

## Goal

Fill q2-preview's Quarto custom-node renderers (Callout, Theorem, Proof, FloatRefTarget, Equation, CrossrefResolvedRef, IncludeExpansion) on top of the framework + Pandoc-base layer that Plan 2B ships. Specifically:

- **Quarto class taxonomy** — fill out `quartoClasses.ts` with the Bootstrap-flavored class names theme CSS targets (`callout`, `callout-{type}`, `theorem`, `theorem-title`, `proof`, `quarto-xref`, etc.), pinned to Rust source line numbers so drift is catchable.
- **Type-specific CustomNode components** — Callout, Theorem, Proof, FloatRefTarget, Equation, CrossrefResolvedRef, IncludeExpansion. Class-compatible AND structure-compatible with Rust's HTML output so Quarto's compiled Bootstrap-flavored theme CSS produces matching visuals without per-format CSS forks.
- **Registry merge for user overrides** — `CustomNodeRegistryContext` distributes the merged customNodeRegistry (built-ins layered with user-TSX overrides) to the `CustomBlock` / `CustomInline` dispatchers. Pandoc-tag overrides (already plumbed through 2A/2B's `mergedRegistry`, renamed to `mergedPreviewRegistry` in 2C for symmetry with the new `mergedCustomNodeRegistry`) keep working unchanged.
- **End-to-end verification** — smoke-all fixtures (single-doc + project mode + with-render-components), WASM project-mode safety net for the customNode wire format, demo fork from Elliot's `html.tsx` to `gordon/render-components/`, and a final `cargo xtask verify --e2e` run.

After 2C lands, q2-preview renders documents using the full Quarto authoring surface with element-and-class parity to the HTML pipeline, so loading Quarto's compiled theme CSS produces visually-matching output without per-format CSS forks.

## Checklist

Phases continue from Plan 2B's numbering (Phase 4 + Phase 5).

### Phase 4 — CustomNode components + registry assembly

- [ ] **4.1** `q2-preview/quartoClasses.ts` — extend Plan 2B's stub (which contains only `SECTION`, `SECTION_LEVEL_PREFIX`, footnotes, and appendix constants) with the callout/theorem/proof/crossref taxonomy enumerated in §"`q2-preview/quartoClasses.ts` — class-name extensions" below. **First commit of Phase 4**, per the "enumeration before consumers" rule.
- [ ] **4.2** `q2-preview/utils.ts` extensions — `formatRefLabel`, `composeAttr`, `renderSlot` (Plan 2B already shipped `lookupAssetUrl`, `inlinesToPlainText`, `blocksToPlainText`). Per §"`q2-preview/utils.ts` — additional helpers".
- [ ] **4.3** `q2-preview/custom/*.tsx` — 7 type-specific CustomNode components (`Callout`, `Theorem`, `Proof`, `FloatRefTarget`, `Equation`, `CrossrefResolvedRef`, `IncludeExpansion`) keyed by the canonical `type_name` strings from `crates/quarto-core/src/crossref/mod.rs:60-92`. Plus `Fallback.tsx` registered under key `'__fallback__'` (delegates to `renderChildren` for generic slot walk). Plus `q2-preview/theoremEnvs.ts` (NEW, ~15 LOC) — JS port of the `theorem_env_for` mapping at `crossref_render.rs:388-400`, consumed by `Theorem.tsx`. **`Equation.tsx` is a JS-side port of `crossref_render.rs::render_equation:601`** — appends `\tag{N}` from `plain_data.order.order` because q2-preview excludes `CrossrefRenderTransform`. Plus `q2-preview/custom/index.ts` barrel.
- [ ] **4.4** `q2-preview/registry.ts` extension + `q2-preview/CustomNodeRegistryContext.tsx` (NEW) + `entry.tsx` PreviewRoot extension — extend the existing `previewRegistry` (which Plan 2B leaves with `Block`, `Inline`, Pandoc base-type entries, and `Ast: PreviewDocument`) with `CustomBlock` / `CustomInline` dispatcher entries that read the merged registry from `CustomNodeRegistryContext`. Build `customNodeRegistry: Record<string, ...>` (Custom + `Fallback` under `'__fallback__'`). PreviewRoot computes `mergedCustomNodeRegistry = { ...customNodeRegistry, ...customRegistry }` (same `customRegistry` that already feeds `mergedPreviewRegistry`) and provides via context. After this lands, the muted-gray "(not yet implemented)" miss path stops firing for Pandoc CustomNode wrappers, and user TSX can override either Pandoc tags (already supported) or CustomNode `type_name`s (new). Includes vitest integration test for both override directions (Pandoc tag override + CustomNode override).

### Phase 5 — Verification + demos

(Plan 2B already shipped item 5.1 — vitest tests for Phase 1-3 — and an asset-manifest variant of the project-mode WASM safety net. 2C picks up the Quarto-feature-specific verification.)

- [ ] **5.2** Smoke-all q2-preview fixtures — under `crates/quarto/tests/smoke-all/q2-preview/` (directory exists post-2B's `image-with-attrs.qmd` fixture; 2C extends it):
  - `multi-element-doc.qmd` (single-doc; callout + theorem + cross-reference + equation + image + footnote + `license:` metadata),
  - `multi-element-project/` (default-project with `_quarto.yml` + sibling doc; same multi-element content),
  - `with-render-components/` (project + small `overrides.tsx` exporting both a Pandoc-tag override and a CustomNode override; locks the user-override merge for both registries).

  All `_quarto.tests.run.requires_js: true` so the Playwright runner picks them up. Assertions via `_quarto.tests.q2-preview.ensureHtmlElements`. Fixture mechanics (frontmatter shape, `_quarto.yml` shape) are pinned in §"Smoke-all q2-preview fixtures". **Project-mode fixtures are mandatory, not optional** — see §"Test-tier conventions → Project-context coverage rule" in Plan 2B.
- [ ] **5.3** `customNodeWireFormatProject.wasm.test.ts` — render a `_quarto.yml`-rooted project doc containing a callout, assert the response's `ast_json` contains a `Div` with `__quarto_custom_node` in its classes and `data-custom-type=Callout` in its kvs. Catches drift between Gordon's deny-list refactor (`Q2_PREVIEW_TRANSFORM_EXCLUDED`) and what `unwrapCustomNodes` will see — if `callout-resolve` ever falls out of the exclusion list, the callout becomes plain HTML and unwrap finds nothing. Pattern follows `assetManifestProject.wasm.test.ts` (Plan 2B) at the WASM-bridge layer — its `initWasm` + project setup is closer to 2C's needs than the older `themeFingerprint.wasm.test.ts` pattern, which is a pure CSS-byte assertion that doesn't exercise project-mode AST inspection.

  **`themeFingerprint.wasm.test.ts` preservation note**: Plan 2A's regression test must remain when 2C touches `pass2_renderer.rs`. Documented here so a refactor pass doesn't accidentally weaken it.
- [ ] **5.4** Fork Elliot's demos to `~/docs/demo-playground/gordon/render-components/` per §"Fork Elliot's demos to `gordon/render-components`" — copy, rebase qmd `format` to `q2-preview` and TSX global to `__Q2_PREVIEW_RENDERER__`, prune now-built-in components, update docs (`index.qmd`, `render_components.qmd`). The override path is locked by 5.2's `with-render-components/` smoke fixture, so this item is documentation / demo polish only.
- [ ] **5.5** Run **`cargo xtask verify --e2e`** before declaring 2C complete. Default `cargo xtask verify` skips the Playwright runner (`--e2e` is opt-in per project CLAUDE.md), so the smoke-all fixtures landed in 5.2 are *not* exercised by the standard verify flow. Without this step the iframe boot path, blob-URL minting through the real VFS, manifest distribution, postMessage round-trips, the user-override merge for both registries, and the CustomNode unwrap path all go untested at the integration layer — exactly the surface that hid Plan 2A's two project-mode bugs (see §"Project-context coverage rule" in 2B). Also do a manual browser session against a running hub for sanity (per project CLAUDE.md "End-to-end verification before declaring success"); record the invocation and an inspected-output snippet in the implementation transcript or the plan's checklist comments.

## Scope

### In scope

#### `q2-preview/quartoClasses.ts` — class-name extensions

Plan 2B shipped a stub of `quartoClasses.ts` containing only the constants needed for Pandoc-base + footnotes/appendix rendering (`SECTION`, `SECTION_LEVEL_PREFIX`, `FOOTNOTES`, `FOOTNOTE_REF`, `FOOTNOTE_BACK`, `QUARTO_APPENDIX`, `QUARTO_BIBLIOGRAPHY`, `QUARTO_REUSE`, `QUARTO_COPYRIGHT`, `QUARTO_CITATION`). 2C fills in the Quarto-feature taxonomy:

```ts
// Callout — emitted by CalloutResolveTransform (excluded from q2-preview;
// q2-preview keeps the Callout CustomNode wrapper, but the class names
// must match for theme CSS compatibility).
// Source: crates/quarto-core/src/transforms/callout_resolve.rs:170,172,175,199,215,226,234
export const CALLOUT = 'callout';
export const CALLOUT_TYPE_PREFIX = 'callout-';            // callout-note, callout-warning, callout-tip, callout-important, callout-caution
export const CALLOUT_APPEARANCE_PREFIX = 'callout-appearance-'; // -default, -simple, -minimal
export const CALLOUT_COLLAPSE = 'callout-collapse';
export const CALLOUT_HEADER = 'callout-header';
export const CALLOUT_TITLE_CONTAINER = 'callout-title-container';
export const CALLOUT_ICON_CONTAINER = 'callout-icon-container';
export const CALLOUT_ICON = 'callout-icon';
export const CALLOUT_BODY_CONTAINER = 'callout-body-container';
export const CALLOUT_BODY = 'callout-body';

// Theorem / Proof — crates/quarto-core/src/transforms/crossref_render.rs:346,482,537
export const THEOREM = 'theorem';
export const THEOREM_TITLE = 'theorem-title';
export const PROOF = 'proof';
// NOTE: there is no `proof-title` class. The proof label is an inline
// `<em>Proof.</em>` (italic), not a wrapped Span — see render_proof at
// crossref_render.rs:534-585.

// Equation — crates/quarto-core/src/transforms/crossref_render.rs:601-650
// No specific class; preserves user attr (typically just `id="eq-..."`).
// q2-preview's Equation.tsx wraps the Math in `<span id={id}>` with no
// added classes, matching the Rust output.

// FloatRefTarget — crates/quarto-core/src/transforms/float_ref_target.rs:240,315
// No classes added; preserves user attr verbatim. In Rust HTML output the
// figure subtype maps to a native `<figure>` (no class), other subtypes
// to `<div>` (no class). Identifier carries on the `id` attribute.

// CrossrefResolvedRef — crates/quarto-core/src/transforms/crossref_render.rs:707
export const QUARTO_XREF = 'quarto-xref';
```

**Callout subtype values** (`callout_type` from `CalloutTransform`): `note | warning | tip | important | caution`. **Appearance values**: `default | simple | minimal`. **Theorem environment classes** for non-`thm` ref types — `lemma`, `corollary`, `proposition`, `conjecture`, `definition`, `example`, `exercise` — are added alongside `theorem` per `crossref_render.rs:350`. The mapping is a closed 8-entry table at `crossref_render.rs:388-400` (`theorem_env_for`); ported to JS as `q2-preview/theoremEnvs.ts` rather than living in `quartoClasses.ts` because it's a function (`refType → envName`), not a constant.

**Drift-detection caveat** carries forward from Plan 2B: vitest "Class-compatibility test" (§Test plan) catches JS-constant changes at runtime; smoke-all `multi-element-doc.qmd` catches Rust↔JS class drift end-to-end. The Rust-side `pipeline.rs:1987` / `:2053` validation tests already prevent typos in the exclusion lists from drifting.

#### `q2-preview/utils.ts` — additional helpers

Plan 2B already shipped `lookupAssetUrl`, `inlinesToPlainText`, `blocksToPlainText`. 2C adds:

- `formatRefLabel(kind, number, title?): string` — produces "Theorem 1 (Pythagoras)"-style labels.
- `composeAttr(originalAttr, extraClasses, extraKvs): Attr` — adds classes/attrs without mutating original.
- `renderSlot(slot, setSlot, ctx): ReactNode` — slot dispatcher for CustomNode components:

```ts
function renderSlot(slot, setSlot, ctx) {
  switch (slot.kind) {
    case 'block':   return <Node node={slot.value} setLocalAst={n => setSlot({ kind: 'block', value: n })} {...ctx}/>;
    case 'inline':  return <Node node={slot.value} setLocalAst={n => setSlot({ kind: 'inline', value: n })} {...ctx}/>;
    case 'blocks':  return slot.value.map((b, i) => <Node key={i} node={b} setLocalAst={n => { const next = [...slot.value]; next[i] = n; setSlot({ kind: 'blocks', value: next }); }} {...ctx}/>);
    case 'inlines': return slot.value.map((inl, i) => <Node key={i} node={inl} setLocalAst={n => { const next = [...slot.value]; next[i] = n; setSlot({ kind: 'inlines', value: next }); }} {...ctx}/>);
  }
}
```

Same body shape as `framework/dispatch.tsx`'s `renderChildrenRegistry['CustomBlock'|'CustomInline']` (Plan 2B item 1.3) — both build `<Node>` per slot value with copy-on-write `setLocalAst`. The duplication is intentional: `renderSlot` is the per-component named-slot helper (`renderSlot(slots.title, ...)`); the framework registry entry is the generic-walk fallback. Keeping them separate avoids coupling Fallback's walk to per-component naming conventions (Callout's `title`/`content` vs FloatRefTarget's `caption_long`/`caption_short`).

#### `q2-preview/custom/` — type-specific CustomNode components

The seven concrete `type_name` strings (canonical source: `crates/quarto-core/src/crossref/mod.rs:60-92` plus `crates/quarto-core/src/transforms/callout.rs:32`):

| Component | `type_name` | Producer transform |
|---|---|---|
| `Callout.tsx` | `"Callout"` | `CalloutTransform` |
| `Theorem.tsx` | `"Theorem"` | `TheoremSugarTransform` |
| `Proof.tsx` | `"Proof"` | `ProofSugarTransform` |
| `FloatRefTarget.tsx` | `"FloatRefTarget"` | `FloatRefTargetSugarTransform` |
| `Equation.tsx` | `"Equation"` | `EquationLabelTransform` |
| `CrossrefResolvedRef.tsx` | `"CrossrefResolvedRef"` | `CrossrefResolveTransform` |
| `IncludeExpansion.tsx` | `"IncludeExpansion"` | (Plan 8, dormant) |

**`plain_data` and slot field tables** (audited 2026-05-09 against current sources). Every field a JS component needs to read is listed below with its writer site, JSON type, and reader site.

##### `Callout.tsx` — `type_name: "Callout"`

- **Slots**: `title` (Inlines, optional), `content` (Blocks).
- **`plain_data`** (writer: `crates/quarto-core/src/transforms/callout.rs:210`):
  - `type` (string): callout type — `note | warning | tip | important | caution`. Reader: `callout_resolve.rs:162`.
  - `appearance` (string): `default | simple | minimal`. Reader: `callout_resolve.rs:163`.
  - `collapse` (bool). Reader: `callout_resolve.rs:164`.
  - `icon` (bool): controls whether the icon container is emitted at all. Reader: `callout_resolve.rs:165`.
  - Optional `ref_type` / `kind` / `identifier` (strings) — populated only when the callout has a crossref id (`callout.rs:224-226`); not used by `Callout.tsx`'s render path.
- **Output structure** (mirroring `callout_resolve.rs:170-234`, q2-preview must emit identically because `callout-resolve` is excluded from the pipeline so the CustomNode survives — see `Q2_PREVIEW_TRANSFORM_EXCLUDED` at `pipeline.rs:1050`):

  ```
  <div class="callout callout-{type} [callout-appearance-{a} if a !== 'default'] [callout-collapse if collapse]">
    <div class="callout-header">
      [if plain_data.icon === true]
        <div class="callout-icon-container">
          <i class="callout-icon"></i>
        </div>
      <div class="callout-title-container flex-fill">
        {render slots.title or default-title-from-type}
      </div>
    </div>
    <div class="callout-body-container callout-body">
      {render slots.content via renderSlot/Node}
    </div>
  </div>
  ```

  **Three-deep nesting is load-bearing for theme CSS.** Bootstrap's callout selectors target `.callout > .callout-header > .callout-title-container`, `.callout > .callout-body-container`, etc. Flattening any level breaks selectors. The `flex-fill` class on `.callout-title-container` is mandatory (it's how the title fills horizontal space next to the icon). The icon's `<i class="callout-icon">` element is what the theme CSS applies a background-image to — emit it even though it's empty content-wise.

  **Default title** when `slots.title` is absent: capitalize the `type` ("Note", "Warning", "Tip", "Important", "Caution"). Mirror `callout_resolve.rs:264` (`capitalize(callout_type)`).

##### `Theorem.tsx` — `type_name: "Theorem"`

- **Slots**: `content` (Blocks), `title` (Inlines, optional).
- **`plain_data`** (writer: `crates/quarto-core/src/transforms/theorem.rs:145`):
  - `ref_type` (string): theorem prefix (`thm | lem | cor | prp | cnj | def | exm | exr`). Used to compute the env class.
  - `kind` (string): display name (`Theorem | Lemma | Corollary | …`). Used in the rendered label.
  - `identifier` (string): full id (e.g. `thm-pythagoras`).
  - Optional `order: { section: number[], order: number }`: filled by `CrossrefIndexTransform`. The number used in the rendered label is `order.order`.
- **Env class derivation**: `theorem_env_for(ref_type)` is a hardcoded 8-entry mapping at `crossref_render.rs:388-400`. **Not stored in `plain_data`** — the JS port must replicate it. **New file**: `q2-preview/theoremEnvs.ts` (~15 LOC):

  ```ts
  export function theoremEnvFor(refType: string): string {
    switch (refType) {
      case 'thm': return 'theorem';
      case 'lem': return 'lemma';
      case 'cor': return 'corollary';
      case 'prp': return 'proposition';
      case 'cnj': return 'conjecture';
      case 'def': return 'definition';
      case 'exm': return 'example';
      case 'exr': return 'exercise';
      default:    return '';
    }
  }
  ```

  Sync convention: matches `theorem_env_for` at `crossref_render.rs:388-400`. Update both together when new theorem-like ref types land.

- **Output structure** (mirroring `crossref_render.rs:321-378`):

  ```
  <div id="{identifier}" class="theorem [{env} if env !== 'theorem']">
    <p>
      <span class="theorem-title"><strong>{kind} {order.order} [({title inlines})]</strong></span>
      {content's first paragraph inlines}
    </p>
    {content's remaining blocks}
  </div>
  ```

  Mirrors the `prepend_theorem_label` Rust helper: the bold label inserts into the *first paragraph* of `content` (or a synthesized `<p>` if content starts with a non-paragraph). Title is parenthesized and italic-prefixed when present. Number comes from `plain_data.order.order` if present, otherwise omit.

##### `Proof.tsx` — `type_name: "Proof"`

- **Slots**: `content` (Blocks), `title` (Inlines, optional).
- **`plain_data`** (writer: `crates/quarto-core/src/transforms/proof.rs:145`):
  - `kind` (string, hardcoded `"Proof"`): not actually used by the Rust renderer — the label is hardcoded `"Proof."`.
  - **No `ref_type`** — proofs are not numbered.
- **Output structure** (mirroring `crossref_render.rs:534-586`):

  ```
  <div id="{identifier|empty}" class="proof">
    <p><em>Proof.</em> [or <em>{title inlines}</em>] {content's first paragraph inlines}</p>
    {content's remaining blocks}
  </div>
  ```

  **No `proof-title` class** — the label is an inline italic `<em>Proof.</em>` (or the user's title) prepended to the body's first paragraph. The default label is the literal string `"Proof."` (period included). User title (if present) replaces the entire label, still wrapped in `<em>`.

##### `FloatRefTarget.tsx` — `type_name: "FloatRefTarget"`

- **Slots**: `content` (Blocks), `caption_long` (Blocks, optional), `caption_short` (Inlines, optional).
- **`plain_data`** (writer: `crates/quarto-core/src/transforms/float_ref_target.rs:292-295`):
  - `ref_type` (string): `fig | tbl | lst | <user-defined>`. **The figure-vs-div discriminator.**
  - `kind` (string): display name (e.g. `"Figure"`, `"Table"`).
  - `identifier` (string).
  - Optional `order: { section, order }`: filled by `CrossrefIndexTransform`.
- **Output discriminator** (`crossref_render.rs:263-290`):
  - `ref_type === "fig"` → emit a `<figure>` block.
  - All other ref types (`tbl`, `lst`, user-defined) → emit a `<div>`.
- **Output structure**:
  - **Figure case**: `<figure id="{identifier}">{render slots.content}<figcaption>{caption with prepended "{kind} {order.order}: " if numbered}</figcaption></figure>`. Caption-prepended numbering mirrors `prepend_caption_kind` at `crossref_render.rs:651-700`-ish.
  - **Div case**: `<div id="{identifier}">{render slots.content}{caption-block-list if caption_long is non-empty}</div>`. No special class on the wrapper — preserves user attr verbatim.
- **No additional classes** — the wrapper picks up only the user's original attr classes (passed through unchanged from `node.attr`).

##### `Equation.tsx` — `type_name: "Equation"`

- **Slots**: `content` (Inlines, single `Math(DisplayMath)` inline).
- **`plain_data`** (writer: `crates/quarto-core/src/transforms/equation_label.rs:316`):
  - `ref_type` (string, hardcoded `"eq"`).
  - `kind` (string, hardcoded `"Equation"`).
  - `identifier` (string).
  - Optional `order: { section, order }`.
- **Output structure** (mirroring `crossref_render.rs:601-650`, **JS-side port** because q2-preview's pipeline excludes `crossref-render` at `pipeline.rs:1061`):
  1. Read `id = node.attr[0]` (e.g. `"eq-einstein"`) and `order = node.plain_data?.order?.order` (number, optional).
  2. Pull the math inline out of `slots["content"]`. **Expected shape**: `Inlines` slot with exactly one element of `{ t: 'Math', c: [{ t: 'DisplayMath' }, latex] }` (verified: `equation_label.rs:316` always produces this single-element Inlines).
  3. If `order` is a number, rebuild the math inline with text `` `${latex}\\tag{${order}}` ``. KaTeX renders `\tag{}` natively.
  4. Slot-render the (possibly modified) math through `q2-preview/inlines/Math.tsx` (shipped by Plan 2B), wrapped in `<span id={id}>` for `@eq-xxx` anchor linking.

  **Defensive fallback for non-canonical slot contents.** If `slots["content"].value` does not match the single-Math-inline shape (e.g. a future transform inserts adjacent inlines, or an extension produces a multi-element Inlines slot), the component must not crash. Behavior:
  - **Empty `Inlines`**: render the empty `<span id={id}>` and stop (matches the `id`-only anchor case).
  - **First inline is not `Math` or not `DisplayMath`**: render every inline through `<Node>` unchanged, wrap in `<span id={id}>`, and emit a `console.warn` once per render with the unexpected shape. Do not append `\tag{N}` (the tag is meaningless without a math target).
  - **First inline is `Math(DisplayMath)`, others present**: append `\tag{N}` to the first inline's LaTeX as in step 3, render every inline through `<Node>`, wrap in `<span id={id}>`. The trailing inlines pass through verbatim.

  The `\tag{N}` append is JS-side because the q2-preview pipeline keeps the CustomNode wrapper for editing affordances; the HTML pipeline does the same append in Rust at `crossref_render.rs:631` and discards the wrapper.

##### `CrossrefResolvedRef.tsx` — `type_name: "CrossrefResolvedRef"`

- **Slots**: `suffix` (Inlines, optional). The suffix carries any text that followed the `@ref` in the original citation (e.g. `@fig-1 (and onwards)` → suffix is `[Space, Str("(and"), Space, Str("onwards)")]`).
- **`plain_data`** (writer: `crates/quarto-core/src/transforms/crossref_resolve.rs:316`):
  - `identifier` (string): the referenced id (e.g. `"fig-1"`).
  - `ref_type` (string): the prefix (`fig`, `tbl`, `thm`, `eq`, …).
  - `kind` (string): display name (`Figure`, `Table`, …).
  - `resolved` (bool): true iff the indexer found a matching target.
  - `kind_source` (string: `"builtin" | "custom" | "promised"`): tracks where the kind came from; not used in render.
  - Optional `order: { section, order }`: filled only when `resolved === true`.
- **Output structure** (mirroring `crossref_render.rs:704-715`):

  ```
  <a class="quarto-xref" href="#{identifier}">{kind} {order.order}</a>{render slots.suffix}
  ```

  Where the link text is:
  - `resolved && order` → `` `${kind} ${order.order}` `` (kind + non-breaking space + number).
  - `resolved && !order` → `kind` alone (rare; numbered targets always have order).
  - `!resolved` → `` `?${identifier}?` `` (the broken-ref affordance).

  **Atomic** — `isAtomicCustomNode("CrossrefResolvedRef") === true` (per `hub-client/src/utils/atomicCustomNodes.ts`). The framework's atomic gate (Plan 2B Phase 1.3) handles this; the component itself just renders.

##### `IncludeExpansion.tsx` — `type_name: "IncludeExpansion"`

Dormant placeholder until Plan 8 produces these. Will be added to `atomicCustomNodes.ts` when Plan 8 lands. v1 renders as a transparent passthrough (renders the `content` slot blocks); registration in place from the start so when Plan 8's wrapper starts appearing in the AST it renders correctly. `plain_data` shape TBD by Plan 8.

A user-authored TSX export named `IncludeExpansion` would shadow this dormant placeholder via `mergedCustomNodeRegistry`. Until Plan 8 ships, the shadowed entry never fires (no AST node ever has `type_name === 'IncludeExpansion'`), so the shadow is inert. Once Plan 8 lands, the user's component takes precedence — same "User overrides win" rule as for every other CustomNode type.

##### `Fallback.tsx`

Registered under `customNodeRegistry['__fallback__']` for unknown `type_name` values. Styled box that displays `node.type_name` and recursively walks all slots via `renderChildren({ node })` (which routes through `renderChildrenRegistry`'s `'CustomBlock'` / `'CustomInline'` entries — Plan 2B Phase 1.3). Useful for extension-defined CustomNodes that haven't shipped a per-type component yet.

#### `q2-preview/registry.ts` — extend with CustomBlock/CustomInline + CustomNodeRegistryContext

Plan 2B's `registry.ts` ships as:

```ts
export const previewRegistry: FormatRegistry = {
  ...Blocks,
  ...Inlines,
  Block,
  Inline,
  Ast: PreviewDocument,
};
```

(no `CustomBlock`/`CustomInline` keys — the muted-gray placeholder fires for CustomNode wrappers post-2B.)

2C extends to:

```ts
import { useContext } from 'react';
import * as Custom from './custom';
import { CustomNodeRegistryContext } from './CustomNodeRegistryContext';

// CustomBlock / CustomInline dispatchers read the *merged* customNodeRegistry
// from context (built-ins layered with any user-TSX overrides — see §Design
// decisions "User overrides win"). Module-level lookup against the un-merged
// `customNodeRegistry` would skip user overrides; the context indirection is
// what makes user overrides of CustomNodes work.
const CustomBlockDispatcher = ({ node, ...args }: any) => {
  const merged = useContext(CustomNodeRegistryContext);
  const Comp = merged[node.type_name] ?? merged['__fallback__'];
  return <Comp node={node} {...args} />;
};

const CustomInlineDispatcher = ({ node, ...args }: any) => {
  const merged = useContext(CustomNodeRegistryContext);
  const Comp = merged[node.type_name] ?? merged['__fallback__'];
  return <Comp node={node} {...args} />;
};

export const previewRegistry: FormatRegistry = {
  ...Blocks,
  ...Inlines,
  Block,
  Inline,
  Ast: PreviewDocument,
  CustomBlock: CustomBlockDispatcher,
  CustomInline: CustomInlineDispatcher,
};

// Built-in CustomNode registry. The user-merged version is computed in
// PreviewRoot (entry.tsx) and rides through CustomNodeRegistryContext;
// see §Design decisions "User overrides win".
export const customNodeRegistry: Record<string, (props: any) => React.ReactNode> = {
  ...Custom,
  __fallback__: Fallback,
};
```

`previewRegistry` keeps `FormatRegistry` (the typed shape from `framework/types.ts:163` that enforces `Ast`/`Block`/`Inline` keys; line shifted from 89 in 2A after Plan 2B added the CustomNode shape types ahead of it); `customNodeRegistry` is a looser `Record<string, ...>` since its keys are dynamic `type_name` strings. The CustomBlock/CustomInline dispatchers are tiny wrapper components, not closure literals, because they need to call `useContext` (which can only run inside a React render).

#### `q2-preview/CustomNodeRegistryContext.tsx`

New file (sibling of `AssetManifestContext` shipped by 2B). Distributes the **merged** customNodeRegistry — built-ins layered with any user-TSX overrides — to the `CustomBlock` / `CustomInline` dispatchers. See §Design decisions "User overrides win" for the design rationale.

```tsx
import { createContext } from 'react';
import { customNodeRegistry } from './registry';

export const CustomNodeRegistryContext = createContext<
  Record<string, (props: any) => React.ReactNode>
>(customNodeRegistry);
```

The default value is the built-in registry so the context is usable in tests that mount `<Ast>` without a wrapping `PreviewRoot`. In production, `PreviewRoot` always provides a merged value:

```ts
// inside PreviewRoot, alongside the existing AssetManifestContext.Provider
// (Plan 2B item 2.3) and NoteNumberingContext.Provider (Plan 2B Phase 3.4):
const mergedCustomNodeRegistry = {
  ...customNodeRegistry,
  ...customRegistry,  // same export bag that feeds mergedPreviewRegistry
};

// Note: 2C also renames Plan 2B's `mergedRegistry` to
// `mergedPreviewRegistry` for symmetry with `mergedCustomNodeRegistry`
// — mechanical 1-line change in entry.tsx (around line 228).
return (
  <PreviewContext.Provider value={{ currentFilePath: props.currentFilePath }}>
    <AssetManifestContext.Provider value={props.assetManifest}>
      <NoteNumberingContext.Provider value={noteNumbers}>
        <CustomNodeRegistryContext.Provider value={mergedCustomNodeRegistry}>
          <Ast {...props} registry={mergedPreviewRegistry} />
        </CustomNodeRegistryContext.Provider>
      </NoteNumberingContext.Provider>
    </AssetManifestContext.Provider>
  </PreviewContext.Provider>
);
```

The same `customRegistry` value flows into both merged maps. Disjoint namespaces (Pandoc tags vs CustomNode `type_name`s) make this unambiguous: a user export named `Image` lands meaningfully only in the Pandoc-tag map; an export named `Callout` lands meaningfully only in the CustomNode map. Exports that don't collide with either built-in are dead code (warn-or-not is up to the harness — out of scope for v1).

#### Fork Elliot's demos to `gordon/render-components`

The original Plan 2 framing was that q2-preview's components ship as pasted-into-demos `html.tsx` and `custom.tsx` drafts. Under the restructure, those components are q2-preview's built-in registry — pasted demos are no longer needed for basic rendering. The demo-playground role shifts from "this is how to render real HTML" to "here are the genuine custom-component overrides worth showcasing."

Action items, all under `~/docs/demo-playground/gordon/render-components/` (new directory, parallel to `elliot/`):

- **Fork**: copy Elliot's TSX and qmd files into `gordon/render-components/` as a starting point.
- **Rebase for q2-preview**: change `format: q2-debug` → `format: q2-preview` in qmd files where appropriate; change `window.__REACT_AST_DEBUG_RENDERER__` → `window.__Q2_PREVIEW_RENDERER__` in TSX files.
- **Prune the now-built-in**: remove TSX files / individual exports that q2-preview ships natively after 2B+2C. Most of `html.tsx`'s contents (Para, Header, Str, Space, Emph, Strong, Code, Link, Image, Figure, Span, Quoted, Math, Div, RawBlock, etc.) become redundant. Keep only the components that demonstrate genuine *override* behavior beyond the built-ins.
- **Keep live demos**: `comment.tsx` (Slack-like commenting UI), `kanban.tsx` (drag-and-drop kanban), `drag.tsx` (generic drag helper), and any `slide.tsx` if applicable — these are real extensions, not gap-fillers.
- **Update docs**: rewrite `index.qmd` and `render_components.qmd` to reference the new path, the new format, the new global, and the post-2B+2C "what's built-in vs. what you can override" model. The originals at `~/docs/demo-playground/elliot/` stay unchanged — q2-debug demos keep working there.
- **Override path is locked by the `with-render-components/` smoke-all fixture** (see §"Smoke-all q2-preview fixtures") — that fixture is the automated end-to-end verification for the override merge. Manual confirmation by pasting `comment.tsx` into a doc and watching it render in the browser is no longer the gate; the smoke-all fixture is.

### Out of scope

- Layout / chrome components (TOC sidebar, navbar, footer, page-nav strip rendering as page chrome). Deferred per Plan 2A.
- Edit affordances (theorem-rename UI, callout-type changer, etc.). v1 is structural-only rendering.
- Drift-detection contract test (Rust HTML output ↔ React render). Useful long-term; defer.
- Body-classes derivation, navbar brand-fallback. Deferred per Plan 2A.
- Quarto-specific Image extensions: `fig-align`, `fig-link`, `fig-alt`, `lightbox`, subfigures, `fig-cap-location`. Tier 3 — defer to a follow-up plan parallel to "q2-preview layout chrome." Not 2C work either; flagged so 2C doesn't try to land them.
- Citeproc / bibliography rendering (the `c[0]` citations array of Cite, plus `<div id="refs">` from `CiteprocTransform`). `CiteprocTransform` is not in q2-preview's pipeline; the `AppendixStructureTransform`'s bibliography branch is inert until Citeproc lands. Out of scope for 2C.

### Defensive variants

- **Out-of-band**: `Shortcode` (desugared by `ShortcodeResolveTransform`), `NoteReference` / `InlineAttr` / `CaptionBlock` (defensive errors Q-3-21 / Q-3-31 / Q-3-32). Plan 2B handles these via fallback rendering. 2C does not change that behavior.

## Design decisions

- **CustomNode components as built-in registry, layered with user overrides.** Plan 2B's registry has no `CustomBlock`/`CustomInline` entries; 2C adds them as wrapper components that read a merged registry from context. User TSX overrides via `render-components: [...]` (Plan 2A item 13) shadow the built-ins for colliding keys.
- **`q2-preview/custom/` as a directory tree of one component per file.** Easier to navigate, override, and test than a single `custom.tsx`. Barrel file (`q2-preview/custom/index.ts`) provides name-keyed re-exports for the registry.
- **Two registries, disjoint namespaces.** `previewRegistry` keyed by Pandoc `node.t`; `customNodeRegistry` keyed by `type_name`. Pandoc tags (`Para`, `Header`, …) are framework-fixed; CustomNode type_names (`Callout`, `Theorem`, …) come from Quarto transforms. The two namespaces never collide today, so a single user-TSX export bag can feed both maps unambiguously.
- **User overrides win — for both Pandoc tags and CustomNode types.** When a user supplies a TSX file via `render-components: [...]` that exports a name colliding with a built-in (e.g. their own `Image` shadowing the Pandoc inline, or their own `Callout` shadowing the Callout CustomNode component), the user's component shadows the built-in. The framework-as-defaults model: built-ins are a starting point users can rewire piecewise without forking the renderer.

  The merge happens in two places because q2-preview has two registries with disjoint namespaces. **Pandoc tags** (`Para`, `Header`, `Image`, …) live in `previewRegistry` and merge through Plan 2A's `mergedRegistry` plumbing (currently at `entry.tsx:228-231` post-2B; renamed to `mergedPreviewRegistry` as part of 2C for symmetry with the new `mergedCustomNodeRegistry`). **CustomNode `type_name`s** (`Callout`, `Theorem`, `Equation`, …) live in `customNodeRegistry` and merge through 2C's new `CustomNodeRegistryContext`:

  ```ts
  const mergedPreviewRegistry: FormatRegistry = {
      ...previewRegistry,
      ...customRegistry,        // user TSX exports (Plan 2A surface)
  } as FormatRegistry;

  const mergedCustomNodeRegistry: Record<string, ...> = {
      ...customNodeRegistry,
      ...customRegistry,        // same exports — disjoint namespaces, no conflict
  };
  ```

  This design preserves the existing user API — `render-components: [...]` with named exports — and adds zero ceremony for users who want to override CustomNodes. The reverse merge order (built-ins after user) would make `render-components` useless for replacing either kind of component.
- **CustomBlock / CustomInline dispatchers are `useContext`-reading wrappers, not closure literals.** They need to call `useContext(CustomNodeRegistryContext)` which can only run inside a React render. Module-level lookups against an un-merged `customNodeRegistry` would skip user overrides.
- **Visual + structural parity target carries forward from Plan 2B.** Element parity (`<figure>` for figures, `<div class="callout">` for callouts), class parity (per `quartoClasses.ts`), explicit "where divergence is allowed" / "where divergence is forbidden" boundaries. The §"Visual + structural parity target" rationale in Plan 2B applies unchanged to 2C's CustomNode renderers.
- **Atomic CustomNode read-only — gate inherited from Plan 2B.** The framework's atomic gate (Plan 2B Phase 1.3) detects atomic CustomNode types via `isAtomicCustomNode(node.type_name)` and no-ops `setLocalAst` for them. 2C's CrossrefResolvedRef component runs through this gate automatically — no per-component awareness needed. Same pattern for Plan 8's `IncludeExpansion` once it lands.

## Multi-plan contracts

### Consumed: Plan 2B (Session A)

Plan 2C depends on Plan 2B landing first. Specifically:

- **Framework changes (Phase 1)**: `framework/customNode.ts` (unwrap/rewrap walks), `framework/types.ts` (`CustomBlockNode`, `CustomInlineNode`, `Slot`, `CustomNodeBase`, `CiteInline`), `framework/dispatch.tsx` (atomic gate, `renderChildrenRegistry['CustomBlock'|'CustomInline']`, `blockTypes` extension), `framework/Ast.tsx` co-edits (discriminated input, unwrap, sourceInfoPool extraction).
- **Asset manifest plumbing (Phase 2)**: `q2-preview/assetWalker.ts`, `q2-preview/AssetManifestContext.tsx`, `Q2PreviewIframe.tsx` walker integration, `entry.tsx` provider wiring. 2C builds on the same `PreviewRoot` skeleton; the new `CustomNodeRegistryContext.Provider` slots in alongside the existing `AssetManifestContext.Provider`.
- **q2-preview leaf components (Phase 3)**: every Pandoc Block / Inline. 2C's CustomNode components dispatch to these via `<Node>` recursion (e.g. Callout's body slot renders through the framework's `Block` dispatcher into 2B's blocks/).
- **Stub `quartoClasses.ts`**: 2B ships the file with footnote/appendix/section constants; 2C extends it with the callout/theorem/proof/crossref taxonomy. Consumed as constants by 2C's components.
- **`q2-preview/utils.ts` partial**: 2B ships `lookupAssetUrl`, `inlinesToPlainText`, `blocksToPlainText`. 2C adds `formatRefLabel`, `composeAttr`, `renderSlot`.
- **Pipeline exclusions stay locked**: `Q2_PREVIEW_TRANSFORM_EXCLUDED` keeps excluding `crossref-render` and `callout-resolve` so the CustomNode wrappers survive into the iframe AST. 2B's pipeline change (removing `"footnotes"` and `"appendix-structure"`) is upstream of 2C and required.

### Provided: full Quarto visual parity for q2-preview

After 2C lands, documents using callouts, theorems, proofs, figures, equations, images, and cross-references render with visual fidelity matching the HTML format. Plans 4 / 6 / 7 / 8 add to this incrementally without 2C needing amendment.

### Soft activation dependencies

- **Plan 4** introduces `Synthetic { by: By }` and `Derived { from, by }` SourceInfo variants. Until Plan 4 lands, no inline can have Derived source_info — but the gate is already wired in 2B.
- **Plan 6** populates Derived source_info on shortcode resolutions. After Plan 6, the dispatcher's atomic detection activates for shortcode-resolved inlines.
- **Plan 8** introduces `IncludeExpansion` CustomNode and amends `atomicCustomNodes.ts` to add it. 2C's `IncludeExpansion` component is registered from the start.

## Test plan

### Test-tier conventions

Same tiers as Plan 2B (vitest unit / vitest integration / smoke-all WASM / Playwright e2e). The **project-context coverage rule** carries forward unchanged: every WASM-path-significant feature must have at least one test covering single-doc, default-project, and (where applicable) website-project. 2C's CustomNode wire-format test (item 5.3) and the project-mode multi-element fixture (item 5.2) satisfy this for CustomNode work.

### Vitest integration tests (jsdom, mounting `<Ast registry={q2PreviewRegistry}>`)

(Plan 2B's `framework/customNode.test.ts` already covers the unwrap/rewrap round-trip property for the six in-tree `type_name`s plus the inline-CustomNode asymmetry case, plus the cross-language Rust → JS → Rust round-trip. 2C does not duplicate those; the algorithm is unchanged. 2C's tests below mount the per-type *components* against the registry — a different layer.)

- **Per-component snapshot tests**: render each CustomNode component with a fixed `plain_data` + slots input; snapshot the rendered DOM. One test per component (Callout, Theorem, Proof, FloatRefTarget, Equation, CrossrefResolvedRef, IncludeExpansion).
- **Generic fallback test**: register a CustomNode with a `type_name` not in `customNodeRegistry` (e.g. `"UnknownExtension"`); assert the `Fallback` component renders with the type name visible.
- **Class-compatibility test**: for each component, assert the rendered classes match the documented class taxonomy in `quartoClasses.ts`. Pandoc-base class compatibility (Section/levelN, footnotes/appendix) is Plan 2B's responsibility; 2C covers callout/theorem/proof/quarto-xref classes.
- **Atomic CustomNode read-only test (registry-routed)**: render a `CrossrefResolvedRef` wrapper through the populated `customNodeRegistry`; assert children don't receive a usable `setLocalAst`. **Different from Plan 2B's atomic gate test** — 2B's test verifies the gate fires when the dispatcher misses (muted-gray placeholder path); 2C's test verifies the gate still fires when a per-type component renders.
- **CustomNode override integration test**: mount `<Ast>` with a user-TSX export named `Callout` that renders `<div class="my-callout">`; assert the user component fires, not the built-in. Mount with a user-TSX export named `Para` (Pandoc-tag override, plumbed by Plan 2A) AND `Callout` (CustomNode override, plumbed by 2C's `CustomNodeRegistryContext`) simultaneously; assert both fire. Locks the dual-merge mechanism.

### Component-specific tests

- **Callout structure test**: assert the three-deep nesting (`.callout > .callout-header > .callout-title-container.flex-fill`, `.callout > .callout-body-container.callout-body`); assert default-title fallback ("Note", "Warning", etc.) when `slots.title` is absent; assert `<i class="callout-icon">` is emitted when `plain_data.icon === true` and absent otherwise.
- **Theorem env-class test**: render Theorem with `ref_type` of each value in the 8-entry `theoremEnvFor` mapping; assert the corresponding env class is emitted alongside `theorem`.
- **Proof title-via-em test**: assert the literal `<em>Proof.</em>` is emitted; assert `proof-title` class is NOT present (regression guard against the previous plan revision that included it).
- **FloatRefTarget figure-vs-div discriminator test**: render with `ref_type: "fig"` and `ref_type: "tbl"`; assert `<figure>` and `<div>` respectively.
- **Equation `\tag{N}` test**: render Equation with `plain_data.order.order = 5`; assert KaTeX output contains the rendered `\tag{5}`. Render with no `order`; assert no tag is appended. Plus the three defensive-fallback branches (empty Inlines / non-Math first inline / Math first plus siblings).
- **CrossrefResolvedRef text format test**: render with `resolved: true, order: { order: 3 }`; assert link text is `"Figure 3"` (NBSP). Render with `resolved: false`; assert link text is `"?fig-1?"`.

### WASM integration tests (project-mode safety net)

- **`customNodeWireFormatProject.wasm.test.ts`**: render a `_quarto.yml`-rooted project doc containing a callout (`::: {.callout-note} body :::`). Assert the response's `ast_json` contains a `Div` with `__quarto_custom_node` in its classes and `data-custom-type=Callout` in its kvs. This catches drift between Gordon's deny-list refactor (`Q2_PREVIEW_TRANSFORM_EXCLUDED`) and what `unwrapCustomNodes` will see — if `callout-resolve` ever falls out of the exclusion list, the callout becomes plain HTML and unwrap finds nothing.
- **`themeFingerprint.wasm.test.ts`** (already exists, **must remain**): locks Plan 2A's `theme_fingerprint` field on `RenderResponse` and the dual-write of theme CSS to `styles.css` for both single-doc and project modes. When 2C's registry assembly modifies `pass2_renderer.rs` (if it does — likely not, since CustomNode unwrap is JS-only), do not delete or weaken this test.

### Smoke-all q2-preview fixtures

The directory `crates/quarto/tests/smoke-all/q2-preview/` exists post-2B (which ships `image-with-attrs.qmd`). 2C adds three fixtures:

#### Frontmatter shape (verified, carried forward from 2B)

`requires_js` and `ensureHtmlElements` live under `_quarto.tests`:

```yaml
---
title: Multi-element rendering
format: q2-preview
_quarto:
  tests:
    run:
      requires_js: true
    q2-preview:
      ensureHtmlElements:
        - ['div.callout-note']
        - ['div.theorem']
        - ['a.quarto-xref']
        - ['span#eq-einstein']
        - ['sup.footnote-ref']
        - ['div#quarto-appendix > div.footnotes']
        - ['div#quarto-appendix > div.quarto-reuse']
---
```

`ensureHtmlElements` is an **array of arrays of CSS selectors** — each inner array is a list of selectors that must all be present in the iframe DOM for that assertion line to pass.

#### Default-project `_quarto.yml` shape (verified, carried forward from 2B)

Minimal, no website chrome:

```yaml
project:
  title: Multi-element project
```

No `type:` key.

#### Fixtures

- **`q2-preview/multi-element-doc.qmd`** (single-doc) + supporting assets. One callout, one theorem, one cross-reference, one equation, one embedded image, **one footnote written using inline syntax `^[footnote body]`** (exercises the `FootnotesTransform` inclusion 2B landed — its `<sup class="footnote-ref">` and the `<div class="footnotes">` section), and YAML metadata containing a `license:` value (exercises the `AppendixStructureTransform` inclusion 2B landed — `<div class="quarto-reuse">` should appear inside `<div id="quarto-appendix">`). Frontmatter assertion checks each component's expected class set is present in the rendered iframe DOM.

  **Use inline footnote syntax (`^[body]`), not reference syntax (`[^1]: body`).** Pampa's parser postprocess at `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:1134-1146` converts `Inline::NoteReference` to an empty `Span(class="quarto-note-reference")` before any quarto-core transform runs; nothing downstream resolves those Spans, so reference-style footnotes don't render in either q2-preview or the HTML pipeline today (verified during 2B implementation against `q2 render --to html`). Inline `^[body]` syntax goes through `Inline::Note` directly and is what `FootnotesTransform` actually processes. A separate beads issue is filed to track the upstream parser fix; until then, this fixture must use inline syntax or the `sup.footnote-ref` and `div.footnotes` selectors will fail.
- **`q2-preview/multi-element-project/`** (project-mode) — directory containing `_quarto.yml` (minimal `project:\n  title: ...` — no `type:` key, so it's the default project type, not a website) + `index.qmd` mirroring the multi-element-doc content + a sibling `notes.qmd` so the orchestrator's pass-1 indexes more than one file. Same `ensureHtmlElements` assertions as `multi-element-doc.qmd`. **This fixture is what enforces the project-context coverage rule for 2C's smoke layer** — without it the project pass-2 renderer path (`pass2_renderer.rs::RenderToPreviewAstRenderer`) goes untested for CustomNode rendering.
- **`q2-preview/with-render-components/`** (project-mode override safety net) — directory containing `_quarto.yml` + `index.qmd` with `format: q2-preview` and `render-components: [overrides.tsx]` + a small `overrides.tsx` exporting **two** components — one Pandoc-tag override (`Para` rendering as `<p class="my-para">`) and one CustomNode override (`Callout` rendering as `<div class="my-callout">`). Asserts both fire: the iframe contains `.my-para` (not just default `<p>`) and `.my-callout` (not the built-in `.callout` class). Tests both branches of the merged-registry mechanism (`mergedPreviewRegistry` for Pandoc tags, `mergedCustomNodeRegistry` for CustomNode types — see §Design decisions "User overrides win").

All three fixtures use `_quarto.tests.run.requires_js: true` so the CLI smoke-all runner skips them and the Playwright runner picks them up.

## Risk areas

- **Element-and-structure drift between Rust's HTML output and React rendering.** §Design decisions "Visual + structural parity target" (carried forward from Plan 2B) pins q2-preview to Bootstrap-flavored HTML matching Pandoc's writer choices (element + class + nesting). Drift surfaces in two ways: (a) a React component picks the wrong element (e.g. `<section>` instead of `<div>` for a Callout), making child-selector CSS rules miss; (b) a component emits the right element with the right class but at the wrong nesting depth (e.g. the callout title outside its container instead of inside it), making descendant-combinator CSS rules miss. Mitigation: §"Class-compatibility test" extends to element-and-structure assertions; the smoke-all `multi-element-doc.qmd` fixture's `ensureHtmlElements` selectors include element-with-structure selectors (e.g. `figure > figcaption`, `div.callout > div.callout-header`, `div#quarto-appendix > div.footnotes`).
- **Equation `\tag{N}` is appended in JS, not Rust.** q2-preview's pipeline excludes `CrossrefRenderTransform`, so `Equation.tsx` ports the `\tag{N}` append from `crossref_render.rs:631` into JS. KaTeX renders `\tag{}` natively; the smoke-all q2-preview `multi-element-doc.qmd` fixture is the safety net for the end-to-end render. Risk: future changes to `crossref_render.rs::render_equation` (e.g. different tag format, added wrapping span attributes) won't propagate to JS automatically — keep the §"`Equation.tsx`" entry's `crossref_render.rs:601` line reference current.
- **Class-taxonomy enumeration completeness**. Phase 4.1 enumerates classes from the named Rust source files. Mitigation: cross-check against actual q2-preview demo renders during the manual browser session at the end of Phase 5.5.
- **CustomNode override mechanism — symmetric merge gotcha**. The two merge sites (`mergedPreviewRegistry` for Pandoc tags, `mergedCustomNodeRegistry` for CustomNode types) consume the same `customRegistry` user-export bag. A user export that doesn't collide with either built-in lands in both merges as dead code. v1 ignores this; future enhancement could warn. Documented so an implementor doesn't try to "deduplicate" the merge by routing exports to one map or the other based on heuristics — the disjoint-namespaces guarantee makes the dual-merge unambiguous.
- **Recursion-contract bypass in user CustomNode overrides** (carried forward from Plan 2B). The atomic gate fires only when nodes enter via framework's `<Node>`; user TSX components are free to ignore the contract by iterating slots into hand-rolled JSX, silently disabling atomicity for their descendants. v1 has no edit affordances so the failure is latent, but it becomes a real corruption vector once editing ships. Mitigation: same vitest fixture pattern as Plan 2B's bypass test, extended to a CustomNode override (e.g. a user `Callout` that walks `node.slots.content.value` directly — assert children miss the atomic gate).
- **Fallback `__fallback__` accidental-shadowing**. A user TSX export literally named `__fallback__` would replace the built-in fallback. Improbable (the underscore-padded key is non-idiomatic), but not impossible. v1 accepts the behavior; would be worth a documented warning if it ever bites.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| q2-preview/quartoClasses.ts extensions (callout/theorem/proof/quarto-xref) | ~50 |
| q2-preview/utils.ts extensions (formatRefLabel, composeAttr, renderSlot) | ~70 |
| q2-preview/custom/*.tsx (7 files + Fallback; Equation grows ~30 LOC for JS-side `\tag{N}` port) | ~390 |
| q2-preview/theoremEnvs.ts (8-entry refType→env mapping) | ~15 |
| q2-preview/registry.ts CustomBlock/CustomInline dispatcher entries | ~30 |
| q2-preview/CustomNodeRegistryContext.tsx (NEW) + PreviewRoot extension + provider wiring | ~30 |
| Vitest integration tests (per-component snapshot + structure, generic fallback, override integration, atomic-via-registry, class-compat) | ~210 |
| customNodeWireFormatProject.wasm.test.ts | ~50 |
| Smoke-all q2-preview fixtures (3 fixtures: multi-element-doc + multi-element-project + with-render-components + assets) | ~110 |
| Demo fork (gordon/render-components/) | ~80 |
| **Total** | **~1035** |

Reasonable for one focused session.

**Sub-ordering**: quartoClasses.ts extensions land first (the enumeration commit) per the "enumeration before consumers" rule. Then theoremEnvs.ts + utils.ts extensions. Then the 7 custom components + Fallback. Then registry.ts + CustomNodeRegistryContext + entry.tsx wiring. Then verification.

## Dependencies

### Hard dependencies

- **Plan 2B (Session A)** — must land first. 2C has no value without 2B's framework changes (CustomNode types, unwrap/rewrap, atomic gate, dispatch entries, blockTypes extension), Pandoc-base leaves (referenced by every CustomNode component's body recursion), asset manifest (Image rendering inside Callout / Theorem / FloatRefTarget bodies), and pipeline exclusion list (which keeps `crossref-render` and `callout-resolve` out so the wrappers survive into the iframe).
- **Plan 2pre** ✅ — directory restructure.
- **Plan 2A** ✅ — q2-preview surface scaffolding.
- **Plan 1** ✅ — pipeline + format detection.

### Soft / activation dependencies

(See §"Soft activation dependencies" above.) Plans 4, 6, 7, 8 add to the AST shape 2C watches for; until they land, the relevant detection arms stay dormant.

### Blocks

Nothing structurally. Plans 4 / 5 / 6 / 7 / 8 can land in parallel with 2C; they decorate the AST that 2C's components render.

## Related beads issues

Tracked work *outside* 2C's scope that 2C's design assumes or that 2C's temporary measures hand off to:

- **bd-1kly** — *Complete `FootnotesTransform` for `reference-location: block`/`section`.* Upstream Rust fix for the gap that Plan 2B's `Note.tsx` tooltip-body fallback works around. When closed, Plan 2B's `Note.tsx`, `NoteNumberingContext`, and the JS-side numbering walk all become inert and can be deleted (~30 LOC removed). Also unblocks the tippy.js popup integration.

Future plans that decorate the AST 2C renders (Plans 4 / 5 / 6 / 7 / 8) are tracked in §"Soft activation dependencies" rather than here.

## Notes

- Plan 2C is the second half of a two-plan split that Plan 2B's prior monolithic version (1781 lines, ~2375 LOC implementation surface) outgrew the realistic single-session context budget. Plan 2B (Session A) handles framework recursion semantics + asset manifest + Pandoc-base leaves; Plan 2C (Session B) handles Quarto custom-node renderers + verification. Session-A and Session-B run sequentially, not in parallel. Hand-off is via 2B's checklist completion plus the in-tree state of completed code; 2C's implementor reads 2C + spot-checks 2B's `previewRegistry` + utility files, no transcript needed.
- Following the user's lead: q2-preview is intended to evolve toward a system component (likely a Quarto extension), but the bundling / distribution mechanics are out of scope for 2B and 2C.

## References

### Rust side (read during implementation; not modified by 2C)

- `crates/quarto-pandoc-types/src/{block,inline,custom}.rs` — canonical Block / Inline / CustomNode / Slot enums.
- `crates/pampa/src/writers/json.rs::write_custom_block` (line 1297), `write_custom_inline` (line 1381) — wire format (mirrored by 2B's `framework/customNode.ts`).
- `crates/pampa/src/readers/json.rs::read_custom_block_from_div` (line 2220), `read_custom_inline_from_span` (line 2358) — Rust-side decode (mirrored by 2B's `framework/customNode.ts`).
- `crates/quarto-core/src/transforms/callout_resolve.rs` — Callout HTML structure source.
- `crates/quarto-core/src/transforms/crossref_render.rs` — Theorem/Proof/FloatRefTarget/Equation/CrossrefResolvedRef HTML rendering.
- `crates/quarto-core/src/transforms/callout.rs` — `"Callout"` type_name + plain_data writer.
- `crates/quarto-core/src/transforms/theorem.rs` — plain_data writer.
- `crates/quarto-core/src/transforms/proof.rs` — plain_data writer.
- `crates/quarto-core/src/transforms/float_ref_target.rs` — plain_data writer.
- `crates/quarto-core/src/transforms/equation_label.rs` — plain_data writer.
- `crates/quarto-core/src/transforms/crossref_resolve.rs` — plain_data writer.
- `crates/quarto-core/src/crossref/mod.rs:60-92` — canonical `type_name` strings.

### hub-client side (modified by 2C)

- `hub-client/src/components/render/q2-preview/quartoClasses.ts` — extend Plan 2B's stub with callout/theorem/proof/crossref taxonomy.
- `hub-client/src/components/render/q2-preview/utils.ts` — extend with `formatRefLabel`, `composeAttr`, `renderSlot`.
- `hub-client/src/components/render/q2-preview/custom/*.tsx` (NEW) — type-specific CustomNode components + Fallback.
- `hub-client/src/components/render/q2-preview/theoremEnvs.ts` (NEW) — `theoremEnvFor(refType)` port of `theorem_env_for` (8-entry mapping).
- `hub-client/src/components/render/q2-preview/CustomNodeRegistryContext.tsx` (NEW) — iframe-side context for the merged customNodeRegistry.
- `hub-client/src/components/render/q2-preview/registry.ts` — extend with `CustomBlock` / `CustomInline` dispatcher entries.
- `hub-client/src/components/render/q2-preview/entry.tsx` — extend Plan 2B's PreviewRoot with `CustomNodeRegistryContext.Provider` and `mergedCustomNodeRegistry` computation.

### Demo files

- `~/docs/demo-playground/elliot/{html,custom,kanban,comment,simple,drag,slide}.tsx` — fork target. After 2B+2C, pruned forks land at `~/docs/demo-playground/gordon/render-components/`.

## Revision history

- **2026-05-09**: initial split from Plan 2B. Plan 2B's monolithic version (1781 lines, ~2375 LOC implementation surface) was too large for a single agent session even at 1M-token context. The natural cut is "Pandoc base layer through framework" vs "Quarto custom-node taxonomy + verification" — Plan 2B keeps the former, Plan 2C takes the latter. Two amendments to Plan 2B that this split implies: (1) Plan 2B ships a stub `quartoClasses.ts` with footnote/appendix/section constants only (the ones any non-CustomNode component will reference); 2C fills in the callout/theorem/proof/crossref constants. (2) Plan 2B pulls vitest integration coverage for everything Phase 1-3 touches into its scope (was deferred to Phase 5.1), plus the asset-manifest variant of `assetManifestProject.wasm.test.ts` from 5.3 — so 2B is self-locking and 2C doesn't inherit a verification debt for 2B's work.

- **2026-05-10 (post-2B-implementation amendments)**: cross-checked Plan 2C against what Plan 2B actually shipped on `feature/q2-preview` and the current state of the Rust transform sources. Five mechanical corrections, no design changes:
  - **Stale Rust line refs updated** — transforms have shifted since 2C was written 2026-05-09. Plan 2C's plain_data writer line numbers now match the current sources: `theorem.rs:282-285` → `theorem.rs:145`; `equation_label.rs:215-217` → `equation_label.rs:316` (two occurrences); `crossref_resolve.rs:294-314` → `crossref_resolve.rs:316`. Verified-correct refs not changed: `callout.rs:210`, `proof.rs:145`, `float_ref_target.rs:292-295`, `crossref_render.rs:388-400` (theorem_env_for), `:534-585` (render_proof), `:601-650` (render_equation), pampa wire-format references.
  - **`framework/types.ts:89` → `:163`** — Plan 2B added `Slot`, `CustomNodeBase`, `CustomBlockNode`, `CustomInlineNode`, `CiteInline`, plus block-level gap-fill types (`LineBlockBlock`, `DefinitionListBlock`, `TableBlock`, `Attr` alias) ahead of the `FormatRegistry` definition; the line shifted but the type itself is unchanged.
  - **`entry.tsx:179-182` → `:228-231`** — Plan 2B's PreviewRoot grew the asset-manifest forwarding (`AssetManifestContext.Provider`) and the Note-numbering `useMemo` walk, pushing the `mergedRegistry` block down. The variable's behavior is unchanged; only its location moved.
  - **Provider stack updated to include `NoteNumberingContext.Provider`** — Plan 2B's Phase 3.4 added it between `AssetManifestContext.Provider` and `<Ast>`. 2C's `CustomNodeRegistryContext.Provider` slots in immediately above `<Ast>`, so the full stack post-2C is: `PreviewContext → AssetManifest → NoteNumbering → CustomNodeRegistry → Ast`. Code sample in §"`q2-preview/CustomNodeRegistryContext.tsx`" updated to show the four-deep wrapping.
  - **Naming convention pinned to `mergedPreviewRegistry`** — Plan 2C's body text and code samples used both `mergedRegistry` (legacy 2A name) and `mergedPreviewRegistry` (new symmetric name) inconsistently. Standardized on `mergedPreviewRegistry` throughout; 2C's implementation does a 1-line rename of the existing variable (`entry.tsx:228`) for symmetry with the new `mergedCustomNodeRegistry`. Goal §, Phase 4.4, §"User overrides win", and §"`q2-preview/CustomNodeRegistryContext.tsx`" all updated with the rename note.
  - **Multi-element fixture footnote syntax pinned to inline `^[body]`** — discovered during Plan 2B implementation that pampa's postprocess at `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:1134-1146` converts `Inline::NoteReference` to an empty `Span(class="quarto-note-reference")` before any quarto-core transform runs; nothing downstream resolves those Spans, so reference-style footnotes (`[^1]: body`) don't render in either q2-preview or the HTML pipeline today. Verified by `q2 render --to html` against a reference-style fixture during 2B implementation. The smoke-all `multi-element-doc.qmd` fixture must use inline `^[body]` syntax — that's what `FootnotesTransform` actually processes (via `Inline::Note`). A separate beads issue is filed to track the upstream parser fix; until then, the fixture spec is explicit about the syntax requirement so the `sup.footnote-ref` and `div.footnotes` selectors land on actual markup.
  - **`customNodeWireFormatProject.wasm.test.ts` template recommendation updated** — Plan 2B shipped `assetManifestProject.wasm.test.ts`, which uses the same `initWasm` + project-mode setup pattern that 2C's wire-format test needs. Plan 2C's item 5.3 now points there as the closer template; the older `themeFingerprint.wasm.test.ts` reference is preserved as a secondary touchstone.

  Informational notes (no plan changes needed):
  - **bd-3gtn fixed in Plan 2B (`c8a684bd`)** — the WASM artifact-flush loop now skips empty-content entries, so user-uploaded image bytes survive across renders. Plan 2C's smoke fixtures with images don't need the post-render-add workaround that 2B's first-pass test originally used.
  - **Plan 2B added 5 inline gap-fill `renderChildrenRegistry` entries** (Underline, Strikeout, Superscript, Subscript, SmallCaps) via a `makeFlatInlineRenderer` helper in `framework/dispatch.tsx`. Additive and doesn't affect 2C's design; flagged so a 2C implementor isn't surprised by the extra entries when reading the framework dispatch table.
  - **Plan 1's "fragile-by-design" assertion #3 was never implemented**; the post-fix counterpart (assertion #5: bytes survive) landed natively in `crates/quarto-core/tests/render_page_in_project.rs::website_q2_preview_renders_through_orchestrator` (commit `07e5205f`) and at the WASM-bridge layer in `assetManifestProject.wasm.test.ts`. 2C inherits both as belt-and-suspenders coverage.
