/**
 * The moved message (G25, GE-9 spec 7): sent as an ephemeral message on the old doc,
 * shown through `onMoved`; a missing or malformed message is ignored.
 */
import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { Presence, type Identity } from './presence';
import { startHub, waitFor } from './test-support';
import { ensureWasm } from './wasm';

let hub: TestHub;
const repos: Repo[] = [];
const presences: Presence[] = [];
beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
afterAll(async () => {
  await hub.stop();
});
afterEach(async () => {
  presences.splice(0).forEach((p) => p.dispose());
  for (const r of repos.splice(0)) await r.shutdown().catch(() => {});
});

const alice: Identity = { userId: 'a', userName: 'Alice', userColor: '#111111' };
const bob: Identity = { userId: 'b', userName: 'Bob', userColor: '#222222' };

async function twoClients() {
  const a = new Repo({ network: [new BrowserWebSocketClientAdapter(hub.url)] });
  const b = new Repo({ network: [new BrowserWebSocketClientAdapter(hub.url)] });
  repos.push(a, b);
  const ha = a.create<{ text: string }>({ text: 'hello' });
  expect(await hub.hubHasHeads(ha.documentId, ha.heads() as string[], 15_000)).toBe(true);
  const hb = (await b.find<{ text: string }>(ha.url)) as DocHandle<{ text: string }>;
  await hb.whenReady();
  return { ha, hb };
}

describe('moved message', () => {
  it('names the sender when its presence is known, and is anonymous otherwise', async () => {
    const { ha, hb } = await twoClients();
    const seen: (string | undefined)[] = [];
    const pa = new Presence(ha, alice, () => {});
    const pb = new Presence(hb, bob, () => {}, (name) => seen.push(name));
    presences.push(pa, pb);
    // Anonymous: Alice's presence has not reached Bob yet.
    pa.sendMoved();
    await waitFor(() => seen.length === 1, 'the first moved message');
    expect(seen[0]).toBeUndefined();
    // Named: after a presence message, the name is known.
    pa.update(0, null);
    await new Promise((r) => setTimeout(r, 300));
    pa.sendMoved();
    await waitFor(() => seen.length === 2, 'the second moved message');
    expect(seen[1]).toBe('Alice');
  });

  it('ignores a missing or malformed message', async () => {
    const { ha, hb } = await twoClients();
    const seen: (string | undefined)[] = [];
    presences.push(new Presence(hb, bob, () => {}, (name) => seen.push(name)));
    ha.broadcast({ type: 'moved' }); // no peerId
    ha.broadcast({ type: 'moved', peerId: 42 }); // wrong type
    ha.broadcast(null);
    ha.broadcast('moved');
    ha.broadcast({ type: 'moved', peerId: 'ok-peer' }); // well formed: this one is shown
    await waitFor(() => seen.length >= 1, 'the well-formed message');
    await new Promise((r) => setTimeout(r, 300));
    expect(seen).toHaveLength(1);
  });
});
