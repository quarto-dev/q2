/**
 * File Tree Utilities
 *
 * Pure functions for building and manipulating a nested file tree structure
 * from a flat list of file entries. Used by FileSidebar for rendering
 * a collapsible folder hierarchy.
 */

import type { FileEntry } from '../types/project';

/**
 * A node in the file tree, representing either a folder or a file.
 */
export interface FileTreeNode {
  /** Just the folder/file name (e.g., "components") */
  name: string;
  /** Full path from root (e.g., "src/components") */
  path: string;
  /** Whether this is a folder or file */
  type: 'folder' | 'file';
  /** Child nodes (folders and files). Empty for file nodes. */
  children: FileTreeNode[];
  /** For file nodes, the original FileEntry. Undefined for folders. */
  file?: FileEntry;
}

/**
 * Recursively sort children of a tree node.
 * Folders come before files, both sorted alphabetically by name.
 */
function sortTreeChildren(node: FileTreeNode): void {
  node.children.sort((a, b) => {
    // Folders before files
    if (a.type !== b.type) {
      return a.type === 'folder' ? -1 : 1;
    }
    // Alphabetically within same type
    return a.name.localeCompare(b.name);
  });

  // Recurse into folders
  for (const child of node.children) {
    if (child.type === 'folder') {
      sortTreeChildren(child);
    }
  }
}

/**
 * Build a nested file tree from a flat list of file entries.
 *
 * @param files - Array of FileEntry objects with path and docId
 * @returns Root node of the tree (type 'folder', empty name/path)
 *
 * @example
 * const files = [
 *   { path: 'src/components/Button.tsx', docId: '1' },
 *   { path: 'index.qmd', docId: '2' },
 * ];
 * const tree = buildFileTree(files);
 * // tree.children contains:
 * //   - folder "src" with nested "components" folder containing Button.tsx
 * //   - file "index.qmd"
 */
export function buildFileTree(files: FileEntry[]): FileTreeNode {
  const root: FileTreeNode = {
    name: '',
    path: '',
    type: 'folder',
    children: [],
  };

  for (const file of files) {
    const segments = file.path.split('/');
    let current = root;

    // Create/traverse folder nodes for all but the last segment
    for (let i = 0; i < segments.length - 1; i++) {
      const segment = segments[i];
      const folderPath = segments.slice(0, i + 1).join('/');

      let child = current.children.find(
        (c) => c.type === 'folder' && c.name === segment
      );

      if (!child) {
        child = {
          name: segment,
          path: folderPath,
          type: 'folder',
          children: [],
        };
        current.children.push(child);
      }
      current = child;
    }

    // Add the file as a leaf
    const fileName = segments[segments.length - 1];
    current.children.push({
      name: fileName,
      path: file.path,
      type: 'file',
      children: [],
      file,
    });
  }

  // Sort children recursively
  sortTreeChildren(root);
  return root;
}

/**
 * Get all ancestor folder paths for a file path.
 *
 * @param filePath - Full path to a file (e.g., "src/components/ui/Button.tsx")
 * @returns Array of ancestor folder paths, from shallowest to deepest
 *
 * @example
 * getAncestorPaths('src/components/ui/Button.tsx')
 * // Returns: ['src', 'src/components', 'src/components/ui']
 *
 * getAncestorPaths('index.qmd')
 * // Returns: []
 */
export function getAncestorPaths(filePath: string): string[] {
  const segments = filePath.split('/');
  const ancestors: string[] = [];

  // All but the last segment (the filename)
  for (let i = 1; i < segments.length; i++) {
    ancestors.push(segments.slice(0, i).join('/'));
  }

  return ancestors;
}

/**
 * Compute the set of folders that should be expanded to show the selected file.
 * Adds ancestor paths of the selected file to the existing expanded set.
 *
 * @param currentExpanded - Current set of expanded folder paths
 * @param selectedFilePath - Path of the currently selected file, or null
 * @returns New set with ancestors of selected file added (or same set if no file)
 *
 * @example
 * const expanded = new Set(['other']);
 * const result = computeExpandedFolders(expanded, 'src/components/Button.tsx');
 * // result contains: 'other', 'src', 'src/components'
 */
export function computeExpandedFolders(
  currentExpanded: Set<string>,
  selectedFilePath: string | null
): Set<string> {
  if (!selectedFilePath) {
    return currentExpanded;
  }

  const ancestors = getAncestorPaths(selectedFilePath);

  // If all ancestors are already expanded, return the same set (no change)
  if (ancestors.every((a) => currentExpanded.has(a))) {
    return currentExpanded;
  }

  // Create new set with ancestors added
  const result = new Set(currentExpanded);
  for (const ancestor of ancestors) {
    result.add(ancestor);
  }

  return result;
}
