/**
 * setNetworkAdapterWrapper — module-level injection point (same pattern
 * as setSyncLogger) letting a host application wrap the sync client's
 * network adapter at Repo construction, e.g. hub-client's debug
 * message tap (bd-6ogrov5r; plan:
 * claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md).
 *
 * Contract: when set, the wrapper is applied to the freshly built
 * websocket adapter on every subsequent connect()/createNewProject();
 * traffic flows through the wrapped adapter unchanged (sync still
 * works). Clearing with null stops wrapping future connections.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import {
  NetworkAdapter,
  type Message,
  type PeerId,
  type PeerMetadata,
} from '@automerge/automerge-repo';

import {
  createSyncClient,
  setNetworkAdapterWrapper,
  type SyncClient,
} from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';

/** Minimal fully-forwarding wrapper that records message types. */
class RecordingAdapter extends NetworkAdapter {
  constructor(
    private wrapped: NetworkAdapter,
    private record: { direction: 'incoming' | 'outgoing'; type: string }[],
  ) {
    super();
    this.wrapped.on('message', (msg: Message) => {
      this.record.push({ direction: 'incoming', type: msg.type });
      this.emit('message', msg);
    });
    this.wrapped.on('peer-candidate', (p) => this.emit('peer-candidate', p));
    this.wrapped.on('peer-disconnected', (p) =>
      this.emit('peer-disconnected', p),
    );
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
    this.record.push({ direction: 'outgoing', type: message.type });
    this.wrapped.send(message);
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
  for (const c of liveClients.splice(0)) {
    await c.disconnect();
  }
  await hub.stop();
});

function client(): SyncClient {
  const c = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
  liveClients.push(c);
  return c;
}

describe('setNetworkAdapterWrapper', () => {
  it('wraps the adapter on createNewProject and connect; sync still works', async () => {
    const record: { direction: 'incoming' | 'outgoing'; type: string }[] = [];
    setNetworkAdapterWrapper((adapter) => new RecordingAdapter(adapter, record));

    const creator = client();
    const result = await creator.createNewProject({
      syncServer: hub.url,
      files: [{ path: 'main.qmd', content: 'tap test\n', contentType: 'text' }],
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
    });
    // Wait for the hub to hold every doc before the creator disconnects
    // (same discipline as sync-diagnostics.test.ts — creation syncs in
    // the background).
    expect(await hub.hubHasDoc(result.indexDocId, 8000)).toBe(true);
    for (const f of result.files) {
      expect(await hub.hubHasDoc(f.docId, 8000)).toBe(true);
    }
    await creator.disconnect();

    // Creation traffic went through the wrapper.
    const createTraffic = record.length;
    expect(createTraffic).toBeGreaterThan(0);
    expect(record.some((m) => m.direction === 'outgoing')).toBe(true);
    expect(record.some((m) => m.direction === 'incoming')).toBe(true);

    // A reader connect is wrapped too, and the file syncs through it.
    const reader = client();
    await reader.connect(
      hub.url,
      result.indexDocId,
      undefined,
      undefined,
      undefined,
      { storage: 'memory', peerTimeoutMs: 10000, requireOnline: true },
    );
    expect(reader.getFileContent('main.qmd')).toBe('tap test\n');
    expect(record.length).toBeGreaterThan(createTraffic);
    expect(record.some((m) => m.type === 'sync')).toBe(true);
  });

  it('clearing the wrapper stops wrapping subsequent connections', async () => {
    const record: { direction: 'incoming' | 'outgoing'; type: string }[] = [];
    setNetworkAdapterWrapper((adapter) => new RecordingAdapter(adapter, record));
    setNetworkAdapterWrapper(null);

    const creator = client();
    const result = await creator.createNewProject({
      syncServer: hub.url,
      files: [{ path: 'main.qmd', content: 'no tap\n', contentType: 'text' }],
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
    });
    expect(await hub.hubHasDoc(result.indexDocId, 8000)).toBe(true);
    expect(record).toEqual([]);
  });
});
