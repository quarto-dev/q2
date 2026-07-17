/**
 * createNewProject must honor the resolveActorId three-valued contract
 * (see authService.resolveActorId in hub-client):
 *   string    → actor resolved; author under it
 *   undefined → auth disabled; proceed with no actor
 *   null      → auth failure; ABORT the creation
 *
 * Previously null was swallowed (`?? undefined`), silently creating a
 * hub-wired project with no session — the app then tore it down on the
 * next render (a "flash" of the editor before bouncing to the selector).
 *
 * No mocks: the "server" is a port nothing listens on (offline fallback),
 * so the actor step is reached exactly as in the browser.
 */

import { describe, it, expect } from 'vitest';

import { createSyncClient, ActorAuthRequiredError } from './client.js';

// Port 9 (discard) on localhost: connection refused immediately.
const UNREACHABLE = 'ws://127.0.0.1:9/ws';

function client() {
  return createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
}

describe('createNewProject + resolveActorId contract', () => {
  it('rejects with ActorAuthRequiredError when resolveActorId returns null', async () => {
    const c = client();
    await expect(
      c.createNewProject(
        {
          syncServer: UNREACHABLE,
          files: [{ path: 'a.qmd', content: 'x', contentType: 'text' }],
          storage: 'memory',
          peerTimeoutMs: 50,
        },
        undefined,
        'Someone',
        '#123456',
        async () => null,
      ),
    ).rejects.toBeInstanceOf(ActorAuthRequiredError);
    // Nothing was created: the client holds no readable project state.
    expect(c.getFileContent('a.qmd')).toBeNull();
  });

  it('proceeds with no actor when resolveActorId returns undefined (auth disabled)', async () => {
    const c = client();
    const result = await c.createNewProject(
      {
        syncServer: UNREACHABLE,
        files: [{ path: 'a.qmd', content: 'x', contentType: 'text' }],
        storage: 'memory',
        peerTimeoutMs: 50,
      },
      undefined,
      'Someone',
      '#123456',
      async () => undefined,
    );
    expect(result.indexDocId).toBeTruthy();
    await c.disconnect();
  });

  it('authors under the actor when resolveActorId returns a string', async () => {
    const c = client();
    const actor = 'aa'.repeat(16);
    await c.createNewProject(
      {
        syncServer: UNREACHABLE,
        files: [{ path: 'a.qmd', content: 'x', contentType: 'text' }],
        storage: 'memory',
        peerTimeoutMs: 50,
      },
      undefined,
      'Someone',
      '#123456',
      async () => actor,
    );
    expect(c.getActorId()).toBe(actor);
    await c.disconnect();
  });
});
