/**
 * Resolve the default destination folder for an asset upload.
 *
 * Priority:
 * 1. Drop target: walk up from the element to the nearest ancestor carrying
 *    `data-folder-path`. FileSidebar tags folder headers and file rows with
 *    this attribute: folders use their own path, files use their parent's.
 * 2. Selection fallback: the currently focused file's parent folder.
 * 3. Root fallback: project root (`""`).
 */

export const FOLDER_PATH_ATTR = 'data-folder-path';

export interface ResolveDefaultDestinationOpts {
  /** DOM element that received a drop (e.g. `event.target`). */
  dropTarget?: EventTarget | null;
  /** Path of the currently selected file, or null/undefined for no selection. */
  selection?: string | null;
}

export function resolveDefaultDestination(
  opts: ResolveDefaultDestinationOpts
): string {
  const fromDrop = readFolderPathFromTarget(opts.dropTarget);
  if (fromDrop !== null) {
    return fromDrop;
  }

  if (opts.selection) {
    return parentFolder(opts.selection);
  }

  return '';
}

function readFolderPathFromTarget(target: EventTarget | null | undefined): string | null {
  if (!target || !(target instanceof Element)) {
    return null;
  }
  const el = target.closest(`[${FOLDER_PATH_ATTR}]`);
  if (!el) {
    return null;
  }
  return el.getAttribute(FOLDER_PATH_ATTR);
}

function parentFolder(path: string): string {
  const lastSlash = path.lastIndexOf('/');
  return lastSlash >= 0 ? path.slice(0, lastSlash) : '';
}
