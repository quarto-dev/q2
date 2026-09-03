/**
 * Invite preview payload codec (bd-fxdcxbpq).
 *
 * Invite URLs carry an optional, display-only `preview=` payload so the
 * landing card can show what the recipient is being invited to before any
 * connection is made. Collection invites additionally carry a `start=`
 * target: the project + file to open right after joining.
 *
 * SECURITY: preview payloads are display-only — they must never contain
 * document ids or anything that grants access. The start target does carry a
 * project index doc id, which is acceptable because the collection invite
 * already grants access to every entry via the collection document itself.
 *
 * Wire format is base64url-encoded compact JSON with single-letter keys
 * (matching the `entries={d,s,n}` precedent in routing.ts) and a version
 * marker. Decoders are tolerant: absent, malformed, oversized, or
 * unknown-version payloads decode to `undefined`, never throw — legacy links
 * must keep working.
 */

// ============================================================================
// Types (decoded, app-facing)
// ============================================================================

export interface CollectionPreviewProject {
  name: string;
  /** Up to MAX_PREVIEW_FILES representative file paths. */
  topFiles: string[];
  fileCount: number;
  /** Contributor initials for the facepile (display-only). */
  contributorInitials: string[];
}

export interface CollectionInvitePreview {
  kind: 'collection';
  /** Up to MAX_PREVIEW_PROJECTS projects. */
  projects: CollectionPreviewProject[];
  /** Total project count in the collection (may exceed projects.length). */
  totalProjects: number;
  /** First names for the "Carlos, Jenny and Mine work here" line. */
  memberFirstNames: string[];
}

/**
 * A `#/share/…` invite grants access to a whole *project* (the index
 * document); the file is only where the editor opens. The preview
 * therefore describes the project's contents, not one document.
 */
export interface ProjectInvitePreview {
  kind: 'project';
  /** The file the invite opens at; listed first among the contents. */
  fileName: string;
  /** Up to MAX_PREVIEW_FILES other paths in the project. */
  topFiles: string[];
  fileCount: number;
  contributorInitials: string[];
}

export type InvitePreview = CollectionInvitePreview | ProjectInvitePreview;

/** Post-join navigation target for collection invites. */
export interface InviteStart {
  /** Index doc id of the project to open (without 'automerge:' prefix). */
  indexDocId: string;
  /** File to open in the editor. */
  filePath: string;
}

// ============================================================================
// Size caps (keep invite URLs short)
// ============================================================================

export const MAX_PREVIEW_PROJECTS = 3;
export const MAX_PREVIEW_FILES = 2;
const MAX_INITIALS = 4;
const MAX_MEMBER_NAMES = 3;

const WIRE_VERSION = 1;

// ============================================================================
// base64url helpers (UTF-8 safe)
// ============================================================================

function toBase64Url(json: string): string {
  const bytes = new TextEncoder().encode(json);
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function fromBase64Url(value: string): string | undefined {
  try {
    const binary = atob(value.replace(/-/g, '+').replace(/_/g, '/'));
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return undefined;
  }
}

const isStringArray = (v: unknown): v is string[] =>
  Array.isArray(v) && v.every((s) => typeof s === 'string');

// ============================================================================
// Preview codec
// ============================================================================

export function encodeInvitePreview(preview: InvitePreview): string {
  if (preview.kind === 'collection') {
    return toBase64Url(
      JSON.stringify({
        v: WIRE_VERSION,
        k: 'c',
        p: preview.projects.slice(0, MAX_PREVIEW_PROJECTS).map((p) => ({
          n: p.name,
          f: p.topFiles.slice(0, MAX_PREVIEW_FILES),
          c: p.fileCount,
          i: p.contributorInitials.slice(0, MAX_INITIALS),
        })),
        t: preview.totalProjects,
        m: preview.memberFirstNames.slice(0, MAX_MEMBER_NAMES),
      }),
    );
  }
  return toBase64Url(
    JSON.stringify({
      v: WIRE_VERSION,
      k: 'p',
      f: preview.fileName,
      s: preview.topFiles.slice(0, MAX_PREVIEW_FILES),
      c: preview.fileCount,
      i: preview.contributorInitials.slice(0, MAX_INITIALS),
    }),
  );
}

export function decodeInvitePreview(value: string | null | undefined): InvitePreview | undefined {
  if (!value) return undefined;
  const json = fromBase64Url(value);
  if (json === undefined) return undefined;
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch {
    return undefined;
  }
  if (typeof raw !== 'object' || raw === null) return undefined;
  const wire = raw as Record<string, unknown>;
  if (wire.v !== WIRE_VERSION) return undefined;

  if (wire.k === 'c') {
    if (!Array.isArray(wire.p) || typeof wire.t !== 'number' || !isStringArray(wire.m)) {
      return undefined;
    }
    const projects: CollectionPreviewProject[] = [];
    for (const entry of wire.p.slice(0, MAX_PREVIEW_PROJECTS)) {
      const p = entry as Record<string, unknown>;
      if (typeof p?.n !== 'string' || !isStringArray(p.f) || typeof p.c !== 'number' || !isStringArray(p.i)) {
        return undefined;
      }
      projects.push({
        name: p.n,
        topFiles: p.f.slice(0, MAX_PREVIEW_FILES),
        fileCount: p.c,
        contributorInitials: p.i.slice(0, MAX_INITIALS),
      });
    }
    return { kind: 'collection', projects, totalProjects: wire.t, memberFirstNames: wire.m };
  }

  // 'p' is the current project marker; 'd' is the short-lived 'document'
  // spelling from this feature's own development, accepted so links
  // generated while dogfooding keep resolving.
  if (wire.k === 'p' || wire.k === 'd') {
    if (typeof wire.f !== 'string' || !isStringArray(wire.s) || typeof wire.c !== 'number' || !isStringArray(wire.i)) {
      return undefined;
    }
    return {
      kind: 'project',
      fileName: wire.f,
      topFiles: wire.s.slice(0, MAX_PREVIEW_FILES),
      fileCount: wire.c,
      contributorInitials: wire.i.slice(0, MAX_INITIALS),
    };
  }

  return undefined;
}

// ============================================================================
// Start-target codec
// ============================================================================

export function encodeInviteStart(start: InviteStart): string {
  return toBase64Url(JSON.stringify({ v: WIRE_VERSION, d: start.indexDocId, f: start.filePath }));
}

export function decodeInviteStart(value: string | null | undefined): InviteStart | undefined {
  if (!value) return undefined;
  const json = fromBase64Url(value);
  if (json === undefined) return undefined;
  try {
    const wire = JSON.parse(json) as Record<string, unknown>;
    if (wire?.v !== WIRE_VERSION || typeof wire.d !== 'string' || typeof wire.f !== 'string') {
      return undefined;
    }
    return { indexDocId: wire.d, filePath: wire.f };
  } catch {
    return undefined;
  }
}
