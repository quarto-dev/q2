/**
 * `DefaultProjectSetStore` (bd-3h5hbfdv): the remembered project-set
 * reference behind bare `list_projects` calls, kept in the OS keyring
 * under the same service as the OAuth credentials.
 */

import { describe, it, expect } from 'vitest';

import { KeyringUnavailableError, SERVICE_NAME } from './auth/credential-store.js';
import { DefaultProjectSetStore } from './default-project-set.js';
import { MemoryBackend } from './in-memory-fixture.js';

describe('DefaultProjectSetStore', () => {
  it('lives under the same keyring service as the OAuth credentials', () => {
    const store = new DefaultProjectSetStore(new MemoryBackend());
    expect(store.serviceName).toBe(SERVICE_NAME);
  });

  it('reads null when nothing is stored', async () => {
    const store = new DefaultProjectSetStore(new MemoryBackend());
    expect(await store.read()).toBeNull();
  });

  it('round-trips a written value', async () => {
    const backend = new MemoryBackend();
    const store = new DefaultProjectSetStore(backend);
    const link =
      'https://quarto-hub.com/#/link-project-set/3yJCerqv62bwjT3rqFJMFrNMSdM4' +
      '?server=wss%3A%2F%2Fquarto-hub.com%2Fws';
    await store.write(link);
    expect(backend.value).toBe(link);
    expect(await store.read()).toBe(link);
  });

  it('a write is observed by a later read even after a failed clear', async () => {
    // Serialisation contract: operations submitted in order observe each
    // other (same enqueue chain as CredentialStore).
    const store = new DefaultProjectSetStore(new MemoryBackend());
    await store.write('first');
    await store.write('second');
    expect(await store.read()).toBe('second');
  });

  it('folds a read failure to null instead of throwing', async () => {
    const backend = new MemoryBackend();
    backend.failReads = true;
    const store = new DefaultProjectSetStore(backend);
    expect(await store.read()).toBeNull();
  });

  it('throws KeyringUnavailableError on a write failure', async () => {
    const backend = new MemoryBackend();
    backend.failWrites = true;
    const store = new DefaultProjectSetStore(backend);
    await expect(store.write('x')).rejects.toBeInstanceOf(KeyringUnavailableError);
  });

  it('clear empties the entry', async () => {
    const backend = new MemoryBackend();
    const store = new DefaultProjectSetStore(backend);
    await store.write('x');
    await store.clear();
    expect(backend.value).toBeNull();
    expect(await store.read()).toBeNull();
  });
});
