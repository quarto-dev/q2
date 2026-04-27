# Hub-client visibility gating for text sync + presence

## Repro confirmation (2026-04-24)

Two-line diagnostic added at the top of `handleImmediateSync` and on a
`visibilitychange` listener in `useAutomergeSync.ts`. Manual two-window
repro: background window A for ~30 s while typing from window B, then
refocus window A. Observed console pattern:

```
[diag] vis hidden …
[diag] sync hidden …   (repeated — one per remote change)
[diag] sync hidden …
…
[diag] vis visible …
```

Conclusion: remote Automerge change events reach `handleImmediateSync`
**during the hidden period**, not in a post-visibility burst. The
visibility gate proposed below therefore catches them — while hidden
the stash replaces per-change `executeEdits`; the single flush on
`visibilitychange → visible` applies the latest content once. This
rules out the "events buffered upstream and delivered after refocus"
mechanism that would have made the gate a no-op.

Diagnostic lines reverted before proceeding.

## Context / problem

When a hub-client tab is backgrounded during active collaboration, then
refocused, the user sees the missed remote edits replay as an animation:
text appears to be typed at super-speed and remote cursors sweep
through intermediate positions. This is a render-path artifact, not
stale data — `presenceService.ts:357` already stores only the latest
cursor per peer — but the pipeline applies each queued Automerge
change and each queued ephemeral presence message as a separate
Monaco/React update once the tab becomes visible again.

Root cause:

- `automergeSync.ts:91` → `useAutomergeSync.ts:93`
  (`handleImmediateSync`) calls `editor.executeEdits('remote-sync',
  edits)` **per remote Automerge change**. Each remote keystroke is
  one `executeEdits` call.
- `presenceService.ts:331` (`handleEphemeralMessage`) calls
  `notifySubscribers()` **per ephemeral message**, each triggering a
  `setRemoteUsers` state update and a `useLayoutEffect` render in
  `usePresence.ts:214` that recomputes Monaco decorations. Senders
  broadcast at ~20 Hz (`broadcastThrottleMs: 50` in
  `presenceService.ts:76`).
- Hidden tabs throttle `setTimeout`/`setInterval`, pause
  `requestAnimationFrame`, and queue MessagePort tasks from any
  SharedWorker-hosted Repo onto a backpressured main-thread task
  queue. WebSockets and workers keep producing messages, so a
  backlog accumulates. React 18 automatic batching only coalesces
  setStates **within a single task** — N backlogged tasks produce N
  renders.
- On `visibilitychange → visible`, the queue drains: `executeEdits`
  fires once per stashed change and `deltaDecorations` fires once per
  stashed presence message. The user perceives the fast succession
  of intermediate states as an animation.

Fixes covered by this plan:

1. **Text-sync visibility gate** in `useAutomergeSync.ts` —
   while hidden, stash the latest remote content in a ref instead of
   calling `executeEdits`. On `visibilitychange → visible`, run one
   `diffToMonacoEdits` + one `executeEdits`.
2. **Presence-notify visibility gate** in `presenceService.ts` —
   while hidden, continue updating `remotePresences` (data stays fresh)
   but suppress `notifySubscribers()`. On `visibilitychange → visible`,
   call it once with the coalesced snapshot.

## Why this is safe

- **Correctness invariant for text sync (PR #102 motivation):** the
  `immediateFileChangeCallback` exists so that remote edits land in
  Monaco **before the user's next keystroke** can read stale
  positions. A hidden tab cannot receive keystrokes on Monaco — it is
  not the focused tab — so deferring `executeEdits` until visibility
  returns does not weaken that invariant. We just need to flush
  before the first keystroke after refocus. Neither the HTML spec
  nor the Page Visibility spec pins down the order of
  `visibilitychange` vs. window `focus`, and there are documented
  cases where `visibilitychange` does not fire at all (Chrome/Edge
  DevTools "Emulate a focused page"; Firefox macOS Cmd-H →
  Cmd-Tab restore, Bugzilla 777825; headless Chrome on tab switch,
  webdriverio#9694; Brave/Chrome on HTTP with DevTools open,
  brave-browser#42566). We therefore attach the flush to **both**
  `document` `visibilitychange` and `window` `focus`; the flush is
  idempotent (checks and clears the stash ref) so double-firing is
  harmless.
- **Presence correctness:** `remotePresences` is a full-snapshot map
  (`presenceService.ts:357` overwrites per peer on each message), so
  suppressing intermediate notifications never drops data — it just
  drops redundant intermediate renders. Stale-cleanup runs via
  `cleanupStalePresences` on a `setInterval`; that timer gets
  throttled in hidden tabs anyway, so behaviour there is unchanged.

## Scope / non-goals

**In scope:**

- Changes to `useAutomergeSync.ts` (text gate + visibility listener).
- Changes to `presenceService.ts` (notify gate + visibility listener).
- Tests for both, using vitest/jsdom.
- `hub-client/changelog.md` entry.

**Out of scope (may become follow-ups):**

- Coalesce text application via `requestAnimationFrame` as a
  belt-and-braces measure. RAF-batching would cap per-frame cost
  during heavy remote activity and produce natural coalescing when
  visible too, but the visibility gate on its own is expected to
  eliminate the user-visible symptom.
- Broaden `broadcastThrottleMs` in `presenceService.ts:76` from 50
  to 100-150 ms. That would halve the backlog size per hidden
  interval without a noticeable latency cost, but it's an
  independent tuning change.
- The reconciliation-on-mount path in `useAutomergeSync.ts:123`.
  That path reads *live* `getFileContent(currentFile.path)` on each
  run, so even under a burst of renders it converges to the current
  state (intermediate runs are no-ops when Monaco already matches).
  If follow-up testing shows otherwise we'll gate it too.
- `modelVersion` coupling in `usePresence.ts:157`. Unrelated to this
  bug; worth a separate pass.

## Test strategy

### Visibility stub

jsdom exposes `document.visibilityState` as a read-only getter on
`Document.prototype`. Rather than reaching for `Object.defineProperty`
— which permanently mutates the descriptor and leaks between test
files in the same vitest worker — the helper uses `vi.spyOn` on the
getters so vitest's built-in `vi.restoreAllMocks()` teardown
auto-restores the originals. The helper also exposes a way to raise
`focus` independently so we can exercise each listener path in
isolation:

```ts
import { vi } from 'vitest';

let visibilitySpy: ReturnType<typeof vi.spyOn> | null = null;
let hiddenSpy: ReturnType<typeof vi.spyOn> | null = null;

export function setVisibility(state: 'visible' | 'hidden'): void {
  if (!visibilitySpy) {
    visibilitySpy = vi.spyOn(document, 'visibilityState', 'get');
    hiddenSpy = vi.spyOn(document, 'hidden', 'get');
  }
  visibilitySpy.mockReturnValue(state);
  hiddenSpy!.mockReturnValue(state === 'hidden');
  document.dispatchEvent(new Event('visibilitychange'));
}

export function resetVisibility(): void {
  visibilitySpy?.mockRestore();
  hiddenSpy?.mockRestore();
  visibilitySpy = null;
  hiddenSpy = null;
}

export function fireWindowFocus(): void {
  window.dispatchEvent(new Event('focus'));
}
```

### Test isolation strategy

1. **Spy-based override, not property redefinition.** Use
   `vi.spyOn` on the `visibilityState` and `hidden` getters so
   `vi.restoreAllMocks()` teardown auto-restores them — no
   `Object.defineProperty` surgery that would leak across test
   files in the same vitest worker. Listener-registration
   assertions (cleanup tests) use `vi.spyOn(document,
   'removeEventListener')` / `vi.spyOn(window,
   'removeEventListener')` for the same reason.
2. **`afterEach` teardown in both touched test files:**
   ```ts
   afterEach(() => {
     resetVisibility();
     vi.restoreAllMocks();
   });
   ```
3. **Unmount discipline.** Every `renderHook(...)` call captures
   `unmount` and runs it (explicitly or via
   `@testing-library/react`'s `cleanup`). Leaked
   `visibilitychange`/`focus` listeners on the jsdom globals are
   the most likely cross-test footgun.
4. **Vitest environment pin.** Both files declare
   `// @vitest-environment jsdom` at the top (verify during
   Phase 1).

Place the helpers in `hub-client/src/test-utils/visibility.ts`.

### Test cases — text sync (`useAutomergeSync.test.ts`)

Extend the existing file. Required cases:

1. **Baseline / regression guard:** when visible, `handleImmediateSync`
   calls `executeEdits` once per remote change (already the de-facto
   behaviour; pin it explicitly).
2. **Hidden gate:** after `visibilityState` flips to `'hidden'`, three
   successive `handleImmediateSync` calls with different contents
   produce **zero** `executeEdits` calls.
3. **Flush round-trip:** (a) after step 2, flipping back to
   `'visible'` produces **exactly one** `executeEdits` call
   computed from the latest stashed content (verify via
   `diffToMonacoEdits` mock call args:
   `(currentMonacoContent, lastStashedContent)`); (b) a second
   hide→visible cycle with nothing stashed in between produces
   zero `executeEdits` calls.
4. **File switch while hidden:** stash for file A, switch
   `currentFile` to B while still hidden, become visible — no stale
   edit for A is applied to B's editor. (Guards against a footgun in
   our ref design.)
5. **Cleanup:** unmounting removes both the `document`
   `visibilitychange` and `window` `focus` listeners. Assert via
   spying on `document.removeEventListener` and
   `window.removeEventListener`.
6. **React state (`content`) still updates** while hidden so that
   `Preview` / other consumers stay in sync — only Monaco is
   deferred.
7. **Focus-only flush (visibilitychange never fires):** with
   `visibilityState` still reporting `'hidden'` (simulating Chrome
   DevTools "Emulate a focused page" or Firefox Cmd-H/Cmd-Tab),
   stash content via `handleImmediateSync`, then fire a `window`
   `focus` event without a preceding `visibilitychange`. Expect
   exactly one `executeEdits` call computed from the latest stashed
   content, and the stash ref cleared.
8. **Idempotent double-fire:** stash once, then fire
   `visibilitychange → visible` followed immediately by a `window`
   `focus`. Expect exactly one `executeEdits` call total (the
   second handler finds the ref already cleared and no-ops).

### Test cases — presence (`presenceService.test.ts`)

Extend the existing file. Required cases:

1. **Baseline:** while visible, N ephemeral messages produce N
   `notifySubscribers` invocations (pin existing behaviour).
2. **Hidden gate:** while `visibilityState === 'hidden'`, N ephemeral
   messages produce **zero** subscriber invocations, but
   `getRemotePresences()` returns the latest per peer (data-model
   freshness preserved).
3. **Flush round-trip:** (a) flipping to `'visible'` triggers
   exactly one `notifySubscribers` call whose snapshot matches the
   final `remotePresences` map; (b) a subsequent hide→visible
   cycle with no updates in between emits zero notifications.
4. **Leave messages during hidden:** verify they still mutate the map
   (user removal) and are included in the single flush on refocus.
5. **Cleanup:** `cleanupPresence()` removes both the
   `visibilitychange` and `focus` listeners.
6. **Focus-only flush:** with `visibilityState === 'hidden'`, push
   N ephemeral messages (zero `notifySubscribers` calls), then fire
   a `window` `focus` event. Expect exactly one `notifySubscribers`
   call with the coalesced snapshot.
7. **Idempotent double-fire:** with pending updates, fire
   `visibilitychange → visible` then `window` `focus`. Expect
   exactly one `notifySubscribers` call total.

## Implementation plan

### Phase 1: test scaffolding

- [ ] Add `hub-client/src/test-utils/visibility.ts` with
      `setVisibility(state)`, `resetVisibility()`, and
      `fireWindowFocus()` using the `vi.spyOn` pattern described
      above.
- [ ] Confirm `presenceService.test.ts` carries
      `// @vitest-environment jsdom` (add it if missing).
- [ ] Add the `afterEach` teardown (`resetVisibility()` +
      `vi.restoreAllMocks()`) to both touched test files, even
      before any new test cases land. Run the existing suites
      once to confirm nothing regresses.

### Phase 2: text-sync gate (TDD)

- [ ] Write tests 1-8 in `useAutomergeSync.test.ts`. Expect 2-5, 7,
      and 8 to fail (test 1 already passes; test 6 also already
      passes but is kept as a regression).
- [ ] Run `cd hub-client && npm test -- useAutomergeSync` and
      confirm the new tests fail for the expected reason (executeEdits
      called N times / called 0 times on flush).
- [ ] Implement the gate in `useAutomergeSync.ts`:
  - Add a `pendingRemoteContentRef = useRef<string | null>(null)`.
  - Extract a `flushPendingRemote()` helper that: reads
    `pendingRemoteContentRef.current`; returns if null; otherwise
    diffs against the current Monaco value, applies one
    `executeEdits` under `applyingRemoteRef`, and clears the ref.
    This function is intentionally idempotent — calling it when the
    ref is already null is a no-op.
  - In `handleImmediateSync`, before `executeEdits`: if
    `document.visibilityState === 'hidden'`, stash `newContent` in
    the ref, call `setContent(newContent)` so React state stays
    current, and return early.
  - Add an effect that registers two listeners:
    `document.addEventListener('visibilitychange', onVisibilityChange)`
    where `onVisibilityChange` calls `flushPendingRemote()` when
    `document.visibilityState === 'visible'`, and
    `window.addEventListener('focus', flushPendingRemote)` as a
    belt-and-braces against the documented cases where
    `visibilitychange` doesn't fire (Chrome/Edge DevTools
    focus-emulation, Firefox Cmd-H/Cmd-Tab, headless Chrome, Brave
    HTTP+DevTools).
  - On unmount and on `currentFile` change, unsubscribe both
    listeners and clear `pendingRemoteContentRef`. Discarding the
    stash on switch is correct because the reconciliation effect
    reads live `getFileContent(currentFile.path)` on every file
    switch, so both the new file and any later revisit of the old
    file catch up from Automerge source rather than from a stale
    diff (satisfies test 4).
- [ ] Re-run tests. All pass.

### Phase 3: presence gate (TDD)

- [ ] Write tests 1-7 in `presenceService.test.ts`. Expect 2-4, 6,
      and 7 to fail (existing tests cover 1 and 5 implicitly —
      verify).
- [ ] Run `cd hub-client && npm test -- presenceService` and confirm
      the new tests fail.
- [ ] Implement in `presenceService.ts`:
  - Add `pendingNotify: boolean` to `PresenceServiceState` (initial
    `false`).
  - Add `visibilityHandler` and `focusHandler` fields for listener
    cleanup.
  - Extract a `flushPendingNotify()` internal helper that calls
    `notifySubscribers()` and clears `pendingNotify` when the flag
    is set; idempotent no-op otherwise.
  - In `initPresence`, attach two listeners:
    `document.addEventListener('visibilitychange', ...)` that calls
    `flushPendingNotify()` when `document.visibilityState ===
    'visible'`, and `window.addEventListener('focus',
    flushPendingNotify)` for the documented
    `visibilitychange`-doesn't-fire cases.
  - In `notifySubscribers`, gate on `document.visibilityState`:
    if `'hidden'`, set `state.pendingNotify = true` and return
    without calling subscriber callbacks. Otherwise behave as today.
  - In `cleanupPresence` / `_resetForTesting`, remove both listeners
    and clear `pendingNotify`.
- [ ] Re-run tests. All pass.

### Phase 4: build + workspace verification

Manual E2E is skipped for this plan. The Repro confirmation section
at the top of this file already proved the mechanism (remote
Automerge changes arrive during the hidden period and each one
triggers a separate `executeEdits`), so the gate's effect is
deterministic from the unit tests: 0 `executeEdits` while hidden,
1 on flush. If the final build reveals anything unexpected, add a
Verification section then.

- [ ] `cd hub-client && npm run build:all` — stricter than vitest
      (project references mode), required per CLAUDE.md. Also
      rebuilds the WASM, so no separate `cargo xtask verify` is
      needed for this change (no Rust code touched).
- [ ] `cd hub-client && npm run test:ci` — full hub-client suite,
      not just the edited files.

### Phase 5: commit + changelog

Per CLAUDE.md two-commit workflow for hub-client changes:

- [ ] Commit 1: source + test changes. Message: terse title + one
      short paragraph (per user's saved preference for commit
      verbosity).
- [ ] Commit 2: update `hub-client/changelog.md` with the short hash
      from commit 1, grouped under today's date (`2026-04-24`).
      Entry: "Defer remote edit application and presence notifications
      while the tab is hidden to avoid replay-animation on refocus."
- [ ] Stop and **ask** the user for push approval before `git push`.

## Design decisions worth calling out

- **Listener lives in the service, not a component.** For presence,
  attaching `visibilitychange` inside `initPresence` keeps the gate
  symmetric with the rest of the lifecycle (`cleanupInterval`,
  `cleanupPresence`). Attaching in `usePresence` would require each
  subscriber to opt-in, and there is only one subscriber pattern
  today.
- **Keep `setContent` firing while hidden.** React state consumers
  (e.g. `Preview`) do not animate on burst updates, so gating them
  is unnecessary and would drift the preview out of sync with
  Automerge. Only Monaco's `executeEdits` is deferred.
- **Use refs, not state, for `pendingRemoteContentRef`.** State
  writes would re-render `useAutomergeSync` consumers on every
  deferred message, defeating the point.
- **Clear stash on `currentFile` change.** Discard any pending
  content on switch rather than flushing it to the outgoing
  editor. The reconciliation effect reads live Automerge content
  on every file switch, so both the new file and any later
  revisit of the old file catch up from source — no stale diff
  can leak across editors.
- **Dual flush listeners with an idempotent flush function.**
  `visibilitychange` is the primary trigger but has documented
  non-firing cases (Chrome/Edge DevTools "Emulate a focused page",
  Firefox macOS Cmd-H→Cmd-Tab, headless Chrome, Brave HTTP+DevTools).
  We attach a secondary `window` `focus` listener. Because the
  flush reads-then-clears the stash ref (or the `pendingNotify`
  flag), the second-firing handler no-ops. This closes the
  non-firing-visibilitychange class at roughly zero cost.
- **Do not touch `cleanupStalePresences`.** It's a `setInterval`
  that's already throttled in hidden tabs. Letting it drain on
  refocus is fine — any stale entries will notify once via the
  coalesced post-visibility flush.

## Risks / things to watch

- **`visibilitychange` is unreliable in several documented cases,
  not just theoretically.** Chrome/Edge DevTools "Emulate a focused
  page" suppresses it on tab switch; Firefox on macOS does not fire
  it when the app is hidden with Cmd-H and restored with Cmd-Tab
  (Bugzilla 777825); headless Chrome misses it on tab switch
  (webdriverio#9694); Brave and Chrome over HTTP with DevTools open
  skip it (brave-browser#42566). The HTML and Page Visibility specs
  do not pin an order for `visibilitychange` vs. `focus` either.
  **Mitigation is in the design**, not deferred: attach the
  idempotent flush to both `document` `visibilitychange` and
  `window` `focus`. Downside of the dual listener: a `focus` event
  on a still-hidden page (rare but possible with programmatic
  `window.focus()` from an extension) triggers the flush without
  visibility changing. That's still safe — the flushed content is
  current, and Monaco edits on an unrendered document are harmless;
  they just paint on next visibility.
- **Test flakiness from `document` / `window` mutation** is designed
  out in the Test isolation strategy section (spy-based overrides +
  `vi.restoreAllMocks()` + entry canary + unmount discipline). It
  stays on the watch-list only because jsdom upgrades occasionally
  change which `Document.prototype` getters are `configurable: true`,
  which could break `vi.spyOn` on the relevant property. If that
  happens the helper throws on first use — obvious, easy to
  diagnose — and we fall back to captured-descriptor
  `Object.defineProperty` for that property specifically.
- **Multiple Monaco editors:** `useAutomergeSync` is keyed on a
  single current file; if hub-client ever opens multiple editors
  concurrently the ref model would need to become a Map keyed by
  path. Today's app only has one editor at a time — note this in the
  code comment for future readers.

## Work items

- [x] Phase 1: visibility test helper
- [x] Phase 2.1: write text-sync tests (TDD failing)
- [x] Phase 2.2: implement text-sync gate
- [x] Phase 2.3: text-sync tests green
- [x] Phase 3.1: write presence tests (TDD failing)
- [x] Phase 3.2: implement presence gate
- [x] Phase 3.3: presence tests green
- [x] Phase 4: `build:all`, `test:ci`
- [ ] Phase 5: commits + changelog + ask for push
