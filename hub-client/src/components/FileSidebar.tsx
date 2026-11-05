/**
 * The editor's file sidebar: `FileSidebarToolbar` pinned above `FileTree`.
 * Same props as before the split, so hosts compose nothing themselves.
 */

import FileTree, { type FileTreeProps } from './FileTree';
import FileSidebarToolbar, { type FileSidebarToolbarProps } from './FileSidebarToolbar';

export type FileSidebarProps = Omit<FileTreeProps, 'toolbar'> & Omit<FileSidebarToolbarProps, 'currentFile' | 'onUploadFiles'>;

export default function FileSidebar({ onNewFile, onOpenSearch, importDocument, ...tree }: FileSidebarProps) {
  return (
    <FileTree
      {...tree}
      toolbar={
        <FileSidebarToolbar
          currentFile={tree.currentFile}
          onNewFile={onNewFile}
          onNewFolder={tree.onNewFolder}
          onUploadFiles={tree.onUploadFiles}
          onOpenSearch={onOpenSearch}
          importDocument={importDocument}
        />
      }
    />
  );
}
