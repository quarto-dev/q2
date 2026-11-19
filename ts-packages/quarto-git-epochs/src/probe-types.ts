/**
 * The probe interface and the plain data it returns. Browser-safe (types only); the subprocess
 * implementation `createGitProbe` lives in the node-only `./probe` entry.
 */

/** A gate marker found on disk. `lock` markers go stale after 60 s; `state` markers pause the gate indefinitely. */
export interface MarkerState {
  name: string;
  kind: 'lock' | 'state';
  mtimeMs: number;
}

/** What `cheapState()` reads with no subprocess: marker stats and the reflog file's size and mtime. */
export interface CheapState {
  markers: MarkerState[];
  /** null when there is no reflog (no `logs/HEAD`, e.g. `core.logAllRefUpdates=false`, or colocated jj only). Not an error. */
  reflog: { size: number; mtimeMs: number } | null;
}

/**
 * A place in the HEAD reflog. `sha` is the new-sha of the entry; `lines` is the entry count at that point.
 * With `lines` the match is exact; without it the last entry with that sha is used.
 */
export interface ReflogMark {
  sha: string;
  lines?: number;
}

export interface ReflogSlice {
  /** Subjects of the entries after the mark, oldest first. */
  subjects: string[];
  /** The mark for the newest entry (null when the reflog is empty). */
  tail: ReflogMark | null;
  /** False when the mark's sha was absent from the reflog: `subjects` then holds every entry. */
  markFound: boolean;
}

export interface TrackedFile {
  /** Project-relative, always `/` separated. */
  path: string;
  /** Git object id of the blob at HEAD. */
  blob: string;
  /** `100644` or `100755`; symlinks and submodules are filtered out. */
  mode: string;
}

/** Git is missing, refused to run here, or this is not a repository. The controller goes `off`. */
export class GitUnavailableError extends Error {
  readonly code = 'git-unavailable';
  constructor(message: string) {
    super(message);
    this.name = 'GitUnavailableError';
  }
}

export interface GitProbe {
  /** HEAD's commit id; null for an unborn repository. Rejects with `GitUnavailableError`, or another error when HEAD cannot be resolved right now. */
  headSha(): Promise<string | null>;
  /** The project path inside the repo, no trailing slash; empty at the repo root. */
  showPrefix(): Promise<string>;
  rootCommits(): Promise<string[]>;
  isShallow(): Promise<boolean>;
  /** Tracked regular files under the project prefix, at HEAD. */
  lsTree(): Promise<TrackedFile[]>;
  /** Blob content by id through one `cat-file --batch` stream; missing ids are absent from the map. */
  catBlobs(ids: string[]): Promise<Map<string, Uint8Array>>;
  /** Reflog entries after the mark; null when there is no reflog. */
  reflogSince(mark: ReflogMark | null): Promise<ReflogSlice | null>;
  /** Synchronous, no subprocess. */
  cheapState(): CheapState;
}

/** The part of the probe the controller uses. */
export type ControllerProbe = Pick<GitProbe, 'headSha' | 'reflogSince' | 'cheapState'>;
