/**
 * Expand a drop's DataTransfer into files with folder-relative paths,
 * walking dropped directories (Chromium/WebKit `webkitGetAsEntry`;
 * Firefox supports it too). Falls back to the flat file list where the
 * entry API is unavailable, in which case folders can't be read.
 */

export interface DroppedFile {
  file: File;
  /** Path relative to the drop destination, e.g. `chapters/intro.qmd`. */
  relativePath: string;
}

export interface DroppedEntries {
  files: DroppedFile[];
  /** Every dropped directory (relative), including empty ones. */
  folders: string[];
}

type Entry = FileSystemEntry & {
  isFile: boolean;
  isDirectory: boolean;
  file?: (ok: (f: File) => void, err: (e: unknown) => void) => void;
  createReader?: () => { readEntries: (ok: (es: Entry[]) => void, err: (e: unknown) => void) => void };
};

function readAll(reader: ReturnType<NonNullable<Entry['createReader']>>): Promise<Entry[]> {
  // readEntries returns in batches; call until an empty batch.
  return new Promise((resolve, reject) => {
    const out: Entry[] = [];
    const step = () =>
      reader.readEntries((batch) => {
        if (batch.length === 0) resolve(out);
        else {
          out.push(...batch);
          step();
        }
      }, reject);
    step();
  });
}

async function walk(entry: Entry, prefix: string, into: DroppedEntries): Promise<void> {
  if (entry.isFile && entry.file) {
    const file = await new Promise<File>((ok, err) => entry.file!(ok, err));
    into.files.push({ file, relativePath: prefix + entry.name });
  } else if (entry.isDirectory && entry.createReader) {
    const dir = prefix + entry.name;
    into.folders.push(dir);
    for (const child of await readAll(entry.createReader())) {
      await walk(child, dir + '/', into);
    }
  }
}

export async function collectDroppedEntries(dt: DataTransfer): Promise<DroppedEntries> {
  const out: DroppedEntries = { files: [], folders: [] };
  const items = Array.from(dt.items ?? []);
  const entries = items
    .map((it) => (typeof it.webkitGetAsEntry === 'function' ? (it.webkitGetAsEntry() as Entry | null) : null));
  if (entries.length > 0 && entries.every((e) => e !== null)) {
    for (const e of entries) await walk(e!, '', out);
    return out;
  }
  // No entry API: flat files only.
  for (const file of Array.from(dt.files)) out.files.push({ file, relativePath: file.name });
  return out;
}
