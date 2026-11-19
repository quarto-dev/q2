/** `.quarto-hub/project.json`: the device-local binding of a folder to a project. */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { META_DIR } from './ignore';

export const META_FILE = 'project.json';
export const DEFAULT_SYNC_SERVER = 'wss://sync.automerge.org';

export interface ProjectMeta {
  indexDocId: string;
  syncServer: string;
  createdAt: string;
}

export async function readProjectMeta(root: string): Promise<ProjectMeta | null> {
  try {
    return JSON.parse(await fs.readFile(path.join(root, META_DIR, META_FILE), 'utf8')) as ProjectMeta;
  } catch {
    return null;
  }
}

export async function writeProjectMeta(root: string, meta: ProjectMeta): Promise<void> {
  await fs.mkdir(path.join(root, META_DIR), { recursive: true });
  await fs.writeFile(path.join(root, META_DIR, META_FILE), JSON.stringify(meta, null, 2) + '\n');
}
