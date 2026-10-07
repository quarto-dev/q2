/**
 * Hand-off between the block editor and the span comment chrome (span
 * comments prototype). The rich editor's 💬 button wraps the selection in a
 * plain span and commits; the block re-renders from the round trip with a
 * fresh pool id, so the span cannot be identified by id. Instead the editor
 * records the wrapped text here and the first comment-less span whose text
 * matches (rendered within `TTL_MS`) claims it and opens its add-comment
 * bubble. Kept import-free so both sides can use it without a module cycle.
 */
const TTL_MS = 15_000;

let pending: { text: string; at: number } | null = null;

const normalize = (s: string) => s.replace(/\s+/g, ' ').trim();

/** Ask for the next rendered span with this text to open its add-comment bubble. */
export function requestOpenCommentOnSpan(text: string): void {
    pending = { text: normalize(text), at: Date.now() };
}

/** Claim the pending request if `text` matches (one-shot). */
export function claimPendingCommentOpen(text: string): boolean {
    if (!pending) return false;
    if (Date.now() - pending.at > TTL_MS) {
        pending = null;
        return false;
    }
    if (normalize(text) !== pending.text) return false;
    pending = null;
    return true;
}
