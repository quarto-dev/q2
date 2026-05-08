# Plan — Live re-transpile render-components on TSX content change

**Date:** 2026-05-07
**Branch:** TBD (independent)
**Status:** Future plan, parked
**Milestone:** Render-components TSX files hot-reload when their content changes — no YAML twiddle, no page reload required.

## Goal

When a user edits a TSX file listed under `render-components: [...]` in their qmd, the iframe re-renders with the new component logic on its own. Today the transpilation memo at `ReactRenderer.tsx:114-139` keys on the *list of paths* only; content edits to those paths are ignored until the path list itself changes (see comment on line 113: *"only when component paths list changes (not when their contents change)"*).

This plan upgrades that contract: TSX content edits are picked up live, throttled by a debounce so typing doesn't thrash the transpiler.

## Why this is a feature plan, not a beads bug

The current behavior is **intentional**, not accidental. The comment in `ReactRenderer.tsx:113` says so explicitly. The memoization shape was a deliberate design choice — probably for one or more of:

- **Performance**: `transpileTSX` runs `@babel/standalone` synchronously in the parent. Transpiling on every keystroke would be slow.
- **Stability during in-progress edits**: TSX files are often syntactically broken mid-edit. Re-transpiling on every keystroke would surface error popups while the user types.
- **Render-components are conceptually paste-once**: the original UX was "drop a TSX file, see it work" — not live editing.

This plan upgrades the contract: live editing **should** work, with carefully chosen mitigations for the perf and stability concerns.

It's not a bug fix; it's a new capability. Not filed as beads — beads tracks bugs and out-of-scope discoveries, not feature additions.

## Why this lives outside the 2pre/2A/2B series

Independent of the parallel-formats restructure. The same change would have applied before 2pre and would still apply after 2B. It touches `ReactRenderer.tsx`, which 2A also modifies (items 12 format dispatch + 13 render-components gate extension), but the change is logically separable from any of that work. Bundling would muddle the goals of the parallel-formats work.

This plan can land before 2pre, between 2A and 2B, after 2B, or independently of the series entirely. It's most useful **after** 2A ships — q2-preview's audience will be the largest beneficiary, since iterative TSX development becomes more common as q2-preview matures.

## Scope

### Core change in `ReactRenderer.tsx`

#### 1. Content-keyed transpilation cache

A `useRef` map keyed by path, storing `{ content: string, jsCode: string }`. The transpile loop checks the cache before invoking `transpileTSX`:

```tsx
const transpileCache = useRef<Map<string, { content: string; jsCode: string }>>(new Map());

const customComponentsCode = useMemo(() => {
  if (!componentPathsKey) return {};
  const paths = JSON.parse(componentPathsKey) as string[];
  const code: Record<string, string> = {};

  for (const path of paths) {
    const tsxCode = fileContents.get(path);
    if (!tsxCode) {
      console.warn(`[ReactRenderer] Component file not found: ${path}`);
      continue;
    }

    const cached = transpileCache.current.get(path);
    if (cached?.content === tsxCode) {
      code[path] = cached.jsCode;
      continue;
    }

    try {
      const jsCode = transpileTSX(tsxCode);
      code[path] = jsCode;
      transpileCache.current.set(path, { content: tsxCode, jsCode });
    } catch (err) {
      console.error(`[ReactRenderer] Failed to transpile component ${path}:`, err);
      // Preserve last-good cached output so a syntax error mid-edit doesn't blank the iframe
      if (cached) {
        code[path] = cached.jsCode;
      }
    }
  }

  return code;
}, [componentPathsKey, fileContents]);
```

Properties:
- Cache hit when content unchanged — no transpile work.
- Cache miss when content changed — transpile only the changed file.
- Syntax error during transpile — keep last-good cached version, log error. Iframe doesn't blank.
- Files removed from `componentPaths` — naturally drop out of the returned object on next iteration; cache entries linger but are cheap.

#### 2. Debouncing

The cache alone doesn't solve the keystroke-storm problem — every keystroke in any tracked TSX file would still iterate the loop and (for cache misses) transpile. Add debouncing so transpilation only fires after an idle window:

```tsx
const [debouncedCode, setDebouncedCode] = useState<Record<string, string>>({});

useEffect(() => {
  // Compute synchronously on first render (don't delay initial transpile by 500ms)
  if (Object.keys(debouncedCode).length === 0 && Object.keys(customComponentsCode).length > 0) {
    setDebouncedCode(customComponentsCode);
    return;
  }
  const handle = setTimeout(() => setDebouncedCode(customComponentsCode), 500);
  return () => clearTimeout(handle);
}, [customComponentsCode]);
```

`<AstIframe customComponentsCode={debouncedCode} />` (was `customComponentsCode`).

The 500ms delay is a starting point — debounceable, tunable. Future iterations could expose this in dev tools.

#### 3. Iframe entry handles repeat `LOAD_CUSTOM_COMPONENTS`

When debouncedCode reference changes, `AstIframe.tsx:42-54`'s effect fires `LOAD_CUSTOM_COMPONENTS` to the iframe. The iframe entry's `loadCustomComponents` rebuilds `customRegistry` from scratch each time. This already works today for the YAML-twiddle workflow; with the live-reload, it just fires more often.

Verify during implementation: after a `LOAD_CUSTOM_COMPONENTS` arrives without an `UPDATE_AST` immediately following, does the iframe re-render with the new registry? If not, the iframe entry needs an explicit re-render trigger after `loadCustomComponents` completes — likely a `root.render(...)` call with the last-known AST. ~5 LOC.

### What's not in scope

These are the rabbit-holes to consciously avoid:

- **Source maps for user TSX debugging**. `transpileTSX` may or may not produce maps; whether the iframe's DevTools picks them up is its own investigation. Out of scope.
- **Custom error UI for transpilation failures**. Today: `console.error` + preserve last-good. A future UX pass could surface errors as a toast or inline overlay; this plan does not.
- **Hot-module-replacement state preservation**. When a TSX file changes and reloads, the iframe's React tree re-renders with the new registry. Component-local state is lost. HMR-style preservation (Vite's pattern) is more complex and out of scope; this plan does whole-tree re-render.
- **Per-component selective re-transpile**. The current implementation transpiles all paths-with-cache-misses on each invalidation. Selective re-import of just the changed module in the iframe is more complex (URL.revokeObjectURL of the old blob, blob URL for just the changed module, splice into the registry). Out of scope; the whole-list approach is simpler and correct.
- **Tunable debounce window in production UI**. Debounce constant lives in code; a future preferences UI could expose it.
- **Save indicator**. The CRDT model has no save action — every keystroke is "saved." A "transpilation pending / live" indicator could help user understand when their edits will land in the iframe; not in this plan's scope.
- **Error boundary integration**. `ReactRenderer.tsx` already wraps the iframe in an `ErrorBoundary`. Failed component imports already surface there; no additional integration needed.

## CRDT considerations

The user noted:

> "we don't have any explicit save points since we are crdt"

In a CRDT-backed editor, every keystroke is effectively committed and propagated immediately. No "Save" button means there's no natural moment to trigger expensive side effects like transpilation. This makes the debounce window more important than in a traditional save-on-Cmd-S UX.

Mitigations baked into this plan:

- **500ms debounce default.** Wait for idle before transpiling. A user typing continuously for 5 seconds straight sees no iframe update during that window — but pauses at the natural rhythm of editing (think, type, look) trigger updates.
- **Last-good preservation on syntax errors.** Typing through an unparseable intermediate state (`function foo() {` → `function foo() { return` → `function foo() { return null; }`) doesn't blank the iframe. The iframe shows the last *successfully-transpiled* version until the user reaches another valid state.
- **Cache-on-content-key.** Repeated transpile-attempts of the same content (which can happen when rapid keystrokes coalesce in the debounce) hit the cache. No wasted work.

What this plan does NOT solve:

- **Visible "stale" output during long typing runs.** A user typing continuously for 30 seconds sees the iframe stuck at the pre-typing state. This may be confusing. Mitigation could be a small UI indicator ("transpiling…" badge) but is out of scope for this plan.
- **Performance during very large edits.** A bulk paste of 1000 lines into a TSX file triggers one debounced transpile of the whole file. Possibly slow. The cache helps for unchanged files; this scenario is one big file. Acceptable for v1.

## Test plan

### Unit tests (vitest)

- **Cache hit**: render with `fileContents` containing TSX path A with content C1, allow transpile, then re-render with same fileContents Map identity preserved → cache hit, no transpile call.
- **Content-change-triggers-retranspile**: render with content C1, allow transpile, re-render with `fileContents` updated to content C2 → cache miss for that path, transpile fires.
- **Multi-file edit isolation**: render with paths A, B both transpiled (cache populated). Re-render with only A's content changed → transpile fires for A only; B uses cached output.
- **Syntax-error preserves last-good**: render with valid TSX, allow transpile, re-render with TSX that throws on transpile → returned code map contains the *last-good* JS for that path; console.error fires.
- **Empty starting cache + valid first transpile**: confirms initial mount produces output synchronously without debounce delay.
- **Debounce timing**: render with content C1, immediately re-render with C2, then C3, then wait → only one transpile of C3 fires (debounce coalesces).
- **Cache survives across re-renders**: confirm `transpileCache` is a `useRef`, identity-stable across renders.

### Integration test (vitest)

- Mount `<ReactRenderer>` with a fixture containing a render-component path, allow first transpile, simulate `fileContents` update with edited content, advance timers past the debounce, assert `<Q2DebugIframe>` receives a `customComponentsCode` prop with the new transpiled code. (Note: post-Plan-2pre the iframe component is `Q2DebugIframe`, not `AstIframe`.)

### E2E (single Playwright spec)

`hub-client/e2e/q2-debug-render-components-live-reload.spec.ts` — sister to the existing `q2-debug-render-components.spec.ts` from commit `dc828c53` on main. Mirrors that spec's `bootstrapProjectSet` + `createProjectOnServer` + `seedProjectInBrowser` setup. One spec file, two test cases.

**Setup.** Create a project containing one qmd with `format: q2-debug` + `render-components: [reactji.tsx]`, plus the `reactji.tsx` from the existing smoke-all q2-debug fixture (or a tiny purpose-built TSX — anything that produces a visible difference between pre/post-edit).

**Test 1: live reload.** Mount the page; assert the iframe renders the pre-edit DOM (e.g., `❤️ 1` from the reactji counter). Programmatically mutate the TSX content via `page.evaluate(({path, content}) => updateFileContent(path, content), { path: 'reactji.tsx', content: editedTsx })` — calling into the hub's automerge layer in-page (the `dev-only `window.quartoDebug` API from commit `0f103490`, or whatever the established mutation handle is at the time). Wait the debounce window plus a small jitter (~700ms total). Assert the iframe DOM updates to reflect the edited TSX (e.g., counter button now shows a different label or different rendered shape).

**Test 2: syntax-error preserves last-good.** Same setup. Assert pre-edit DOM. Mutate to invalid TSX (`function foo() {`). Wait through the debounce. Assert the iframe DOM is **unchanged** from pre-edit (last-good output preserved). Then mutate to valid TSX with a different output. Wait. Assert the iframe DOM updates to the new output.

This spec covers the end-to-end debounce → load-custom-components → iframe re-render flow. The vitest unit/integration tests above cover the *logic* (cache-hit, content-change, syntax-error, debounce timing); the e2e spec is the safety net for the message-passing path that vitest can't reach.

Cost: ~80 LOC for the spec file. Runs slowly (Playwright spinup) so it's gated behind `npm run test:e2e`, not `test:ci`. Worth the budget because live-reload is the kind of UX that silently degrades in subtle ways (debounce off-by-one, postMessage timing race) without anyone noticing until users complain.

## Risk areas

- **Iframe re-render after `LOAD_CUSTOM_COMPONENTS` without `UPDATE_AST`.** Today's flow assumes `LOAD_CUSTOM_COMPONENTS` comes before each `UPDATE_AST`. With live reload, `LOAD_CUSTOM_COMPONENTS` arrives mid-session without a fresh AST. The iframe entry needs to re-render the last-known AST against the new registry. Verify during implementation; ~5 LOC fix if not already correct.
- **Memory: blob URLs from transpiled modules.** The iframe's `loadCustomComponents` does `URL.createObjectURL(blob)` and `URL.revokeObjectURL(url)` already. With more reloads, more allocs and revokes — verify the revoke fires reliably. Probably fine; mention for awareness.
- **React StrictMode double-mount in dev.** Mount/unmount/mount cycle may run the debounce effect twice on first render. Cleanup function in the effect should handle it. Test under StrictMode.
- **Performance on very-large render-components projects.** If a user lists 20 TSX files in `render-components`, the loop iterates 20 times on each invalidation (mostly cache hits, some misses). Bounded but worth measuring on a synthetic stress fixture.
- **Stale-cache footgun.** If `transpileCache.current` is a `useRef` but the component instance gets recreated (e.g., format change unmounts ReactRenderer), the cache is lost. Acceptable — fresh instance, fresh transpile. But worth documenting so a future contributor doesn't try to "optimize" it into a module-level cache and break tests.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| Content-keyed transpilation cache logic | ~25 |
| Debounce useState + useEffect | ~20 |
| Iframe entry re-render trigger (if needed) | ~5 |
| Tests (cache-hit, content-change, multi-file, syntax-error, debounce, integration) | ~120 |
| E2E spec (`q2-debug-render-components-live-reload.spec.ts`) | ~80 |
| **Total** | **~250** |

One focused session. Tests are the biggest line-count item; the implementation itself is tight.

## Dependencies

### Hard

None. Independent of all 2pre/2A/2B work. Touches `ReactRenderer.tsx`, which 2A also modifies (items 12, 13) — if landed concurrently with 2A, watch for trivial merge conflicts in that file.

### Useful prerequisites (not strict)

- After Plan 2A ships, q2-preview's audience grows. Live-reload becomes more valuable as more users iterate on TSX overrides.
- After Plan 2B ships, the gordon/render-components fork demonstrates real iterative TSX work. Live-reload becomes a natural improvement to that experience.

### Blocks

Nothing. This plan is independent.

## Related work

- **bd-3day** — `customRegistry` accumulator bug. Same code area (`ast-renderer-entry.tsx`'s `loadCustomComponents`), different correctness issue. Independent fix; could land alongside this plan or separately.
- **Plan 2A item 9** — q2-preview's `entry.tsx` mirrors q2-debug's pattern but with the bd-3day fix. This plan's iframe-re-render-trigger work (if needed) applies to both entries.

## Notes

- This plan is parked, not scheduled. Land it whenever the iterative TSX-development experience becomes a real friction point — which is most likely after q2-preview matures and gordon/render-components is in active use.
- The "debounce window is 500ms" is a starting point. If user testing reveals it should be longer (give time to think) or shorter (more responsive), tune it. Not worth a config knob in v1.
- Per CLAUDE.md's hub-client policy, this plan's PR needs the standard two-commit pattern (commit + changelog entry with the hash).
- The CRDT-no-save-point reality is the most interesting design pressure. If the debounce-only approach feels wrong in practice, future iterations could explore explicit "edit complete" signals (e.g., a Cmd+Shift+R "force re-transpile" shortcut, or detection of "user paused and looked at the preview").
