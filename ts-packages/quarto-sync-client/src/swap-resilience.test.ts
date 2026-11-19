/**
 * GE-7 test specification 8, sync-client part: offline clients, unconfirmed edits, and late joiners.
 *
 * The guest's network adapter is wrapped with a gate so a test can cut its traffic without touching the hub: with
 * `dropOutboundFor` set the guest's messages about one doc are dropped (its edits to that doc stay unconfirmed while
 * it still hears the owner and can load the new doc); with `outbound` set all its messages are dropped; with
 * `inbound` set the hub's messages are dropped as well, which is the closest in-process thing to being offline. The
 * window `online` event then forces the reconnect the browser would do on wake.
 *
 * What today's behavior already satisfied: a client that joins after a swap (the last test) needed no new code, because
 * a fresh connect reads the index as it is and subscribes to the current doc, so there is nothing for it to replace.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { NetworkAdapter, type Message, type PeerId, type PeerMetadata } from '@automerge/automerge-repo';
import type { DocumentId } from '@automerge/automerge-repo';

import { createSyncClient, setNetworkAdapterWrapper, type SyncClient } from './client.js';
import type { ReplacedEvent, SyncClientCallbacks } from './types.js';
import { startTestHub, type TestHub } from './test-hub.js';

interface Gate {
  outbound: boolean;
  inbound: boolean;
  /** When set, only outbound messages about this document are dropped (the rest of the traffic flows). */
  dropOutboundFor?: string;
}

class GatedAdapter extends NetworkAdapter {
  constructor(
    private wrapped: NetworkAdapter,
    private gate: Gate,
  ) {
    super();
    this.wrapped.on('message', (msg: Message) => {
      if (!this.gate.inbound) this.emit('message', msg);
    });
    this.wrapped.on('peer-candidate', (p) => this.emit('peer-candidate', p));
    this.wrapped.on('peer-disconnected', (p) => this.emit('peer-disconnected', p));
    this.wrapped.on('close', () => this.emit('close'));
  }
  isReady(): boolean {
    return this.wrapped.isReady();
  }
  whenReady(): Promise<void> {
    return this.wrapped.whenReady();
  }
  connect(peerId: PeerId, peerMetadata?: PeerMetadata): void {
    this.peerId = peerId;
    this.peerMetadata = peerMetadata;
    this.wrapped.connect(peerId, peerMetadata);
  }
  send(message: Message): void {
    const dropped =
      this.gate.outbound ||
      (this.gate.dropOutboundFor !== undefined &&
        (message as { documentId?: string }).documentId === this.gate.dropOutboundFor);
    if (!dropped) this.wrapped.send(message);
  }
  disconnect(): void {
    if (this.peerId) this.wrapped.disconnect();
  }
}

let hub: TestHub;
const liveClients: SyncClient[] = [];

beforeEach(async () => {
  hub = await startTestHub();
});

afterEach(async () => {
  setNetworkAdapterWrapper(null);
  vi.unstubAllGlobals();
  for (const c of liveClients.splice(0)) {
    await c.disconnect();
  }
  await hub.stop();
});

const ONLINE = {
  storage: 'memory' as const,
  peerTimeoutMs: 10000,
  requireOnline: true,
  findDocRetry: { attempts: 1, baseDelayMs: 10 },
  retryIntervalMs: 250,
};

async function createProject(text: string): Promise<string> {
  const owner = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onBinaryChanged: () => {},
    onFileRemoved: () => {},
  });
  const created = await owner.createNewProject({
    syncServer: hub.url,
    files: [{ path: 'index.qmd', content: text, contentType: 'text' }],
    ...ONLINE,
  });
  expect(await hub.hubHasHeadsOf(owner, 8000)).toBe(true);
  await owner.disconnect();
  return created.indexDocId;
}

function guestClient(replaced: ReplacedEvent[]): SyncClient {
  const callbacks: SyncClientCallbacks = {
    onFileAdded: () => {},
    onFileChanged: () => {},
    onBinaryChanged: () => {},
    onFileRemoved: () => {},
    onFileReplaced: (e) => replaced.push(e),
  };
  const c = createSyncClient(callbacks);
  liveClients.push(c);
  return c;
}

function makeDocOnHub(text: string): string {
  return hub.repo.create<{ text: string }>({ text }).documentId as string;
}

async function flip(indexDocId: string, docId: string): Promise<void> {
  const handle = await hub.repo.find<{ files: Record<string, string> }>(indexDocId as DocumentId);
  handle.change((d) => {
    d.files['index.qmd'] = docId;
  });
}

async function until(cond: () => boolean, timeoutMs = 10000, what = 'condition'): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!cond()) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 25));
  }
}

describe('stranded edits at swap start', () => {
  it('counts an edit the server never confirmed: the done event carries stranded of at least 1', async () => {
    const indexDocId = await createProject('A\n');
    const replaced: ReplacedEvent[] = [];
    const gate: Gate = { outbound: false, inbound: false };
    setNetworkAdapterWrapper((a) => new GatedAdapter(a, gate));
    const guest = guestClient(replaced);
    await guest.connect(hub.url, indexDocId, undefined, undefined, undefined, ONLINE);
    setNetworkAdapterWrapper(null);
    const oldHandle = guest.getFileHandle('index.qmd')!;

    // The guest can still hear the owner and load doc B, but nothing it writes to doc A reaches the hub.
    gate.dropOutboundFor = oldHandle.documentId as string;
    guest.updateFileContent('index.qmd', 'typed while cut off\n');
    expect(oldHandle.doc()!.text).toBe('typed while cut off\n');

    await flip(indexDocId, makeDocOnHub('B\n'));
    await until(() => replaced.some((e) => e.phase === 'done'), 10000, 'done');

    const done = replaced.find((e) => e.phase === 'done')!;
    expect(done.stranded ?? 0).toBeGreaterThanOrEqual(1);
    expect(guest.getFileHandle('index.qmd')!.doc()!.text).toBe('B\n');
  }, 30000);
});

describe('a client offline across two flips', () => {
  it('lands on the final doc only, and keeps its offline edit in the old doc', async () => {
    const indexDocId = await createProject('A\n');
    const replaced: ReplacedEvent[] = [];
    const gate: Gate = { outbound: false, inbound: false };
    const windowTarget = new EventTarget();
    vi.stubGlobal('window', windowTarget);
    setNetworkAdapterWrapper((a) => new GatedAdapter(a, gate));
    const guest = guestClient(replaced);
    await guest.connect(hub.url, indexDocId, undefined, undefined, undefined, ONLINE);
    setNetworkAdapterWrapper(null);
    const oldHandle = guest.getFileHandle('index.qmd')!;
    const oldDocId = oldHandle.documentId as string;

    gate.outbound = true;
    gate.inbound = true;
    guest.updateFileContent('index.qmd', 'edited offline\n', oldDocId);

    const docB = makeDocOnHub('B\n');
    const docC = makeDocOnHub('C\n');
    await flip(indexDocId, docB);
    await flip(indexDocId, docC);
    // Nothing reached the guest.
    expect(guest.getFileHandle('index.qmd')!.documentId).toBe(oldDocId);
    expect(replaced).toEqual([]);

    gate.outbound = false;
    gate.inbound = false;
    windowTarget.dispatchEvent(new Event('online'));

    await until(
      () => replaced.some((e) => e.phase === 'done' && e.newDocId === docC),
      15000,
      'done at C',
    );
    expect(guest.getFileHandle('index.qmd')!.doc()!.text).toBe('C\n');

    // A write against the old doc still reaches the old doc, and the offline edit is intact there.
    guest.updateFileContent('index.qmd', 'edited offline, again\n', oldDocId);
    expect(oldHandle.doc()!.text).toBe('edited offline, again\n');
    expect(guest.getFileHandle('index.qmd')!.doc()!.text).toBe('C\n');
  }, 40000);
});

describe('a client that joins after a swap', () => {
  it('loads the current doc and sees no replacement', async () => {
    const indexDocId = await createProject('A\n');
    const docB = makeDocOnHub('B\n');
    await flip(indexDocId, docB);

    const replaced: ReplacedEvent[] = [];
    const added: Array<{ docId?: string; text?: string }> = [];
    const c = createSyncClient({
      onFileAdded: (_p, f) => added.push({ docId: f.docId, text: f.type === 'text' ? f.text : undefined }),
      onFileChanged: () => {},
      onBinaryChanged: () => {},
      onFileRemoved: () => {},
      onFileReplaced: (e) => replaced.push(e),
    });
    liveClients.push(c);
    await c.connect(hub.url, indexDocId, undefined, undefined, undefined, ONLINE);

    expect(c.getFileHandle('index.qmd')!.documentId).toBe(docB);
    expect(added.at(-1)).toEqual({ docId: docB, text: 'B\n' });
    expect(replaced).toEqual([]);
  }, 30000);
});
