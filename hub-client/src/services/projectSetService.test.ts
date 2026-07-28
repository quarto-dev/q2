import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => {
  class FakeHandle {
    documentId = 'offline-root-doc';
    currentDoc: Record<string, unknown> | undefined = {
      projects: {},
      version: 1,
    };
    doc = vi.fn(() => this.currentDoc);
    whenReady = vi.fn(async () => {});
    on = vi.fn();
    off = vi.fn();
    change = vi.fn();
  }

  class FakeRepo {
    static instances: FakeRepo[] = [];
    static flushError: Error | undefined;
    static importError: Error | undefined;

    handle = new FakeHandle();
    flush = vi.fn(async () => {});
    import = vi.fn(() => {
      if (FakeRepo.importError) throw FakeRepo.importError;
      return this.handle;
    });
    find = vi.fn(async () => this.handle);
    peerHandler: (() => void) | undefined;
    networkSubsystem = {
      on: vi.fn((_event: string, handler: () => void) => {
        this.peerHandler = handler;
      }),
    };

    constructor() {
      if (FakeRepo.flushError) {
        this.flush.mockRejectedValue(FakeRepo.flushError);
      }
      FakeRepo.instances.push(this);
    }
  }

  class FakeWebSocketAdapter {
    disconnect = vi.fn();
  }

  return {
    FakeRepo,
    FakeWebSocketAdapter,
    automergeFrom: vi.fn((initial: Record<string, unknown>) => initial),
    automergeSerialize: vi.fn(() => new Uint8Array([1, 2, 3])),
  };
});

vi.mock('@automerge/automerge-repo', () => ({
  Repo: mocks.FakeRepo,
}));

vi.mock('@automerge/automerge-repo-network-websocket', () => ({
  BrowserWebSocketClientAdapter: mocks.FakeWebSocketAdapter,
}));

vi.mock('@automerge/automerge-repo-storage-indexeddb', () => ({
  IndexedDBStorageAdapter: class {},
}));

vi.mock('@automerge/automerge', () => ({
  from: mocks.automergeFrom,
  save: mocks.automergeSerialize,
}));

import * as projectSetService from './projectSetService';

describe('projectSetService creation policy', () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mocks.FakeRepo.instances.length = 0;
    mocks.FakeRepo.flushError = undefined;
    mocks.FakeRepo.importError = undefined;
    await projectSetService.disconnect();
  });

  afterEach(async () => {
    await projectSetService.disconnect();
    vi.useRealTimers();
  });

  it('creates and flushes a personal root without waiting for a server peer', async () => {
    const onConnectionChange = vi.fn();
    projectSetService.setProjectSetHandlers({ onConnectionChange });
    const created = projectSetService.createProjectSet(
      'wss://offline.example/ws',
      'My projects',
    );

    await expect(created).resolves.toBe('offline-root-doc');
    const repo = mocks.FakeRepo.instances[0];
    expect(repo.flush).toHaveBeenCalledWith(['offline-root-doc']);
    expect(mocks.automergeFrom).toHaveBeenCalledWith(
      expect.objectContaining({ name: 'My projects', projects: {} }),
    );
    expect(onConnectionChange).toHaveBeenLastCalledWith(false);

    repo.peerHandler?.();
    await Promise.resolve();
    expect(onConnectionChange).toHaveBeenLastCalledWith(true);
  });

  it('surfaces a local storage flush failure for a personal root', async () => {
    const error = new Error('IndexedDB write failed');
    mocks.FakeRepo.flushError = error;
    const creation = projectSetService.createProjectSet(
      'wss://offline.example/ws',
    );

    await expect(creation).rejects.toThrow('IndexedDB write failed');
    expect(projectSetService.isConnected()).toBe(false);
  });

  it('surfaces a local document import failure for a personal root', async () => {
    mocks.FakeRepo.importError = new Error('Automerge import failed');

    await expect(
      projectSetService.createProjectSet('wss://offline.example/ws'),
    ).rejects.toThrow('Automerge import failed');
    expect(projectSetService.isConnected()).toBe(false);
  });

  it('keeps ordinary shared collection creation server-gated', async () => {
    const creation = projectSetService.createCollection(
      'wss://offline.example/ws',
      'Shared collection',
    );

    let settled = false;
    void creation.then(
      () => {
        settled = true;
      },
      () => {
        settled = true;
      },
    );
    await Promise.resolve();
    expect(settled).toBe(false);

    await vi.advanceTimersByTimeAsync(10000);
    await expect(creation).rejects.toThrow(
      'Could not reach sync server. Please check your connection and try again.',
    );
    expect(mocks.FakeRepo.instances[0].import).not.toHaveBeenCalled();
  });

  it('still rejects an uncached existing collection while offline', async () => {
    const connection = projectSetService.connectCollection({
      projectSetDocId: 'uncached-doc',
      syncServer: 'wss://offline.example/ws',
    });
    const repo = mocks.FakeRepo.instances[0];
    repo.handle.currentDoc = undefined;

    await Promise.resolve();
    const rejection = expect(connection).rejects.toThrow(
      'Collection not found in local storage. Connect online first to sync.',
    );
    await vi.advanceTimersByTimeAsync(5000);
    await rejection;
  });
});
