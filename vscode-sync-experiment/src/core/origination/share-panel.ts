/**
 * The three copyable forms of a shared project (GE-12, G24), shown only after the server confirmed the heads:
 * a share URL, the `q2 preview` command, and the server and id for the Join command. Pure.
 */
import { buildShareUrl } from '@quarto/share-url';

export interface ShareInfo {
  server: string;
  indexDocId: string;
  /** The `_quarto.yml` title or the folder name. */
  name: string;
  /** The file the link opens; `index.qmd` when the project has none to name. */
  file?: string;
}

export interface ShareForm {
  label: string;
  detail: string;
  /** What lands on the clipboard. */
  text: string;
}

export function shareForms(i: ShareInfo): ShareForm[] {
  const url = buildShareUrl({ server: i.server, indexDocId: i.indexDocId, name: i.name, file: i.file ?? 'index.qmd' });
  const command = `q2 preview --hub-url ${i.server} --project ${i.indexDocId} --ui editor --allow-edit`;
  const join = `server: ${i.server}\nproject: ${i.indexDocId}`;
  return [
    { label: 'Share link', detail: url, text: url },
    { label: 'q2 preview command', detail: command, text: command },
    { label: 'Server and project id (for Join)', detail: `${i.server}  ${i.indexDocId}`, text: join },
  ];
}
