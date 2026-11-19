/**
 * Attach a folder that already holds a git clone to a project (GE-10, G21): the preconditions, the identity check
 * (root commits; blob overlap for a shallow clone), and the mapping between the folder and the project's path prefix.
 * Pure of `vscode`. Every refusal says why and whether a mirror is the way out.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { META_DIR } from '../ignore';
import { META_FILE } from '../meta';

/** The `repo` entry of a version 4 index (contract: "Index version 4"). */
export interface RepoIdentity {
  roots: string[];
  prefix: string;
}

/** What attach needs to know about the share. */
export interface AttachShare {
  repo?: RepoIdentity;
  /** The registry's `(path, blob)` pairs, for the shallow-clone fallback. Project-relative paths. */
  registry?: { path: string; blob: string }[];
}

export type AttachRefusalReason = 'no-repo-entry' | 'not-a-repo' | 'already-bound' | 'identity-mismatch' | 'wrong-subdirectory';

export type AttachResult =
  | { ok: true; projectRoot: string; identity: RepoIdentity; shallow: boolean }
  | { ok: false; reason: AttachRefusalReason; message: string; offerMirror: boolean };

/** A shallow clone passes when at least this share of the registry's paths hold the same blob in the clone's HEAD. */
export const SHALLOW_OVERLAP_MIN = 0.5;

const refuse = (reason: AttachRefusalReason, message: string, offerMirror: boolean): AttachResult => ({
  ok: false,
  reason,
  message,
  offerMirror,
});

const bound = (dir: string) => fs.stat(path.join(dir, META_DIR, META_FILE)).then(() => true, () => false);

export interface CheckAttachOptions {
  /** The folder the user picked: the repository root or a directory inside it. */
  dir: string;
  share: AttachShare;
}

export async function checkAttach({ dir, share }: CheckAttachOptions): Promise<AttachResult> {
  const identity = share.repo;
  if (!identity) {
    return refuse('no-repo-entry', 'This project was not shared from a git repository, so there is nothing to attach a clone to.', true);
  }
  if (await bound(dir)) {
    return refuse('already-bound', `${dir} is already bound to a project (${META_DIR}/${META_FILE} exists). Rebind it instead.`, false);
  }
  const { createGitProbe } = await import('@quarto/quarto-git-epochs/probe');
  const probe = createGitProbe({ cwd: dir });
  let here: string;
  try {
    await probe.ready();
    here = await probe.showPrefix();
  } catch {
    return refuse('not-a-repo', `${dir} is not inside a git repository.`, true);
  }

  // Map the folder to the project root: the project lives at `identity.prefix` inside the repository.
  const want = identity.prefix.replace(/^\/+|\/+$/g, '');
  const have = here.replace(/^\/+|\/+$/g, '');
  let projectRoot: string;
  if (have === want) projectRoot = dir;
  else if (have === '' && want !== '') projectRoot = path.join(dir, ...want.split('/'));
  else {
    return refuse(
      'wrong-subdirectory',
      `This project lives in "${want || '(the repository root)'}" of its repository, but ${dir} is "${have || '(the repository root)'}". Open the repository root or the project folder.`,
      false,
    );
  }
  if (projectRoot !== dir && (await bound(projectRoot))) {
    return refuse('already-bound', `${projectRoot} is already bound to a project. Rebind it instead.`, false);
  }
  const rootProbe = projectRoot === dir ? probe : createGitProbe({ cwd: projectRoot });
  if (projectRoot !== dir) await rootProbe.ready().catch(() => {});

  const verified = await verifyIdentity(rootProbe, identity, share.registry);
  if (!verified.ok) return refuse('identity-mismatch', verified.message, true);
  return { ok: true, projectRoot, identity, shallow: verified.shallow };
}

type Probe = Awaited<ReturnType<typeof import('@quarto/quarto-git-epochs/probe').createGitProbe>>;

/**
 * The identity check, also run at every startup (G19). Root commits when the clone has its history; the blob overlap
 * with the registry when it is shallow (its root is then its oldest fetched commit, not the project's).
 */
export async function verifyIdentity(
  probe: Pick<Probe, 'rootCommits' | 'isShallow' | 'lsTree'>,
  identity: RepoIdentity,
  registry?: { path: string; blob: string }[],
): Promise<{ ok: true; shallow: boolean } | { ok: false; message: string }> {
  let shallow = false;
  try {
    shallow = await probe.isShallow();
    const roots = await probe.rootCommits();
    if (roots.some((r) => identity.roots.includes(r))) return { ok: true, shallow };
  } catch {
    return { ok: false, message: 'The repository has no commits yet, so it cannot be checked against the project.' };
  }
  if (shallow && registry && registry.length > 0) {
    const mine = new Map((await probe.lsTree()).map((f) => [f.path, f.blob]));
    const paths = new Set(registry.map((e) => e.path));
    const matched = new Set(registry.filter((e) => mine.get(e.path) === e.blob).map((e) => e.path));
    if (matched.size / paths.size >= SHALLOW_OVERLAP_MIN) return { ok: true, shallow };
    return { ok: false, message: `This shallow clone shares only ${matched.size} of ${paths.size} files with the project, so it does not look like the same repository.` };
  }
  return { ok: false, message: 'This repository does not share a root commit with the project, so it is not the same repository.' };
}
