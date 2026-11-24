/**
 * Find the `q2` binary (plan D-D): the `q2.path` setting, then `q2` on
 * PATH, then `~/.local/bin` (the default of both install.sh and
 * install.ps1). No subprocess is spawned to search: PATH is walked here so
 * the lookup behaves the same on every platform.
 */
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';

export interface BinaryLookup {
  /** The configured `q2.path`, possibly empty. */
  configured: string;
  /** `process.env.PATH`. */
  pathEnv: string | undefined;
  /** `process.env.PATHEXT` (Windows). */
  pathExt: string | undefined;
  platform: NodeJS.Platform;
  home: string;
  /** Workspace folder used to resolve a relative `q2.path`. */
  workspaceRoot?: string;
  isFile?: (p: string) => boolean;
}

export type BinaryResult =
  | { found: true; path: string; source: 'setting' | 'path' | 'install-dir' }
  | { found: false; reason: string };

const defaultIsFile = (p: string): boolean => {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
};

function expandHome(p: string, home: string): string {
  if (p === '~') return home;
  if (p.startsWith('~/') || p.startsWith('~\\')) return path.join(home, p.slice(2));
  return p;
}

function candidatesFor(dir: string, platform: NodeJS.Platform, pathExt: string | undefined): string[] {
  if (platform !== 'win32') return [path.join(dir, 'q2')];
  const exts = (pathExt ?? '.EXE;.CMD;.BAT').split(';').filter((e) => e.length > 0);
  return exts.map((ext) => path.join(dir, `q2${ext.toLowerCase()}`));
}

export function findQ2(lookup: BinaryLookup): BinaryResult {
  const isFile = lookup.isFile ?? defaultIsFile;
  const configured = lookup.configured.trim();
  if (configured !== '') {
    let p = expandHome(configured, lookup.home);
    if (!path.isAbsolute(p) && lookup.workspaceRoot) p = path.join(lookup.workspaceRoot, p);
    if (isFile(p)) return { found: true, path: p, source: 'setting' };
    // `target/debug/q2` names `q2.exe` on Windows.
    if (lookup.platform === 'win32' && path.extname(p) === '' && isFile(`${p}.exe`)) {
      return { found: true, path: `${p}.exe`, source: 'setting' };
    }
    return { found: false, reason: `q2.path is set to "${configured}", but no file exists at ${p}.` };
  }
  const sep = lookup.platform === 'win32' ? ';' : ':';
  for (const dir of (lookup.pathEnv ?? '').split(sep)) {
    if (dir === '') continue;
    for (const candidate of candidatesFor(dir, lookup.platform, lookup.pathExt)) {
      if (isFile(candidate)) return { found: true, path: candidate, source: 'path' };
    }
  }
  const installDir = path.join(lookup.home, '.local', 'bin');
  const installed = path.join(installDir, lookup.platform === 'win32' ? 'q2.exe' : 'q2');
  if (isFile(installed)) return { found: true, path: installed, source: 'install-dir' };
  return { found: false, reason: 'Could not find `q2` on PATH or in ~/.local/bin.' };
}

export function findQ2FromEnvironment(configured: string, workspaceRoot?: string): BinaryResult {
  return findQ2({
    configured,
    pathEnv: process.env.PATH,
    pathExt: process.env.PATHEXT,
    platform: process.platform,
    home: os.homedir(),
    workspaceRoot,
  });
}
