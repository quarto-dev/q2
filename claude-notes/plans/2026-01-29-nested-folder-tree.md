# Nested Collapsible Folder Tree for FileSidebar

**Issue**: kyoto-cvr
**Created**: 2026-01-29
**Status**: Complete

## Overview

Replace the current flat directory grouping in `FileSidebar` with a proper nested tree structure. Currently, a file at `src/components/ui/Button.tsx` shows under a single directory header `src/components/ui/`. The goal is to render this as a proper tree:

```
📁 src/
  📁 components/
    📁 ui/
      📄 Button.tsx
```

Each folder can be independently expanded or collapsed by clicking.

## Current Implementation

**File**: `hub-client/src/components/FileSidebar.tsx`

The current `groupFilesByDirectory()` function (lines 73-96) creates a flat `Map<string, FileEntry[]>` where keys are full directory paths. This means:
- `src/components/Button.tsx` → key: `"src/components"`
- `src/components/ui/Modal.tsx` → key: `"src/components/ui"`

These are rendered as separate, unrelated directory groups with no hierarchy.

## Design

### Code Organization

Extract tree-building logic into a separate utility file for testability:

```
hub-client/src/
  components/
    FileSidebar.tsx        # React component (rendering, state, events)
    FileSidebar.css        # Styles
  utils/
    fileTree.ts            # Pure functions: buildFileTree, getAncestorPaths, etc.
    fileTree.test.ts       # Unit tests for tree logic
```

### Data Structure

Define a tree node type in `utils/fileTree.ts`:

```typescript
import type { FileEntry } from '../types/project';

export interface FileTreeNode {
  name: string;           // Just the folder/file name (e.g., "components")
  path: string;           // Full path (e.g., "src/components")
  type: 'folder' | 'file';
  children: FileTreeNode[];  // For folders: child folders and files
  file?: FileEntry;          // For files: the original FileEntry
}
```

### Tree Building Algorithm

1. Start with an empty root node
2. For each file, split its path by `/`
3. Walk the path segments, creating folder nodes as needed
4. Insert the file as a leaf node
5. Sort children at each level: folders first (alphabetically), then files (alphabetically)

```typescript
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
        c => c.type === 'folder' && c.name === segment
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
```

### Path Expansion Utilities

Pure functions to compute which folders should be expanded:

```typescript
/**
 * Get all ancestor folder paths for a file path.
 * E.g., "src/components/ui/Button.tsx" → ["src", "src/components", "src/components/ui"]
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
 */
export function computeExpandedFolders(
  currentExpanded: Set<string>,
  selectedFilePath: string | null
): Set<string> {
  if (!selectedFilePath) {
    return currentExpanded;
  }

  const ancestors = getAncestorPaths(selectedFilePath);
  const result = new Set(currentExpanded);

  for (const ancestor of ancestors) {
    result.add(ancestor);
  }

  return result;
}
```

### State Management

In `FileSidebar.tsx`:

```typescript
// Track which folders are expanded (by path)
// Initially empty - only folders containing selected file will be expanded
const [expandedFolders, setExpandedFolders] = useState<Set<string>>(new Set());

// Toggle a folder's expanded state
const toggleFolder = useCallback((path: string) => {
  setExpandedFolders(prev => {
    const next = new Set(prev);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
    }
    return next;
  });
}, []);

// Auto-expand folders when selected file changes
useEffect(() => {
  if (currentFile) {
    setExpandedFolders(prev => computeExpandedFolders(prev, currentFile.path));
  }
}, [currentFile?.path]);
```

**Default expansion behavior**: Folders start collapsed. When a file is selected (including on initial load), expand only the folders needed to reveal that file. Manual expand/collapse by the user is preserved.

### Recursive Rendering

```typescript
const renderTreeNode = (node: FileTreeNode, depth: number = 0) => {
  if (node.type === 'file') {
    return renderFileItem(node.file!, depth);
  }

  const isExpanded = expandedFolders.has(node.path);

  return (
    <div key={node.path} className="tree-folder">
      <div
        className="folder-header"
        style={{ paddingLeft: `${12 + depth * 16}px` }}
        onClick={() => toggleFolder(node.path)}
      >
        <span className="folder-chevron">
          {isExpanded ? '▼' : '▶'}
        </span>
        <span className="folder-icon">📁</span>
        <span className="folder-name">{node.name}</span>
      </div>
      {isExpanded && (
        <div className="folder-children">
          {node.children.map(child => renderTreeNode(child, depth + 1))}
        </div>
      )}
    </div>
  );
};
```

### CSS Changes

```css
/* Tree structure */
.tree-folder {
  /* Container for a folder and its children */
}

.folder-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 12px;
  color: #888;
  font-size: 12px;
  cursor: pointer;
  transition: background 0.15s;
}

.folder-header:hover {
  background: #1f3460;
}

.folder-chevron {
  font-size: 10px;
  width: 12px;
  text-align: center;
  color: #666;
  transition: transform 0.15s;
}

.folder-icon {
  font-size: 12px;
}

.folder-name {
  font-weight: 500;
}

.folder-children {
  /* Children are indented via inline paddingLeft on items */
}

/* File items with depth-based indentation */
.file-item {
  /* Add paddingLeft dynamically based on depth */
}
```

### Edge Cases

1. **Root-level files**: Files without any directory (e.g., `index.qmd`) should render directly under the root, not inside any folder.

2. **Empty folders**: Won't occur since we only create folder nodes when there are files inside them.

3. **Currently selected file**: Auto-expand parent folders so the selected file is visible. This runs via `useEffect` when `currentFile` changes.

## Work Items

- [x] Create `utils/fileTree.ts` with `FileTreeNode` type
- [x] Implement `buildFileTree()` function
- [x] Implement `sortTreeChildren()` helper
- [x] Implement `getAncestorPaths()` function
- [x] Implement `computeExpandedFolders()` function
- [x] Write unit tests for all pure functions in `fileTree.test.ts`
- [x] Add `expandedFolders` state to FileSidebar
- [x] Add `toggleFolder()` callback
- [x] Add `useEffect` to auto-expand on file selection
- [x] Implement recursive `renderTreeNode()` function
- [x] Update `renderFileItem()` to accept depth parameter for indentation
- [x] Handle root-level files (no parent folder)
- [x] Update CSS for tree structure (chevrons, hover states, indentation)
- [x] Remove old `groupFilesByDirectory()` and `renderDirectory()` code
- [x] Manual testing with various folder structures
- [x] Update changelog

## Testing Plan

### Unit Tests (`src/utils/fileTree.test.ts`)

The tree-building and path utilities are pure functions, making them ideal for unit testing. Tests run via `npm test` using vitest.

#### `buildFileTree()` tests

```typescript
describe('buildFileTree', () => {
  it('returns empty root for empty file list', () => {
    const tree = buildFileTree([]);
    expect(tree.children).toHaveLength(0);
    expect(tree.type).toBe('folder');
  });

  it('places root-level files directly under root', () => {
    const files = [
      { path: 'index.qmd', docId: '1' },
      { path: 'README.md', docId: '2' },
    ];
    const tree = buildFileTree(files);
    expect(tree.children).toHaveLength(2);
    expect(tree.children.every(c => c.type === 'file')).toBe(true);
  });

  it('creates single-level folder structure', () => {
    const files = [
      { path: 'images/foo.png', docId: '1' },
      { path: 'images/bar.png', docId: '2' },
    ];
    const tree = buildFileTree(files);
    expect(tree.children).toHaveLength(1);
    expect(tree.children[0].name).toBe('images');
    expect(tree.children[0].type).toBe('folder');
    expect(tree.children[0].children).toHaveLength(2);
  });

  it('creates nested folder structure', () => {
    const files = [
      { path: 'src/components/ui/Button.tsx', docId: '1' },
    ];
    const tree = buildFileTree(files);

    // src folder
    expect(tree.children).toHaveLength(1);
    const src = tree.children[0];
    expect(src.name).toBe('src');
    expect(src.path).toBe('src');

    // components folder
    expect(src.children).toHaveLength(1);
    const components = src.children[0];
    expect(components.name).toBe('components');
    expect(components.path).toBe('src/components');

    // ui folder
    expect(components.children).toHaveLength(1);
    const ui = components.children[0];
    expect(ui.name).toBe('ui');
    expect(ui.path).toBe('src/components/ui');

    // Button.tsx file
    expect(ui.children).toHaveLength(1);
    expect(ui.children[0].name).toBe('Button.tsx');
    expect(ui.children[0].type).toBe('file');
  });

  it('handles mixed depths correctly', () => {
    const files = [
      { path: 'index.qmd', docId: '1' },
      { path: 'src/main.ts', docId: '2' },
      { path: 'src/components/App.tsx', docId: '3' },
    ];
    const tree = buildFileTree(files);

    // Root should have: index.qmd (file) and src (folder)
    expect(tree.children).toHaveLength(2);
  });

  it('sorts folders before files at each level', () => {
    const files = [
      { path: 'zebra.txt', docId: '1' },
      { path: 'alpha/file.txt', docId: '2' },
      { path: 'beta.txt', docId: '3' },
    ];
    const tree = buildFileTree(files);

    // Order should be: alpha (folder), beta.txt (file), zebra.txt (file)
    expect(tree.children[0].name).toBe('alpha');
    expect(tree.children[0].type).toBe('folder');
    expect(tree.children[1].name).toBe('beta.txt');
    expect(tree.children[2].name).toBe('zebra.txt');
  });

  it('sorts alphabetically within folders and files', () => {
    const files = [
      { path: 'c.txt', docId: '1' },
      { path: 'a.txt', docId: '2' },
      { path: 'b.txt', docId: '3' },
    ];
    const tree = buildFileTree(files);

    expect(tree.children[0].name).toBe('a.txt');
    expect(tree.children[1].name).toBe('b.txt');
    expect(tree.children[2].name).toBe('c.txt');
  });

  it('preserves FileEntry reference in file nodes', () => {
    const file = { path: 'test.txt', docId: 'doc-123' };
    const tree = buildFileTree([file]);

    expect(tree.children[0].file).toBe(file);
  });
});
```

#### `getAncestorPaths()` tests

```typescript
describe('getAncestorPaths', () => {
  it('returns empty array for root-level file', () => {
    expect(getAncestorPaths('index.qmd')).toEqual([]);
  });

  it('returns single ancestor for one-level nesting', () => {
    expect(getAncestorPaths('src/main.ts')).toEqual(['src']);
  });

  it('returns all ancestors for deep nesting', () => {
    expect(getAncestorPaths('src/components/ui/Button.tsx')).toEqual([
      'src',
      'src/components',
      'src/components/ui',
    ]);
  });
});
```

#### `computeExpandedFolders()` tests

```typescript
describe('computeExpandedFolders', () => {
  it('returns existing set when no file selected', () => {
    const existing = new Set(['foo']);
    const result = computeExpandedFolders(existing, null);
    expect(result).toEqual(existing);
  });

  it('adds ancestor paths to existing expanded set', () => {
    const existing = new Set(['other']);
    const result = computeExpandedFolders(existing, 'src/components/Button.tsx');

    expect(result.has('other')).toBe(true);
    expect(result.has('src')).toBe(true);
    expect(result.has('src/components')).toBe(true);
  });

  it('does not modify original set', () => {
    const existing = new Set(['foo']);
    computeExpandedFolders(existing, 'src/main.ts');
    expect(existing.size).toBe(1);
  });

  it('handles root-level file (no ancestors)', () => {
    const existing = new Set(['foo']);
    const result = computeExpandedFolders(existing, 'index.qmd');
    expect(result).toEqual(existing);
  });
});
```

### Manual Testing Checklist

After implementing, verify these scenarios in the browser:

1. **Empty project**: No files → shows empty state message
2. **Root-level files only**: `index.qmd`, `README.md` → files listed without folders
3. **Single folder**: `images/a.png`, `images/b.png` → one collapsible folder
4. **Nested folders**: `src/components/ui/Button.tsx` → three nested collapsible folders
5. **Mixed depths**: Files at root, one level, two levels → correct hierarchy
6. **Initial selection**: Load with a nested file selected → ancestors expanded
7. **Click to collapse**: Click expanded folder → children hidden, chevron changes
8. **Click to expand**: Click collapsed folder → children shown, chevron changes
9. **Select nested file**: Click file in collapsed branch → ancestors auto-expand
10. **File operations**: Rename/delete files in nested folders → tree updates correctly
11. **Sorting**: Verify folders appear before files, both sorted alphabetically

## Future Enhancements (Out of Scope)

- Drag-and-drop to move files between folders
- Create new folder (currently files implicitly create folders)
- Collapse/expand all buttons
- Remember expanded state across sessions (localStorage)
- Keyboard navigation (arrow keys to expand/collapse)
