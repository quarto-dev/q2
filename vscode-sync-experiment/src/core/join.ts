/**
 * The Join command's logic (GE-11, G22): classify what the user pasted, read the hub's `/health` and the share's index,
 * decide which modes apply, and write the binding. No `vscode` import; the extension layer owns the prompts.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { Repo } from '@automerge/automerge-repo/slim';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import { readIndexFields } from '@quarto/quarto-git-epochs';
import { parseProjectRef } from '@quarto/share-url';
import { excludeMetaDir } from './git-exclude';
import { META_DIR } from './ignore';
import { writeProjectMeta, type ProjectMeta } from './meta';
import { findDoc, makeRepo, webSocketAdapter } from './repo';
import type { RepoIdentity } from './clone/attach';

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

export type JoinInput =
  | { kind: 'ticket'; ticket: string }
  | { kind: 'ws'; url: string }
  | { kind: 'http'; url: string }
  | { kind: 'share'; indexDocId: string; server?: string }
  | { kind: 'invalid'; message: string };

const INVALID: JoinInput = { kind: 'invalid', message: 'That is not a ticket, a URL, or a share link.' };

/** The ticket alphabet, checked before anything is spawned: a `.cmd` spawn uses a shell, so nothing else may reach it. */
const TICKET = /^q2preview[A-Za-z0-9_-]+$/;

export function classifyJoinInput(raw: string): JoinInput {
  const input = raw.trim();
  if (TICKET.test(input)) return { kind: 'ticket', ticket: input };
  if (/^q2preview/.test(input)) return INVALID;
  if (/#\/share\//.test(input)) {
    const ref = parseProjectRef(input);
    const indexDocId = ref.project.replace(/^.*automerge:/, '');
    return indexDocId ? { kind: 'share', indexDocId, ...(ref.server ? { server: ref.server } : {}) } : INVALID;
  }
  let url: URL;
  try {
    url = new URL(input);
  } catch {
    return INVALID;
  }
  if (!url.hostname) return INVALID;
  if (url.protocol === 'ws:' || url.protocol === 'wss:') return { kind: 'ws', url: input };
  if (url.protocol === 'http:' || url.protocol === 'https:') return { kind: 'http', url: deriveWsUrl(input) };
  return INVALID;
}

/** `http(s)://host:port/anything` to `ws(s)://host:port/ws`. */
export function deriveWsUrl(httpUrl: string): string {
  const u = new URL(httpUrl);
  return `${u.protocol === 'https:' ? 'wss' : 'ws'}://${u.host}/ws`;
}

/** The `/health` address of a hub given its websocket URL. */
export function healthUrlFor(wsUrl: string): string {
  const u = new URL(wsUrl);
  return `${u.protocol === 'wss:' ? 'https' : 'http'}://${u.host}/health`;
}

export function isLoopbackUrl(url: string): boolean {
  try {
    const h = new URL(url).hostname;
    return h === '127.0.0.1' || h === 'localhost' || h === '[::1]' || h === '::1';
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Health, index, root comparison
// ---------------------------------------------------------------------------

export interface HubHealth {
  indexDocId: string;
  projectRoot?: string;
}

/** The hub's `/health`, or null for any failure: other servers may not serve it, and the caller then asks for the id. */
export async function fetchHealth(url: string, timeoutMs = 3000): Promise<HubHealth | null> {
  try {
    const res = await fetch(url, { signal: AbortSignal.timeout(timeoutMs) });
    if (!res.ok) return null;
    const body = (await res.json()) as { index_document_id?: unknown; project_root?: unknown };
    if (typeof body.index_document_id !== 'string' || !body.index_document_id) return null;
    return { indexDocId: body.index_document_id, ...(typeof body.project_root === 'string' ? { projectRoot: body.project_root } : {}) };
  } catch {
    return null;
  }
}

export interface IndexInspection {
  gitEnabled: boolean;
  repo?: RepoIdentity;
}

/** Read the index doc (5 s by default) to learn whether the share carries a `repo` entry. */
export async function inspectIndex(serverUrl: string, indexDocId: string, timeoutMs = 5000): Promise<IndexInspection> {
  const repo = new Repo({ network: [webSocketAdapter(serverUrl)] });
  let connected = false;
  repo.networkSubsystem.on('peer', () => (connected = true));
  try {
    const index = await findDoc<IndexDocument>(repo, indexDocId, AbortSignal.timeout(timeoutMs)).catch(() => null);
    if (!index) {
      throw new Error(connected ? 'Project not found on this server.' : `Could not connect to ${serverUrl}.`);
    }
    const fields = readIndexFields(index.doc());
    return fields.repo ? { gitEnabled: true, repo: fields.repo } : { gitEnabled: false };
  } finally {
    await repo.shutdown().catch(() => {});
  }
}

async function canonical(p: string, platform: NodeJS.Platform): Promise<string> {
  let out = await fs.realpath(p).catch(() => p);
  if (platform === 'win32') out = out.replace(/\\/g, '/');
  out = out.replace(/\/+$/, '') || '/';
  return platform === 'darwin' || platform === 'win32' ? out.toLowerCase() : out;
}

/** Does the hub's project root name the open folder? Real paths, no trailing separators, case folded on macOS and Windows. */
export async function projectRootMatches(openFolder: string, hubRoot: string | undefined, platform: NodeJS.Platform = process.platform): Promise<boolean> {
  if (!hubRoot) return false;
  return (await canonical(openFolder, platform)) === (await canonical(hubRoot, platform));
}

// ---------------------------------------------------------------------------
// Offers
// ---------------------------------------------------------------------------

export interface OfferInput {
  /** The server host is 127.0.0.1, localhost, or ::1. */
  loopback: boolean;
  /** A workspace folder is open. */
  hasFolder: boolean;
  /** The hub's `project_root` names the open folder. */
  rootMatches: boolean;
  /** The share's index carries a `repo` entry. */
  gitEnabled: boolean;
  /** GE-10's attach check accepts the open folder for that entry. */
  passes: boolean;
  /** The open folder already has `.quarto-hub/project.json`. */
  bound: boolean;
}

export type OfferedMode = 'rebind' | 'clone' | 'mirror' | 'editor-only';

export interface Offer {
  mode: OfferedMode;
  label: string;
  detail: string;
  /** Ask "This looks like the hub's own project folder. Use it as editor only?" before binding. */
  confirm?: boolean;
}

const OFFER: Record<OfferedMode, Omit<Offer, 'mode'>> = {
  rebind: { label: 'Rebind this folder to the new session', detail: 'Keep this folder; delete its local sync history.' },
  clone: { label: 'Attach this folder', detail: 'Sync this git clone with the project.' },
  mirror: { label: 'Create a mirror folder', detail: 'Make a new folder that follows the project.' },
  'editor-only': { label: 'Editor only', detail: 'Edit live in this folder; the hub owns the files.' },
};

const offer = (mode: OfferedMode, extra: Partial<Offer> = {}): Offer => ({ mode, ...OFFER[mode], ...extra });

/** The modes G22 offers for this join, default first. */
export function offeredModes(i: OfferInput): Offer[] {
  if (!i.hasFolder) return [offer('mirror')];
  // A bound folder cannot be attached or made editor-only until it is rebound (attach refuses `already-bound`).
  if (i.bound) return [offer('rebind'), offer('mirror')];
  const out: Offer[] = [];
  const attach = i.gitEnabled && i.passes;
  if (i.loopback) {
    // A match means the hub's own directory: two full agents on one disk is unsupported, so attach is not offered.
    if (i.rootMatches) out.push(offer('editor-only', { confirm: true }), offer('mirror'));
    else out.push(...(attach ? [offer('clone')] : []), offer('mirror'));
  } else if (i.rootMatches) {
    out.push(...(attach ? [offer('clone')] : []), offer('mirror'), offer('editor-only', { confirm: true, detail: 'Use this only if the hub runs on this machine.' }));
  } else {
    out.push(...(attach ? [offer('clone')] : []), offer('mirror'));
  }
  return out;
}

// ---------------------------------------------------------------------------
// Binding
// ---------------------------------------------------------------------------

/**
 * Pull the project's index and file docs into the folder's local store, writing no file. Every non-clone mode starts
 * offline from that store (project-sync.ts, Phase 1), so a folder bound without one would find nothing.
 */
async function seedStore(root: string, syncServer: string, indexDocId: string, timeoutMs: number): Promise<void> {
  await fs.mkdir(path.join(root, META_DIR, 'automerge'), { recursive: true });
  const repo = makeRepo(root, syncServer);
  let connected = false;
  repo.networkSubsystem.on('peer', () => (connected = true));
  try {
    const index = await findDoc<IndexDocument>(repo, indexDocId, AbortSignal.timeout(timeoutMs)).catch(() => null);
    if (!index) throw new Error(connected ? 'Project not found on this server.' : `Could not connect to ${syncServer}.`);
    for (const docId of Object.values(index.doc()?.files ?? {})) await findDoc(repo, docId);
    await repo.flush();
  } finally {
    await repo.shutdown().catch(() => {});
  }
}

/**
 * Bind an existing folder in editor-only mode: seed the local store, then write `.quarto-hub/project.json`. Nothing is
 * written when the project cannot be read. The ticket is never an input here.
 */
export async function bindFolder(o: {
  root: string;
  indexDocId: string;
  syncServer: string;
  mode: 'editor-only';
  timeoutMs?: number;
}): Promise<ProjectMeta> {
  await seedStore(o.root, o.syncServer, o.indexDocId, o.timeoutMs ?? 5000);
  await excludeMetaDir(o.root).catch(() => false);
  const meta: ProjectMeta = { indexDocId: o.indexDocId, syncServer: o.syncServer, createdAt: new Date().toISOString(), mode: o.mode };
  await writeProjectMeta(o.root, meta);
  return meta;
}
