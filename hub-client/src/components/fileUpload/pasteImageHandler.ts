/**
 * Ingest orchestration for images pasted into the source editor
 * (bd-706b0ixu; design:
 * claude-notes/plans/2026-08-27-paste-image-clipboard.md §D4/§D5).
 *
 * A factory over injected deps so the whole flow — size validation,
 * sequential ingest, filename generation, selection-as-alt-text,
 * multi-file joining, file-switch guard — is unit-testable without
 * Monaco or jsdom. Editor.tsx supplies the real deps and the DOM-facing
 * `paste`-event wrapper.
 */

import { buildDropMarkdown } from './dropMarkdown';
import { pastedImageFilename, sanitizeAltText } from './pasteImages';
import { resolveDefaultDestination } from './resolveDefaultDestination';

/** Line/column range in Monaco's 1-based coordinates. */
export interface PasteRange {
  startLineNumber: number;
  startColumn: number;
  endLineNumber: number;
  endColumn: number;
}

/** The slice of the Monaco editor the handler needs. */
export interface PasteImageEditor {
  /** Current selection (a cursor is an empty selection); null if none. */
  getSelection(): PasteRange | null;
  /** Text covered by a range (`''` for an empty range). */
  getTextInRange(range: PasteRange): string;
  /** Replace `range` with `text` as a user-undoable edit. */
  replaceRange(range: PasteRange, text: string): void;
}

export interface CreatePasteImageHandlerDeps {
  /** Project-root-relative path of the open document, or null. */
  getCurrentFilePath(): string | null;
  getEditor(): PasteImageEditor | null;
  /** Read bytes + hash + MIME (resourceService.processFileForUpload). */
  processFile(file: File): Promise<{
    content: Uint8Array;
    mimeType: string;
    hash: string;
  }>;
  /** CRDT binary-file create; returns the final (possibly renamed) path. */
  createBinaryFile(
    path: string,
    content: Uint8Array,
    mimeType: string
  ): Promise<{ path: string }>;
  /** FILE_SIZE_LIMITS.MAX_FILE_SIZE in production. */
  maxFileSize: number;
  onError(message: string): void;
}

export type PasteImageHandler = (files: File[]) => Promise<boolean>;

/**
 * Build the async ingest handler. The caller has already classified the
 * payload as 'take-over' (see `classifyPastePayload`) and called
 * `preventDefault()`; this function does the rest and resolves to true
 * iff a markdown reference was inserted.
 */
export function createPasteImageHandler(
  deps: CreatePasteImageHandlerDeps
): PasteImageHandler {
  return async (files: File[]): Promise<boolean> => {
    const editor = deps.getEditor();
    if (!editor) return false;

    // Capture insertion context at paste time: the async ingest below
    // must not land in a different document (§D4 cursor guard).
    const pathAtPaste = deps.getCurrentFilePath();
    const selection = editor.getSelection();
    if (!selection) return false;
    const selectedText = editor.getTextInRange(selection);

    const destination = resolveDefaultDestination({ selection: pathAtPaste });

    const createdPaths: string[] = [];
    for (const file of files) {
      if (file.size > deps.maxFileSize) {
        const sizeMB = (file.size / (1024 * 1024)).toFixed(2);
        const maxMB = deps.maxFileSize / (1024 * 1024);
        deps.onError(
          `Pasted image (${sizeMB} MB) exceeds the maximum allowed size (${maxMB} MB)`
        );
        continue;
      }

      try {
        const { content, mimeType, hash } = await deps.processFile(file);
        const filename = pastedImageFilename(hash, mimeType);
        if (!filename) {
          // Unreachable when the caller classified the payload, but the
          // classifier and this handler are separately callable.
          deps.onError(`Unsupported pasted image type: ${mimeType}`);
          continue;
        }
        const targetPath = destination ? `${destination}/${filename}` : filename;
        const result = await deps.createBinaryFile(targetPath, content, mimeType);
        createdPaths.push(result.path);
      } catch (err) {
        deps.onError(
          `Failed to ingest pasted image: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    }

    if (createdPaths.length === 0) return false;

    // File-switch guard: if the user changed documents while we hashed
    // and created, inserting at the captured range would corrupt the
    // wrong file. The created binaries are kept — they are ordinary
    // project files the user can reference or delete.
    if (deps.getCurrentFilePath() !== pathAtPaste) return false;
    const editorNow = deps.getEditor();
    if (!editorNow) return false;

    // Selection-as-alt only for a single-file paste; multi-file pastes
    // get plain references, space-separated (§D4 — newlines would rely
    // on lazy continuation inside blockquotes/indented lists).
    const alt = createdPaths.length === 1 ? sanitizeAltText(selectedText) : '';
    const markdown = createdPaths
      .map((p, i) => buildDropMarkdown('image', pathAtPaste, p, i === 0 ? alt : ''))
      .join(' ');

    editorNow.replaceRange(selection, markdown);
    return true;
  };
}
