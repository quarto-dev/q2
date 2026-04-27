# Replace OT cursor tracking with Automerge cursors in hub-client presence

Beads: TBD (tracks [issue #113](https://github.com/quarto-dev/q2/issues/113))

## Overview

Hub-client's presence system (`hub-client/src/hooks/usePresence.ts`) currently
broadcasts remote cursor/selection positions as raw character offsets and then
runs hand-rolled operational transformation locally on every
`onDidChangeContent` to keep stored offsets valid between presence ticks. The
OT code includes two heuristic guards (`anticipatingEditRef` and a
same-line check) that exist solely to paper over the async gap between
presence messages and matching content changes.

Issue #113 suggests replacing this with [Automerge
cursors](https://automerge.org/automerge/automerge/struct.Cursor.html). The
file content is already an Automerge Text sequence edited via `splice(doc,
['text'], ...)` (`ts-packages/quarto-sync-client/src/client.ts:538`), so a
cursor anchored to `['text']` is the natural position-stable token: it
shifts automatically under concurrent edits and resolves, on the receiver,
to a valid offset in *their* current doc state.

The JS binding gives us exactly what we need:

- `A.getCursor(doc, ['text'], position: number | 'start' | 'end', move?: 'before' | 'after'): Cursor` (string)
- `A.getCursorPosition(doc, ['text'], cursor): number`

### Goals

1. Delete the OT machinery (`transformOffset`, `PeerCursorState` OT fields,
   the `onDidChangeContent` OT effect, `anticipatingEditRef`, and the
   same-line guard). Keep `modelVersion` (still needed to drive
   re-resolution of Automerge cursors when the doc changes — see
   "Receiver path").
2. Preserve the fix from PR #94 (deletion in one paragraph must not shift
   remote cursors in subsequent paragraphs).
3. Leave the wire-level regression surface the same or better: in particular,
   no new cursor flicker on the typing-plus-lag scenario the anticipation
   code was protecting against.

### Non-goals

- Changing the transport (still Automerge ephemeral messaging).
- Changing how selections are modelled beyond "two cursors, start and end".
- Any server-side changes.

## Design sketch

### Wire format change

`PresenceMessage` (`hub-client/src/services/presenceService.ts:30`) currently
carries:

```ts
cursor: number | null;
selection: { start: number; end: number } | null;
```

Change to:

```ts
cursor: string | null;                                // Automerge Cursor
selection: { start: string; end: string } | null;     // two cursors
```

Cursor strings are opaque; both ends of the wire treat them as blobs and
only resolve on the receiver's own doc.

**Mixed-version rollout.** This is a breaking wire-format change for
ephemeral presence messages. During the window between deploy and all
clients refreshing:

- Old clients receiving new messages interpret `cursor` as a number and
  pass the string through `model.getPositionAt(...)`, which coerces to
  `NaN`. The existing `try { ... } catch {}` around decoration placement
  in `usePresence.ts` swallows the error, so the old client silently drops
  the new client's cursor decoration.
- New clients receiving old messages pass a number into
  `A.getCursorPosition`, which throws `RangeError`. Phase 3's per-peer
  `try/catch (RangeError) { continue; }` swallows it — the new client
  silently drops the old client's cursor decoration.

Net effect: mixed-version peers can't see each other's cursors, but no
one crashes and decorations recover as soon as both sides are on new
code. Ephemeral messages are not persisted, so nothing lingers past a
refresh. This is acceptable for the hub-client's auto-update model; no
schema-version field or transitional dual-send is added.

### Sender path

In `presenceService.ts`, `updatePresence(cursorOffset, selectionRange)` still
receives Monaco offsets (callers in `usePresence.ts:466-476` don't change).
Inside the service, convert each offset to an Automerge cursor using the
current `state.currentHandle.doc()` (`docSync()` is deprecated as of
`@automerge/automerge-repo` 2.5.1 — see
`node_modules/@automerge/automerge-repo/dist/DocHandle.d.ts:79-83`):

```ts
const doc = state.currentHandle.doc();
const cursor = offset !== null ? A.getCursor(doc, ['text'], offset) : null;
```

**Ordering is already satisfied by the existing architecture** (no fix
needed, but worth verifying with a regression test — see Phase 4):

Monaco fires `onDidChangeModelContent` synchronously before
`onDidChangeCursorSelection`. The content path
(`useAutomergeSync.ts:155-163` `handleEditorChange` → `App.tsx:429-431`
`handleContentOperations` → `automergeSync.ts:185-186`
`applyEditorOperations` → `ts-packages/quarto-sync-client/src/client.ts:535-540`
`handle.change(doc => splice(...))`) is entirely synchronous. By the
time the cursor-selection handler in `usePresence.ts:480` runs, the
Automerge doc already reflects the edit. So any `handle.doc()` +
`A.getCursor(...)` call from `broadcastPresence` — whether it fires
immediately on the throttle boundary or from the trailing-edge
`setTimeout` — sees post-edit state.

The previously stated fallback (sender broadcasts cursor anchored to
pre-edit position, self-heals on next tick) is therefore moot in the
current architecture. Phase 4 adds a regression test to prevent a
future refactor from breaking this invariant.

### Receiver path

`handleEphemeralMessage` stores the cursor/selection strings as received
(no offset resolution). `PresenceState.cursor` becomes `string | null`
and `PresenceState.selection` becomes `{ start: string; end: string } |
null` — the same types as the wire. Resolution to numeric offsets
happens inside `usePresence.ts`, per-render, against the current local
doc:

```ts
let offset: number;
try {
  offset = A.getCursorPosition(doc, ['text'], cursor);
} catch {
  // Cursor references an op we haven't synced yet; skip decoration
  // until the content change arrives and a re-render resolves it.
  continue;
}
```

`getCursorPosition` throws `RangeError` when the cursor references an op
the local doc doesn't have (see
`node_modules/@automerge/automerge/dist/mjs/next_slim.js:373-382`).
Treat the throw as "not yet resolvable" and drop the decoration for this
render; the next `onDidChangeContent` after the op syncs will re-render
successfully.

`usePresence.ts` becomes much simpler:

- No `PeerCursorState` map, no OT effect, no `anticipatingEditRef`.
- On each render (triggered by `remoteUsers` change *and* by an
  `onDidChangeContent`-driven bump), resolve each peer's cursor/selection
  strings against the current doc and place Monaco decorations.
- Keep a minimal `modelVersion` bump from `onDidChangeContent`, but for a
  different reason than today: stored cursor strings don't change, but
  their *resolved offsets* do when the underlying doc changes (local or
  remote). Without a re-render, decorations would stick at stale offsets
  until the next presence message arrived. Remote edits reach this path
  via `automergeSync.ts`' `immediateFileChangeCallback`, which applies
  remote diffs to Monaco and triggers `onDidChangeContent`, so the same
  bump covers both local and remote edits.

#### New coupling: `usePresence.ts` → `automergeSync.ts`

Today, `usePresence.ts` talks only to `presenceService.ts`. To resolve
cursor strings it now needs the current Automerge doc, which lives on
the handle returned by `automergeSync`'s `getFileHandle(path)`. Rather
than push resolution into `presenceService` (which would need to
re-resolve whenever the doc changes, duplicating the `modelVersion`
bump), add an import in `usePresence.ts`:

```ts
import { getFileHandle } from '../services/automergeSync';
// ...
const handle = getFileHandle(currentFilePath);
const doc = handle?.doc();
```

This is a minor coupling increase but keeps resolution co-located with
the render that consumes it.

### Edge cases

Resolved empirically against `@automerge/automerge` 2.2.9 with a probe
script (node REPL + `A.getCursor`/`getCursorPosition` on `A.from({text:
''})` etc.):

- **Cursor at EOF**: no bias needed. `getCursor(doc, ['text'], docLength)`
  returns the sentinel `"e"` (end) which tracks the end of the sequence
  through subsequent insertions automatically. The `move` bias turns out
  not to matter at EOF — default, `'after'`, and `'before'` all produce
  the same result.
- **Empty doc**: `getCursor(doc, ['text'], 0)` on empty text returns the
  `"e"` sentinel and resolves back to 0. Works fine, no special case.
- **Past EOF**: `getCursor(doc, ['text'], 999)` on a short doc doesn't
  throw — it clamps and returns the end sentinel. Robust to offset drift
  at the sender if it ever happens.
- **Mid-doc bias (cursor and selection start)**: default `move` is
  `'after'` (`node_modules/@automerge/automerge/dist/wasm_types.d.ts:32-34`),
  which anchors to the character *at* the given offset, so subsequent
  insertions-at-that-offset push the cursor forward (the cursor stays on
  the logical character). This is the behaviour we want for carets and
  for the **start** of a selection — no need to pass `move` explicitly.
- **Selection end**: the plan originally called for `move: 'before'` to
  prevent the selection from growing when a char is inserted at the end
  boundary. **A re-probe against the pinned `@automerge/automerge` 2.2.9
  during implementation showed this does not work as documented in that
  version**: `'before'` and `'after'` produce different cursor strings
  (`N@…` vs `-N@…`) but resolve to *identical* positions after both
  single-doc insertions and merged concurrent insertions. The `'before'`
  bias is a no-op in the current binding. If a future Automerge upgrade
  fixes this, the Phase 5 probe fixture will catch the change and we can
  revisit; for now, use the default `'after'` for both selection
  boundaries. The observable consequence is that inserts at the end
  boundary grow a remote peer's selection — the OT path today has no
  boundary-aware logic either, so this is not a regression.
- **Selection collapsed** (`start === end`): treat as cursor-only, same as
  today.
- **Peer on a file we don't have the doc for yet**: can't resolve; drop the
  decoration until the doc arrives. Today's code has the equivalent
  behaviour through `getFileHandle(filePath)` returning null.

### What `anticipatingEditRef` and the same-line guard did, and why we don't
### need them anymore

Both exist because, with offset-based presence, a fresh presence message
from peer A carrying a *post-insert* offset could arrive before A's content
change synced into our doc. The OT code would then wrongly shift the
already-post-insert offset again when the content change landed.

With Automerge cursors, the receiver *always* resolves against its own
current doc state. If A's content change hasn't arrived, one of two
things happens when we call `getCursorPosition`:

1. The cursor references an op *we already have* (typical — Automerge
   sync tends to deliver ops in causal order). Resolves to a sensible
   offset in our pre-insert text.
2. The cursor references an op *we haven't received yet*.
   `getCursorPosition` throws `RangeError`
   (`node_modules/@automerge/automerge/dist/mjs/next_slim.js:373-382`).
   We catch and skip the decoration for this render. Automerge does
   **not** have a documented "resolve to nearest known position"
   fallback; an unknown cursor throws.

In either case, once the content change syncs, the next render
(triggered by `onDidChangeContent` via `immediateFileChangeCallback`)
resolves the same cursor to the correct offset. No double-shift is
possible because we never shift — we query.

## Test plan (written first, TDD)

### Phase 1 tests — lock in PR #94 behaviour before refactoring

File: `hub-client/src/hooks/usePresence.test.ts` (new).

These tests exercise the hook against a mocked Monaco editor + mocked
Automerge doc and assert on the decoration positions the hook would apply.

- [x] **Test: deletion in an earlier paragraph does not shift a remote
      cursor in a later paragraph** — the original #9 regression. Set up a
      three-paragraph document, place a remote cursor in paragraph 3,
      delete two characters from paragraph 1 locally, assert the remote
      cursor's resolved offset decreases by 2 (i.e., it tracks the same
      logical character).
- [x] **Test: local insertion before a remote cursor shifts it forward**
      by the inserted length.
- [x] **Test: local deletion across a remote cursor clamps it to the end
      of the replacement text**.
- [x] **Test: remote cursor arrives before its corresponding content
      change** — the race that `anticipatingEditRef` guards today. Simulate
      a presence update that anticipates a 1-char insertion, then deliver
      the content change, and assert the cursor ends at the right offset
      without double-shift.
- [x] **Test: remote cursor at end-of-line, presence before content
      change** — the scenario PR #110 (3bd3ebc0) fixed with the same-line
      guard. Simulate a peer typing a character at EOL, deliver the
      presence update (with post-edit offset) before the content change,
      and assert the remote cursor decoration does not appear on the
      following line at any point. Under OT this exercises the same-line
      guard; under Automerge cursors it's impossible by construction
      (an unsynced op either resolves in the current doc or throws and we
      skip). Locking it in guards against a future regression.

These tests must pass on the current OT implementation *before* starting
the refactor. If any fail, fix the test or surface the bug via a separate
issue first.

### Phase 2 tests — presenceService wire-format tests

File: `hub-client/src/services/presenceService.test.ts` (extend).

- [x] **Test: broadcast carries cursor as an Automerge cursor string, not
      a number**. Mock `getFileHandle` to return a fake handle with
      `doc()` returning a doc with a known `['text']`; call
      `updatePresence(5, null)`; assert the ephemeral message's `cursor`
      field is a string whose `A.getCursorPosition` resolves back to 5.
- [x] **Test: selection carries start+end cursor strings** that resolve
      back to the original range. (The originally-planned assertion about
      `move: 'before'` preventing the end from growing on boundary inserts
      was dropped — the `move` parameter is a no-op in Automerge 2.2.9.
      See "Design sketch → Edge cases → Selection end".)
- [x] **Test: null cursor/selection pass through as null**.
- [x] **Test: broadcast is a no-op if the handle is unavailable** — we
      can't make a cursor without a doc; skip the broadcast rather than
      crash (matches today's behaviour when `state.currentHandle` is
      null).
- [x] **Test: broadcast is a no-op if `handle.doc()` throws** — mock a
      handle whose `doc()` throws (simulating the deleted/unavailable
      case flagged in `DocHandle.d.ts`); assert `broadcast` is not
      called and no exception escapes `broadcastPresence`.
- [x] **Test: incoming ephemeral messages store cursor strings unchanged**
      in `remotePresences`.

### Phase 3 tests — end-to-end race scenarios

Extend Phase 1 tests to verify the new implementation:

- [x] **Test: presence-before-content-change no longer needs anticipation**.
      Receive a presence message whose cursor string references an op
      not yet present in our doc; assert that `getCursorPosition` throws
      `RangeError` and the hook skips the decoration for this render
      (no crash, no other peers' decorations affected). Then apply the
      content change and assert the cursor resolves to the intended
      offset on the next render.
- [x] **Test: two concurrent remote edits** — two peers insert into the
      same paragraph. Assert a third peer's cursor, anchored after the
      insertions, ends at the correct offset regardless of merge order.
      This was not testable under OT because the OT code assumed a single
      linear stream of local edits.

## Work Items

### Phase 1 — characterise current behaviour

- [x] Create `hub-client/src/hooks/usePresence.test.ts` with a minimal
      Monaco + Automerge test harness (mock `@monaco-editor/react`, use a
      real Automerge doc for fidelity).
- [x] Write the five Phase-1 tests above (four PR #94 tests plus the
      PR #110 EOL regression test) and verify they pass on the current
      OT implementation.
- [x] Commit as "test: cover OT cursor tracking race cases before refactor".

### Phase 2 — presenceService wire-format change + usePresence.ts rewrite

**Phases 2 and 3 land as a single commit.** Splitting them leaves an
intermediate state that doesn't typecheck: as soon as
`PresenceState.cursor` becomes `string | null`, the OT code in
`usePresence.ts` (which does numeric arithmetic on `state.cursor` and
compares `state.lastPresenceCursor !== user.cursor`) is nonsense. The
refactor is structured as two phases below for clarity of review, but
the work items are ordered so the code compiles continuously: update
the types and broadcast path, then immediately delete the OT machinery
in the *same* commit.

- [x] Change `PresenceMessage.cursor` to `string | null` and
      `PresenceMessage.selection` to `{ start: string; end: string } | null`.
- [x] Change `PresenceState.cursor` to `string | null` and
      `PresenceState.selection` to `{ start: string; end: string } | null`
      (same types as the wire — the service stores what it received).
      Update all consumers to reflect the new types; numeric resolution
      moves into `usePresence.ts` in Phase 3.
- [x] Update `broadcastPresence` to call `A.getCursor` with the current
      handle's `doc()` (not `docSync()` — deprecated in
      `@automerge/automerge-repo` 2.5.1) before sending. Use default
      `move: 'after'` for all cursors (caret, selection.start,
      selection.end). The `move: 'before'` call originally planned for
      selection.end was dropped — see "Design sketch → Edge cases →
      Selection end" for the re-probe finding.
- [x] Handle both the handle-unavailable and doc-unavailable cases: skip
      the broadcast when `state.currentHandle` is null (as today) *and*
      wrap the `handle.doc()` call in `try/catch` — per
      `node_modules/@automerge/automerge-repo/dist/DocHandle.d.ts:76`,
      `doc()` throws on deleted or unavailable documents. A thrown
      `doc()` should be a silent skip, matching the handle-null
      behaviour; a stale or absent doc isn't a crash-worthy condition
      in a 50 ms-throttled broadcaster. Empty doc is not a special
      case: `A.getCursor(doc, ['text'], 0)` on empty text returns the
      end sentinel per the probe script — verify at test time.
- [x] Write and pass the Phase-2 tests.
- [x] Continue immediately into Phase 3 in the same commit — do *not*
      commit wire-format changes in isolation (see phase-merge note
      above).

### Phase 3 — usePresence.ts simplification (same commit as Phase 2)

- [x] Delete `transformOffset`, `PeerCursorState`, `peerStateRef`,
      `anticipatingEditRef`, the `onDidChangeContent` OT effect, and the
      same-line guard.
- [x] Keep the `modelVersion` state + `onDidChangeContent` bump (still
      needed — see "Design sketch → Receiver path") but slim the effect
      down to only the bump, no OT loop. Update the comment to reflect
      the new role ("re-resolve Automerge cursors against the updated
      doc").
- [x] Add `import { getFileHandle } from '../services/automergeSync'`
      (new coupling — see "Design sketch → Receiver path → New
      coupling"). In the render effect, obtain the doc via
      `getFileHandle(currentFilePath)?.doc()`. Wrap the `doc()` call in
      try/catch as well — `DocHandle.d.ts` flags `doc()` as throwing on
      deleted/unavailable docs, and a throw here would kill the render
      effect for every peer, not just the unsynced one. On throw, bail
      out of this render (no decorations applied); the next content
      change or `remoteUsers` update re-runs the effect.
- [x] In the render effect, call `A.getCursorPosition(doc, ['text'],
      cursor)` for each peer's stored cursor/selection strings and place
      Monaco decorations from the resolved offsets. Wrap each call in
      `try/catch (RangeError)` and `continue` on throw — an unsynced
      cursor should drop the decoration for this render, not crash the
      effect or block other peers' decorations.
- [x] Add Phase-3 tests and verify them pass.
- [x] Verify Phase-1 tests still pass under the new implementation.
- [x] Commit as "refactor(presence): replace OT offset tracking with
      Automerge cursors (fixes #113)".

### Phase 4 — sender-side ordering regression test

Ordering is already satisfied by the existing architecture (see "Design
sketch → Sender path"): Monaco fires content changes synchronously
before cursor-selection changes, and the content path through
`handleEditorChange` → `applyEditorOperations` → `splice(...)` is fully
synchronous. This phase locks that invariant in with a test so a future
refactor can't quietly break it.

- [x] Add a test in `hub-client/src/services/presenceService.test.ts`
      that drives a synthetic Monaco edit (via the mocked editor +
      `handleEditorChange` path) immediately followed by a cursor
      broadcast, and asserts the broadcast cursor string resolves to
      the post-edit offset on a separate receiver doc. If the sender
      read pre-edit state, the cursor would resolve to a stale offset
      and the test would fail.
- [x] Commit as "test(presence): lock in sender-side cursor ordering".

### Phase 5 — verification and changelog

- [x] Run `cd hub-client && npm run build:all` (per CLAUDE.md this is
      stricter than vitest + tsc --noEmit).
- [x] Run `cd hub-client && npm run test:ci`.
- [x] Run `cargo xtask verify --skip-rust-tests` to confirm the
      hub-client build and tests pass in the xtask harness. (We skip
      Rust tests because this is a pure TS change — `quarto-sync-client`
      is a TypeScript package under `ts-packages/`, not a Rust crate, so
      Rust test behaviour cannot regress from this work.)
- [ ] Run the hub-client dev server, open two browser tabs against the
      same project, type in one tab, and verify remote cursor behaviour
      by eye — specifically the scenarios PR #94 added: deleting in one
      paragraph while another peer has a cursor in a later paragraph.
- [ ] **Performance check**: profile keystroke latency in the hub-client
      dev server before and after the refactor, using Chrome devtools'
      Performance panel.
      - **Simulating peers**: open 3 browser tabs against the same
        project (two peers + one recorder). Move the cursor to different
        locations in the two non-recording tabs so their cursor
        decorations render on the recorder. This exercises the
        per-render resolution loop over multiple peers.
      - **Document**: use a ~10k-char qmd file (copy a real project
        README or a paragraph-repeated fixture; commit as
        `hub-client/test-fixtures/perf-10k.qmd` if one doesn't exist).
      - **Measurement**: on the recorder, start the Performance panel
        recording, hold a key to auto-repeat for ~5 s in the editor,
        stop recording, and read the mean "Scripting" time per
        keystroke.
      - **Target**: the added cost of
        `getCursor`/`getCursorPosition` WASM calls should be well under
        1 ms per keystroke. Investigate if it exceeds that.
- [ ] **Only if the profile shows regression**: add a per-peer resolved-
      offset cache (`Map<peerId, number>`) in `usePresence.ts`, invalidated
      when (a) the peer's incoming cursor string changes, or (b)
      `onDidChangeContent` fires. This avoids re-resolving the same cursor
      on every render without reintroducing OT or the anticipation
      heuristic. Do not add this speculatively — it's a small amount of
      state that's easy to get wrong, and the plan's whole point is to
      delete such state.
- [x] Commit the Automerge cursor-semantics probe script as an
      executable fixture at
      `hub-client/src/services/automergeCursor.probe.test.ts` (a vitest
      file that asserts the edge-case behaviours enumerated in "Design
      sketch → Edge cases": EOF, empty doc, past-EOF clamp, mid-doc
      bias, selection-end `'before'` bias). This pins the behaviour to
      our pinned `@automerge/automerge` version and makes future
      upgrades catchable via CI.
- [x] Two-commit workflow for `hub-client/changelog.md`: first commit the
      refactor, then a second commit adding a changelog entry with the
      refactor's hash under today's date header.

## Open questions

All three pre-implementation questions were resolved empirically against
`@automerge/automerge` 2.2.9 with a probe script; see "Edge cases" and
"Design sketch → Receiver path" above. No open questions remain at plan
time. If implementation surfaces new ones, add them here.
