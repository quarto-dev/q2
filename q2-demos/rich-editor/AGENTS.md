# rich-editor (prototype)

Read `README.md` first: it describes the editor, how to run it, and the tests.

## The e2e specs are the specification

The specs in `e2e/` are the most valuable part of this prototype. They
describe the editing experience as a person sees it and as the source text
holds it, not this implementation, and they are meant to be complete:
someone with only the specs should be able to build the editor again.

- **Behaviour that no spec describes is a gap in the specs.** When you add
  or change behaviour, add or change its spec in the same commit, spec
  first. When you find behaviour without a spec, add the spec.
- Write a spec against what the page shows and what the source text is
  (`Editor` and `Document` in `e2e/helpers.ts`), never against the engine's
  internals.
- Every spec runs in Chromium, Firefox and WebKit. A spec that can't hold
  in one engine is skipped there, with the reason where it's skipped.
- Behaviour we want and don't have yet is a `test.fixme`.
- A new spec file goes in the list in `README.md` (§ Tests).

Run them with `npm run test:e2e` (needs the WASM build; see `README.md`).
While iterating, run Chromium only, on the specs you're changing:
`npm run test:e2e:chromium -- e2e/<file>.spec.ts`. Run every browser on those
specs before committing a finished piece of work, and the whole suite in every
browser before merging. Always use the npm scripts, not `npx playwright test`:
they queue runs across worktrees so that agents running the suite at once
don't slow each other down. Time spent waiting in the queue counts against the
Bash tool's timeout, so run a whole suite in the background.
Run the unit tests (`npm run test:unit`) and `npm run typecheck` too
before committing.
