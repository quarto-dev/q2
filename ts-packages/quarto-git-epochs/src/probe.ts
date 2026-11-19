/**
 * Node-only entry (`@quarto/quarto-git-epochs/probe`): the git subprocess probe. Plumbing only, no shell,
 * `GIT_OPTIONAL_LOCKS=0`, a timeout per call, forward-slash output (G3, G4). Browser clients import only the
 * package root; this file uses `node:child_process`, `node:fs`, and `node:crypto`.
 */
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { normalizeText } from './arrival.js';
import { toForwardSlashes } from './paths.js';
import {
  type CheapState,
  type GitProbe,
  type MarkerState,
  type ReflogMark,
  type ReflogSlice,
  type TrackedFile,
  GitUnavailableError,
} from './probe-types.js';

export * from './probe-types.js';

export interface ProbeOptions {
  /** The project directory (inside the repository). */
  cwd: string;
  /** The git executable; default `git` (Node finds `git.exe` on Windows). */
  gitPath?: string;
  /** Per-call timeout; default 15 000 ms. */
  timeoutMs?: number;
}

/** Text compares after CRLF normalization; a file with a NUL byte is binary and compares by byte hash. */
export function contentKey(bytes: Uint8Array): string {
  const buf = Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (buf.subarray(0, 8000).includes(0)) return `sha256:${createHash('sha256').update(buf).digest('hex')}`;
  return normalizeText(buf.toString('utf8'));
}

const STATE_MARKERS = ['MERGE_HEAD', 'CHERRY_PICK_HEAD', 'REVERT_HEAD', 'rebase-merge', 'rebase-apply', 'sequencer', 'BISECT_LOG'];
const LOCK_MARKERS = ['index.lock'];
const JJ_LOCK = '.jj/working_copy/working_copy.lock';

interface RunResult {
  code: number;
  stdout: Buffer;
  stderr: string;
}

export interface NodeGitProbe extends GitProbe {
  /** Resolves the marker and reflog paths once (`--git-path`). `cheapState()` and `isColocatedJj()` need it first; any async call does it too. */
  ready(): Promise<void>;
  isColocatedJj(): boolean;
}

export function createGitProbe(opts: ProbeOptions): NodeGitProbe {
  const gitPath = opts.gitPath ?? 'git';
  const timeoutMs = opts.timeoutMs ?? 15_000;
  const env = { ...process.env, GIT_OPTIONAL_LOCKS: '0', GIT_TERMINAL_PROMPT: '0' };

  function run(args: string[], input?: Buffer | string): Promise<RunResult> {
    return new Promise((resolve, reject) => {
      let child;
      try {
        child = spawn(gitPath, args, { cwd: opts.cwd, env, shell: false, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
      } catch (e) {
        reject(new GitUnavailableError(`cannot start git: ${(e as Error).message}`));
        return;
      }
      const out: Buffer[] = [];
      const err: Buffer[] = [];
      let settled = false;
      const done = (f: () => void) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        f();
      };
      const timer = setTimeout(() => {
        child.kill();
        done(() => reject(new Error(`git ${args[0]} timed out after ${timeoutMs} ms`)));
      }, timeoutMs);
      child.stdout.on('data', (d: Buffer) => out.push(d));
      child.stderr.on('data', (d: Buffer) => err.push(d));
      child.on('error', (e: NodeJS.ErrnoException) =>
        done(() =>
          reject(e.code === 'ENOENT' || e.code === 'EACCES' ? new GitUnavailableError(`git not found or not runnable (${gitPath})`) : e),
        ),
      );
      child.on('close', (code) =>
        done(() => resolve({ code: code ?? -1, stdout: Buffer.concat(out), stderr: Buffer.concat(err).toString('utf8') })),
      );
      child.stdin.on('error', () => {}); // git may exit before reading all input
      child.stdin.end(input);
    });
  }

  async function ok(args: string[], input?: Buffer | string): Promise<string> {
    const r = await run(args, input);
    if (r.code !== 0) {
      if (/not a git repository|dubious ownership|safe\.directory/i.test(r.stderr)) {
        throw new GitUnavailableError(r.stderr.trim().split('\n')[0]);
      }
      throw new Error(`git ${args[0]} failed (${r.code}): ${r.stderr.trim()}`);
    }
    return r.stdout.toString('utf8');
  }

  interface Resolved {
    toplevel: string;
    prefix: string;
    markers: { name: string; kind: 'lock' | 'state'; path: string }[];
    reflogPath: string;
    jj: boolean;
  }
  let resolved: Promise<Resolved> | null = null;
  let resolvedSync: Resolved | null = null;

  /** Resolved once with `--git-path`, so linked worktrees and `GIT_DIR` setups find the right files. */
  function resolve(): Promise<Resolved> {
    resolved ??= (async () => {
      const names = [...LOCK_MARKERS, ...STATE_MARKERS, 'logs/HEAD'];
      const args = ['rev-parse', '--path-format=absolute', '--show-toplevel', '--show-prefix'];
      for (const n of names) args.push('--git-path', n);
      const lines = (await ok(args)).split('\n');
      const [toplevel, prefix, ...paths] = lines;
      if (!toplevel) throw new GitUnavailableError('not inside a work tree');
      const markers = [
        ...LOCK_MARKERS.map((name, i) => ({ name, kind: 'lock' as const, path: paths[i] })),
        ...STATE_MARKERS.map((name, i) => ({ name, kind: 'state' as const, path: paths[LOCK_MARKERS.length + i] })),
      ];
      let jj = false;
      try {
        jj = statSync(join(toplevel, '.jj')).isDirectory();
      } catch {
        /* no .jj */
      }
      const r: Resolved = {
        toplevel,
        prefix: prefix.replace(/\/$/, ''),
        markers,
        reflogPath: paths[names.length - 1],
        jj,
      };
      resolvedSync = r;
      return r;
    })();
    return resolved;
  }

  const statOrNull = (p: string) => {
    try {
      return statSync(p);
    } catch {
      return null;
    }
  };

  function parseReflog(path: string): { sha: string; subject: string }[] | null {
    let text: string;
    try {
      text = readFileSync(path, 'utf8');
    } catch {
      return null; // no logs/HEAD: "no reflog", not an error
    }
    return text
      .split('\n')
      .filter((l) => l.length > 0)
      .map((l) => {
        const tab = l.indexOf('\t');
        const head = tab < 0 ? l : l.slice(0, tab);
        return { sha: head.split(' ')[1] ?? '', subject: tab < 0 ? '' : l.slice(tab + 1) };
      });
  }

  const probe: NodeGitProbe = {
    async ready() {
      await resolve();
    },

    async headSha() {
      await resolve();
      const r = await run(['rev-parse', '--verify', '-q', 'HEAD']);
      if (r.code === 0) return r.stdout.toString('utf8').trim();
      // Unborn: HEAD names a branch that has no commit yet. Anything else is an unresolvable HEAD.
      const sym = await run(['symbolic-ref', '-q', 'HEAD']);
      if (sym.code === 0) {
        const ref = sym.stdout.toString('utf8').trim();
        const exists = await run(['show-ref', '--verify', '-q', ref]);
        if (exists.code !== 0) return null;
      }
      throw new Error(`cannot resolve HEAD: ${r.stderr.trim() || 'unknown error'}`);
    },

    async showPrefix() {
      return (await resolve()).prefix;
    },

    async rootCommits() {
      const out = await ok(['rev-list', '--max-parents=0', 'HEAD']);
      return out.split('\n').filter(Boolean);
    },

    async isShallow() {
      return (await ok(['rev-parse', '--is-shallow-repository'])).trim() === 'true';
    },

    async lsTree(): Promise<TrackedFile[]> {
      if ((await probe.headSha()) === null) return [];
      // Run in the project directory: paths come back relative to it and limited to it.
      const out = await ok(['ls-tree', '-r', '-z', 'HEAD']);
      const files: TrackedFile[] = [];
      for (const rec of out.split('\0')) {
        if (!rec) continue;
        const tab = rec.indexOf('\t');
        const [mode, type, blob] = rec.slice(0, tab).split(' ');
        if (type !== 'blob' || (mode !== '100644' && mode !== '100755')) continue;
        files.push({ path: toForwardSlashes(rec.slice(tab + 1)), blob, mode });
      }
      return files;
    },

    async catBlobs(ids: string[]) {
      const result = new Map<string, Uint8Array>();
      for (let i = 0; i < ids.length; i += 500) {
        const chunk = ids.slice(i, i + 500);
        const r = await run(['cat-file', '--batch'], chunk.join('\n') + '\n');
        if (r.code !== 0) throw new Error(`git cat-file failed (${r.code}): ${r.stderr.trim()}`);
        const buf = r.stdout;
        let pos = 0;
        while (pos < buf.length) {
          const nl = buf.indexOf(0x0a, pos);
          if (nl < 0) break;
          const header = buf.toString('utf8', pos, nl).split(' ');
          pos = nl + 1;
          if (header[1] === 'missing') continue;
          const size = Number(header[2]);
          result.set(header[0], new Uint8Array(buf.subarray(pos, pos + size)));
          pos += size + 1; // the content is followed by a newline
        }
      }
      return result;
    },

    async reflogSince(mark: ReflogMark | null): Promise<ReflogSlice | null> {
      const { reflogPath } = await resolve();
      const entries = parseReflog(reflogPath);
      if (!entries) return null;
      const tail = entries.length > 0 ? { sha: entries[entries.length - 1].sha, lines: entries.length } : null;
      const subjects = (from: number) => entries.slice(from).map((e) => e.subject);
      if (!mark) return { subjects: subjects(0), tail, markFound: true };
      if (mark.lines !== undefined && mark.lines <= entries.length && entries[mark.lines - 1]?.sha === mark.sha) {
        return { subjects: subjects(mark.lines), tail, markFound: true };
      }
      for (let i = entries.length - 1; i >= 0; i--) {
        if (entries[i].sha === mark.sha) return { subjects: subjects(i + 1), tail, markFound: true };
      }
      return { subjects: subjects(0), tail, markFound: false };
    },

    cheapState(): CheapState {
      const r = resolvedSync;
      if (!r) throw new Error('cheapState() before the first async probe call; call ready() first');
      const markers: MarkerState[] = [];
      for (const m of r.markers) {
        const st = statOrNull(m.path);
        if (st) markers.push({ name: m.name, kind: m.kind, mtimeMs: st.mtimeMs });
      }
      if (r.jj) {
        // The jj lock file may persist between operations; it is a `lock`, so it goes stale after 60 s and stops holding the gate.
        const st = statOrNull(join(r.toplevel, JJ_LOCK));
        if (st) markers.push({ name: JJ_LOCK, kind: 'lock', mtimeMs: st.mtimeMs });
      }
      const rl = statOrNull(r.reflogPath);
      return { markers, reflog: rl ? { size: rl.size, mtimeMs: rl.mtimeMs } : null };
    },

    isColocatedJj() {
      if (!resolvedSync) throw new Error('isColocatedJj() before ready()');
      return resolvedSync.jj;
    },
  };
  return probe;
}
