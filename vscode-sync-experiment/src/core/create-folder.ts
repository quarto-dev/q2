/** Create a new project folder from a project's index document. No `vscode` import. */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import type { Repo } from '@automerge/automerge-repo/slim';
import {
  isBinaryDocument,
  isTextDocument,
  type BinaryDocumentContent,
  type IndexDocument,
} from '@quarto/quarto-automerge-schema';
import type { Logger } from './host';
import { META_DIR } from './ignore';
import { DEFAULT_SYNC_SERVER, writeProjectMeta, type ProjectMeta } from './meta';
import { findDoc, makeRepo } from './repo';
import { UnsafePathError, safeJoin } from './safe-path';
import type { TextDoc } from './text-binding';

export type FileDoc = TextDoc | BinaryDocumentContent;

/** Write one file doc under `root`; `rel` comes from a doc, so it is validated (G36). */
export async function writeFileDoc(root: string, rel: string, doc: FileDoc): Promise<void> {
  const abs = await safeJoin(root, rel);
  await fs.mkdir(path.dirname(abs), { recursive: true });
  if (isTextDocument(doc)) await fs.writeFile(abs, doc.text, 'utf8');
  else if (isBinaryDocument(doc)) await fs.writeFile(abs, doc.content);
}

/**
 * Folder name for a project. The hub's display name lives in each user's
 * project-set document, not in the index, so the best available source is
 * the `title:` in `_quarto.yml`; otherwise fall back to the id.
 */
async function projectFolderName(repo: Repo, files: Record<string, string>, indexDocId: string): Promise<string> {
  let title: string | undefined;
  if (files['_quarto.yml']) {
    const yml = (await findDoc<FileDoc>(repo, files['_quarto.yml'])).doc();
    if (isTextDocument(yml)) title = /^\s*title:\s*["']?(.+?)["']?\s*$/m.exec(yml.text)?.[1];
  }
  // Same sanitisation as hub-client's export (quarto-sync-client/project-folder-name.ts).
  const cleaned = (title ?? '')
    .replace(/[\u0000- <>:"/\\|?*]/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-+/, '')
    .replace(/[-. ]+$/, '');
  return cleaned || `quarto-hub-${indexDocId.slice(0, 8)}`;
}

/**
 * Write every file and explicit folder of the project's index under `root`.
 * Each key comes from a doc, so it is validated (G36): a rejected key is
 * skipped and logged, and the other keys are still written.
 */
export async function writeProjectFiles(repo: Repo, root: string, index: IndexDocument | undefined, log: Logger): Promise<void> {
  const files = index?.files ?? {};
  for (const [rel, docId] of Object.entries(files)) {
    try {
      await safeJoin(root, rel);
    } catch (err) {
      if (!(err instanceof UnsafePathError)) throw err;
      log.appendLine(`skipped index key: ${err.message}`);
      continue;
    }
    const handle = await findDoc<FileDoc>(repo, docId);
    const doc = handle.doc();
    if (doc) await writeFileDoc(root, rel, doc);
    log.appendLine(`wrote ${rel}`);
  }
  for (const folder of Object.keys(index?.folders ?? {})) {
    try {
      await fs.mkdir(await safeJoin(root, folder), { recursive: true });
    } catch (err) {
      if (!(err instanceof UnsafePathError)) throw err;
      log.appendLine(`skipped index folder: ${err.message}`);
    }
  }
}

export interface CreateFolderOptions {
  /** The sync server to dial and to record in `project.json`. */
  syncServer?: string;
}

/**
 * Moves the finished temp folder into place. Windows refuses a directory rename with `EPERM` (or `EBUSY`)
 * while any handle is open inside it, and a scanner or indexer can hold one for a moment after our own
 * writes settle, so the rename retries a few times with a growing pause (about a second in all). If it still
 * fails, the temp folder stays where it is and the error is thrown: there is no copy fallback, because a
 * copy that fails halfway would leave a plausible-looking project with missing files.
 */
export async function renameFolderIntoPlace(from: string, to: string): Promise<void> {
  for (let attempt = 0; ; attempt++) {
    try {
      await fs.rename(from, to);
      return;
    } catch (err) {
      const code = (err as NodeJS.ErrnoException).code;
      if ((code === 'EPERM' || code === 'EBUSY') && attempt < 8) {
        await new Promise((r) => setTimeout(r, 25 * (attempt + 1)));
        continue;
      }
      throw err;
    }
  }
}

/** Create `<parentDir>/<project name>` from the project and return its path. */
export async function createProjectFolder(
  indexDocId: string,
  parentDir: string,
  log: Logger,
  options: CreateFolderOptions = {},
): Promise<string> {
  const syncServer = options.syncServer ?? DEFAULT_SYNC_SERVER;
  // The storage dir is only known once we have a name; sync to a temp store
  // first, then move it into place.
  const tmpRoot = await fs.mkdtemp(path.join(parentDir, '.quarto-hub-tmp-'));
  await fs.mkdir(path.join(tmpRoot, META_DIR, 'automerge'), { recursive: true });
  const repo = makeRepo(tmpRoot, syncServer);
  let root = tmpRoot;
  try {
    const index = await findDoc<IndexDocument>(repo, indexDocId);
    const files = index.doc()?.files ?? {};
    const base = await projectFolderName(repo, files, indexDocId);
    root = path.join(parentDir, base);
    for (let n = 2; await fs.stat(root).then(() => true, () => false); n++) root = path.join(parentDir, `${base}-${n}`);
    const meta: ProjectMeta = { indexDocId, syncServer, createdAt: new Date().toISOString(), mode: 'mirror' };
    await writeProjectMeta(tmpRoot, meta);
    await writeProjectFiles(repo, tmpRoot, index.doc(), log);
    // Open files are written by VS Code's autosave (see ownership rule in
    // project-sync.ts); a formatter would broadcast its reflow to every collaborator.
    await fs.mkdir(path.join(tmpRoot, '.vscode'), { recursive: true });
    await fs.writeFile(
      path.join(tmpRoot, '.vscode', 'settings.json'),
      JSON.stringify({ 'files.autoSave': 'afterDelay', 'files.autoSaveDelay': 300, 'editor.formatOnSave': false }, null, 2) + '\n',
    );
    await repo.flush();
  } finally {
    await repo.shutdown().catch(() => {});
  }
  // `repo.shutdown()` has flushed and settled every storage write (the Node storage adapter keeps no file
  // open between calls and has no close of its own), so what can still hold the folder is outside this process.
  await renameFolderIntoPlace(tmpRoot, root);
  return root;
}
