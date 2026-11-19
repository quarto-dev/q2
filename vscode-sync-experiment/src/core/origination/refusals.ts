/**
 * Refusals for "Share this folder" (GE-12, G24): a bound folder, a repository with no commits, and git missing inside
 * a repository. A plain folder needs no git. Also the folder pick for a window with several workspace folders.
 */
import { createGitProbe, GitUnavailableError } from '@quarto/quarto-git-epochs/probe';
import { InvalidProjectMetaError, readProjectMeta } from '../meta';
import { insideGitRepo } from './select';

export type OriginateRefusal = 'already-shared' | 'unborn' | 'git-unavailable';

export type OriginateCheck = { ok: true } | { ok: false; reason: OriginateRefusal; message: string };

export const MSG_ALREADY_SHARED = 'Already shared; showing its share information.';
export const MSG_UNBORN = 'Make an initial commit first.';
export const MSG_GIT_UNAVAILABLE = 'git is not available on the extension host.';

export async function checkOriginate(dir: string, opts: { gitPath?: string } = {}): Promise<OriginateCheck> {
  let bound: boolean;
  try {
    bound = (await readProjectMeta(dir)) !== null;
  } catch (e) {
    if (!(e instanceof InvalidProjectMetaError)) throw e;
    bound = true; // a folder with a project.json this version cannot read is still bound
  }
  if (bound) return { ok: false, reason: 'already-shared', message: MSG_ALREADY_SHARED };
  if (!(await insideGitRepo(dir))) return { ok: true };
  const probe = createGitProbe({ cwd: dir, ...(opts.gitPath ? { gitPath: opts.gitPath } : {}) });
  try {
    await probe.ready();
    if ((await probe.headSha()) === null) return { ok: false, reason: 'unborn', message: MSG_UNBORN };
  } catch (e) {
    if (e instanceof GitUnavailableError) return { ok: false, reason: 'git-unavailable', message: MSG_GIT_UNAVAILABLE };
    throw e;
  }
  return { ok: true };
}

/** One folder is used as is; several are put to a QuickPick (`ask`); none, or a cancelled pick, is null. */
export async function pickFolder(folders: string[], ask: (choices: string[]) => Promise<string | null>): Promise<string | null> {
  if (folders.length === 0) return null;
  if (folders.length === 1) return folders[0]!;
  return ask(folders);
}
