/**
 * Registry operations over the flat sidecar (G6, G7, G8, G11, G12). Read operations are pure functions of a
 * `RegistryView`; write operations take and return an Automerge doc, one `A.change` each. See the contract,
 * "Sidecar doc" and "Operations".
 */
import * as A from '@automerge/automerge';

export interface Entry {
  path: string;
  blob: string;
  doc: string;
  gen: number;
}

export interface Sidecar {
  version: number;
  entries: Record<string, number>;
  retired: Record<string, number>;
}

export interface RegistryView {
  entries: Entry[];
  retired: Entry[];
}

const NUL = '\u0000';

/** `path NUL blob NUL doc`. Paths always use `/` (a native Windows path is a caller bug). */
export function registryKey(path: string, blob: string, doc: string): string {
  if (path.includes('\\')) throw new Error(`registry path must use forward slashes: ${path}`);
  return `${path}${NUL}${blob}${NUL}${doc}`;
}

export function parseKey(key: string): { path: string; blob: string; doc: string } {
  const [path, blob, doc] = key.split(NUL);
  return { path, blob, doc };
}

const int = (n: number) => new A.Int(n) as unknown as number;

/** A new sidecar; both maps are created in this one change, by this one writer. */
export function createSidecar(): A.Doc<Sidecar> {
  return A.change(A.init<Sidecar>(), (d) => {
    d.version = 1;
    d.entries = {};
    d.retired = {};
  });
}

const toEntries = (m: Record<string, number> | undefined): Entry[] =>
  Object.entries(m ?? {}).map(([k, gen]) => ({ ...parseKey(k), gen: Number(gen) }));

export const readEntries = (d: A.Doc<Sidecar>): Entry[] => toEntries(d.entries);
export const readRetired = (d: A.Doc<Sidecar>): Entry[] => toEntries(d.retired);
export const viewOf = (d: A.Doc<Sidecar>): RegistryView => ({ entries: readEntries(d), retired: readRetired(d) });

const isRetired = (v: RegistryView, path: string, blob: string, doc: string) =>
  v.retired.some((e) => e.path === path && e.blob === blob && e.doc === doc);

/** Bytewise comparison of two strings (identical in Rust and TypeScript; not `localeCompare`). */
const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** Lowest `(generation, docId)` among the non-retired entries of `(path, blob)`; null when there is none. */
export function canonical(v: RegistryView, path: string, blob: string): Entry | null {
  const live = v.entries.filter((e) => e.path === path && e.blob === blob && !isRetired(v, e.path, e.blob, e.doc));
  live.sort((a, b) => a.gen - b.gen || cmp(a.doc, b.doc));
  return live[0] ?? null;
}

export const entriesFor = (v: RegistryView, path: string): Entry[] => v.entries.filter((e) => e.path === path);

/** `max + 1` over every entry of the path on any blob; the first doc of a path gets 0. */
export function nextGeneration(v: RegistryView, path: string): number {
  return entriesFor(v, path).reduce((m, e) => Math.max(m, e.gen), -1) + 1;
}

/** Put-if-absent; returns the same doc object when the key exists. */
export function register(d: A.Doc<Sidecar>, e: Entry): A.Doc<Sidecar> {
  const k = registryKey(e.path, e.blob, e.doc);
  if (Object.prototype.hasOwnProperty.call(d.entries, k)) return d;
  return A.change(d, (x) => {
    x.entries[k] = int(e.gen);
  });
}

/** One change: add `(path, newBlob, doc)` with the doc's generation and remove the old key (G8). */
export function moveForward(
  d: A.Doc<Sidecar>,
  a: { path: string; oldBlob: string; newBlob: string; doc: string },
): A.Doc<Sidecar> {
  const oldKey = registryKey(a.path, a.oldBlob, a.doc);
  const newKey = registryKey(a.path, a.newBlob, a.doc);
  const has = Object.prototype.hasOwnProperty.call(d.entries, oldKey);
  const gen = has ? Number(d.entries[oldKey]) : nextGeneration(viewOf(d), a.path);
  return A.change(d, (x) => {
    x.entries[newKey] = int(gen);
    if (has) delete x.entries[oldKey];
  });
}

/** The `reset --hard` step: grow-only `retired`; the entry stays in `entries`. */
export function retire(d: A.Doc<Sidecar>, a: { path: string; blob: string; doc: string }): A.Doc<Sidecar> {
  const k = registryKey(a.path, a.blob, a.doc);
  if (Object.prototype.hasOwnProperty.call(d.retired, k)) return d;
  const gen = Object.prototype.hasOwnProperty.call(d.entries, k) ? Number(d.entries[k]) : 0;
  return A.change(d, (x) => {
    x.retired[k] = int(gen);
  });
}

/**
 * After a sidecar sync: an attached doc with no entry on any blob, and not retired, is registered again under
 * its HEAD blob with its persisted generation. Never revives a retired entry, never re-registers a doc that
 * moved forward.
 */
export function repair(
  d: A.Doc<Sidecar>,
  attached: { path: string; doc: string; gen: number },
  headBlob: string,
): A.Doc<Sidecar> {
  const v = viewOf(d);
  const anyEntry = v.entries.some((e) => e.path === attached.path && e.doc === attached.doc);
  const anyRetired = v.retired.some((e) => e.path === attached.path && e.doc === attached.doc);
  if (anyEntry || anyRetired) return d;
  return register(d, { path: attached.path, blob: headBlob, doc: attached.doc, gen: attached.gen });
}

/** Paths whose lowercased form collides with another path's: they fall back to ordinary sync (G12). */
export function caseCollisions(v: RegistryView): Set<string> {
  const byLower = new Map<string, Set<string>>();
  for (const e of v.entries) {
    const l = e.path.toLowerCase();
    (byLower.get(l) ?? byLower.set(l, new Set()).get(l)!).add(e.path);
  }
  const out = new Set<string>();
  for (const s of byLower.values()) if (s.size > 1) for (const p of s) out.add(p);
  return out;
}

/** True when the doc is registered under a path other than `path` (a doc is live under at most one path). */
export const liveElsewhere = (v: RegistryView, doc: string, path: string): boolean =>
  v.entries.some((e) => e.doc === doc && e.path !== path);

/** Read a string in either form: a scalar string, an `ImmutableString`, or a Text object. */
export function asString(v: unknown): string {
  if (typeof v === 'string') return v;
  if (A.isImmutableString(v)) return v.toString();
  return String(v);
}

/** The git-epochs keys of a version 4 index doc, accepting both string forms. Absent keys come back undefined. */
export function readIndexFields(d: any): { epochs?: string; owner?: string; repo?: { roots: string[]; prefix: string } } {
  const out: { epochs?: string; owner?: string; repo?: { roots: string[]; prefix: string } } = {};
  if (d.epochs != null) out.epochs = asString(d.epochs);
  if (d.owner != null) out.owner = asString(d.owner);
  if (d.repo != null) out.repo = { roots: Array.from(d.repo.roots as unknown[], asString), prefix: asString(d.repo.prefix) };
  return out;
}
