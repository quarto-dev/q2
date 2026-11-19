/** `.quarto-hub/project.json`: the device-local binding of a folder to a project. */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { META_DIR } from './ignore';
import type { RepoIdentity } from './clone/attach';

export const META_FILE = 'project.json';
export const DEFAULT_SYNC_SERVER = 'wss://sync.automerge.org';

/**
 * How the folder relates to the project (G19). `mirror`: a folder the
 * extension created from a project; it follows the project owner and rewrites
 * its own disk. `editor-only`: VS Code opened on a directory a hub serves; the
 * hub owns the disk and the extension never writes a file. `clone`: a folder
 * with a repository of its own (GE-10).
 */
export const PROJECT_MODES = ['mirror', 'editor-only', 'clone'] as const;
export type ProjectMode = (typeof PROJECT_MODES)[number];

export interface ProjectMeta {
  indexDocId: string;
  syncServer: string;
  createdAt: string;
  /** Always set on a read: a file with no mode is a mirror. */
  mode?: ProjectMode;
  /** Clone mode: the repository identity verified at attach and re-verified at every startup (G19, G21). */
  repo?: RepoIdentity;
  /** Set only in the folder that originated the project (GE-12, G16); it must equal the index's `owner` key to write the pointer. */
  ownerId?: string;
  /** Originating folder: the project name and the file a share link opens (GE-12). */
  name?: string;
  file?: string;
}

/** `project.json` parses but says something this version does not understand. */
export class InvalidProjectMetaError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'InvalidProjectMetaError';
  }
}

/** The metadata, or null when the file is missing or is not JSON. Throws {@link InvalidProjectMetaError} for an unknown mode. */
export async function readProjectMeta(root: string): Promise<(ProjectMeta & { mode: ProjectMode }) | null> {
  const file = path.join(root, META_DIR, META_FILE);
  let meta: ProjectMeta;
  try {
    meta = JSON.parse(await fs.readFile(file, 'utf8')) as ProjectMeta;
  } catch {
    return null;
  }
  const mode = meta.mode ?? 'mirror';
  if (!(PROJECT_MODES as readonly string[]).includes(mode)) {
    throw new InvalidProjectMetaError(
      `${file} has an unknown mode ${JSON.stringify(mode)}; the modes are ${PROJECT_MODES.join(', ')}.`,
    );
  }
  return { ...meta, mode };
}

export async function writeProjectMeta(root: string, meta: ProjectMeta): Promise<void> {
  await fs.mkdir(path.join(root, META_DIR), { recursive: true });
  await fs.writeFile(path.join(root, META_DIR, META_FILE), JSON.stringify(meta, null, 2) + '\n');
}
