/**
 * The import transaction (document import I22): store an import's images,
 * then its qmd, and undo what this import created if any step fails.
 *
 * Write order. Images first, the qmd last: the qmd is the commit point. A
 * crash or closed tab midway leaves at worst unreferenced images in
 * `<stem>_media/`, never a qmd whose links are broken. No Automerge write is
 * atomic across files, so a failure rolls back by deleting what this import
 * created. A deduplicated image (the project already held identical bytes at
 * that path) is not ours to delete, and neither is a path another client has
 * replaced since we wrote it.
 *
 * The sync calls are injected so the transaction is testable without a
 * mounted editor; opening the new qmd is the caller's job (it needs editor
 * state).
 */
import { createBinaryFile, createFileIfAbsent, deleteFile, getIndexHandle } from '@quarto/preview-runtime';
import type { ImportDiagnostic, ImportHostCode, ImportOutcome } from '../pandoc/importService';
import { importDoc } from '../strings';

export interface ImportStorageDeps {
  createBinaryFile(path: string, bytes: Uint8Array, mimeType: string): Promise<{ docId: string; path: string; deduplicated: boolean }>;
  createFileIfAbsent(path: string, content: string): Promise<{ created: boolean; docId?: string }>;
  deleteFile(path: string): void;
  /** The document id the project's index holds for `path` right now, if any. */
  indexedDocId(path: string): string | undefined;
}

export type CommitResult =
  /** `diagnostics` is the import's own report (no storage diagnostics on success). */
  | { ok: true; qmdPath: string; qmdDocId: string; qmd: string; diagnostics: ImportDiagnostic[] }
  /** `diagnostics` leads with the storage failure, then any cleanup problems, then the import's own report. */
  | { ok: false; diagnostics: ImportDiagnostic[] };

const hostError = (code: ImportHostCode, message: string, path: string): ImportDiagnostic => ({ origin: 'host', kind: 'error', code, message, path });

const causeOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Raised internally to stop at the failing step with its diagnostic. */
class StepFailed extends Error {
  readonly diagnostic: ImportDiagnostic;
  constructor(diagnostic: ImportDiagnostic) {
    super(diagnostic.origin === 'host' ? diagnostic.message : 'import step failed');
    this.diagnostic = diagnostic;
  }
}

export async function commitImport(
  outcome: Extract<ImportOutcome, { ok: true }>,
  qmdPath: string,
  deps: ImportStorageDeps,
): Promise<CommitResult> {
  /** What this import created, in creation order. */
  const created: { path: string; docId: string }[] = [];
  const total = outcome.media.length;
  let qmdResult: Awaited<ReturnType<ImportStorageDeps['createFileIfAbsent']>> | undefined;

  try {
    for (const [i, m] of outcome.media.entries()) {
      const step = importDoc.stepImage(i + 1, total);
      let result: Awaited<ReturnType<ImportStorageDeps['createBinaryFile']>>;
      try {
        result = await deps.createBinaryFile(m.projectPath, m.bytes, m.mimeType);
      } catch (e) {
        throw new StepFailed(hostError('import-write-failed', importDoc.writeFailed(step, m.projectPath, causeOf(e)), m.projectPath));
      }
      // Nothing was written for a hit on identical bytes: not ours to undo.
      if (result.deduplicated) continue;
      // Record before comparing paths, so a write the client renamed is still cleaned up.
      created.push({ path: result.path, docId: result.docId });
      if (result.path !== m.projectPath) {
        throw new StepFailed(hostError('import-write-failed', importDoc.writeFailed(step, m.projectPath, importDoc.writeFailedRenamed(m.projectPath, result.path)), m.projectPath));
      }
    }

    try {
      qmdResult = await deps.createFileIfAbsent(qmdPath, outcome.qmd);
    } catch (e) {
      throw new StepFailed(hostError('import-write-failed', importDoc.writeFailed(importDoc.stepDocument, qmdPath, causeOf(e)), qmdPath));
    }
    if (!qmdResult?.created) {
      throw new StepFailed(hostError('import-write-failed', importDoc.writeFailed(importDoc.stepDocument, qmdPath, importDoc.writeFailedAppeared(qmdPath)), qmdPath));
    }
  } catch (e) {
    const failure = e instanceof StepFailed ? e.diagnostic : hostError('import-write-failed', importDoc.writeFailed(importDoc.stepDocument, qmdPath, causeOf(e)), qmdPath);
    return { ok: false, diagnostics: [failure, ...rollback(created, deps), ...outcome.diagnostics] };
  }

  return { ok: true, qmdPath, qmdDocId: qmdResult?.docId ?? '', qmd: outcome.qmd, diagnostics: outcome.diagnostics };
}

/** Delete what this import created, newest first. Returns a diagnostic for each file that could not be removed. */
function rollback(created: { path: string; docId: string }[], deps: ImportStorageDeps): ImportDiagnostic[] {
  const problems: ImportDiagnostic[] = [];
  for (const { path, docId } of [...created].reverse()) {
    try {
      // Only delete what is still ours: another client may have replaced the entry.
      if (deps.indexedDocId(path) !== docId) {
        problems.push(hostError('import-cleanup-failed', importDoc.cleanupReplaced(path), path));
        continue;
      }
      deps.deleteFile(path);
    } catch (e) {
      problems.push(hostError('import-cleanup-failed', importDoc.cleanupDeleteFailed(path, causeOf(e)), path));
    }
  }
  return problems;
}

/** The real sync layer. */
export function defaultImportStorageDeps(): ImportStorageDeps {
  return {
    createBinaryFile,
    createFileIfAbsent,
    deleteFile,
    indexedDocId: (path) => getIndexHandle()?.doc()?.files?.[path],
  };
}
