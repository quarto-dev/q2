/**
 * Snippet extraction for search results.
 *
 * Given a file's raw text and the query terms that matched it (as returned in
 * {@link SearchResult.terms}), produce a short, highlightable excerpt centered
 * on the first match. Pure and presentation-agnostic: the caller renders
 * {@link SnippetSegment}s with `match: true` as `<mark>` (or similar).
 *
 * Kept out of the {@link SearchProvider} on purpose — the provider need not
 * store full document text, and snippet shaping is a presentation concern.
 */

/** One run of snippet text; `match` segments are the highlighted terms. */
export interface SnippetSegment {
  text: string;
  match: boolean;
}

export interface SnippetOptions {
  /** Characters of context to keep on each side of the match. Default 40. */
  context?: number;
  /** Max length of the leading slice used when no term matches. Default 80. */
  maxLength?: number;
}

const ELLIPSIS = '…';

/** Lowercased, de-duplicated, non-empty terms, longest first (greedy match). */
function normalizeTerms(terms: string[]): string[] {
  const seen = new Set<string>();
  for (const t of terms) {
    const lt = t.toLowerCase();
    if (lt) seen.add(lt);
  }
  return [...seen].sort((a, b) => b.length - a.length);
}

/** Find the earliest occurrence of any term; returns null if none match. */
function firstMatch(
  haystack: string,
  terms: string[]
): { index: number; length: number } | null {
  let best: { index: number; length: number } | null = null;
  for (const term of terms) {
    const idx = haystack.indexOf(term);
    if (idx !== -1 && (best === null || idx < best.index)) {
      best = { index: idx, length: term.length };
    }
  }
  return best;
}

/** Character range of a match in `content`. */
export interface MatchRange {
  index: number;
  length: number;
}

/**
 * Earliest occurrence of any query term in `content` (case-insensitive),
 * or null. The same match `buildSnippet` centers its excerpt on, so a
 * caller can open the document to exactly what the snippet showed.
 */
export function findFirstMatch(content: string, terms: string[]): MatchRange | null {
  const normTerms = normalizeTerms(terms);
  if (normTerms.length === 0 || content === '') return null;
  return firstMatch(content.toLowerCase(), normTerms);
}

export function buildSnippet(
  content: string,
  terms: string[],
  opts: SnippetOptions = {}
): SnippetSegment[] {
  if (content === '') return [];

  const context = opts.context ?? 40;
  const maxLength = opts.maxLength ?? 80;
  const normTerms = normalizeTerms(terms);
  const lower = content.toLowerCase();

  const hit = normTerms.length > 0 ? firstMatch(lower, normTerms) : null;

  // No match: return a leading slice as a single, unmarked segment.
  if (hit === null) {
    const slice = content.slice(0, maxLength);
    const text = slice.length < content.length ? slice + ELLIPSIS : slice;
    return text === '' ? [] : [{ text, match: false }];
  }

  // Window around the first match.
  const start = Math.max(0, hit.index - context);
  const end = Math.min(content.length, hit.index + hit.length + context);
  const window = content.slice(start, end);
  const windowLower = lower.slice(start, end);

  // Walk the window, emitting alternating unmatched / matched segments by
  // scanning for any term at each position (greedy, longest term first).
  const segments: SnippetSegment[] = [];
  let plainStart = 0;
  let i = 0;
  const pushPlain = (upto: number) => {
    if (upto > plainStart) {
      segments.push({ text: window.slice(plainStart, upto), match: false });
    }
  };
  while (i < window.length) {
    let matchedLen = 0;
    for (const term of normTerms) {
      if (windowLower.startsWith(term, i)) {
        matchedLen = term.length;
        break;
      }
    }
    if (matchedLen > 0) {
      pushPlain(i);
      segments.push({ text: window.slice(i, i + matchedLen), match: true });
      i += matchedLen;
      plainStart = i;
    } else {
      i++;
    }
  }
  pushPlain(window.length);

  // Add ellipses where the window was clipped, by prefixing/suffixing the
  // adjacent plain segment (or inserting one).
  if (start > 0) {
    if (segments[0]?.match === false) {
      segments[0] = { text: ELLIPSIS + segments[0].text, match: false };
    } else {
      segments.unshift({ text: ELLIPSIS, match: false });
    }
  }
  if (end < content.length) {
    const last = segments[segments.length - 1];
    if (last?.match === false) {
      segments[segments.length - 1] = { text: last.text + ELLIPSIS, match: false };
    } else {
      segments.push({ text: ELLIPSIS, match: false });
    }
  }

  return segments;
}
