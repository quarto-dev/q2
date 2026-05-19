/**
 * In-memory cache of loaded user-grammar highlighters — Phase 4.5.
 *
 * The cache's single responsibility is to avoid re-parsing a grammar's
 * `.wasm` every render. On each `sync()` it compares the caller-supplied
 * descriptors against what it already holds, loads anything new or
 * changed, and disposes anything that disappeared. `registerInto()`
 * then wires the highlighters' `highlight(source)` callbacks into a
 * `JsUserGrammars` handle that the wasm-bindgen bridge consumes.
 *
 * Cache key: the grammar's **class** (unique across the project). The
 * cached entry also stores a content hash of the `(wasm bytes, scm
 * string)` so `sync()` can detect edits and reload without requiring
 * the caller to invalidate explicitly.
 *
 * Dependencies are injected via the constructor so the cache is
 * unit-testable without web-tree-sitter or Automerge: tests stub
 * `loadUserGrammar` with an in-memory factory.
 */

import type { GrammarDescriptor } from './Discovery';
import type {
  LoadUserGrammarArgs,
  UserGrammarHighlighter,
} from './Highlight';

/**
 * Minimal surface that the cache needs from a `JsUserGrammars`
 * instance. Lets us unit-test `registerInto()` without constructing
 * a real wasm-bindgen handle.
 */
export interface RegistrableGrammars {
  register(
    languageClass: string,
    highlightFn: (class_: string, source: string) => string | null | undefined,
  ): void;
}

export interface LoadFailure {
  readonly class: string;
  readonly reason: string;
}

export interface SyncResult {
  /** Classes currently registered after this sync, sorted alphabetically. */
  readonly classes: string[];
  /** Per-descriptor failures encountered during this sync. */
  readonly failures: LoadFailure[];
}

export interface UserGrammarCacheDeps {
  /** Load a grammar. Mirrors the signature of `loadUserGrammar`. */
  loadUserGrammar: (args: LoadUserGrammarArgs) => Promise<UserGrammarHighlighter>;
  /** Retrieve binary content for a project path. Returns null if absent. */
  getBinaryContent: (path: string) => Promise<Uint8Array | null>;
  /** Retrieve text content for a project path. Returns null if absent. */
  getTextContent: (path: string) => Promise<string | null>;
}

interface CacheEntry {
  contentHash: string;
  highlighter: UserGrammarHighlighter;
}

export class UserGrammarCache {
  private readonly deps: UserGrammarCacheDeps;
  private readonly entries = new Map<string, CacheEntry>();

  constructor(deps: UserGrammarCacheDeps) {
    this.deps = deps;
  }

  /**
   * Reconcile the cache with a new set of descriptors: load new or
   * changed grammars, keep unchanged ones, and dispose removed ones.
   * Safe to call concurrently with the previous sync only if the
   * caller serializes the promises — this class does not guard
   * against overlapping syncs.
   */
  async sync(descriptors: readonly GrammarDescriptor[]): Promise<SyncResult> {
    const failures: LoadFailure[] = [];
    const targetClasses = new Set<string>();

    for (const desc of descriptors) {
      targetClasses.add(desc.class);
      try {
        const wasmBytes = await this.deps.getBinaryContent(desc.wasmPath);
        if (!wasmBytes) {
          failures.push({
            class: desc.class,
            reason: `missing binary content at ${desc.wasmPath}`,
          });
          continue;
        }
        const scm = await this.deps.getTextContent(desc.highlightsPath);
        if (scm === null) {
          failures.push({
            class: desc.class,
            reason: `missing text content at ${desc.highlightsPath}`,
          });
          continue;
        }

        const hash = await contentHash(wasmBytes, scm);
        const existing = this.entries.get(desc.class);
        if (existing && existing.contentHash === hash) {
          continue; // unchanged — keep the cached highlighter
        }
        existing?.highlighter.dispose();

        const highlighter = await this.deps.loadUserGrammar({
          name: desc.class,
          wasmBytes,
          highlightsScm: scm,
        });
        this.entries.set(desc.class, { contentHash: hash, highlighter });
      } catch (err) {
        failures.push({
          class: desc.class,
          reason: err instanceof Error ? err.message : String(err),
        });
      }
    }

    // Dispose any cached grammar whose class is no longer discoverable.
    for (const [cls, entry] of this.entries) {
      if (!targetClasses.has(cls)) {
        entry.highlighter.dispose();
        this.entries.delete(cls);
      }
    }

    const classes = Array.from(this.entries.keys()).sort();
    return { classes, failures };
  }

  /**
   * Register each cached highlighter's `highlight(source)` as a
   * callback on `handle`. The `class` argument on the callback is
   * redundant (each highlighter handles a single class, the one the
   * cache registered it under) but required by the bridge's signature.
   */
  registerInto(handle: RegistrableGrammars): void {
    for (const [cls, entry] of this.entries) {
      const highlighter = entry.highlighter;
      handle.register(cls, (_cls, source) => highlighter.highlight(source));
    }
  }

  /** Dispose every cached highlighter and clear the cache. */
  disposeAll(): void {
    for (const entry of this.entries.values()) {
      entry.highlighter.dispose();
    }
    this.entries.clear();
  }
}

/**
 * Compute a stable content hash for a (`wasmBytes`, `highlightsScm`)
 * pair. SHA-256 via `crypto.subtle` — available in both browsers and
 * Node 20+ via `globalThis.crypto`.
 */
async function contentHash(wasmBytes: Uint8Array, highlightsScm: string): Promise<string> {
  const scmBytes = new TextEncoder().encode(highlightsScm);
  // Prepend length-prefix markers so the hash can't collide between
  // (wasm, scm) and (wasm || scm, "") by manipulating byte boundaries.
  const lengthHeader = new Uint8Array(16);
  const dv = new DataView(lengthHeader.buffer);
  dv.setBigUint64(0, BigInt(wasmBytes.byteLength), false);
  dv.setBigUint64(8, BigInt(scmBytes.byteLength), false);
  const combined = new Uint8Array(
    lengthHeader.byteLength + wasmBytes.byteLength + scmBytes.byteLength,
  );
  combined.set(lengthHeader, 0);
  combined.set(wasmBytes, lengthHeader.byteLength);
  combined.set(scmBytes, lengthHeader.byteLength + wasmBytes.byteLength);
  const digest = await globalThis.crypto.subtle.digest('SHA-256', combined);
  return bufferToHex(digest);
}

function bufferToHex(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf);
  let hex = '';
  for (let i = 0; i < bytes.length; i++) {
    hex += bytes[i].toString(16).padStart(2, '0');
  }
  return hex;
}
