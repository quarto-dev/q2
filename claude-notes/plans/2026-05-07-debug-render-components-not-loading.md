# Debug: render-components not loading in q2-debug iframe

**Date:** 2026-05-07
**Status:** Open — handoff for a fresh debugging session
**Severity:** High — blocks manual smoke-testing of any q2-debug feature that depends on user-TSX overrides; surfaced while validating Plan 2pre Phase 1.
**Branch context:** Reproducible on `main` and on `feature/q2-preview-work` HEAD `f58ed2b6` (pre-Plan-2pre). NOT introduced by Plan 2pre; Plan 2pre's Phase 1 has been verified clean independently.

## Symptoms

Open `~/docs/demo-playground/elliot/index.qmd` in the running hub-client. The page has frontmatter:

```yaml
format: q2-debug
render-components:
  - /elliot/simple.tsx
  - /elliot/html.tsx
  - /elliot/comment.tsx
  - /elliot/kanban.tsx
```

Expected behavior:
- q2-debug bordered-box defaults render unless overridden.
- `kanban.tsx` exports a `Div` component that turns `:::: {.kanban}` blocks into a drag-and-drop kanban widget. (Ignore the bd-3day registry-accumulator bug for now — even with that bug, `kanban.tsx` is the *last* file in the list, so its `Div` override should still apply.)

Observed behavior:
- Bordered debug aesthetic IS visible (so framework + q2-debug registry is wired correctly).
- No user overrides apply at all. The `:::: {.kanban}` block renders as a bordered `Div` of bordered children, never the kanban widget.
- DevTools console (top-frame, NOT the iframe) shows:
  ```
  [ReactRenderer] Component file not found: /elliot/simple.tsx
  [ReactRenderer] Component file not found: /elliot/html.tsx
  [ReactRenderer] Component file not found: /elliot/comment.tsx
  [ReactRenderer] Component file not found: /elliot/kanban.tsx
  ```
  (Twice — once per render of `<ReactRenderer>`. The warning fires from `ReactRenderer.tsx:126`.)

## Where the bug lives

`hub-client/src/components/render/ReactRenderer.tsx:114-139`:

```tsx
const customComponentsCode = useMemo(() => {
    if (!componentPathsKey) {
      return {};
    }

    const componentPaths = JSON.parse(componentPathsKey) as string[];

    // Transpile each component using the latest fileContents from the ref
    const componentsCode: Record<string, string> = {};
    for (const path of componentPaths) {
      const tsxCode = fileContents.get(path);   // line 124
      if (!tsxCode) {
        console.warn(`[ReactRenderer] Component file not found: ${path}`);
        continue;
      }

      try {
        const jsCode = transpileTSX(tsxCode);
        componentsCode[path] = jsCode;
      } catch (err) {
        console.error(`[ReactRenderer] Failed to transpile component ${path}:`, err);
      }
    }

    return componentsCode;
  }, [componentPathsKey]);
```

The deps array is `[componentPathsKey]` only — `fileContents` is intentionally omitted (per the preceding line-113 comment "only when component paths list changes (not when their contents change)"). The intent is to avoid re-transpiling on every keystroke.

But that creates a **race**: if `astJson` (and therefore `componentPathsKey`) arrives before `fileContents` has populated the user-TSX entries, this memo runs once with an empty map, fires four warnings, returns `{}`, and never re-runs. The iframe never receives `LOAD_CUSTOM_COMPONENTS`, so no overrides apply. The race is somehow lost on Gordon's local environment as of 2026-05-07.

## What we know

- **Not Phase 2pre's fault.** Reproduces on `f58ed2b6` (pre-Plan-2pre). Stashing Plan-2pre's working tree and re-rendering shows the same warnings with the same paths.
- **Not the iframe's fault.** The iframe never receives the `LOAD_CUSTOM_COMPONENTS` postMessage in the first place — the bug is upstream, in the parent component (`ReactRenderer`).
- **Not bd-3day.** bd-3day is the *registry-accumulator* bug inside `loadCustomComponents` — it overwrites `customRegistry` each iteration. That bug only matters if files actually load. Here, no files load.
- **Not a path-key mismatch.** The lookup uses `/elliot/simple.tsx` (etc.) as the literal key from `ast.meta['render-components']`. The CLAUDE.md note about VFS using `/project/` prefix is for the *VFS layer*; whatever map produces `fileContents` should be using whichever convention `ReactRenderer` expects, and the bug is that the map is *empty* at lookup time, not that the keys are wrong. (Probably worth verifying, but the empty-map hypothesis fits the symptoms cleanly.)
- **DevTools observation.** Inside the iframe, `Object.keys(window.__REACT_AST_DEBUG_RENDERER__).sort()` yields `["Ast","Block","Inline","Node","blockStyle","componentRegistry","inlineStyle","renderChildren","renderNode"]` — exactly what Plan 2pre Phase 1 should expose. The iframe-side stack is fine; this is a parent-side data-loading issue.
- **Used to work on quarto-hub.** Gordon recalls the elliot demos working at some point. They don't right now on either branch. He notes that quarto-hub's iframe has its own (different) issue right now, so we can't A/B against it directly.

## Hypotheses to test (prioritised)

1. **Race fix.** Add `fileContents` to the `useMemo` deps array. The original commit's intent (avoid re-transpiling on keystroke) is undermined when `fileContents` isn't ready in time — the more important property is "transpile when the input is available." If `transpileTSX` cost is a real concern, a more nuanced fix is to gate on a **stable identity** for the relevant subset of `fileContents` (a small map keyed by `componentPaths`), not the whole map. But start by adding `fileContents` to deps and confirm the warnings disappear.

2. **Path-key audit.** Inspect the runtime values of `componentPaths` (the array post-`JSON.parse`) and the keys of `fileContents` at the time the memo runs. Add a one-shot `console.log(componentPaths, [...fileContents.keys()])` immediately above line 124 and reload. If the paths and keys differ in convention (one uses `/elliot/simple.tsx`, the other uses `/project/elliot/simple.tsx` or `elliot/simple.tsx` etc.), the fix is path normalisation.

3. **`fileContents` lifecycle.** Trace the prop. Where does `<ReactRenderer fileContents={...}>` get its value? `git grep "fileContents=" hub-client/src/`. Is the parent populating it asynchronously? Is there a `useEffect` somewhere that should be loading the user-TSX files into the map but isn't firing on this branch?

4. **Recent regression.** `git log --oneline --since=3.months -- hub-client/src/components/render/ReactRenderer.tsx hub-client/src/services/`. Cross-reference with what Gordon describes as "used to work on quarto-hub." If a specific commit changed how `fileContents` is populated for `format: q2-debug`, the regression date narrows down the search.

5. **Project-scoping.** `index.qmd` is at `~/docs/demo-playground/elliot/index.qmd`. The render-components paths are `/elliot/simple.tsx` etc. — relative to the project root (`~/docs/demo-playground/`). Hub-client must be loading the project at the demo-playground level for those paths to resolve. Check what Gordon's hub server is serving: is the *project root* `~/docs/demo-playground/` or something narrower? If narrower, `/elliot/simple.tsx` won't be in `fileContents` because hub doesn't know about that subtree.

## Reproduction recipe

1. Worktree at `.worktrees/q2-preview-work` is the live one; HEAD is `f58ed2b6` for the pre-Plan-2pre baseline. (Plan-2pre's Phase 1 commit also reproduces but doesn't add information — start at `f58ed2b6`.)
2. Run the hub server pointing at `~/docs/demo-playground/`.
   ```bash
   cd .worktrees/q2-preview-work
   cargo run --bin q2 -- hub serve ~/docs/demo-playground   # adjust to whatever Gordon uses
   ```
3. From a separate shell:
   ```bash
   cd .worktrees/q2-preview-work/hub-client
   npm run dev
   ```
4. Open the dev URL, navigate to `/elliot/index.qmd`.
5. DevTools → top-frame Console.
6. Reload. Expect the four `[ReactRenderer] Component file not found` warnings. The kanban widget should be missing; bordered Para/Header/Str everywhere.

## Other manual-test fixtures to verify against

When the fix lands, regression-test against these (in order of priority):

| Path | Format | Render-components | What it exercises |
|---|---|---|---|
| `~/docs/demo-playground/elliot/index.qmd` | q2-debug | simple, html, comment, kanban | **Primary touchstone.** Multiple components; kanban widget visible when working; tests four user-TSX files. |
| `~/docs/demo-playground/gordon/tldraw-shortcode/example.qmd` | q2-debug | html.tsx | Smaller blast radius. One override file. Tests RawBlock for shortcode HTML. |
| `~/docs/demo-playground/elliot/render_components.qmd` | (presumed q2-debug; confirm frontmatter) | text-heavy doc; tests inline overrides if any. |
| (synthesise) | q2-debug | none | Confirms the bordered-box baseline still renders without `render-components` set — separates the iframe-side path from the user-TSX-loading path. |
| `~/docs/demo-playground/elliot/slides.qmd` | revealjs | n/a | Different render path entirely (slides). Confirms the slide-side renderer isn't a casualty of any fix. |
| `~/docs/demo-playground/cscheid/index.qmd` | (no format, default html / q2-preview) | n/a | Confirms q2-preview path isn't a casualty. |

After the fix:
1. Verify `elliot/index.qmd` shows the kanban widget (kanban's `Div` override applies). The other elliot files' overrides will still be silently swallowed by **bd-3day**, which is the next bug in the queue (Plan 2pre Phase 2.7 fixes it as part of the entry rewrite).
2. Verify `gordon/tldraw-shortcode/example.qmd` renders the tldraw shortcode through `html.tsx`'s RawBlock override.
3. Verify `slides.qmd` and `cscheid/index.qmd` still render normally (no regression).

## Definitions of done

- The four `[ReactRenderer] Component file not found` warnings stop appearing on `elliot/index.qmd`.
- `elliot/index.qmd` shows the kanban widget for the `:::: {.kanban}` block.
- `npm run build:all` and `npm run test:ci` from `hub-client/` both pass.
- Beads issue created with the root cause and the fix; commit message references it.
- This file gets updated with a "Resolution" section documenting what the actual root cause was and what changed.

## Out of scope

- Plan 2pre Phase 2 itself (proceeds independently once unblocked).
- bd-3day registry accumulator bug (will be folded into Plan-2pre Phase 2.7's entry rewrite).
- Iframe-side code paths (already verified working from the DevTools check).

## Notes for the agent picking this up

- Start at `f58ed2b6` if you want the cleanest baseline. Reproduce there first. Plan 2pre's Phase 1 commit is on top but doesn't change the symptom.
- Gordon's environment may have something specific about hub-server project-rooting. If you can't reproduce, the next thing to ask him is what command he uses to start the hub server and what the configured project root is.
- The `fileContents` ref-vs-prop history matters here. If `fileContents` is a prop wrapped in a ref so the memo can read "the latest" without re-running, the fix is more delicate than just adding it to deps. Read the ref-handling code carefully.
- Don't reach for `useEffect` + manual transpile if `useMemo` with corrected deps suffices. The simplest fix that provably resolves the race is the right one.
