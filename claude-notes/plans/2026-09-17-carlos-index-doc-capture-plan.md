# Capturing a real specimen of the index-document staleness bug (Carlos, on his own machine)

## Overview

Context: `claude-notes/research/2026-09-16-automerge-index-doc-staleness.md`,
strand bd-6f21d4c6. We've confirmed two mechanisms (H1: a peer-registration
race in `CollectionSynchronizer.addPeer`; H2: a per-storageId sync-state
persistence throttle) are *real and manufacturable* against the vendored
automerge-repo v2.5.6 source — but that's source-reading and synthetic
repro, not a diagnosis. The fact that actually defines this bug, and that
any candidate mechanism has to account for, is: **a plain page reload
does not fix a stuck session; only clearing the `automerge` IndexedDB
database does.** That's the whole reason this is worth investigating —
if reload were sufficient, this would just be an ordinary reconnect
hiccup. It already rules out H1 as a *sufficient* standalone explanation
(its state is pure in-memory and must reset on reload — hub-client runs
its `Repo` directly in the page, no `SharedWorker`/`ServiceWorker` to
survive one), and it's in tension with the assumption that a
stale-but-valid persisted sync state should always self-heal via
continued message exchange (H2's "ruled out" reasoning).

A live production error trace surfaced on 2026-09-17 during this
discussion looks like a much stronger, concrete lead than either H1 or
H2 — see "What to capture" step 3 below. It's a `RangeError: duplicate
seq N found for actor <id>` thrown inside automerge-wasm's
`receiveSyncMessage`, silently swallowed by a bare `console.log` in
automerge-repo's `Repo.ts` with no retry. If confirmed, this fits the
reload/wipe fact *better* than H1 or H2, because the poisoned state is
baked into the locally persisted document's own change history (not
just in-memory peer bookkeeping or a persisted sync-state entry) — a
reload reloads the same poisoned bytes and hits the identical rejection
immediately; only discarding the local copy removes the colliding
change.

**Goal of this plan:** the next time Carlos sees the bug (or if it's
currently reproducing), capture real specimens — live in-memory state,
the raw persisted IndexedDB bytes, and (new, highest priority) the full
console error whenever "duplicate seq" or "error receiving message"
fires — so we can test the hypotheses against actual data instead of
source-reading and synthetic repros. Written to be run entirely by
Carlos, in Chrome, on his own machine. Nothing here requires code
changes; it's an evidence-capture runbook, and it is **entirely
read-only / non-destructive** — it does not tell Carlos to apply the
known IndexedDB-clear workaround. He already knows how to do that and is
deliberately not doing it yet, because clearing storage would destroy
the only reproduction we have.

## Checklist

- [ ] **Step 0 (do this now, proactively, once):** enable the in-app
      debug API, and turn on "Preserve log" in the DevTools console so
      intermittent errors aren't lost.
- [ ] **Step 1 (when you notice the bug, or right now if it's currently
      reproducing):** confirm the debug API is live.
- [ ] **Step 2:** capture the live in-memory snapshot via
      `quartoDebug.am`.
- [ ] **Step 3 (highest priority — do this even if you skip everything
      else):** capture the full, untruncated console error the next
      time "duplicate seq" / "error receiving message" appears.
- [ ] **Step 4:** capture the raw persisted IndexedDB bytes via the
      console export script.
- [ ] **Step 5 (optional — a reload doesn't fix anything per the fact
      above, so this is safe, but skip it if you'd rather leave the
      session exactly as-is):** repeat steps 2–4 after one plain reload,
      to get a matched before/after pair from the same incident.
- [ ] **Step 6:** jot down the context notes below.
- [ ] **Step 7:** hand off the captured files + notes.

## Details

### Step 0 — enable the debug API and console log persistence, proactively

`window.quartoDebug` (bd-q93tkglb) is gated by
`import.meta.env.DEV || localStorage.quartoDebug === '1'`, and that gate
is checked **once, on page load** (`hub-client/src/App.tsx`, a mount-only
`useEffect`). Turn it on now, once, in whatever browser profile/tab you
normally use for the affected project, so it's already active the next
time this reproduces:

```js
localStorage.setItem('quartoDebug', '1');
location.reload();
```

After the reload, confirm it's live:

```js
typeof quartoDebug // should be 'object', not 'undefined'
```

Also enable **"Preserve log"** in the DevTools Console panel (the
checkbox at the top of the Console tab). Without it, Chrome clears the
console on every navigation/reload, so an intermittent error that fires
while you're not actively watching gets lost the moment anything
reloads the page. With it on, console history survives reloads for the
rest of this investigation.

Both settings are harmless and can stay on indefinitely.

### Step 1 — when you next notice the bug (or right now, if reproducing)

Confirm the debug API survived:

```js
typeof quartoDebug
```

If it's `'undefined'`, enable it now and reload — safe, since reload
alone doesn't fix the bug (that's the defining fact, not something this
one reload risks disproving):

```js
localStorage.setItem('quartoDebug', '1');
location.reload();
```

### Step 2 — capture the live in-memory state

```js
copy(JSON.stringify({
  capturedAt: new Date().toISOString(),
  repos: quartoDebug.am.repos(),        // peerId + connectedPeers[] per repo — the direct H1 check
  docs: quartoDebug.am.docs(),          // per-doc heads/handleState, including the index doc
  syncStatus: quartoDebug.am.syncStatus(),
  doctor: quartoDebug.am.doctor(),      // existing consistency checks; unlikely to flag this shape, but cheap
  messages: quartoDebug.am.messages(),  // sync-protocol traffic ring buffer, only useful if the tap attached before this session got stuck
}, null, 2))
```

`copy(...)` puts it on your clipboard — paste into a text file.
`repos()[*].connectedPeers` is the direct H1 check: if the hub's peerId
is listed there, the transport layer believes it's connected while the
index doc isn't advancing.

### Step 3 — capture the full "duplicate seq" error (highest priority)

The error posted during this discussion:

```
error receiving message {err: RangeError: duplicate seq 1 found for actor 4c2101fe42b5a6dd0e26fd976972c821cdb071d0615f2055bf3c1809537a17c5, message: {…}}
```

The `message` object in that log line is a full sync message —
`{ type: 'sync', documentId, senderId, targetId, data }` — and it's
worth far more than the truncated text; the browser console shows the
full object when you expand `{…}` inline or right-click → "Copy object"
on it. With "Preserve log" on (Step 0), the next time this appears:

1. Expand the logged object fully (click the `▶`/triangle next to it,
   or the `{…}` for `message`) so `documentId`, `senderId`, `targetId`,
   and `data.byteLength` are all visible.
2. Right-click the top-level logged object → **"Copy object"** (or
   "Store as global variable" if you want to inspect it further in the
   console — Chrome will bind it to `temp1`, `temp2`, etc.).
3. Save that alongside your other captures. If you can also grab the
   *number* in "duplicate seq N" and the exact actor id string, note
   those explicitly even if the copy above already has them — they're
   the two facts most worth double-checking by hand.
4. If it's reproducible on demand for you right now (i.e. it just
   happened and might happen again on the next reconnect attempt),
   running Step 2's snapshot immediately after would be extremely
   valuable — it'd show whether `quartoDebug.am.docs()`'s heads for that
   `documentId` match what you'd expect, right at the moment of the
   failure.

### Step 4 — capture the raw persisted bytes

```js
(async () => {
  const db = await new Promise((res, rej) => {
    const req = indexedDB.open('automerge'); // fixed defaults: db "automerge", store "documents"
    req.onsuccess = () => res(req.result);
    req.onerror = () => rej(req.error);
  });

  const toBase64 = (bytes) => {
    let s = '';
    for (let i = 0; i < bytes.length; i += 0x8000) {
      s += String.fromCharCode(...bytes.subarray(i, i + 0x8000)); // chunked to avoid a stack-size blowup on large snapshots
    }
    return btoa(s);
  };

  const records = await new Promise((res, rej) => {
    const store = db.transaction('documents', 'readonly').objectStore('documents');
    const out = [];
    const cursor = store.openCursor();
    cursor.onsuccess = (e) => {
      const c = e.target.result;
      if (!c) return res(out);
      out.push({ key: c.value.key, binaryBase64: toBase64(c.value.binary), byteLength: c.value.binary.byteLength });
      c.continue();
    };
    cursor.onerror = () => rej(cursor.error);
  });

  const blob = new Blob([JSON.stringify({ exportedAt: new Date().toISOString(), records }, null, 2)], { type: 'application/json' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `automerge-documents-export-${Date.now()}.json`;
  document.body.appendChild(a); a.click(); a.remove();
  console.log(`Exported ${records.length} records.`);
})();
```

Downloads a JSON file with every `[documentId, "snapshot" |
"incremental" | "sync-state", ...]` key and its bytes (base64, lossless)
— decodable later with `@automerge/automerge` to inspect the actual
change graph, including whichever actor/seq the "duplicate seq" error
was complaining about.

### Step 5 — optional: a matched before/after reload pair

Skip freely if you'd rather leave the session exactly as it is. If you
do want it: repeat Steps 2–4 once more after a plain reload (no storage
clearing), naming the files `...-after-reload-<timestamp>` so they don't
collide with the first set. This gives a direct, same-incident
before/after comparison rather than a general impression.

### Step 6 — context notes to jot down alongside the captures

- The project and the specific file(s) whose links are showing the Q-13-4
  squiggle (missing-document reference on a file that actually exists).
- The index doc id (`quartoDebug.am.docs()` output from Step 2 has it —
  the entry with `role: 'index'`) — check whether it matches the
  `documentId` in any "duplicate seq" error you capture.
- Roughly how long this session/tab has been open, and roughly how long
  the staleness has been building.
- Whether *only* the index doc looks stale, or whether specific file docs
  also seem to be missing peer updates.
- Whether you've had multiple tabs, windows, or devices open on the same
  project/account around the time this started — directly relevant if
  the "duplicate seq" mechanism holds up, since it would implicate two
  concurrent sessions writing under the same Automerge actor id.

### Step 7 — hand off

Send along whatever you managed to capture:
- The Step 2 (and, if done, Step 5's "after") JSON text.
- The Step 3 error capture(s) — these matter most.
- The Step 4 (and, if done, Step 5's "after") downloaded export file(s).
- The Step 6 notes.

Whatever's easiest — attach to a comment on strand bd-6f21d4c6, or hand
the files to a follow-up Claude Code session against this repo; either
way, referencing bd-6f21d4c6 keeps it linked to the research doc and the
two characterization tests already committed on
`explore/automerge-index-resync`.
