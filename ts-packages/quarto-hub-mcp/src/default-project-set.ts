/**
 * Default-project-set storage for quarto-hub-mcp (bd-3h5hbfdv).
 *
 * `list_projects` with no `project_set` argument reads the remembered
 * default from here; the first explicit call that reads a set successfully
 * persists its argument, and the `set_default_project_set` tool overwrites
 * it. One `@napi-rs/keyring` entry under the same service as the OAuth
 * credentials (see auth/credential-store.ts), account `default-project-set`.
 *
 * The stored value is the argument verbatim — a bare index doc id or any
 * link form {@link parseProjectRef} accepts — so a `#/link-project-set/`
 * URL's `server=` keeps routing the read and there is no blob schema to
 * version. The entry is not a credential; it names a document the user
 * already asked this server to read.
 *
 * Error contract mirrors CredentialStore: read errors fold to `null` (a
 * missing/unreadable default just means "ask again"), while write/clear
 * errors throw a typed `KeyringUnavailableError` so the caller can decide
 * — `list_projects` degrades to a warning, the setter fails.
 */

import { AsyncEntry } from '@napi-rs/keyring';

import {
  KeyringUnavailableError,
  SERVICE_NAME,
  type KeyringBackend,
} from './auth/credential-store.js';
import { redactTokens } from './auth/redact.js';

const ACCOUNT_NAME = 'default-project-set';

/** Default backend (wraps @napi-rs/keyring); tests inject their own. */
export function defaultProjectSetBackend(): KeyringBackend {
  const entry = new AsyncEntry(SERVICE_NAME, ACCOUNT_NAME);
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

export class DefaultProjectSetStore {
  readonly serviceName: string = SERVICE_NAME;
  readonly accountName: string = ACCOUNT_NAME;

  private readonly backend: KeyringBackend;
  // Tail of the in-process mutex chain, same serialisation contract as
  // CredentialStore: a read submitted after a write observes that write.
  private tail: Promise<void> = Promise.resolve();
  // Memo of the last *definitive* keyring state (`undefined` = unknown), so
  // repeated bare `list_projects` calls skip the OS-IPC round-trip. Only
  // definitive outcomes populate it; a transient read failure retries next
  // time. This process is the only writer of its keyring entry.
  private cache: { value: string | null } | undefined;

  constructor(backend?: KeyringBackend) {
    this.backend = backend ?? defaultProjectSetBackend();
  }

  /** The stored project-set reference, or null when none / unreadable. */
  async read(): Promise<string | null> {
    return await this.enqueue(async () => {
      if (this.cache !== undefined) return this.cache.value;
      let raw: string | null;
      try {
        raw = await this.backend.read();
      } catch (err) {
        // Read is never fatal — the caller falls back to asking for an
        // explicit `project_set`. Don't cache the failure.
        const msg = redactTokens(err instanceof Error ? err.message : String(err));
        console.warn(`DefaultProjectSetStore: keyring read failed (${msg})`);
        return null;
      }
      const value = raw !== null && raw.trim() !== '' ? raw : null;
      this.cache = { value };
      return value;
    });
  }

  /** Persist a project-set reference (verbatim id or link). */
  async write(value: string): Promise<void> {
    await this.enqueue(async () => {
      try {
        await this.backend.write(value);
      } catch (err) {
        const msg = redactTokens(err instanceof Error ? err.message : String(err));
        throw new KeyringUnavailableError(
          `Failed to write the default project set to OS keyring (service '${SERVICE_NAME}'): ${msg}`,
        );
      }
      this.cache = { value };
    });
  }

  /** Remove the remembered default. */
  async clear(): Promise<void> {
    await this.enqueue(async () => {
      try {
        await this.backend.clear();
      } catch (err) {
        const msg = redactTokens(err instanceof Error ? err.message : String(err));
        throw new KeyringUnavailableError(
          `Failed to clear the default project set from OS keyring (service '${SERVICE_NAME}'): ${msg}`,
        );
      }
      this.cache = { value: null };
    });
  }

  // Run `op` after every previously-enqueued operation completes, whether
  // it resolved or rejected; one failure does not poison the chain.
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
