/**
 * useFileActions — everything the file tree and its toolbar can do to the
 * project in `ProjectProvider`: create, upload, drop, move, rename and
 * delete files and folders, with the dialogs those need.
 *
 * PROTOTYPE. Lifted from `Editor.tsx`, which still has its own copy. Left
 * out: document import (pandoc) on drop, and inserting a dropped image
 * into the open document.
 *
 * Returns `actions` (for `FileTree` and `FileSidebarToolbar`) and `dialogs`,
 * the dialogs those actions open. Each caller has its own dialog state, so
 * whoever takes the actions renders the dialogs.
 */

import { useCallback, useMemo, useState, type ReactNode } from 'react';
import {
  createBinaryFile,
  createFile,
  createFolder,
  deleteFile,
  deleteFolder,
  renameFile,
  type FileEntry,
} from '@quarto/preview-runtime';
import { isTextExtension, normalizeProjectPath } from '@quarto/preview-renderer/types/project';
import NewAssetDialog from '../components/NewAssetDialog';
import NewFileDialog from '../components/NewFileDialog';
import NewFolderDialog from '../components/NewFolderDialog';
import PlaceFileDialog, { type PlaceFileRequest } from '../components/PlaceFileDialog';
import type { FileSidebarProps } from '../components/FileSidebar';
import { processAssetFiles } from '../components/fileUpload/processAssetFiles';
import { processFileForUpload, sanitizeFilename } from '../services/resourceService';
import { dialogs as dialogStrings, fileSidebar } from '../strings';
import type { DroppedEntries } from '../utils/droppedEntries';
import { getAncestorPaths } from '../utils/fileTree';
import { joinPath } from '../utils/uniquePath';
import { useProject } from './ProjectProvider';

/** The handlers this hook provides, a subset of the file sidebar's props. */
export type FileActions = Required<Pick<
  FileSidebarProps,
  | 'onNewFile'
  | 'onNewFileIn'
  | 'onNewFolder'
  | 'onDeleteFolder'
  | 'onMoveFile'
  | 'onMoveFolder'
  | 'onUploadFiles'
  | 'onDropFiles'
  | 'onDeleteFile'
  | 'onRenameFile'
>>;

/** Sanitize each segment of a dropped relative path, keeping its folders. */
function sanitizeRelativePath(rel: string): string {
  return rel.split('/').filter((segment) => segment !== '').map(sanitizeFilename).join('/');
}

export function useFileActions(): { actions: FileActions; dialogs: ReactNode } {
  const { files, folders, currentFile, openFile } = useProject();
  const currentPath = currentFile?.path ?? null;

  // Dialog state. Null (or an empty queue) means closed.
  const [newFileName, setNewFileName] = useState<string | null>(null);
  const [newFolderParent, setNewFolderParent] = useState<string | null>(null);
  const [assetUpload, setAssetUpload] = useState<{ files: File[]; destination: string } | null>(null);
  // Moves and conflicting drops line up; the head is shown.
  const [placeQueue, setPlaceQueue] = useState<PlaceFileRequest[]>([]);

  const existingPaths = useMemo(() => files.map((file) => file.path), [files]);
  // Explicitly created folders plus those implied by file paths.
  const allFolders = useMemo(() => {
    const set = new Set<string>(folders);
    for (const file of files) for (const ancestor of getAncestorPaths(file.path)) set.add(ancestor);
    return Array.from(set).sort();
  }, [files, folders]);

  const createTextFile = useCallback(async (rawPath: string, content: string) => {
    const path = normalizeProjectPath(rawPath);
    try {
      await createFile(path, content);
      openFile(path);
    } catch (err) {
      console.error('Failed to create file:', err);
    }
  }, [openFile]);

  // Text becomes a text document so the editor can show it; the rest is binary.
  const uploadAsset = useCallback(async (file: File, targetPath: string) => {
    try {
      if (isTextExtension(targetPath)) {
        await createTextFile(targetPath, await file.text());
        return;
      }
      const { content, mimeType } = await processFileForUpload(file);
      await createBinaryFile(targetPath, content, mimeType);
    } catch (err) {
      console.error('Failed to upload file:', err);
    }
  }, [createTextFile]);

  const rename = useCallback((file: FileEntry, rawNewPath: string) => {
    const newPath = normalizeProjectPath(rawNewPath);
    if (newPath === '' || newPath === file.path) return;
    try {
      renameFile(file.path, newPath);
      if (currentPath === file.path) openFile(newPath);
    } catch (err) {
      console.error('Failed to rename file:', err);
      alert(`Failed to rename file: ${err instanceof Error ? err.message : String(err)}`);
    }
  }, [currentPath, openFile]);

  const actions = useMemo<FileActions>(() => ({
    // Seeded with the open file's folder.
    onNewFile: () => {
      const slash = currentPath === null ? -1 : currentPath.lastIndexOf('/');
      setNewFileName(currentPath !== null && slash >= 0 ? currentPath.slice(0, slash + 1) : '');
    },
    onNewFileIn: (folder) => setNewFileName(folder !== '' ? `${folder}/` : ''),
    onNewFolder: (parent) => setNewFolderParent(parent),
    // Only offered for empty folders, so no file is affected.
    onDeleteFolder: (path) => {
      if (!window.confirm(fileSidebar.confirmDeleteFolder(path))) return;
      try {
        deleteFolder(path);
      } catch (err) {
        console.error('Failed to delete folder:', err);
      }
    },
    onMoveFile: (file, preset) => setPlaceQueue((queue) => [...queue, { kind: 'move', file, ...preset }]),
    // Refused if any file would land on an existing one; otherwise each
    // file is renamed and the explicit folder markers follow.
    onMoveFolder: (folder, destination) => {
      const newFolder = joinPath(destination, folder.split('/').pop() ?? folder);
      const prefix = `${folder}/`;
      const taken = new Set(existingPaths);
      const renames = files
        .filter((file) => file.path.startsWith(prefix))
        .map((file) => [file, `${newFolder}/${file.path.slice(prefix.length)}`] as const);
      if (renames.some(([, to]) => taken.has(to))) {
        alert(dialogStrings.moveFile.errorFolderConflict(newFolder));
        return;
      }
      for (const [file, to] of renames) rename(file, to);
      for (const marker of allFolders) {
        if (marker !== folder && !marker.startsWith(prefix)) continue;
        try {
          createFolder(`${newFolder}${marker.slice(folder.length)}`);
          deleteFolder(marker);
        } catch (err) {
          console.error('Failed to move folder marker:', err);
        }
      }
    },
    onUploadFiles: (picked, destination) => setAssetUpload({ files: picked, destination }),
    // Dropped from outside the browser: added directly, unless the name is
    // taken, in which case the place-file dialog asks where to put it.
    onDropFiles: (dropped: DroppedEntries, destination) => {
      const taken = new Set(existingPaths);
      for (const folder of dropped.folders) {
        try {
          createFolder(joinPath(destination, sanitizeRelativePath(folder)));
        } catch (err) {
          console.error('Failed to create folder:', err);
        }
      }
      const relativePaths = new Map(dropped.files.map((entry) => [entry.file, entry.relativePath]));
      for (const { file, error } of processAssetFiles(dropped.files.map((entry) => entry.file))) {
        if (error !== undefined) {
          console.warn(`[file tree drop] skipped ${file.name}: ${error}`);
          continue;
        }
        const rel = sanitizeRelativePath(relativePaths.get(file) ?? file.name);
        const path = joinPath(destination, rel);
        if (taken.has(path)) {
          const slash = rel.lastIndexOf('/');
          const folder = joinPath(destination, slash >= 0 ? rel.slice(0, slash) : '');
          const name = slash >= 0 ? rel.slice(slash + 1) : rel;
          setPlaceQueue((queue) => [...queue, { kind: 'add', file, folder, name }]);
          continue;
        }
        taken.add(path);
        void uploadAsset(file, path);
      }
    },
    // Deleting the open file leaves the provider to fall back to its default.
    onDeleteFile: (file) => {
      try {
        deleteFile(file.path);
      } catch (err) {
        console.error('Failed to delete file:', err);
      }
    },
    onRenameFile: rename,
  }), [allFolders, currentPath, existingPaths, files, rename, uploadAsset]);

  const dialogs = (
    <>
      {/* Mounted only while open, so every open is a fresh form. */}
      {newFileName !== null && (
        <NewFileDialog
          isOpen
          existingPaths={existingPaths}
          folders={allFolders}
          initialFilename={newFileName}
          onClose={() => setNewFileName(null)}
          onCreateTextFile={(path, content) => void createTextFile(path, content)}
        />
      )}
      <NewFolderDialog
        isOpen={newFolderParent !== null}
        parent={newFolderParent ?? ''}
        existingFolders={allFolders}
        existingPaths={existingPaths}
        onClose={() => setNewFolderParent(null)}
        onCreateFolder={(path) => {
          try {
            createFolder(path);
          } catch (err) {
            console.error('Failed to create folder:', err);
          }
        }}
      />
      {assetUpload !== null && (
        <NewAssetDialog
          isOpen
          existingPaths={existingPaths}
          defaultDestination={assetUpload.destination}
          folders={allFolders}
          initialFiles={assetUpload.files}
          onClose={() => setAssetUpload(null)}
          onUploadAsset={uploadAsset}
        />
      )}
      <PlaceFileDialog
        request={placeQueue[0] ?? null}
        folders={allFolders}
        existingPaths={existingPaths}
        onClose={() => setPlaceQueue((queue) => queue.slice(1))}
        onConfirm={(request, newPath) => {
          if (request.kind === 'move') rename(request.file, newPath);
          else void uploadAsset(request.file, newPath);
        }}
      />
    </>
  );

  return { actions, dialogs };
}
