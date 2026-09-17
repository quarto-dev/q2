# Capturing a real specimen for H4 (Carlos, on his own machine)

## Overview

Context: `claude-notes/research/2026-09-16-automerge-index-doc-staleness.md`
§ H4, strand bd-6f21d4c6. Leading candidate: a `RangeError: duplicate
seq N found for actor <id>` thrown inside automerge-wasm's
`receiveSyncMessage`, silently swallowed by a bare `console.log` in
automerge-repo's `Repo.ts` with no retry — a permanent, silent stall
that survives reload because the poison is baked into the locally
persisted document's own change history in IndexedDB. (Why H1/H2 are no
longer the focus: see the research doc.)

This is a short, manual, read-only runbook — nothing here requires code
changes, and it does **not** tell Carlos to apply the known
IndexedDB-clear fix. He already knows how and is deliberately not doing
it yet, so the reproduction survives.

## Checklist

- [ ] **Step 0:** turn on "Preserve log" in DevTools.
- [ ] **Step 1 (do this first):** export the persisted IndexedDB bytes.
- [ ] **Step 2:** try to trigger the error again, on demand.
- [ ] **Step 3:** capture the full error object.
- [ ] **Step 4:** jot down a few notes.
- [ ] **Step 5:** hand off.

## Details

### Step 0 — Preserve log

DevTools → Console panel → check **"Preserve log"** (top of the panel).
Without it, Chrome clears the console on every reload, so an
intermittent error you're not actively watching for gets lost.

### Step 1 — export the persisted bytes now

Do this first, regardless of whether the console error is currently
visible — the poison is a committed change already sitting in
IndexedDB, not something that needs to be caught live. Paste into the
console on the stuck tab:

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
"incremental" | "sync-state", ...]` key and its bytes (base64,
lossless). This is what lets us check whether the affected document's
local change graph really does contain two different changes claiming
the same `(actor, seq)` — the specific claim H4 makes. No need to
decode it yourself; just send us the file.

### Step 2 — try to trigger the error again, on demand

If H4 is right, the hub has no reason to stop resending the colliding
change, so any further edit someone else makes to the same document
should provoke a fresh sync push toward Carlos's stuck client — and
reproduce the same error again almost immediately, rather than
requiring an unpredictable wait.

1. Keep the stuck tab open, Console visible, Preserve log on.
2. Have a collaborator (or Carlos from a different device/session) make
   any small edit to the document showing the Q-13-4 squiggle.
3. Watch the console for a bit after that edit lands. If "error
   receiving message" / "duplicate seq" fires again, that's a fast,
   direct, on-demand confirmation.
4. If nothing fires after a couple of tries, note that too — it's a
   real data point, not a dead end.

### Step 3 — capture the full error object

The `message` in that log line is a full sync message —
`{ type: 'sync', documentId, senderId, targetId, data }` — worth far
more than the truncated console text:

1. Expand the logged object fully (the `▶`/`{…}` next to it) so
   `documentId`, `senderId`, `targetId`, and `data.byteLength` are all
   visible.
2. Right-click the top-level logged object → **"Copy object"**.
3. Note the exact number in "duplicate seq N" and the exact actor id
   string by hand too, even though the copy should already have them.

### Step 4 — a few notes

- The project and file(s) showing the Q-13-4 squiggle, and (if known)
  the index doc id.
- How long this session/tab has been open, and roughly how long the
  staleness has been building.
- Whether Carlos has had multiple tabs, windows, or devices open on the
  same project/account around when this started — directly relevant to
  H4's open sub-question (whether a reused, session-stable actor id is
  what let two sessions collide).
- Step 2's outcome — fired again or not, after how many attempts.

### Step 5 — hand off

Send along: the Step 1 export file (matters most), the Step 3 error
capture (second most important), and the Step 4 notes. Attach to a
comment on strand bd-6f21d4c6, or hand the files to a follow-up Claude
Code session against this repo.
