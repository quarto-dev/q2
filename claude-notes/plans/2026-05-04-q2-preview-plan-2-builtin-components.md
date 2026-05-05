# Plan 2 — html.tsx + custom.tsx built-in components

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** M2 (q2-preview looks like the HTML format)

## Goal

Deliver two TSX files — `html.tsx` and `custom.tsx` — that render the
post-q2-preview-pipeline AST faithfully in React. Together they bring q2-preview
to visual parity with the HTML format for documents that use callouts, theorems,
proofs, figures, equations, and cross-references.

For this set of plans, the files are delivered as **drafts** that get manually
pasted into a demo's `render-components: [...]` YAML key (the same mechanism
Elliot's existing demos use). Bundling them as a system component (likely as
a Quarto extension) is deferred to a later effort.

The intention is that this is a draft of something that will become a system
component, so the design choices matter even though the distribution mechanism
is informal.

## Scope

### In scope

- A new `html.tsx` based on Elliot's existing demo, filling Pandoc-base-type
  gaps:
  - `RawInline` (currently missing; needed for callout icons and any HTML-inline
    content).
  - Anything else surfaced during integration testing (Note, EditComment,
    table types if not covered).
- A new `custom.tsx` implementing CustomNode handling:
  - **Encode/decode** symmetry with Rust's `write_custom_block` /
    `read_custom_block_from_div` (terminology: wrap/unwrap/rewrap).
  - **Renderer plumbing** in `ast-renderer-entry.tsx` that intercepts
    `__quarto_custom_node` Divs, unwraps them into JS-native CustomNode
    shapes, and dispatches to type-name components.
  - **Rewrap shim** at the `setLocalAst` / postMessage boundary: when a
    custom.tsx component returns a JS-native CustomNode, the plumbing rewraps
    it to wire format before posting `SET_AST` to the parent.
  - Components for the seven CustomNode type names that q2-preview produces:
    - `Callout`, `Theorem`, `Proof`, `FloatRefTarget`, `Equation` (five
      "structured" types — non-atomic).
    - `CrossrefResolvedRef`, `IncludeExpansion` (two "atomic" types —
      read-only). `IncludeExpansion` comes from Plan 8;
      `CrossrefResolvedRef` is already in the AST today via
      `CrossrefResolveTransform` and per Plan 7's atomic registry is
      treated read-only.
  - **Generic fallback** `CustomNode` component for unknown `type_name` values
    (renders a styled div with the type_name visible, slot contents nested).
- **Derived-provenance read-only treatment for inlines** (new — replaces
  the dropped `ShortcodeResolution` component):
  - The renderer plumbing detects inlines with `Derived` source_info
    (shortcode-resolved Strs, etc.) and renders them in a read-only
    container that doesn't propagate `setLocalAst` to the inline.
  - Visual indicator (subtle background tint or hover badge with the
    Derived `by.kind` — e.g., "shortcode: meta") is a UX nicety; primary
    mechanism is the missing edit affordance.
  - Implemented as a small wrapper helper: `<MaybeReadOnlyInline node={n}
    setLocalAst={...}>{children}</MaybeReadOnlyInline>`. The renderer
    plumbing wraps every inline with this, and the wrapper checks the
    inline's source_info and decides whether to forward setLocalAst.
- **Two-registry design**: Pandoc base components keyed by `node.t` (e.g.
  `Para`, `Header`); CustomNode components keyed by `type_name`. Renderer
  plumbing dispatches to the right registry based on whether the node is a
  wrapper Div.
- **Shared utilities** in custom.tsx:
  - `formatRefLabel(kind, number, title?)` — produces "Theorem 1 (Pythagoras)"
    style labels.
  - `composeAttr(originalAttr, extraClasses, extraKvs)` — adds classes/attrs
    to a Pandoc Attr without mutating original.
  - `renderSlot(slot, dispatch)` — given a `Slot::Block | Slot::Inline |
    Slot::Blocks | Slot::Inlines`, renders via the appropriate dispatch.
  - Class-name constants module mirroring Rust's class taxonomy
    (`callout`, `callout-header`, `callout-title-container`, `theorem`,
    `theorem-title`, `section`, `levelN`, etc.).
- **JS reimplementation of the small per-Render-transform logic** that was
  skipped (see Plan 1):
  - Sidebar **body-classes** derivation (replicates `SidebarRenderTransform`'s
    class output for the `<body>` element). ~20 lines.
  - Navbar **brand-title fallback** (`navbar.title || website.title || document.title`).
    ~5 lines.
- **Iframe CSS loading from VFS** (resolves the §Risk note that was
  previously deferred). Discussed with Elliot; the visual-fidelity
  strategy is **class-compatible-with-bootstrap** — components emit
  the same class names as Rust's HTML output, and the iframe loads
  Quarto's compiled theme CSS so visuals match. Concretely:
  - Modify `hub-client/public/ast-renderer.html` (or inject from
    `ast-renderer-entry.tsx`) to load `/.quarto/project-artifacts/styles.css`
    from VFS. Plan 1's "theme CSS artifact contract" guarantees this
    artifact exists post-render.
  - Bootstrap base (vendored at build time, not from VFS) bundled
    with the iframe's HTML; theme CSS layered on top via injected
    `<link rel="stylesheet">`.
  - Account for the iframe's `sandbox="allow-scripts allow-same-origin"`
    setting — VFS-served CSS reaches the iframe via the same channel
    images do. ~30 lines of HTML/iframe-message work.
- **Page-scoped artifact handling** (resolves Plan 1's §Open question).
  q2-preview's `RenderToPreviewAstRenderer` mirrors `RenderToHtmlRenderer`'s
  Page-scoped artifact handling: image artifacts produced by
  `ResourceCollectorTransform` land in VFS under
  `/.quarto/project-artifacts/`, served to the iframe as `<img src=...>`
  via the same channel as theme CSS. Bootstrap-CSS-compatible markup
  needs working images for figure rendering, so this default is
  load-bearing for visual fidelity.

### Out of scope

- Layout/chrome components (TOC sidebar, navbar, footer, page-nav strip
  rendering as page chrome). Defer to a future plan; q2-preview v1 renders
  the body only. Navigation data is still populated in metadata by Plan 1's
  pipeline; rendering it is a separate UI concern.
- Edit affordances (theorem-rename UI, callout-type changer, etc.). v1 is
  structural-only rendering.
- Bundling / distribution. Files are pasted into demos via
  `render-components: [...]` YAML.
- Drift-detection contract test (Rust HTML output ↔ React render). Useful
  long-term; defer.

## Design decisions (settled in conversation)

- **html.tsx uses raw wrapper (Option A)**: components receive raw AST nodes,
  including `__quarto_custom_node` wrapper Divs if the renderer plumbing
  doesn't intercept first. Components keep their existing simple shape.
- **custom.tsx uses unwrapped CustomNode form (Option B)**: components
  receive the JS-native CustomNode shape (`{ type_name, slots, plain_data,
  attr, source_info }`) so they can read `node.slots.title` directly.
- **Renderer plumbing intercepts wrapper Divs before component dispatch**.
  Both files stay independent of each other — the unwrap/rewrap layer lives
  in `ast-renderer-entry.tsx` (the iframe's entry).
- **Two registries**: `componentRegistry` keyed by `node.t`,
  `customNodeRegistry` keyed by `type_name`. User overrides target one or
  the other explicitly. Generic fallback in customNodeRegistry handles
  unknown type_names.
- **Atomic content is read-only**, enforced via two paths:
  - **Atomic CustomNodes** (`IncludeExpansion`, `CrossrefResolvedRef`):
    components do not pass `setLocalAst` to slot children. Identified
    by `type_name` matching `is_atomic_custom_node` from Plan 7.
  - **Derived-provenance inlines** (shortcode resolutions): the
    `MaybeReadOnlyInline` wrapper detects `Derived` source_info on the
    inline and disables propagation of `setLocalAst`.
  - Visual indicator (subtle background tint or hover badge) is a UX
    nicety; primary mechanism is the missing edit affordance.
- **Visual fidelity tier**: class-compatible. Same CSS class names as Rust's
  HTML output where the AST shape diverges, so loading Quarto's CSS produces
  matching visuals. DOM structure may differ where it doesn't affect CSS.
- **Class-name constants live in a TS module within custom.tsx** for v1.
  Long-term the constants are a candidate for code-generation from a single
  Rust source, but not in this plan.

## Encode / decode / rewrap (terminology and operations)

The CustomNode lifecycle has three (or four) operations in our system:

- **Wrap (Rust → wire)**: existing in `pampa/src/writers/json.rs::write_custom_block`.
  Rust CustomNode → wire-format Div with `__quarto_custom_node` class and
  slot-named child Divs.
- **Decode (Rust read)**: existing in `pampa/src/readers/json.rs::read_custom_block_from_div`.
  Wire-format Div → Rust CustomNode.
- **Unwrap (JS, in iframe)**: NEW. Wire-format Div → JS-native CustomNode.
  Mirrors Rust's decode. Lives in `ast-renderer-entry.tsx`.
- **Rewrap (JS, in iframe before postMessage)**: NEW. JS-native CustomNode →
  wire-format Div. Mirrors Rust's wrap. Lives in `ast-renderer-entry.tsx`.

The wire format is the lingua franca; typed shapes are local conveniences on
each side. Rust's wrap and the JS rewrap produce the same wire format from
either typed shape; Rust's decode and JS's unwrap produce typed shapes from
the wire format.

## Open questions for implementation

- **Where exactly does unwrap fire in ast-renderer-entry.tsx?** Probably as
  a transformation step before `<Ast />` renders, walking the AST and
  unwrapping `__quarto_custom_node` Divs in place. Confirm during
  implementation.
- **Slot dispatch for renderSlot**: a `Slot::Block` slot contains a single
  block; how does the React rendering handle this vs. `Slot::Blocks`? Probably
  uniform — single block dispatched as a one-element array. Confirm.
- **Generic fallback for unknown `type_name`**: rendering convention. Probably:
  styled box with `data-custom-type` displayed, slot contents rendered as
  nested children. Useful for extension-defined CustomNodes that q2-preview
  encounters without a specific component.
- **CrossrefResolvedRef**: it's an *inline* CustomNode (vs. all the others
  which are blocks). Wire format uses `Span` wrapper instead of `Div`. Confirm
  the unwrap/rewrap handles both. Same registry, dispatched on `type_name`.
- **Where does the class-name constants module live**: own file
  (`custom.tsx` becomes `custom/index.tsx` + `custom/classes.ts` + maybe
  `custom/util.ts`)? Or single file? Probably split for readability.

## References

- `hub-client/public/ast-renderer.html` — iframe host page.
- `hub-client/src/ast-renderer-entry.tsx` — iframe entry; where unwrap/rewrap
  plumbing lives.
- `hub-client/src/components/render/ReactAstDebugRenderer.tsx` — existing
  registry mechanism; renderChildrenRegistry pattern.
- `hub-client/src/components/render/AstIframe.tsx` — postMessage protocol
  (parent ↔ iframe).
- `hub-client/src/services/tsxTranspiler.ts` — Babel-standalone transpiler
  for user-supplied components.
- Elliot's demo files in `~/docs/demo-playground/elliot/`:
  - `html.tsx` (existing reference for Pandoc base types).
  - `comment.tsx`, `kanban.tsx`, `drag.tsx` (existing custom-component patterns).
- Rust references for the CustomNode wire format:
  - `crates/pampa/src/writers/json.rs::write_custom_block` (line ~1297).
  - `crates/pampa/src/readers/json.rs::read_custom_block_from_div` (line ~2220).
- Rust references for type-specific HTML rendering (mirror in TSX):
  - `crates/quarto-core/src/transforms/callout_resolve.rs` (Callout HTML structure).
  - `crates/quarto-core/src/transforms/crossref_render.rs::render_theorem`
    (line ~321), `render_proof` (~534), `render_float_ref_target` (~223),
    `render_equation` (~601), `render_resolved_ref` (~657).

## Test plan

- **Unwrap/rewrap round-trip property test**: for each known CustomNode type,
  `unwrap(wrap(node)) === node` (deep structural equality on the JS-native
  shape) and `wrap(unwrap(wireDiv)) === wireDiv` (deep equality on the wire
  shape). Catches drift between unwrap/rewrap.
- **Rust-Rust-JS round-trip**: build a CustomNode in Rust, wrap to JSON, ship
  to JS (mock the iframe boundary), unwrap, rewrap, ship back, decode in
  Rust, assert structural equality with the original Rust node.
- **Component snapshot tests**: render each of the 7 CustomNode components
  with a fixed input, snapshot the rendered DOM. Detect unintended changes.
- **Generic fallback test**: render a wrapper Div with `type_name: "Unknown"`
  via the renderer plumbing; assert the fallback component renders with the
  type name visible.
- **Class-compatibility test**: for each component, assert the rendered
  classes match the documented class taxonomy (the constants module).
- **Atomic CustomNode read-only test**: render an `IncludeExpansion` or
  `CrossrefResolvedRef` wrapper; assert children don't receive a
  `setLocalAst` prop (or receive a no-op that triggers a console warning).
- **Derived-inline read-only test**: render a Para containing inlines with
  Derived source_info (a shortcode-resolved title); confirm typing into
  the resolved text doesn't propagate setLocalAst.

## Dependencies

### Hard dependencies

- **Plan 1** — the q2-preview format and AST shape these components
  consume don't exist until Plan 1 lands. Plan 2 cannot ship before Plan 1.

### Soft / activation dependencies

Plan 2 lands all its wiring at once after Plan 1. The Derived-detection
and atomic-registry hooks are inert until later plans populate the AST
shapes and registries they watch for. Each activates organically as the
relevant plan lands:

- **Plan 4** introduces the `Derived` variant. Until Plan 4 lands, no
  inline can have Derived source_info, so `MaybeReadOnlyInline`'s
  Derived-detection arm never fires. Compiles fine — `Derived` is just
  a variant the wrapper checks for; nothing breaks if no value ever
  matches.
- **Plan 6** populates Derived source_info on shortcode resolutions.
  Before Plan 6, the shortcode resolver still emits flat Strs with
  `SourceInfo::default()`. After Plan 6, those Strs have Derived
  source_info and `MaybeReadOnlyInline` activates for them.
- **Plan 7** introduces `is_atomic_custom_node`. Until Plan 7 lands,
  Plan 2's atomic-CustomNode dispatch hardcodes the initial set
  (`["CrossrefResolvedRef"]` — the only atomic type that exists in the
  AST today, via `CrossrefResolveTransform`). After Plan 7, the JS
  side syncs against the Rust function's set.
- **Plan 8** introduces `IncludeExpansion` CustomNode. Plan 2's
  IncludeExpansion component is registered from the start; until Plan 8,
  no IncludeExpansion CustomNodes appear in the AST so the component is
  never instantiated.

This dormant-wiring pattern is intentional. Plan 2's job is to lay down
the React-side scaffolding for everything q2-preview will eventually
need; later plans just plug in.

### Blocks

Nothing structurally. Later plans (4-7) can land in parallel.

## Risk areas

- **CSS theming** (resolved — see §Scope "Iframe CSS loading from VFS").
  q2-preview's iframe loads Bootstrap + Quarto's compiled theme CSS so
  components' bootstrap-compatible class names resolve to matching
  visuals. Risk surface that remains during integration: the iframe's
  `sandbox="allow-scripts allow-same-origin"` setting must permit the
  `<link>` to a VFS-served URL; confirm during implementation.
- **Math (KaTeX) inside Equation CustomNode**: Equation CustomNode in q2-preview
  contains a Math inline (with possibly `\tag{N}` appended by `CrossrefIndex`).
  Rendering goes through KaTeX (`window.katex` per the current iframe
  setup). Confirm tagging works.
- **Drift between Rust's HTML output and our React rendering**: real but
  bounded. We commit to class-compatible (same class names), not
  DOM-equivalent. Where DOM differs, CSS may need adjustment. Catch via
  visual inspection during M2; formalize a contract test in a future plan if
  it becomes a maintenance burden.
- **The `__quarto_custom_node` class polluting rendered DOM**: the unwrap
  plumbing should strip this class before any html.tsx component sees the
  wrapper. But if a user's render-component override on `Div` runs AFTER
  unwrap (which it doesn't, but in case), it should not see the class.
  Verify the dispatch order.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| html.tsx (gap fills) | ~50 |
| custom.tsx components (7 type-specific) | ~360 |
| `MaybeReadOnlyInline` wrapper for Derived inlines | ~30 |
| Renderer plumbing (unwrap/rewrap dispatch + Derived detection) | ~100 |
| Shared utilities (formatRefLabel, composeAttr, renderSlot) | ~80 |
| Class-name constants module | ~80 |
| JS reimpl: sidebar body-classes, navbar brand-fallback | ~30 |
| Tests | ~250 |
| **Total** | **~970** |

Probably fits in one session if the iframe's CSS-loading detail doesn't
overflow. Risk: the iframe-side rewrap shim is the most novel part; budget
extra time for that.

## Notes

- Following the user's lead: this is a *draft* that will eventually become a
  system component shipped as a Quarto extension. The plans treat it as such
  — design conventions matter (two registries, class taxonomy), but
  bundling/distribution mechanics don't need solving here.
- Per design decision: html.tsx and custom.tsx don't import each other.
  Cross-cutting concerns (the unwrap/rewrap plumbing, class-name constants)
  live in their own module that both reference.
