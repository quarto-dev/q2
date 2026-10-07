/**
 * Authorship for a download. A docx comment or tracked change takes its author and date from
 * who wrote the text (the Rust `editorial-marks-ooxml` transform stamps them from attribution
 * data); without data pandoc labels it "unknown". The preview builds this data for its own
 * toggle (`useAttribution`); a download builds it at the click, from the same history.
 */
import { getFileHandle } from '@quarto/preview-runtime';
import type { ActorIdentity } from '@quarto/preview-runtime';
import { buildRunListAttribution } from '../services/attribution-runs';
import { buildAttributionPayload } from '../hooks/useAttribution';

/** Formats whose writer turns editorial marks into Word/PowerPoint comments and changes. */
const STAMPED_FORMATS = new Set(['docx', 'pptx']);

let identities: Record<string, ActorIdentity> = {};

/** The project's author table (Automerge author id to display name); kept current by the editor. */
export function setDownloadIdentities(next: Record<string, ActorIdentity>): void {
  identities = next;
}

/**
 * The transport JSON for `render_pandoc_request`'s `attributionJson`, or undefined when the
 * format has no use for it or the history is unavailable. A download never fails for want of
 * authorship: the marks just keep pandoc's default author.
 */
export async function attributionJsonFor(path: string, format: string, signal?: AbortSignal): Promise<string | undefined> {
  if (!STAMPED_FORMATS.has(format)) return undefined;
  try {
    const handle = getFileHandle(path);
    if (!handle) return undefined;
    const state = await buildRunListAttribution(handle, 'text', signal);
    if (!state || signal?.aborted) return undefined;
    const text = (handle.doc() as { text?: unknown } | undefined)?.text;
    if (typeof text !== 'string') return undefined;
    return buildAttributionPayload(state, text, identities);
  } catch (err) {
    console.warn('[download] authorship unavailable; marks keep the default author:', err);
    return undefined;
  }
}
