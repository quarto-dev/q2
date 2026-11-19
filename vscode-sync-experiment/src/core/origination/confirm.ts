/**
 * The text of the "Share this folder" confirm (GE-12, G24). Pure: the VS Code layer shows it as a modal. The wording
 * is part of the contract with the user (what is published, to whom, whether it can be undone), so tests pin it.
 */
import * as path from 'node:path';
import { PUBLISH_CAP_BYTES } from './select';

export const PUBLIC_SERVER = 'wss://sync.automerge.org';

export interface ConfirmInput {
  server: string;
  projectName: string;
  kind: 'git' | 'plain';
  branch: string | null;
  fileCount: number;
  totalBytes: number;
  skipped: { path: string; size: number }[];
  dirty: string[];
  /** Every published path, for the secret-looking check. */
  published: string[];
}

export interface Confirm {
  title: string;
  body: string;
  confirmLabel: string;
}

const NAME_INDENT = ' '.repeat(20);
const MAX_NAMES = 3;

/** Names that look like credentials: `.env`, `.Renviron`, `*.pem`, `*secret*`, `*token*`. */
export function looksSecret(p: string): boolean {
  const base = path.posix.basename(p.replace(/\\/g, '/')).toLowerCase();
  return /^\.env(\..*)?$/.test(base) || base === '.renviron' || base.endsWith('.pem') || base.includes('secret') || base.includes('token');
}

export function formatBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${Math.ceil(n / 1024)} KB`;
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

function nameLines(names: string[]): string[] {
  const lines = names.slice(0, MAX_NAMES).map((n) => `${NAME_INDENT}${n}`);
  if (names.length > MAX_NAMES) lines.push(`${NAME_INDENT}and ${names.length - MAX_NAMES} more`);
  return lines;
}

export function buildConfirm(i: ConfirmInput): Confirm {
  const isPublic = i.server === PUBLIC_SERVER;
  const lines: string[] = [
    isPublic
      ? "This publishes a copy of your project to a public server. Anyone who gets the link can read and edit it, and it can't be unpublished."
      : 'This publishes a copy of your project to that server. Anyone with access to that server and the project link can read and edit it.',
    '',
  ];
  const what = i.kind === 'git' ? `git repo, ${i.branch ? `branch ${i.branch}` : 'detached HEAD'}` : 'plain folder';
  lines.push(`  Project:        ${i.projectName}  (${what})`);
  lines.push(`  Files:          ${plural(i.fileCount, i.kind === 'git' ? 'tracked file' : 'file', i.kind === 'git' ? 'tracked files' : 'files')}, ${formatBytes(i.totalBytes)}`);
  if (i.skipped.length > 0) {
    lines.push(`  Skipped:        ${plural(i.skipped.length, 'file', 'files')} over ${PUBLISH_CAP_BYTES / (1024 * 1024)} MB (listed in the Quarto Hub output)`);
  }
  if (i.kind === 'git') {
    if (i.dirty.length === 0) lines.push('  Uncommitted:    none');
    else {
      lines.push(`  Uncommitted:    ${i.dirty.length === 1 ? '1 file will be shared as it is' : `${i.dirty.length} files will be shared as they are`} on disk:`);
      lines.push(...nameLines(i.dirty));
    }
  }
  const secrets = i.published.filter(looksSecret);
  if (secrets.length > 0) {
    lines.push('  Secret-looking files:');
    lines.push(...secrets.slice(0, MAX_NAMES).map((n) => `${NAME_INDENT}${n}`));
  }
  return { title: `Share this folder on ${i.server}?`, body: lines.join('\n'), confirmLabel: 'Share' };
}
