/**
 * Phase 5 — OS-native credential storage for quarto-hub-mcp.
 *
 * One opaque JSON blob per `@napi-rs/keyring` entry, scoped by
 * `<issuer>:<client_id>` under service `dev.quarto.hub-mcp`. The
 * platform binding is:
 *
 *   Windows → Credential Manager (DPAPI), bound to current user
 *   macOS   → login Keychain (kSecAttrAccessibleWhenUnlocked)
 *   Linux   → Secret Service / libsecret (default collection)
 *
 * No plaintext file on disk; no silent degradation. Read errors fold
 * to `null` (so try-without-creds-first still works); write/clear
 * errors throw a typed `KeyringUnavailableError`.
 */

import { AsyncEntry } from '@napi-rs/keyring';

import { redactTokens } from './redact.js';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface CredentialBundle {
  readonly idToken: string;
  readonly refreshToken: string;
  readonly idTokenExpiresAt: Date;
  readonly scopes: readonly string[];
}

export interface CredentialStoreConfig {
  readonly issuer: string;
  readonly clientId: string;
}

export interface KeyringBackend {
  read(): Promise<string | null>;
  write(value: string): Promise<void>;
  clear(): Promise<boolean>;
}

export const SERVICE_NAME = 'dev.quarto.hub-mcp';
const SCHEMA_VERSION = 1;

// ---------------------------------------------------------------------------
// Typed errors
// ---------------------------------------------------------------------------

export class KeyringUnavailableError extends Error {
  override readonly name = 'KeyringUnavailableError';
  constructor(message: string) {
    super(message);
  }
}

// ---------------------------------------------------------------------------
// Default backend (wraps @napi-rs/keyring)
// ---------------------------------------------------------------------------

export function defaultKeyringBackend(cfg: CredentialStoreConfig): KeyringBackend {
  const account = `${cfg.issuer}:${cfg.clientId}`;
  const entry = new AsyncEntry(SERVICE_NAME, account);
  return {
    async read() {
      const value = await entry.getPassword();
      return value ?? null;
    },
    async write(value: string) {
      await entry.setPassword(value);
    },
    async clear() {
      return await entry.deleteCredential();
    },
  };
}

// ---------------------------------------------------------------------------
// On-disk blob shape (schema_version 1)
// ---------------------------------------------------------------------------

interface BlobV1 {
  readonly schema_version: 1;
  readonly issuer: string;
  readonly client_id: string;
  readonly id_token: string;
  readonly refresh_token: string;
  readonly id_token_expires_at: string;
  readonly scopes: readonly string[];
}

function serialize(cfg: CredentialStoreConfig, bundle: CredentialBundle): string {
  const blob: BlobV1 = {
    schema_version: SCHEMA_VERSION,
    issuer: cfg.issuer,
    client_id: cfg.clientId,
    id_token: bundle.idToken,
    refresh_token: bundle.refreshToken,
    id_token_expires_at: bundle.idTokenExpiresAt.toISOString(),
    scopes: bundle.scopes,
  };
  return JSON.stringify(blob);
}

function parseBundle(raw: string): CredentialBundle | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== 'object') return null;
  const p = parsed as Record<string, unknown>;
  if (p.schema_version !== SCHEMA_VERSION) return null;
  if (typeof p.id_token !== 'string' || p.id_token === '') return null;
  if (typeof p.refresh_token !== 'string' || p.refresh_token === '') return null;
  if (typeof p.id_token_expires_at !== 'string') return null;
  const expiresAt = new Date(p.id_token_expires_at);
  if (Number.isNaN(expiresAt.getTime())) return null;
  if (!Array.isArray(p.scopes) || !p.scopes.every((s) => typeof s === 'string')) {
    return null;
  }
  return {
    idToken: p.id_token,
    refreshToken: p.refresh_token,
    idTokenExpiresAt: expiresAt,
    scopes: p.scopes as readonly string[],
  };
}

// ---------------------------------------------------------------------------
// CredentialStore
// ---------------------------------------------------------------------------

export class CredentialStore {
  readonly serviceName: string = SERVICE_NAME;
  readonly accountName: string;

  private readonly backend: KeyringBackend;
  private readonly cfg: CredentialStoreConfig;
  // Tail of the in-process mutex chain. Every operation chains onto
  // this promise so reads and writes serialise in submission order.
  private tail: Promise<void> = Promise.resolve();
  // In-memory memo of the last *definitive* keyring state, so repeated
  // reads (every getBearer / connect probe) skip the OS-IPC round-trip.
  // `undefined` = unknown (not yet observed); `{ value }` = known. Only
  // definitive outcomes populate it — a successful read, a successful
  // write, or a clear. A transient read failure leaves it untouched so
  // the next read retries the backend. This process is the only writer
  // of its keyring entry, so a cached value cannot go stale underneath us.
  private cache: { value: CredentialBundle | null } | undefined;

  constructor(cfg: CredentialStoreConfig, backend?: KeyringBackend) {
    this.cfg = cfg;
    this.accountName = `${cfg.issuer}:${cfg.clientId}`;
    this.backend = backend ?? defaultKeyringBackend(cfg);
  }

  async read(): Promise<CredentialBundle | null> {
    return await this.enqueue(async () => {
      // Cache check runs inside the mutex chain so a read submitted after
      // a write still observes that write (ordering contract preserved);
      // we only skip the keyring IPC, not the serialisation.
      if (this.cache !== undefined) return this.cache.value;
      let raw: string | null;
      try {
        raw = await this.backend.read();
      } catch (err) {
        // Read is never fatal — try-without-creds-first depends on it.
        // Don't cache a transient failure: the next read retries.
        const msg = redactTokens(errMessage(err));
        console.warn(`CredentialStore: keyring read failed (${msg})`);
        return null;
      }
      const value = raw === null ? null : parseBundle(raw);
      this.cache = { value };
      return value;
    });
  }

  async write(bundle: CredentialBundle): Promise<void> {
    const blob = serialize(this.cfg, bundle);
    await this.enqueue(async () => {
      try {
        await this.backend.write(blob);
      } catch (err) {
        throw new KeyringUnavailableError(
          `Failed to write credentials to OS keyring (service '${SERVICE_NAME}'): ${redactTokens(errMessage(err))}`
        );
      }
      this.cache = { value: bundle };
    });
  }

  async clear(): Promise<void> {
    await this.enqueue(async () => {
      try {
        await this.backend.clear();
      } catch (err) {
        throw new KeyringUnavailableError(
          `Failed to clear credentials from OS keyring (service '${SERVICE_NAME}'): ${redactTokens(errMessage(err))}`
        );
      }
      this.cache = { value: null };
    });
  }

  // Run `op` after every previously-enqueued operation completes,
  // whether they resolved or rejected. The shared tail records only
  // the "completed" signal (never the rejection), so one failure
  // doesn't poison the chain.
  private enqueue<T>(op: () => Promise<T>): Promise<T> {
    const prev = this.tail;
    let resolveTail!: () => void;
    this.tail = new Promise<void>((r) => {
      resolveTail = r;
    });
    const run = prev.then(op, op);
    return run.finally(resolveTail);
  }
}

function errMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  return String(err);
}
