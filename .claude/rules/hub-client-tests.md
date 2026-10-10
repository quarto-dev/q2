---
paths:
  - "hub-client/src/**/*.test.ts"
  - "hub-client/src/**/*.test.tsx"
  - "hub-client/src/test-utils/**"
  - "hub-client/src/test-hooks.ts"
  - "hub-client/vitest*.ts"
  - "hub-client/e2e/**"
  - "hub-client/playwright*.ts"
  - "ts-packages/quarto-sync-client/src/**/*.test.ts"
  - "ts-packages/quarto-sync-client/src/test-hub.ts"
  - "ts-packages/quarto-hub-mcp/src/**/*.test.ts"
  - "ts-packages/quarto-hub-mcp/src/test-hub.ts"
---

# hub-client tests

## No async work may outlive a test

A test file is finished once its last test resolves. Vitest then tears
the worker down. Anything the code under test started without awaiting,
such as a promise chain, a timer or a `console.log` still being sent to
the runner, fails the **whole run** at teardown:

```
EnvironmentTeardownError: [vitest-worker]: Closing rpc while "onUserConsoleLog" was pending
This error originated in "src/components/<File>.integration.test.tsx"
```

Whether it fires depends on timing, so it passes on a PR and then fails
on `main`. Before PR #734 this happened five times from the two
`ProjectsHome` integration tests. **It is a real leak, not a flake:
do not re-run CI and move on.** Find the unawaited work the file starts
and either mock it out or wait for it.

## Mock storage when rendering top-level components

`ProjectsHome`, `ProjectSelector` and similar components read IndexedDB
from mount effects (`userSettings.getUserIdentity()`,
`projectStorage.listProjects()`). Under the integration config this
opens fake-indexeddb and replays every schema migration. Unless a test
is *about* storage, mock the services it touches:

```ts
vi.mock('../services/projectStorage', () => ({
  listProjects: vi.fn().mockResolvedValue([]),
}));

vi.mock('../services/userSettings', () => ({
  getUserIdentity: vi.fn().mockResolvedValue(null),
  updateUserName: vi.fn(),
  updateUserColor: vi.fn(),
  resetUserIdentity: vi.fn(),
}));
```

The integration setup (`src/test-utils/setup.ts`) also calls
`settleDb()` after every test, which waits for any in-flight database
open and its migrations. That hook is the backstop, not a licence to
skip mocking: it only covers `getDb()`. Other fire-and-forget work,
such as sync connections, timers or polling, still needs a mock or an
explicit wait (e.g. `await waitFor(...)`) before the test returns.

## Diagnosing a teardown error

1. Run the named file on its own with
   `npx vitest run --config vitest.integration.config.ts <file> --reporter=verbose`.
   The default reporter hides stray console output from passing tests.
   Note that migration logs are silent under Vitest's `test` mode, so
   add `--mode development` to see them.
2. Look for `stdout |` lines attributed to a *later* test than the one
   that started the work. That is the leak.
3. Mock the source or wait for it in the test. Don't silence the log or
   set `dangerouslyIgnoreUnhandledErrors`; that hides the next one too.

## Playwright: wait on the state you assert, not on a proxy for it

The E2E version of the same mistake: a spec waits for something visible,
then reads app state *once* and asserts on it, assuming the visible thing
implies the state has settled. `first-run.spec.ts` flaked this way (PR
#734). It waited for the projects home, then read the root doc id and got
`null`, because the home renders while the project set is still
connecting.

- **Read settling state with retrying assertions.** Use web-first
  assertions (`await expect(locator).toHaveText(...)`) or
  `await expect.poll(() => page.evaluate(...))`. Avoid
  `expect(await page.evaluate(...))` and `expect(await locator.textContent())`
  on anything asynchronous.
- **Make the wait discriminate.** Waiting for text that is present both
  before and after the change, or for an element that already existed
  before the click, passes immediately and synchronizes nothing.
- **Don't use `page.waitForTimeout` as a synchronization point.** A fixed
  sleep is a guess that fails under CI load.
- **Project-set readiness:** the bootstrap helpers in
  `e2e/helpers/projectFactory.ts` return only once the set is
  `connected`. Read the root id with `waitForProjectSetDocId(page)`, which
  waits on the `data-project-set-status` attribute App.tsx publishes on
  `<html>`. Don't read `getProjectSetDocId()` directly.
- **Keep helper comments honest.** If a helper's docstring claims an
  ordering ("lands on the home once connected"), check that the app still
  provides it. A stale claim like that is what hid the first-run race.
- **Test hooks wait for the state too.** A hook in `src/test-hooks.ts`
  that returns "once everything has settled" must wait on the events
  that say so (a run started for the last edit, every run ended), not
  sleep one debounce and look. The `typing()` hook returned early that
  way when the viewer iframe, booting on the same thread, delayed the
  debounce timer (bd-c72wsugj).
- **Menus can change under focus.** The ＋ New menu lists a placeholder
  until the WASM registry answers. Wait for the real items before
  pressing keys, and never assume the item focused a moment ago still
  exists.

## Sync tests: presence is not delivery

A client pushes its changes to the hub through automerge-repo's sync
throttle (100 ms per document, trailing edge), so the hub can hold a
document that is still a change or two behind the client that wrote it.
Two index changes separated by an `await` (a binary file's hash, for
example) land in different windows. That is how `doc-inventory.test.ts`
lost its binary file's index entry under load (bd-c72wsugj, filed four
times).

- Before a creator disconnects, or before a second client reads what the
  first wrote, wait for the creator's *heads* on the hub:
  `hub.hubHasHeadsOf(creator)` in `test-hub.ts`, or
  `creator.disconnect({ drainMs })`, which waits for the hub to confirm
  the local heads. `hub.hubHasDoc(id)` only says the hub has heard of
  the document.
- In hub-client E2E, `createProjectOnServer` drains its disconnect for
  the same reason; an HTTP "the document exists" poll is a presence
  check.
- `project-creation-delivery.test.ts` forces the schedule with a slow
  digest; run it after touching project creation or the test hub.
