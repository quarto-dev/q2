# Capturing a real specimen of the index-document staleness bug (Carlos, on his own machine)

## Overview

Context: `claude-notes/research/2026-09-16-automerge-index-doc-staleness.md`,
strand bd-6f21d4c6. The fact that defines this bug, from the start: **a
plain page reload does not fix a stuck session; only clearing the
`automerge` IndexedDB database does.** Every hypothesis is judged against
that fact.

Two mechanisms (H1: a peer-registration race in
`CollectionSynchronizer.addPeer`; H2: a per-storageId sync-state
persistence throttle) were confirmed as *real, manufacturable* behaviors
against automerge-repo v2.5.6 — but both were source-reading and
synthetic repro, and neither actually explains the reload/wipe fact (see
"What this changes" below). A real production error Carlos posted on
2026-09-17 does explain it, directly and completely: **H4**, a
`RangeError: duplicate seq N found for actor <id>` thrown inside
automerge-wasm's `receiveSyncMessage`, silently swallowed by a bare
`console.log` in automerge-repo's `Repo.ts` with no retry, no recovery.
Traced against source: this aborts the peer's sync-state bookkeeping for
that document *before* it updates, and does so from a state (the local
document's own committed change history) that lives in IndexedDB, not
in memory — so a reload reloads the identical poison and fails
identically, and only wiping storage removes it. This plan leads with
confirming or falsifying H4 against a real specimen; H1 and H2 capture
is kept, but demoted to secondary.

Written to be run entirely by Carlos, in Chrome, on his own machine.
Nothing here requires code changes; it's an evidence-capture runbook,
and it is **entirely read-only / non-destructive** — it does not tell
Carlos to apply the known IndexedDB-clear workaround. He already knows
how to do that and is deliberately not doing it yet, because clearing
storage would destroy the only reproduction we have.

## What this new lead changes about H1 and H2

**It doesn't disprove either as real behaviors** — both are still
confirmed, source-grounded mechanisms that genuinely exist in
automerge-repo v2.5.6 (see the characterization tests already committed
on this branch). What changes is whether either is *needed* to explain
Carlos's actual incident:

- **H1** was already demoted before H4 came along: its state
  (`CollectionSynchronizer.#peers` / each `DocSynchronizer`'s peer list)
  is pure in-memory bookkeeping that a page reload provably resets
  (hub-client runs its `Repo` directly in the page — no
  `SharedWorker`/`ServiceWorker` to survive a reload). Reload-alone-
  insufficient already ruled out H1 as a *sufficient* standalone cause.
  H4 doesn't need H1 at all — it's independently sufficient, and (unlike
  H1) explains *why* reload doesn't help, rather than just being
  compatible with reload not mattering to it.
- **H2** was always framed as a compounding factor, not a standalone
  cause, because a dropped *persisted* sync-state write doesn't touch
  the live in-memory sync state that actually drives message exchange —
  the protocol should still converge given continued messages. H4
  provides its own complete, self-sufficient account of a permanent
  wedge, so H2 isn't needed to explain why this is durable either.

**Net effect:** H1 and H2 remain true statements about the library, but
neither is the leading explanation for *this* bug anymore. This plan
treats them as secondary, cheap-to-capture-anyway evidence, not the
primary target. If H4 gets falsified (see the active test in Step 2
below), H1/H2 come back into consideration and the secondary captures
become primary again.

## Checklist

- [ ] **Step 0 (one-time setup):** enable the in-app debug API and
      "Preserve log" in DevTools.
- [ ] **Step 1 (H4, top priority):** export the current persisted
      IndexedDB bytes *right now* — the specimen doesn't need to wait
      for the error to fire again; if Carlos's session is still stuck,
      the poisoned document is already sitting in storage.
- [ ] **Step 2 (H4, top priority — an active test, not passive
      waiting):** ask a collaborator to make any small edit to the
      affected document right now, while watching the console, to try
      to force the same error to fire again on demand.
- [ ] **Step 3 (H4):** capture the full, untruncated "duplicate seq"
      error object, whenever it fires (from Step 2, or organically).
- [ ] **Step 4 (secondary — H1/H2, cheap, worth doing anyway):** capture
      the live in-memory snapshot via `quartoDebug.am`.
- [ ] **Step 5 (secondary, optional, skip freely):** a matched
      before/after plain-reload pair.
- [ ] **Step 6:** jot down the context notes below.
- [ ] **Step 7:** hand off the captured files + notes.

## Details

### Step 0 — enable the debug API and console log persistence

`window.quartoDebug` (bd-q93tkglb) is gated by
`import.meta.env.DEV || localStorage.quartoDebug === '1'`, checked
**once, on page load**. Turn it on now:

```js
localStorage.setItem('quartoDebug', '1');
location.reload();
```

Confirm after reload: `typeof quartoDebug` should be `'object'`.

Also enable **"Preserve log"** in the DevTools Console panel (checkbox
at the top of the Console tab) — without it, Chrome clears the console
on every reload, so an error that fired earlier is gone the moment
anything reloads the page. Both settings are harmless and can stay on
indefinitely.

### Step 1 — export the persisted bytes now (H4's decisive evidence)

Do this **before** anything else, regardless of whether the console
error is currently visible. H4's claim is that the poison lives in the
document's own committed change history, on disk — if Carlos's session
is still stuck, that history is sitting in IndexedDB right now, whether
or not the error happens to be freshly logged in the console at this
exact moment.

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
"incremental" | "sync-state", ...]` key and its bytes (base64, lossless).
This is the file we need to actually **test** H4: decoding it lets us
check, directly, whether the affected document's local change graph
really does contain two different changes claiming the same
`(actor, seq)` pair — the specific, falsifiable claim H4 makes. Nobody
needs to do the decoding live in the browser; just get this file to us.

### Step 2 — try to trigger the error again, on demand

This is the sharpest test available: if H4 is right, the hub has no
reason to stop resending the colliding change, so *any* further edit
that someone else makes to the same document should provoke the hub to
push a fresh sync message toward Carlos's stuck client — and, per the
mechanism, that should reproduce the identical "duplicate seq" error
again almost immediately, rather than requiring an unpredictable wait.

1. With Carlos's stuck tab open, DevTools Console visible, "Preserve
   log" on (Step 0).
2. Have a collaborator (or Carlos himself, from a different
   already-working session/device) make **any** small edit to the
   specific document that's showing the Q-13-4 squiggle — e.g. add a
   character to a meeting-notes line, or rename a file if the index doc
   itself is the affected one.
3. Watch Carlos's console for a few seconds to a minute after that edit
   lands. If "error receiving message" / "duplicate seq" fires again,
   that's a strong, fast, direct confirmation of H4's proposed
   infinite-retry loop — and this reproduces the error **on demand**,
   which nothing else in this investigation has managed yet.
4. If nothing fires after a couple of tries with different edits, that
   doesn't fully kill H4 (the hub's resend behavior might not be
   triggered by every kind of edit, or might already be gated by
   something), but it's a real data point against it, and would be
   worth noting explicitly — see the context notes in Step 6.

### Step 3 — capture the full "duplicate seq" error object

Whether it fires from Step 2's active test or shows up organically, the
`message` in that log line is a full sync message —
`{ type: 'sync', documentId, senderId, targetId, data }`. The console
shows the full object when you expand it:

1. Expand the logged object fully (click the `▶` next to it, or the
   `{…}` for `message`) so `documentId`, `senderId`, `targetId`, and
   `data.byteLength` are all visible.
2. Right-click the top-level logged object → **"Copy object"**.
3. Note the exact number in "duplicate seq N" and the exact actor id
   string by hand too, even though the copy should already have them —
   worth double-checking.

### Step 4 — secondary: live in-memory state (H1/H2 checks)

Cheap to capture alongside everything else, kept for completeness even
though H1/H2 are demoted:

```js
copy(JSON.stringify({
  capturedAt: new Date().toISOString(),
  repos: quartoDebug.am.repos(),        // peerId + connectedPeers[] per repo — the H1 check
  docs: quartoDebug.am.docs(),          // per-doc heads/handleState, including the index doc
  syncStatus: quartoDebug.am.syncStatus(),
  doctor: quartoDebug.am.doctor(),
  messages: quartoDebug.am.messages(),  // sync-protocol traffic ring buffer, only useful if the tap attached before this session got stuck
}, null, 2))
```

`copy(...)` puts it on the clipboard. `repos()[*].connectedPeers` is the
H1 check — if the hub's peerId is listed there while the doc isn't
advancing, that's H1's signature; on its own (per "What this changes"
above) it's no longer expected to be the explanation, but worth
recording regardless.

### Step 5 — optional, skip freely: matched before/after reload

Not needed to test H4 (H4 already predicts and explains reload not
helping). Only useful now as a leftover H1-specific check. Skip unless
Carlos wants to do it anyway: repeat Step 4 after one plain reload (no
storage clearing), naming the copy `...-after-reload`.

### Step 6 — context notes to jot down alongside the captures

- The project and the specific file(s) showing the Q-13-4 squiggle.
- The index doc id (`quartoDebug.am.docs()`'s `role: 'index'` entry) —
  check whether it matches the `documentId` in any captured error.
- Roughly how long this session/tab has been open, and how long the
  staleness has been building.
- Whether *only* the index doc looks stale, or file docs too.
- Whether Carlos has had multiple tabs, windows, or devices open on the
  same project/account around when this started — directly relevant to
  H4's still-unconfirmed sub-question (whether `actorIdFromUserId`'s
  deliberately-stable actor id is what let two sessions collide).
- The outcome of Step 2's active test — fired again or not, and after
  how many attempts / what kind of edit.

### Step 7 — hand off

Send along whatever was captured:
- The Step 1 export file (matters most — it's what lets us actually
  test H4's claim).
- The Step 3 error capture(s) — second most important.
- The Step 4 (and, if done, Step 5) JSON text.
- The Step 6 notes.

Attach to a comment on strand bd-6f21d4c6, or hand the files to a
follow-up Claude Code session against this repo — either way,
referencing bd-6f21d4c6 keeps it linked to the research doc and the
characterization tests already committed on
`explore/automerge-index-resync`.
