/**
 * Phase 5 — credential-store unit tests.
 *
 * Default backend wraps `@napi-rs/keyring`; tests inject an in-memory
 * or failing backend so they never touch the platform keyring. The
 * `[integration]` describe is gated on `KEYRING_INTEGRATION=1` and
 * exercises the real per-platform backend.
 */

import { describe, it, expect, vi } from 'vitest';

import {
  CredentialStore,
  KeyringUnavailableError,
  SERVICE_NAME,
  defaultKeyringBackend,
  type CredentialBundle,
  type CredentialStoreConfig,
  type KeyringBackend,
} from './credential-store.js';

const ISSUER = 'https://accounts.google.com';
const CLIENT_ID = 'test-client.apps.googleusercontent.com';
const OTHER_CLIENT_ID = 'other-client.apps.googleusercontent.com';

const CFG: CredentialStoreConfig = { issuer: ISSUER, clientId: CLIENT_ID };

const SAMPLE: CredentialBundle = {
  idToken:
    'eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJzYW1wbGUifQ.signature-bytes',
  refreshToken: '1//0abcDEF-1234_xyz-sample-refresh-token',
  idTokenExpiresAt: new Date('2030-01-01T00:00:00.000Z'),
  scopes: ['openid', 'email', 'profile'],
};

// ---------------------------------------------------------------------------
// In-memory backends
// ---------------------------------------------------------------------------

interface CallLog {
  reads: number;
  writes: string[];
  clears: number;
}

function memoryBackend(initial: string | null = null): {
  backend: KeyringBackend;
  state: { value: string | null };
  log: CallLog;
} {
  const state = { value: initial };
  const log: CallLog = { reads: 0, writes: [], clears: 0 };
  const backend: KeyringBackend = {
    async read() {
      log.reads += 1;
      return state.value;
    },
    async write(v: string) {
      log.writes.push(v);
      state.value = v;
    },
    async clear() {
      log.clears += 1;
      const existed = state.value !== null;
      state.value = null;
      return existed;
    },
  };
  return { backend, state, log };
}

function unavailableBackend(): KeyringBackend {
  // Mimics the hwchen/keyring-rs / libsecret message shape so the
  // module's substring match has something realistic to land on.
  const msg =
    'Platform secure storage failure: D-Bus error: failed to connect to Secret Service (libsecret)';
  return {
    async read() {
      throw new Error(msg);
    },
    async write() {
      throw new Error(msg);
    },
    async clear() {
      throw new Error(msg);
    },
  };
}

function failingBackend(message: string): KeyringBackend {
  return {
    async read() {
      throw new Error(message);
    },
    async write() {
      throw new Error(message);
    },
    async clear() {
      throw new Error(message);
    },
  };
}

// ---------------------------------------------------------------------------
// Console-sink helpers
// ---------------------------------------------------------------------------

function captureConsole(): {
  blob: () => string;
  restore: () => void;
} {
  const sinks = (['debug', 'log', 'info', 'warn', 'error'] as const).map((m) =>
    vi.spyOn(console, m).mockImplementation(() => undefined)
  );
  return {
    blob: () =>
      sinks
        .flatMap((s) => s.mock.calls.flat())
        .map((c) => (typeof c === 'string' ? c : JSON.stringify(c)))
        .join(' '),
    restore: () => sinks.forEach((s) => s.mockRestore()),
  };
}

// ---------------------------------------------------------------------------
// Naming + scoping
// ---------------------------------------------------------------------------

describe('CredentialStore — naming', () => {
  it("locks service name to 'dev.quarto.hub-mcp'", () => {
    expect(SERVICE_NAME).toBe('dev.quarto.hub-mcp');
  });

  it("derives account from '<issuer>:<client_id>'", () => {
    const { backend } = memoryBackend();
    const store = new CredentialStore(CFG, backend);
    expect(store.serviceName).toBe('dev.quarto.hub-mcp');
    expect(store.accountName).toBe(`${ISSUER}:${CLIENT_ID}`);
  });
});

// ---------------------------------------------------------------------------
// read returns null on absence / corruption
// ---------------------------------------------------------------------------

describe('CredentialStore.read', () => {
  it('returns null when the keyring entry is absent', async () => {
    const { backend } = memoryBackend(null);
    const store = new CredentialStore(CFG, backend);
    expect(await store.read()).toBeNull();
  });

  it('returns null on corrupt JSON without throwing', async () => {
    const { backend } = memoryBackend('{not valid json');
    const store = new CredentialStore(CFG, backend);
    expect(await store.read()).toBeNull();
  });

  it('returns null on schema_version mismatch without throwing', async () => {
    const blob = JSON.stringify({
      schema_version: 999,
      issuer: ISSUER,
      client_id: CLIENT_ID,
      id_token: SAMPLE.idToken,
      refresh_token: SAMPLE.refreshToken,
      id_token_expires_at: SAMPLE.idTokenExpiresAt.toISOString(),
      scopes: SAMPLE.scopes,
    });
    const { backend } = memoryBackend(blob);
    const store = new CredentialStore(CFG, backend);
    expect(await store.read()).toBeNull();
  });

  it('returns null when stored bundle is missing required fields', async () => {
    const blob = JSON.stringify({ schema_version: 1, issuer: ISSUER });
    const { backend } = memoryBackend(blob);
    const store = new CredentialStore(CFG, backend);
    expect(await store.read()).toBeNull();
  });

  it('returns null when the backend is unavailable (read is not fatal)', async () => {
    const store = new CredentialStore(CFG, unavailableBackend());
    expect(await store.read()).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// write then read round-trip + scoping
// ---------------------------------------------------------------------------

describe('CredentialStore.write / read round-trip', () => {
  it('preserves every field on the deep-equality path', async () => {
    const { backend } = memoryBackend();
    const store = new CredentialStore(CFG, backend);
    await store.write(SAMPLE);
    const got = await store.read();
    expect(got).not.toBeNull();
    expect(got!.idToken).toBe(SAMPLE.idToken);
    expect(got!.refreshToken).toBe(SAMPLE.refreshToken);
    expect(got!.idTokenExpiresAt.toISOString()).toBe(
      SAMPLE.idTokenExpiresAt.toISOString()
    );
    expect([...got!.scopes]).toEqual([...SAMPLE.scopes]);
  });

  it('persists the canonical on-disk blob shape', async () => {
    const { backend, state } = memoryBackend();
    const store = new CredentialStore(CFG, backend);
    await store.write(SAMPLE);
    expect(state.value).not.toBeNull();
    const parsed = JSON.parse(state.value!);
    expect(parsed).toMatchObject({
      schema_version: 1,
      issuer: ISSUER,
      client_id: CLIENT_ID,
      id_token: SAMPLE.idToken,
      refresh_token: SAMPLE.refreshToken,
      id_token_expires_at: SAMPLE.idTokenExpiresAt.toISOString(),
      scopes: ['openid', 'email', 'profile'],
    });
  });

  it('entries scoped by client_id do not collide', async () => {
    // Each store owns its own account-keyed backend, simulating two
    // distinct keyring entries on the same machine.
    const a = memoryBackend();
    const b = memoryBackend();
    const storeA = new CredentialStore(CFG, a.backend);
    const storeB = new CredentialStore(
      { issuer: ISSUER, clientId: OTHER_CLIENT_ID },
      b.backend
    );
    expect(storeA.accountName).not.toBe(storeB.accountName);
    await storeA.write(SAMPLE);
    expect(await storeB.read()).toBeNull();
  });

  it('clear removes the entry', async () => {
    const { backend, state, log } = memoryBackend();
    const store = new CredentialStore(CFG, backend);
    await store.write(SAMPLE);
    expect(state.value).not.toBeNull();
    await store.clear();
    expect(state.value).toBeNull();
    expect(log.clears).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// Concurrency
// ---------------------------------------------------------------------------

describe('CredentialStore concurrency', () => {
  it('serialises concurrent writes via in-process mutex (last-wins, never torn)', async () => {
    const observed: string[] = [];
    let inflight = 0;
    let peakInflight = 0;
    const backend: KeyringBackend = {
      async read() {
        return null;
      },
      async write(v: string) {
        inflight += 1;
        peakInflight = Math.max(peakInflight, inflight);
        // Force interleaving opportunity; if mutex is broken, two
        // writers will sit in here at the same time.
        await new Promise((r) => setTimeout(r, 5));
        observed.push(v);
        inflight -= 1;
      },
      async clear() {
        return false;
      },
    };
    const store = new CredentialStore(CFG, backend);
    const a: CredentialBundle = { ...SAMPLE, idToken: 'A.token.value' };
    const b: CredentialBundle = { ...SAMPLE, idToken: 'B.token.value' };
    const c: CredentialBundle = { ...SAMPLE, idToken: 'C.token.value' };
    await Promise.all([store.write(a), store.write(b), store.write(c)]);
    expect(peakInflight).toBe(1);
    expect(observed).toHaveLength(3);
    // Final ordering is deterministic — three observed entries each
    // carrying exactly one of the three idTokens, no torn writes.
    const ids = observed.map((blob) => JSON.parse(blob).id_token).sort();
    expect(ids).toEqual(['A.token.value', 'B.token.value', 'C.token.value']);
  });

  it('serialises read after write so the most recent value is observed', async () => {
    const { backend } = memoryBackend();
    const store = new CredentialStore(CFG, backend);
    const writeP = store.write(SAMPLE);
    const readP = store.read();
    await writeP;
    const got = await readP;
    expect(got).not.toBeNull();
    expect(got!.idToken).toBe(SAMPLE.idToken);
  });
});

// ---------------------------------------------------------------------------
// Headless / unavailable backend
// ---------------------------------------------------------------------------

describe('CredentialStore — headless backend', () => {
  it('write throws KeyringUnavailableError when Secret Service unavailable', async () => {
    const store = new CredentialStore(CFG, unavailableBackend());
    let err: unknown;
    try {
      await store.write(SAMPLE);
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(KeyringUnavailableError);
    expect((err as Error).message).toMatch(/Secret Service|libsecret|keyring/i);
  });

  it('clear throws KeyringUnavailableError when Secret Service unavailable', async () => {
    const store = new CredentialStore(CFG, unavailableBackend());
    await expect(store.clear()).rejects.toBeInstanceOf(KeyringUnavailableError);
  });

  it('keyring error message does not leak credential bytes', async () => {
    // Backend error message embeds the would-be-stored blob; the
    // re-wrapped error must not propagate any token bytes.
    const leakyMessage =
      `error writing entry: payload was ${SAMPLE.idToken} ${SAMPLE.refreshToken}`;
    const store = new CredentialStore(CFG, failingBackend(leakyMessage));
    let err: unknown;
    try {
      await store.write(SAMPLE);
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(Error);
    const msg = (err as Error).message;
    expect(msg).not.toContain(SAMPLE.idToken);
    expect(msg).not.toContain(SAMPLE.refreshToken);
  });
});

// ---------------------------------------------------------------------------
// Logging redaction
// ---------------------------------------------------------------------------

describe('CredentialStore logging redaction', () => {
  it('read does not log credential values', async () => {
    const blob = JSON.stringify({
      schema_version: 1,
      issuer: ISSUER,
      client_id: CLIENT_ID,
      id_token: SAMPLE.idToken,
      refresh_token: SAMPLE.refreshToken,
      id_token_expires_at: SAMPLE.idTokenExpiresAt.toISOString(),
      scopes: SAMPLE.scopes,
    });
    const { backend } = memoryBackend(blob);
    const cap = captureConsole();
    try {
      const store = new CredentialStore(CFG, backend);
      await store.read();
      const blobText = cap.blob();
      expect(blobText).not.toContain(SAMPLE.idToken);
      expect(blobText).not.toContain(SAMPLE.refreshToken);
    } finally {
      cap.restore();
    }
  });

  it('write does not log credential values', async () => {
    const { backend } = memoryBackend();
    const cap = captureConsole();
    try {
      const store = new CredentialStore(CFG, backend);
      await store.write(SAMPLE);
      const blobText = cap.blob();
      expect(blobText).not.toContain(SAMPLE.idToken);
      expect(blobText).not.toContain(SAMPLE.refreshToken);
    } finally {
      cap.restore();
    }
  });

  it('unavailable-backend warning on read does not leak credential values', async () => {
    const leakyBackend = failingBackend(
      `lost connection while reading ${SAMPLE.idToken}`
    );
    const cap = captureConsole();
    try {
      const store = new CredentialStore(CFG, leakyBackend);
      await store.read();
      const blobText = cap.blob();
      expect(blobText).not.toContain(SAMPLE.idToken);
    } finally {
      cap.restore();
    }
  });
});

// ---------------------------------------------------------------------------
// Performance (warm path)
// ---------------------------------------------------------------------------

describe('CredentialStore performance', () => {
  it('warm-path read+write round-trip completes well under 50ms', async () => {
    const { backend } = memoryBackend();
    const store = new CredentialStore(CFG, backend);
    const start = performance.now();
    await store.write(SAMPLE);
    const got = await store.read();
    const elapsed = performance.now() - start;
    expect(got).not.toBeNull();
    expect(elapsed).toBeLessThan(50);
  });
});

// ---------------------------------------------------------------------------
// Default backend wiring (does not touch the real keyring)
// ---------------------------------------------------------------------------

describe('defaultKeyringBackend', () => {
  it('returns an object with read/write/clear methods', () => {
    const backend = defaultKeyringBackend(CFG);
    expect(typeof backend.read).toBe('function');
    expect(typeof backend.write).toBe('function');
    expect(typeof backend.clear).toBe('function');
  });
});

// ---------------------------------------------------------------------------
// Real-keyring integration lane (opt-in)
// ---------------------------------------------------------------------------

const INTEGRATION = process.env.KEYRING_INTEGRATION === '1';

describe.skipIf(!INTEGRATION)('CredentialStore [integration]', () => {
  // Each integration test scopes itself to a per-run client_id so
  // parallel test runs and re-runs don't trip over each other.
  function freshStore(): CredentialStore {
    const runId = `${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const clientId = `integration-${runId}.apps.googleusercontent.com`;
    return new CredentialStore({ issuer: ISSUER, clientId });
  }

  it('round-trips through the real OS keyring', async () => {
    const store = freshStore();
    try {
      await store.write(SAMPLE);
      const got = await store.read();
      expect(got).not.toBeNull();
      expect(got!.idToken).toBe(SAMPLE.idToken);
    } finally {
      await store.clear().catch(() => undefined);
    }
  });

  it('clear removes a previously-written entry', async () => {
    const store = freshStore();
    await store.write(SAMPLE);
    await store.clear();
    expect(await store.read()).toBeNull();
  });
});
