/**
 * The file sidebar's toolbar: new file / new folder / add asset, plus the
 * optional import and search buttons. Composed with `FileTree` by
 * `FileSidebar`; hosts that only want the tree never import this.
 */

import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { resolveDefaultDestination } from './fileUpload';
import { FilePlusIcon, UploadIcon, FolderPlusIcon, SearchIcon } from './icons';
import Tooltip from './Tooltip';
import ImportControl, { type ImportControlProps } from './ImportControl';
import { fileSidebar } from '../strings';

export interface FileSidebarToolbarProps {
  /** Selected file; seeds the destination of new folders and uploads. */
  currentFile: FileEntry | null;
  onNewFile: () => void;
  /** Create an empty folder under `parent` ('' = project root). */
  onNewFolder?: (parent: string) => void;
  /** Open the asset dialog with no pre-filled files, seeded with `destination`. */
  onUploadFiles: (files: File[], destination: string) => void;
  /** Open the search dialog (magnifying-glass button). */
  onOpenSearch?: () => void;
  /** Document import (button next to Add asset); omitted when the host has no importer. */
  importDocument?: ImportControlProps;
}

export default function FileSidebarToolbar({
  currentFile,
  onNewFile,
  onNewFolder,
  onUploadFiles,
  onOpenSearch,
  importDocument,
}: FileSidebarToolbarProps) {
  const defaultDestination = () => resolveDefaultDestination({ selection: currentFile?.path });

  return (
    <div className="sidebar-header">
      <Tooltip content={fileSidebar.newFile}>
        <button
          className="qh-btn small outline new-file-btn"
          onClick={onNewFile}
          aria-label={fileSidebar.newFile}
        >
          <FilePlusIcon />
        </button>
      </Tooltip>
      {onNewFolder && (
        <Tooltip content={fileSidebar.newFolder}>
          <button
            className="qh-btn small outline new-folder-btn"
            onClick={() => onNewFolder(defaultDestination())}
            aria-label={fileSidebar.newFolder}
          >
            <FolderPlusIcon />
          </button>
        </Tooltip>
      )}
      <Tooltip content={fileSidebar.addAsset}>
        <button
          className="qh-btn small outline upload-asset-btn"
          onClick={() => onUploadFiles([], defaultDestination())}
          aria-label={fileSidebar.addAsset}
        >
          <UploadIcon />
        </button>
      </Tooltip>
      {importDocument && <ImportControl {...importDocument} />}
      {onOpenSearch && (
        <Tooltip content={fileSidebar.searchLabel}>
          <button
            className="qh-btn small outline search-files-btn"
            onClick={onOpenSearch}
            aria-label={fileSidebar.searchLabel}
          >
            <SearchIcon />
          </button>
        </Tooltip>
      )}
    </div>
  );
}
