/**
 * Tests for ReadOnlyStorageAdapter.
 *
 * The adapter wraps an underlying StorageAdapterInterface and silently drops
 * all writes. Reads pass through unchanged. The intent is to let the debug
 * page mount a Repo against the same IndexedDB the main app writes to
 * (shared same-origin) WITHOUT risking concurrent writes that could be a
 * source of the bugs we're investigating.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { StorageAdapterInterface } from '@automerge/automerge-repo/slim'
import { ReadOnlyStorageAdapter } from './ReadOnlyStorageAdapter'

function makeStub() {
  return {
    load: vi.fn(async () => new Uint8Array([1, 2, 3])),
    save: vi.fn(async () => {}),
    remove: vi.fn(async () => {}),
    loadRange: vi.fn(async () => [
      { key: ['doc-1', 'snapshot'], data: new Uint8Array([9]) },
    ]),
    removeRange: vi.fn(async () => {}),
  } satisfies StorageAdapterInterface
}

describe('ReadOnlyStorageAdapter', () => {
  let inner: ReturnType<typeof makeStub>
  let adapter: ReadOnlyStorageAdapter

  beforeEach(() => {
    inner = makeStub()
    adapter = new ReadOnlyStorageAdapter(inner)
  })

  it('forwards load() to the wrapped adapter', async () => {
    const result = await adapter.load(['doc-1', 'snapshot'])
    expect(inner.load).toHaveBeenCalledWith(['doc-1', 'snapshot'])
    expect(result).toEqual(new Uint8Array([1, 2, 3]))
  })

  it('forwards loadRange() to the wrapped adapter', async () => {
    const result = await adapter.loadRange(['doc-1'])
    expect(inner.loadRange).toHaveBeenCalledWith(['doc-1'])
    expect(result).toHaveLength(1)
  })

  it('silently drops save()', async () => {
    await adapter.save(['doc-1', 'snapshot'], new Uint8Array([4, 5]))
    expect(inner.save).not.toHaveBeenCalled()
  })

  it('silently drops remove()', async () => {
    await adapter.remove(['doc-1'])
    expect(inner.remove).not.toHaveBeenCalled()
  })

  it('silently drops removeRange()', async () => {
    await adapter.removeRange(['doc-1'])
    expect(inner.removeRange).not.toHaveBeenCalled()
  })
})
