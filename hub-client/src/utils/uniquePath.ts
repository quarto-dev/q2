/**
 * Path helpers for placing a file where a same-named file may already
 * exist: split a name from its extension and find a free variant by
 * appending " 2", " 3", … before the extension.
 */

import { normalizeProjectPath } from '@quarto/preview-renderer/types/project';

export function joinPath(folder: string, name: string): string {
  return normalizeProjectPath(folder ? `${folder}/${name}` : name);
}

export function splitName(name: string): { stem: string; ext: string } {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? { stem: name.slice(0, dot), ext: name.slice(dot) } : { stem: name, ext: '' };
}

/** First of `folder/name`, `folder/name 2`, `folder/name 3`, … not in `taken`. */
export function uniquePath(folder: string, name: string, taken: { has(path: string): boolean }): string {
  const { stem, ext } = splitName(name);
  let path = joinPath(folder, name);
  for (let n = 2; taken.has(path); n++) path = joinPath(folder, `${stem} ${n}${ext}`);
  return path;
}
