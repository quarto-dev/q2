# rich-editor (prototype)

Read `README.md` first: it describes the editor, how to run it, and the tests.

## Work in a worktree of your own

The `rich-editor-prototype` worktree (`~/code/q2.worktrees/rich-editor-prototype`)
holds the prototype's main branch for now. Several agents work on the
prototype at once, so:

- **A piece of work (a feature, a fix) gets a branch and a worktree of its
  own**, off `rich-editor-prototype`, e.g.
  `wt switch --create rich-editor-<name> -b rich-editor-prototype --no-cd`.
  Set it up as `README.md` (§ Running) says, with `npm install` at the repo
  root first. In a shared checkout, other sessions' uncommitted edits end up
  in your test runs, and a full suite there doesn't tell you whether your
  change is green.
- **Only small edits the user asks for on `rich-editor-prototype` itself
  are made in that worktree.** The user also runs the app from there to try
  it out.

### Merging

1. Commit everything on your branch, and merge `rich-editor-prototype` into
   it, so that what you test is what will land.
2. Run the whole suite in every browser on your branch (in the background,
   see below). A failure that also fails on `rich-editor-prototype` isn't
   yours, but say so in your report.
3. Ask the other agents working in the `rich-editor-prototype` worktree
   (list the running sessions, and message each one whose working directory
   is in it) whether you can merge now. Someone may be in the middle of
   editing files your merge changes; wait for them, or agree on an order.
4. Merge in that worktree with `git merge --no-ff rich-editor-<name>`.
   Leave other sessions' uncommitted changes as they are: don't stash or
   reset them. If git refuses because of them, ask their session to commit
   first.
5. Tell the agents you asked that the merge is in, with its hash. Don't push
   unless the user asks.

### Removing your worktree

Once it's merged, remove your worktree and branch when that's safe: the
branch is in `rich-editor-prototype` (`git branch --merged
rich-editor-prototype` lists it), the worktree has nothing uncommitted, and
none of your background runs or subagents are still using it. Use
`git worktree remove <path>` and `git branch -d rich-editor-<name>`, never
with `--force`: they refuse when work would be lost, and if they do, find
out why instead.

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
`npm run test:e2e:chromium -- e2e/<file>.spec.ts`. Before committing a
finished piece of work, run the whole suite in Chromium
(`npm run test:e2e:chromium`), and every browser on the specs you changed:
a change breaks specs you didn't think were related far more often than it
breaks one browser only. Run the whole suite in every browser before merging. Always use the npm scripts, not `npx playwright test`:
they queue runs across worktrees so that agents running the suite at once
don't slow each other down. Small runs (a spec file or a `-g` filter) have a
lane of their own and don't wait for a whole suite. Time spent waiting in the
queue counts against the Bash tool's timeout, so run a whole suite in the
background.
Run the unit tests (`npm run test:unit`) and `npm run typecheck` too
before committing.

Firefox runs need no setup: `playwright.config.ts` gives Firefox the
throwaway home folder it needs to launch from Claude's shell, so don't set
`CFFIXED_USER_HOME` yourself.

## Throwaway repros and scripts

Other sessions often work in the same checkout, so keep throwaway files
out of the places they use:

- **A throwaway spec** (a repro, a probe) goes in `e2e/scratch/`, which git
  ignores. Give it a name of your own. A run leaves `e2e/scratch/` out
  unless its arguments name it, so another session's suite never picks up
  your probe: `npm run test:e2e:chromium -- e2e/scratch/<name>.spec.ts`.
  Delete it when you're done. Don't put throwaway specs in `e2e/` itself.
- **A one-off script** (say, one that drives a mockup in every browser)
  stays in your scratchpad. Run it with `npm run drive -- <path> [args]`:
  imports like `@playwright/test` resolve from this package. Don't copy
  scripts into the package to run them.

## The desktop app is tested for what it adds

The desktop app (`src-tauri/`, `desktop.html`) is the same editor on a
local file. Specify and test editing in the browser app, as above; the
desktop app's tests cover only what it adds: which file it opens,
saving, and changes made on disk. `e2e/desktop.spec.ts` runs its page in
the browsers with the test standing in for the Rust side, and
`npm run test:desktop` tests the Rust side itself. (Tauri has no WebDriver
on macOS, so nothing drives the real window.)
