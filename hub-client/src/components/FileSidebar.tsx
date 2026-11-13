/**
 * The editor's file sidebar: `FileSidebarToolbar` above `FileTree`. Same
 * props as before the split, so hosts compose nothing themselves.
 *
 * The wrapper sets the toolbar height the tree's sticky folder headers
 * offset by; it adds no overflow clipping, so the sidebar column stays
 * the scroll container for both the toolbar and the folder headers.
 */

import FileTree, { type FileTreeProps } from './FileTree';
import FileSidebarToolbar, { type FileSidebarToolbarProps } from './FileSidebarToolbar';

export type FileSidebarProps = FileTreeProps & Omit<FileSidebarToolbarProps, 'currentFile' | 'onUploadFiles'>;

export default function FileSidebar({ onNewFile, onOpenSearch, importDocument, ...tree }: FileSidebarProps) {
  return (
    <div className="file-sidebar-shell">
      <FileSidebarToolbar
        currentFile={tree.currentFile}
        onNewFile={onNewFile}
        onNewFolder={tree.onNewFolder}
        onUploadFiles={tree.onUploadFiles}
        onOpenSearch={onOpenSearch}
        importDocument={importDocument}
      />
      <FileTree {...tree} />
    </div>
  );
}
