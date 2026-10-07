/**
 * Names for an import's qmd and its media folder (document import I11, I12).
 *
 * The qmd is `<stem>.qmd`; its images go in the sibling folder `<stem>_media/`. The two are
 * proposed together, so a collision on either moves both to the first free `<stem> 2`,
 * `<stem> 3`, … (the `uniquePath` convention, which checks one path; this checks two).
 */
import { sanitizeFilename } from '../services/resourceService';
import { joinPath, splitName } from './uniquePath';

const FALLBACK_STEM = 'document';

/**
 * The proposed stem for a source file name: the name without its extension, sanitized.
 *
 * Sanitize the whole `<stem>.qmd`, then split: `sanitizeFilename` turns every dot but the last
 * into a hyphen, so sanitizing the bare stem would mistake its last interior dot for an extension
 * (`my.report.v2` would become `my-report.v2`).
 */
export function importStem(sourceName: string): string {
  const dot = sourceName.lastIndexOf('.');
  const base = dot > 0 ? sourceName.slice(0, dot) : dot === 0 ? '' : sourceName;
  const { stem } = splitName(sanitizeFilename(`${base}.qmd`));
  return stem === '' || stem.startsWith('.') ? FALLBACK_STEM : stem;
}

/** The media folder for a qmd name in `folder`: `<folder>/<stem>_media`. */
export function mediaDirFor(folder: string, qmdName: string): string {
  return joinPath(folder, `${splitName(qmdName).stem}_media`);
}

/** What the project already holds, for collision checks. */
export interface Occupied {
  /** Every file path. */
  paths: ReadonlySet<string>;
  /** Every folder path (explicit and file-derived). */
  folders: ReadonlySet<string>;
}

/** A path is taken by a file, a folder, or anything under it as a folder. */
export function isTaken(path: string, occupied: Occupied): boolean {
  return occupied.paths.has(path) || occupied.folders.has(path);
}

/** The first of `<stem>.qmd`, `<stem> 2.qmd`, … where neither the qmd nor its media folder exists in `folder`. */
export function proposeImportName(sourceName: string, folder: string, occupied: Occupied): string {
  const stem = importStem(sourceName);
  for (let n = 1; ; n++) {
    const name = `${n === 1 ? stem : `${stem} ${n}`}.qmd`;
    if (!isTaken(joinPath(folder, name), occupied) && !isTaken(mediaDirFor(folder, name), occupied)) return name;
  }
}
