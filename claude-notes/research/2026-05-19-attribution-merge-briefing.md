# Merge briefing: `origin/main` → `feature/q2-preview-command`

**Date:** 2026-05-19
**Source:** `origin/main` at `19318ffb` (includes PR #190, "Attribution pipeline")
**Target:** `feature/q2-preview-command` at `e26d8a6e`
**Conflicts:** 5 file-location (Claude will resolve) + 9 content (you resolve)
**Plan reference:** `claude-notes/plans/2026-05-06-attribution-pipeline.md` (this file lives only on `origin/main` until the merge lands — read via `git show origin/main:claude-notes/plans/2026-05-06-attribution-pipeline.md` if you want the full 2340-line rationale)

---

## TL;DR resolution heuristics

Across almost every content conflict, the same two-axis tension repeats:

| Axis | Keep from `main` | Keep from `feature` |
|---|---|---|
| **Attribution feature** | New props, hooks, components, WASM entry-point names, dispatcher wraps | (nothing — feature has no attribution code) |
| **Module layout** | (nothing — main is pre-reorg) | Package-alias imports (`@quarto/preview-renderer/...`, `@quarto/preview-runtime`), the stub-and-real-entry split |

So the recipe per conflict is usually:
1. Start from **feature**'s file (correct imports, correct package boundaries).
2. **Graft in** the new attribution surface from main (new props, hook calls, WASM signature additions, `<AttributionWrap>` wrapping).
3. Re-target any relative imports `main` introduces for attribution helpers to their ts-packages equivalents.

The exception is `q2-preview/entry.tsx` — see file-specific notes.

---

## What PR #190 adds on `origin/main`

### New context + hooks

- **`AttributionLookupContext`** — `createContext<Map<number, NodeAttributionIdentity> | null>(null)`. Keyed by the source-info pool id (`s` field on every JSON node). Provided once per AST render by `framework/Ast.tsx`.
- **`useNodeAttribution(node)`** — `useContext` consumer; returns `NodeAttributionIdentity | null`. Used inside `AttributionWrap` and any consumer that needs to colour/decorate a specific node.
- **`useAttributionHover()`** — Hover-state hook used by hub-client's hover-badge rendering. Imported in some places where it's currently unused (preserve the import for forward compatibility if you see it; main treats this as a public-ish framework export).
- **`useAttribution(...)`** — Higher-level hook that owns the in-flight attribution computation in `ReactPreview`. Drives `onAttributionGeneratingChange` and dispatches the `*WithAttribution` WASM calls.

### New components

- **`<AttributionWrap node={...} as="div" | "span">{inner}</AttributionWrap>`** — Phase 3 wrapper. Wraps every block / inline output with `<div class="q2-attr-wrap" data-sid={s}>...</div>` (or `<span>` for inlines) when attribution is active; pass-through otherwise. **This wrap is on the hot dispatcher path** — both q2-debug and q2-preview dispatchers wrap every node.
- **`AttributionBadge`** — Hover tooltip with author name + relative time.
- **"Authorship" pill** (in hub-client's replay drawer) — toggles `authorshipOn`. Out-of-scope for the dispatcher files; lives in higher-level UI.

### New WASM surface (on main, in `hub-client/src/services/wasmRenderer.ts`)

Two new entry points on the `WasmModuleExtended` interface, plus two new exported wrapper functions:

```ts
// On the WASM module interface:
render_page_in_project_with_attribution(
    path: string,
    user_grammars?: ...,
    attribution_json?: string | null
): Promise<string>;

parse_qmd_to_ast_with_attribution(
    content: string,
    attribution_json: string | null
): Promise<string>;

// Exported wrappers (already in main):
export async function renderPageInProjectWithAttribution(path, userGrammars, attributionJson) { ... }
export async function parseQmdToAstWithAttribution(qmdContent, attributionJson) { ... }
```

The old `render_page_in_project` and `parse_qmd_to_ast` functions still exist on main; the existing exported `parseQmdToAst` / `renderPageInProject` now just delegate to the `*WithAttribution` wrappers with `null` as the attribution JSON. **Backwards-compatible.**

### New module on main (no equivalent on feature)

- **`hub-client/src/components/render/iframeMessageDispatch.ts`** — shared dispatcher used by `q2-debug/entry.tsx` and `q2-preview/entry.tsx` on main. Coordinates three message kinds the parent sends to the iframe. Exposed as `makeIframeMessageDispatcher(...)`.

  **There is no equivalent file anywhere on the feature branch** (`grep -r iframeMessage` finds zero hits in `hub-client/` or `ts-packages/`). On the feature branch, q2-debug/entry.tsx uses a manual `setInterval`-based `componentsLoading` polling pattern, and q2-preview/entry.tsx is a stub (real entry lives in `ts-packages/preview-renderer/src/q2-preview/entry.tsx`).

  **Decision needed during merge:** when the merge lands `iframeMessageDispatch.ts` at the old hub-client path,
  (a) leave it there and have q2-debug/entry.tsx adopt the new pattern;
  (b) move it into `ts-packages/preview-renderer/src/` (or similar) and update both q2-debug and the real q2-preview entry in ts-packages to use it.

  Recommendation: (b), keeps the q2-debug/q2-preview symmetry main intended. But that's a real package-boundary judgment call and may be deferrable.

### New CLI/Rust surface (no merge impact on the 9 TS conflicts, but explains the WASM additions)

The Rust pipeline gains `AttributionGenerateTransform` / `AttributionRenderTransform`; YAML config at `attribution.identities` and `attribution.viewer`; CLI flag `--attribution=git`. The pipeline plan file (`2026-05-06-attribution-pipeline.md` on main) has the full design. None of this needs your attention during the TS merge.

---

## What the feature branch did (ts-packages extraction)

Two new workspace packages live under `ts-packages/`:

- **`@quarto/preview-renderer`** at `ts-packages/preview-renderer/src/`
- **`@quarto/preview-runtime`** at `ts-packages/preview-runtime/src/`

### Path moves

| Lived on main at | Now on feature at |
|---|---|
| `hub-client/src/components/render/framework/*` | `ts-packages/preview-renderer/src/framework/*` |
| `hub-client/src/components/render/q2-preview/*` | `ts-packages/preview-renderer/src/q2-preview/*` (real); `hub-client/src/components/render/q2-preview/entry.tsx` becomes a one-line stub |
| `hub-client/src/types/diagnostic.ts`, `project.ts`, etc. | `ts-packages/preview-renderer/src/types/*` |
| `hub-client/src/services/wasmRenderer.ts` | `ts-packages/preview-runtime/src/wasmRenderer.ts` |
| `hub-client/src/components/render/overlays/PreviewStaticInfoViews.tsx` | `ts-packages/preview-renderer/src/overlays/PreviewStaticInfoViews.tsx` |

### Notably **NOT** moved (still authoritative in hub-client)

- **`hub-client/src/components/render/q2-debug/*`** — all of `components.tsx`, `dispatchers.tsx`, `entry.tsx`, `registry.ts`, etc. still live in hub-client. There is **no** `ts-packages/preview-renderer/src/q2-debug/`. The q2-debug surface didn't make the cut for extraction (yet).
- `hub-client/src/components/render/PreviewRouter.tsx`, `ReactPreview.tsx`, `Preview.tsx`, the various integration tests — still in hub-client.

### Import-style shift on feature

Imports across the feature branch use **package aliases**:

```ts
// Feature-style (correct after reorg):
import { RegistryContext } from '@quarto/preview-renderer/framework';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { parseQmdToAst } from '@quarto/preview-runtime';
import { FallbackView } from '@quarto/preview-renderer/overlays/PreviewStaticInfoViews';

// Main-style (pre-reorg, what the merge will bring in):
import { RegistryContext } from '../framework/RegistryContext';
import type { FileEntry } from '../../types/project';
import { parseQmdToAst } from '../../services/wasmRenderer';
import { FallbackView } from './PreviewStaticInfoViews';
```

**Whenever you see main introducing relative imports for things that have moved into ts-packages, retarget to the package alias.**

---

## File-location conflicts (the 5 Claude is resolving)

All 5 are PR #190's new attribution framework files, added on main at the old `hub-client/src/components/render/framework/` path. They need to land at the new `ts-packages/preview-renderer/src/framework/` path.

| Added on main at | Moves to |
|---|---|
| `hub-client/src/components/render/framework/AttributionLookupContext.tsx` | `ts-packages/preview-renderer/src/framework/AttributionLookupContext.tsx` |
| `hub-client/src/components/render/framework/attribution.tsx` | `ts-packages/preview-renderer/src/framework/attribution.tsx` |
| `hub-client/src/components/render/framework/attribution.test.ts` | `ts-packages/preview-renderer/src/framework/attribution.test.ts` |
| `hub-client/src/components/render/framework/attribution.test.tsx` | `ts-packages/preview-renderer/src/framework/attribution.test.tsx` |
| `hub-client/src/components/render/framework/attribution.styles.test.ts` | `ts-packages/preview-renderer/src/framework/attribution.styles.test.ts` |

Imports inside these files reference siblings (`./AttributionLookupContext`, `./RegistryContext`, etc.) — relative imports stay relative, so most of the file contents don't need internal edits. The one thing to watch is the `import viewerCss from 'virtual:quarto-attribution-viewer-css';` at the top of `attribution.tsx`: that's a Vite virtual module configured in `vite.config.ts`. If `vite.config.ts` on the feature branch doesn't already have the matching plugin (it almost certainly doesn't — it lands as part of this merge), the virtual import will need to be wired up at the ts-packages build level too.

The framework barrel (`ts-packages/preview-renderer/src/framework/index.ts`) needs new re-exports for the attribution surface so consumers can `import { AttributionWrap, useNodeAttribution } from '@quarto/preview-renderer/framework';` — Claude will do this as part of the file-location resolution.

---

## Per-file content-conflict analysis (the 9 you resolve)

### 1. `hub-client/src/components/render/PreviewRouter.tsx`

**Main side adds three props:**
- `identities?: Record<string, ActorIdentity>` — Automerge actor → display identity map. Imported from `'../../services/automergeSync'` on main; on feature, `ActorIdentity` will need to come from wherever the hub-client service file lives (probably still in `hub-client/src/services/automergeSync.ts` — this file wasn't part of the ts-packages move).
- `authorshipOn: boolean` — overlay toggle, owned by Editor.tsx.
- `onAttributionGeneratingChange?: (generating: boolean) => void` — animation state callback.

These props are destructured from `props` on line ~141 (main) and forwarded only to `<ReactPreview>` — `<Preview>` (the non-React iframe path) doesn't get them.

**Feature side** has the same router shape but with imports retargeted:
- `import type { FileEntry } from '@quarto/preview-renderer/types/project';` (was `'../../types/project'`)
- `import { parseQmdToAst, isWasmReady, initWasm } from '@quarto/preview-runtime';` (was `'../../services/wasmRenderer'`)
- `import { FallbackView, NonQmdPlaceholderView } from '@quarto/preview-renderer/overlays/PreviewStaticInfoViews';` (was `'./PreviewStaticInfoViews'`)

**Resolution:** keep feature's imports verbatim. From main's diff, lift in the three new props (interface), the destructuring on line 141, and the three additional props passed to `<ReactPreview>`. `ActorIdentity` imports from `'../../services/automergeSync'` on main can stay relative (`automergeSync.ts` didn't move).

---

### 2. `hub-client/src/components/render/ReactPreview.tsx`

The deepest conflict. Three independent changes overlap:

**Main adds:**
- Two props in `PreviewProps`: `identities` and `authorshipOn` (same shapes as PreviewRouter).
- A `useAttribution(...)` hook call early in the component body. The hook owns the attribution computation state and reports it via `onAttributionGeneratingChange`.
- Swap of WASM calls: `parseQmdToAst` → `parseQmdToAstWithAttribution`, `renderPageInProject` → `renderPageInProjectWithAttribution`. The new calls take an extra `attributionJson` argument that comes out of `useAttribution`.
- The attribution map (`Map<number, NodeAttributionIdentity>`) is built from the WASM response and threaded to the `<AttributionLookupContext.Provider>` wrapped around the renderer.

**Feature changes:**
- All WASM imports come from `@quarto/preview-runtime`.
- Types from `@quarto/preview-renderer/types/...`.
- May have a slightly different prop list around `format` and `fileContents` plumbing (the preview epic touched this area).

**Resolution:** the highest-friction file. Suggested approach:
1. Start from feature's file.
2. Add the two new props to the interface (`identities`, `authorshipOn`) and any callback (`onAttributionGeneratingChange`).
3. Replace `parseQmdToAst` and `renderPageInProject` calls with their `*WithAttribution` variants from `@quarto/preview-runtime`. (These exports will exist on `@quarto/preview-runtime` after you resolve `wasmRenderer.ts` in file 9 below.)
4. Wire up `useAttribution` (imported from `@quarto/preview-renderer/framework`) and use its output to compute the lookup map and the JSON passed to WASM.
5. Wrap the rendered AST in `<AttributionLookupContext.Provider value={lookup}>`.

Take time on this one — bad merge here breaks both attribution AND the React preview path.

---

### 3. `hub-client/src/components/render/q2-debug/components.tsx`

q2-debug is **not moved** to ts-packages.

**Main:** imports are still relative — `'../framework/types'`, etc. Adds an import for `useAttributionHover` from `'../framework'` (the import may be there but unused in this specific file — main is generous about exporting the hook for forward-compat).

**Feature:** imports are package-aliased — `@quarto/preview-renderer/framework` for both `RegistryContext` and types.

**Resolution:** keep feature's package-aliased imports. If main's diff adds an import for `useAttributionHover` and the file actually uses it, retarget the import to `@quarto/preview-renderer/framework`. If the import is unused (very possible per the subagent's read), drop it — better to keep the file clean.

---

### 4. `hub-client/src/components/render/q2-debug/dispatchers.tsx`

**Main:** wraps both `Block` and `Inline` dispatcher outputs in `<AttributionWrap>`:

```tsx
export const Block = (args: NodeArgs<BlockNode>) => {
    // ... existing registry lookup ...
    const inner = Component ? <Component {...args} /> : <div ...>Not registered</div>;
    return <AttributionWrap node={args.node} as="div">{inner}</AttributionWrap>;
};

export const Inline = (args: NodeArgs<InlineNode>) => {
    // ... existing registry lookup ...
    const inner = Component ? <Component {...args} /> : <span ...>Not registered</span>;
    return <AttributionWrap node={args.node} as="span">{inner}</AttributionWrap>;
};
```

`AttributionWrap` imported from `'../framework'` on main.

**Feature:** no wrap. `RegistryContext` and types imported from `@quarto/preview-renderer/framework`.

**Resolution:** keep feature's import alias; add the wrap pattern from main; import `AttributionWrap` from `@quarto/preview-renderer/framework`. (After file-location resolution lands, `AttributionWrap` will be re-exported from the framework barrel.)

---

### 5. `hub-client/src/components/render/q2-debug/entry.tsx`

The trickiest q2-debug file because the two sides disagree about more than imports.

**Main side:**
- Imports `makeIframeMessageDispatcher` from `'../iframeMessageDispatch'` (new file PR #190 added — see context section).
- Adopts the dispatcher pattern: `const dispatch = makeIframeMessageDispatcher({...});` and uses `dispatch.onTheme(...)`, `dispatch.onComponents(...)` style listeners.

**Feature side:**
- No `iframeMessageDispatch.ts` exists anywhere on this branch.
- Uses a manual `setInterval`-based `componentsLoading` polling loop to coordinate component readiness.
- Imports from `@quarto/preview-renderer/framework`, `@quarto/preview-renderer/utils/customRegistry`.

**Resolution — needs a real choice:**
- **Easy path:** keep feature's manual polling; just add any attribution-relevant imports/setup from main that aren't tied to `makeIframeMessageDispatcher`. Defer adoption of the dispatcher pattern to a follow-up.
- **Right path:** decide where `iframeMessageDispatch.ts` should live (likely ts-packages — see context section), move it there as part of this merge, retarget the import in this file, and adopt the dispatcher pattern. This unifies q2-debug and the (real) q2-preview entry around one message-dispatch idiom.

Pick the path that matches your appetite for cross-file work in this merge. The "easy path" can be cleaned up in a separate PR.

---

### 6. `hub-client/src/components/render/q2-preview/entry.tsx`

**This file is a one-line stub on feature:**

```ts
// q2-preview iframe entry — re-imports the real entry from the shared
// `@quarto/preview-renderer` workspace package. The entry was moved out
// of hub-client by bd-hfjj Phase 4 but the script tag in
// `hub-client/q2-preview.html` (and the parity integration test in
// `parity.integration.test.tsx`) keep the original path stable by
// going through this one-line stub.
import '@quarto/preview-renderer/q2-preview/entry';
```

**Main side has the full ~320-line entry implementation** (because the move hadn't happened on main).

**Resolution:** keep feature's stub. **Do not paste main's body in.** Instead, switch your attention to `ts-packages/preview-renderer/src/q2-preview/entry.tsx` (which is the *real* entry now) and verify whether attribution wiring needs to be added there. The conflict in this `hub-client/.../entry.tsx` file is purely an artifact of git not knowing the move happened — `git checkout --ours hub-client/src/components/render/q2-preview/entry.tsx` then `git add`.

If `ts-packages/preview-renderer/src/q2-preview/entry.tsx` doesn't show up in the conflict list, the real entry already has whatever attribution wiring main wanted (which is plausible if PR #190 only touched the hub-client stub-era files). Skim it once just to be sure.

---

### 7. `ts-packages/preview-renderer/src/q2-preview/PreviewDocument.tsx`

**Main side:** imports `useAttributionHover` from `'../framework'`. Per the subagent's read, the hook isn't called in this file — the import looks defensive / for future use.

**Feature side:** no `useAttributionHover` import; otherwise the file is the same.

**Resolution:** if main's diff actually does call the hook (verify by looking at the diff body, not just the imports), keep the call and re-target the import to a relative `'../framework'` (which stays relative inside ts-packages). If it's just an unused import, drop it.

---

### 8. `ts-packages/preview-renderer/src/q2-preview/dispatchers.tsx`

Same shape as conflict #4 (q2-debug dispatchers), one package level down:

**Main:** wraps `Block` and `Inline` outputs in `<AttributionWrap>`, imports it from `'../framework'`.

**Feature:** no wrap.

**Resolution:** add the wrap. Inside ts-packages the import stays relative: `import { AttributionWrap } from '../framework';`. Mirror exactly what you do in file #4 (q2-debug) — both dispatchers should wrap identically.

---

### 9. `ts-packages/preview-runtime/src/wasmRenderer.ts`

The WASM glue. Main adds two new entry-point signatures and two exported wrappers.

**On main, in `WasmModuleExtended` (the TS shape of the WASM module):**

```ts
render_page_in_project_with_attribution: (
    path: string,
    user_grammars: ... | null | undefined,
    attribution_json: string | null,
) => Promise<string>;

parse_qmd_to_ast_with_attribution: (
    content: string,
    attribution_json: string | null,
) => Promise<string>;
```

**Exported wrappers (also added on main):**

```ts
export async function renderPageInProjectWithAttribution(
    path: string,
    userGrammars: ... | null,
    attributionJson: string | null,
): Promise<string> {
    const wasm = await ensureWasm();
    const result = await wasm.render_page_in_project_with_attribution(path, userGrammars, attributionJson);
    return result;
}

export async function parseQmdToAstWithAttribution(
    qmdContent: string,
    attributionJson: string | null,
): Promise<string> {
    const wasm = await ensureWasm();
    const responseJson = await wasm.parse_qmd_to_ast_with_attribution(qmdContent, attributionJson);
    return responseJson;
}
```

**Backwards-compatible delegations:**

```ts
// Old `renderPageInProject` and `parseQmdToAst` on main now just call
// their *WithAttribution counterparts with attributionJson = null.
export function renderPageInProject(path, userGrammars) {
    return renderPageInProjectWithAttribution(path, userGrammars, null);
}
export function parseQmdToAst(qmdContent) {
    return parseQmdToAstWithAttribution(qmdContent, null);
}
```

**Feature side** has the pre-attribution wasmRenderer, located at `ts-packages/preview-runtime/src/wasmRenderer.ts` (was at `hub-client/src/services/wasmRenderer.ts` on main), with imports retargeted into the ts-packages module graph.

**Resolution:** add main's interface members, exported wrappers, and the backwards-compat delegations. Keep feature's import paths (relative-within-ts-packages). The WASM build itself must export `render_page_in_project_with_attribution` and `parse_qmd_to_ast_with_attribution` symbols — these are added on main as part of the Rust pipeline work (PR #190), and the merge of the Rust side will land them in the WASM build automatically. After the merge, do `cargo xtask verify` to confirm the WASM build is producing the new symbols.

---

## Additional conflicts surfaced once the merge actually ran

The initial scoping (off the abort output) caught the 9 TS content conflicts above plus the 5 file-location ones. When the merge re-ran, six more content conflicts surfaced that the abort summary truncated. They have the same overall shape — concurrent signature/structure additions on both branches — and most follow the "combine both sets of additions" pattern.

### 10. `crates/quarto-core/src/pipeline.rs` (single conflict, ~line 1157)

`origin/main` inserts two new entries (`"website-favicon"`, `"attribution-viewer"`) into a list of CLI-only transforms — between `<<<<<<<` and `=======` the feature side is empty (these transforms don't exist on this branch yet from the PR-#190 angle, though the comment on main makes clear that `attribution-viewer` is the CLI-side counterpart to the hub-client's `framework/attribution.tsx`).

**Resolution:** keep main's additions verbatim. The transforms `website-favicon` and `attribution-viewer` exist on main (in the merged crates/quarto-core source) and need to be referenced here.

### 11. `crates/quarto-core/src/project/pass2_renderer.rs` (2 conflicts)

Both branches added new fields (and constructor / builder methods) to the renderer struct:

- **Feature** added `capture: Option<quarto_trace::EngineCapture>` (bd-lucp — the engine-execution capture / splice path).
- **Main** added attribution-related fields (the `attribution_json` transport plumbing).

**Resolution:** these are independent additions. Combine both sets of fields/methods. Mirror the pattern from `wasm-quarto-hub-client/src/lib.rs` below: every function that takes `capture: Option<EngineCapture>` on feature now needs to *also* take the attribution param from main.

### 12. `crates/quarto-core/src/stage/mod.rs` (single conflict, ~line 113)

Concurrent additions to a `pub use crate::stage::stages::{...};` re-export list:

```
<<<<<<< HEAD
    ApplyTemplateStage, AstTransformsStage, CaptureSpliceStage, CompileThemeCssStage,
=======
    ApplyTemplateStage, AstTransformsStage, AttributionGenerateStage, CompileThemeCssStage,
>>>>>>> origin/main
```

**Resolution:** keep both. Final list should include `AstTransformsStage, AttributionGenerateStage, CaptureSpliceStage, CompileThemeCssStage` (alphabetical sort).

### 13. `crates/wasm-quarto-hub-client/src/lib.rs` (9 conflicts) — the heaviest Rust one

Same pattern as `pass2_renderer.rs`, mirrored at the WASM boundary. Two concurrent optional-parameter additions to the same function (`render_single_doc_to_response`) and a couple of its siblings:

- **Feature** added two trailing parameters: `prefer_preview_format: bool` and `capture: Option<quarto_trace::EngineCapture>`. Used by `render_page_for_preview` and the bd-lucp capture-splice path.
- **Main** added one trailing parameter: `attribution_json: Option<String>`. Used by the PR-#190 attribution plumbing.

The 9 conflict markers split into three categories:
1. **Call sites** — `render_single_doc_to_response(path, &content, &project, user_grammars, false, None)` (feature) vs `render_single_doc_to_response(path, &content, &project, user_grammars, None)` (main). Need to merge into `render_single_doc_to_response(path, &content, &project, user_grammars, false, None, None)` once the function signature is unified.
2. **Signature decls** — the function-definition lines themselves (both branches added new params to the same `pub async fn ...`). Combine into one signature carrying both new params (`prefer_preview_format: bool, capture: Option<EngineCapture>, attribution_json: Option<String>`).
3. **Body conflicts** — one block (around line 1714–1722) where feature installs the capture on the renderer (`renderer.with_capture(cap)`) and main installs the attribution provider. These are independent; both should run, each on its own `if let Some(...)`.

**Resolution:** this is the most involved Rust conflict. Recommend doing it in two passes — first unify the function signatures (so the file compiles), then update each call site to thread both `None`s.

### 14. `hub-client/changelog.md` (1 conflict)

Trivial — two dated entries collide because both branches added new entries near the top of the file. The feature side has q2-preview-related entries; main has attribution-pipeline-related entries.

**Resolution:** keep both, in date order (most recent first). No semantic decisions.

### 15. `hub-client/src/components/ReplayDrawer.tsx` (1 conflict, top of file)

Three imports diverge between sides:

```
<<<<<<< HEAD
import { actorColor } from '../hooks/useReplayMode';
import type { ActorIdentity } from '@quarto/preview-runtime';
import { getActorId } from '@quarto/preview-runtime';
=======
import { actorColor } from '../utils/palette';
import type { ActorIdentity } from '../services/automergeSync';
import { getActorId } from '../services/automergeSync';
>>>>>>> origin/main
```

**Resolution:** keep feature's imports verbatim. Feature has done two reorganizations here: `actorColor` moved from `'../utils/palette'` (main) to `'../hooks/useReplayMode'` (feature), and `ActorIdentity` / `getActorId` moved from `'../services/automergeSync'` (main) to `@quarto/preview-runtime` (feature, as part of the ts-packages extraction). Both are legitimate feature-branch refactors that main doesn't know about.

---

## Total conflict accounting (post-merge)

| Category | Count |
|---|---|
| File-location conflicts (Claude resolved by `git add` at ts-packages path) | 5 |
| TS content conflicts (you resolve) | 9 |
| Rust + extra TS content conflicts surfaced after the merge ran (you resolve) | 6 |
| **Total content conflicts requiring your attention** | **15** |

The 6 extra conflicts (sections 10–15 above) follow the same overall pattern as the original 9: combine both sides' additions rather than picking one. The Rust ones (`pass2_renderer.rs`, `wasm-quarto-hub-client/src/lib.rs`) are mechanical concurrent-parameter-addition merges and shouldn't require re-thinking either feature.

---

## After resolution, before committing

Run, in order:

1. `cargo xtask verify --skip-hub-tests` — confirms the Rust crates + WASM build + hub-client TS build all compile. The `--skip-hub-tests` keeps it fast; you can drop the flag to run the vitest suite if you want.
2. `cd hub-client && npm run build:all` (if you didn't run the full verify) — production TS build is stricter than vitest and will catch project-references issues.
3. Spot-check the preview surface that this merge touches:
    - Open the hub-client in dev mode, open a doc, confirm preview renders.
    - Toggle the Authorship pill, confirm hover badges appear and the preview doesn't error.
4. Run `cargo run --bin q2 -- render docs/` — make sure the new docs site still works post-merge (this is the smoke test that caught bd-jjep originally).

The merge commit message can be terse — something like `Merge origin/main into feature/q2-preview-command (PR #190 attribution pipeline)`.

---

## Open questions surfaced by this merge

These don't block the merge but are worth a follow-up:

1. **Where should `iframeMessageDispatch.ts` live?** (See conflict #5.) Keep in hub-client, or move to ts-packages with the rest of the render plumbing?
2. **Does the ts-packages q2-preview real entry need attribution wiring?** (Check by skimming `ts-packages/preview-renderer/src/q2-preview/entry.tsx` against main's `hub-client/src/components/render/q2-preview/entry.tsx` diff.)
3. **`useAttributionHover` — public framework API or internal?** Multiple files on main import it; some appear not to use it. Either trim the unused imports during merge, or commit to the public-API stance and re-export it from the framework barrel.
4. **Vite `virtual:quarto-attribution-viewer-css` plugin** lands at the hub-client `vite.config.ts` level on main. Does the ts-packages preview-renderer build need its own version of this plugin, or does the consumer-side configuration suffice? (See `attribution.tsx` line 6 on main and `resources/attribution/README.md`.)
