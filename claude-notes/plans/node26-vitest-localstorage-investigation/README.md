# bd-lh30hlvd investigation artifacts

Companion to `../2026-09-08-node26-vitest-localstorage.md`.

| File | What it is |
| --- | --- |
| `probe-localstorage.mjs` | Prints how a given Node binary defines the `localStorage` global (descriptor, `typeof`, whether assignment / `defineProperty` succeed). Run with any `node` binary: `node probe-localstorage.mjs`. |
| `webstorage-shim.ts` | Prototype vitest `setupFiles` entry: when Node's own `localStorage`/`sessionStorage` accessor shadows jsdom's, re-point the global at the jsdom window's real `Storage` (vitest exposes the JSDOM instance as `globalThis.jsdom`). |
| `vitest.probe.config.ts` | hub-client's unit-test config plus the shim as a setup file, for trying the shim without touching hub-client. |

## Reproduce / verify (from `hub-client/`)

```bash
node --version                       # v26.8.1 here
npx vitest run src/hooks/usePreference.test.tsx src/services/branchService.test.ts
#  -> 12 failed: TypeError: Cannot read properties of undefined (reading 'clear')

npx vitest run --config ../claude-notes/plans/node26-vitest-localstorage-investigation/vitest.probe.config.ts \
  src/hooks/usePreference.test.tsx src/services/branchService.test.ts \
  src/components/tabs/AboutTab.test.tsx src/services/debugApi.test.ts \
  src/components/SyncStatusBadge.test.tsx src/components/ProjectSelector.import.test.tsx
#  -> Test Files 6 passed (6), Tests 62 passed (62)   (2026-09-08, Node 26.8.1, vitest 4.1.8)
```

## Probe output (2026-09-08)

```
##### /opt/homebrew/opt/node@24/bin/node   (v24.15.0)
descriptor NONE
typeof localStorage undefined
after assign -> assigned
after defineProperty -> defined

##### /opt/homebrew/Cellar/node/26.8.1/bin/node   (v26.8.1)
descriptor { get: true, set: true, configurable: true, enumerable: false }
typeof localStorage undefined
read -> undefined
after assign -> assigned
after defineProperty -> defined
(node:59729) ExperimentalWarning: localStorage is not available because --localstorage-file was not provided.
```

The difference that matters is the first line: on Node 26 `'localStorage' in globalThis`
is **true** (an accessor exists) even though reading it yields `undefined`.
