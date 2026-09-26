---
paths:
  - "hub-client/src/**/*.test.ts"
  - "hub-client/src/**/*.test.tsx"
  - "hub-client/src/test-utils/**"
  - "hub-client/vitest*.ts"
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
