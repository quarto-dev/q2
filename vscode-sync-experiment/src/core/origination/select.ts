/**
 * What "Share this folder" publishes (GE-12, G24). A git clone publishes the tracked regular files under the project
 * prefix, minus the extension's ignore set; a plain folder publishes its own walk. A file over the publish cap is
 * skipped and listed (G32). The content is read from the disk at publish time, so uncommitted edits go as they are;
 * this module only chooses names and sizes. No `vscode` import.
 */
import { execFile } from 'node:child_process';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { isBinaryExtension } from '@quarto/quarto-automerge-schema';
import { createGitProbe, type NodeGitProbe } from '@quarto/quarto-git-epochs/probe';
import { META_DIR, isIgnoredPath, toSlashPath } from '../ignore';
import { META_FILE } from '../meta';

/** G32: files over this many bytes are not published. */
export const PUBLISH_CAP_BYTES = 10 * 1024 * 1024;

export interface PublishFile {
  /** Project-relative, `/` separated. */
  path: string;
  /** Bytes on the disk now. */
  size: number;
  binary: boolean;
  /** The blob at HEAD (git projects only; absent for a file git does not track, which never happens here). */
  blob?: string;
}

export interface Selection {
  kind: 'git' | 'plain';
  files: PublishFile[];
  /** Over the cap, not published. */
  skipped: { path: string; size: number }[];
  /** Bytes of the published files. */
  totalBytes: number;
  /** The current branch; null for a plain folder or a detached HEAD. */
  branch: string | null;
  /** The project path inside the repository (empty at the root and for a plain folder). */
  prefix: string;
  /** Root commits of the repository (empty for a plain folder). */
  roots: string[];
  head: string | null;
}

export interface SelectOptions {
  probe?: NodeGitProbe;
  gitPath?: string;
}

/** True when `dir` or a parent holds a `.git` entry (a directory, or a file for a worktree or submodule). */
export async function insideGitRepo(dir: string): Promise<boolean> {
  let cur = path.resolve(dir);
  for (;;) {
    if (await fs.stat(path.join(cur, '.git')).then(() => true, () => false)) return true;
    const up = path.dirname(cur);
    if (up === cur) return false;
    cur = up;
  }
}

function currentBranch(dir: string, gitPath: string): Promise<string | null> {
  return new Promise((resolve) => {
    execFile(gitPath, ['symbolic-ref', '--short', '-q', 'HEAD'], { cwd: dir, windowsHide: true }, (err, stdout) => {
      const out = String(stdout).trim();
      resolve(err || !out ? null : out);
    });
  });
}

/** Project-relative directories that carry their own `.quarto-hub/project.json`. */
function boundSubfolders(rels: string[]): Set<string> {
  const suffix = `/${META_DIR}/${META_FILE}`;
  const out = new Set<string>();
  for (const r of rels) if (r.endsWith(suffix)) out.add(r.slice(0, -suffix.length));
  return out;
}

async function walk(root: string): Promise<string[]> {
  const all: string[] = [];
  const visit = async (rel: string): Promise<void> => {
    for (const e of await fs.readdir(path.join(root, rel), { withFileTypes: true })) {
      const r = rel ? `${rel}/${e.name}` : e.name;
      if (e.isDirectory()) await visit(r);
      else if (e.isFile()) all.push(r);
    }
  };
  await visit('');
  return all;
}

export async function selectFiles(dir: string, opts: SelectOptions = {}): Promise<Selection> {
  const gitPath = opts.gitPath ?? 'git';
  let kind: Selection['kind'] = 'plain';
  let probe: NodeGitProbe | null = null;
  let rels: { path: string; blob?: string }[];
  let prefix = '';
  let roots: string[] = [];
  let head: string | null = null;
  let branch: string | null = null;
  if (await insideGitRepo(dir)) {
    probe = opts.probe ?? createGitProbe({ cwd: dir, gitPath });
    await probe.ready();
    kind = 'git';
    const [tracked, r, p, h] = await Promise.all([probe.lsTree(), probe.rootCommits(), probe.showPrefix(), probe.headSha()]);
    rels = tracked.map((t) => ({ path: toSlashPath(t.path), blob: t.blob }));
    roots = r;
    prefix = p;
    head = h;
    branch = await currentBranch(dir, gitPath);
  } else {
    rels = (await walk(dir)).map((p) => ({ path: p }));
  }
  const bound = boundSubfolders(rels.map((r) => r.path));
  const files: PublishFile[] = [];
  const skipped: Selection['skipped'] = [];
  let totalBytes = 0;
  for (const r of rels) {
    if (isIgnoredPath(r.path, bound)) continue;
    const st = await fs.stat(path.join(dir, r.path)).catch(() => null);
    if (!st?.isFile()) continue; // tracked but deleted on disk: nothing to publish
    if (st.size > PUBLISH_CAP_BYTES) {
      skipped.push({ path: r.path, size: st.size });
      continue;
    }
    files.push({ path: r.path, size: st.size, binary: isBinaryExtension(r.path), ...(r.blob ? { blob: r.blob } : {}) });
    totalBytes += st.size;
  }
  files.sort((a, b) => a.path.localeCompare(b.path));
  skipped.sort((a, b) => a.path.localeCompare(b.path));
  return { kind, files, skipped, totalBytes, branch, prefix, roots, head };
}
