/**
 * The docs tool's engine (CAP-13, absorbing bd-dn81ol95 + bd-b6cocsxw):
 * Quarto 2 documentation search/fetch over the corpus embedded in the
 * `q2` binary — the same pages `q2 docs llms` serves, reachable from MCP
 * without the agent shelling out.
 *
 * The seam is one subprocess per process lifetime: `q2 docs llms --full`
 * (resolved via `QUARTO_Q2_PATH`, injected by the `q2 mcp` launcher —
 * CAP-12 — falling back to `q2` on PATH). The corpus's per-page
 * `---\ntitle:/url:` marker blocks make page attribution exact, so
 * search and single-page fetches are served from the in-process cache
 * after one fetch.
 */

import { spawn } from 'node:child_process';

import { z } from 'zod';

/** One documentation page from the corpus. */
export interface DocsPage {
  href: string;
  title: string;
  content: string;
}

/** A search hit. `snippet` is present when the body (not just the title) matched. */
export interface DocsHit {
  href: string;
  title: string;
  snippet?: string;
}

/**
 * Split llms-full.txt into pages on the marker blocks the writer emits
 * (`---\ntitle: X\nurl: Y\n---` — crates/quarto-core/src/project/
 * llms_post_render.rs `assemble_llms_full`). A `---` line only opens a
 * page when the `title:` + `url:` lines follow, so YAML-looking fences
 * inside code blocks never split a page.
 */
export function parseFullCorpus(text: string): DocsPage[] {
  const pages: DocsPage[] = [];
  const lines = text.split('\n');
  let i = 0;
  const markerAt = (j: number): { title: string; href: string } | null => {
    if (lines[j] !== '---') return null;
    const t = lines[j + 1]?.match(/^title: (.*)$/);
    const u = lines[j + 2]?.match(/^url: (.*)$/);
    if (!t || !u || lines[j + 3] !== '---') return null;
    return { title: t[1]!.trim(), href: u[1]!.trim() };
  };
  while (i < lines.length) {
    const marker = markerAt(i);
    if (!marker) {
      i++;
      continue;
    }
    // Collect the body until the next marker or EOF.
    let end = i + 4;
    while (end < lines.length && markerAt(end) === null) end++;
    const body = lines
      .slice(i + 4, end)
      .join('\n')
      .replace(/^\n+/, '')
      .replace(/\n+$/, '');
    pages.push({ href: marker.href, title: marker.title, content: body });
    i = end;
  }
  return pages;
}

/** Snippet budget per hit; long lines are centered on the match. */
const SNIPPET_MAX_CHARS = 200;

function makeSnippet(line: string, matchIndex: number, matchLength: number): string {
  const trimmed = line.trim();
  if (trimmed.length <= SNIPPET_MAX_CHARS) return trimmed;
  let start = Math.max(0, matchIndex - Math.floor((SNIPPET_MAX_CHARS - matchLength) / 2));
  const end = Math.min(line.length, start + SNIPPET_MAX_CHARS);
  start = Math.max(0, end - SNIPPET_MAX_CHARS);
  const prefix = start > 0 ? '…' : '';
  const suffix = end < line.length ? '…' : '';
  return `${prefix}${line.slice(start, end).trim()}${suffix}`;
}

/**
 * Rank pages against a query: title substring matches outrank body hits;
 * more body hits outrank fewer. Matching is case-insensitive on the whole
 * query string. Returns ALL hits in score order (stable in page order) —
 * the caller slices for its result cap and reports the rest as truncated.
 */
export function searchDocs(query: string, pages: DocsPage[]): DocsHit[] {
  // Multi-word queries match term by term: a page qualifies on any term
  // and ranks on the sum, so AND-matches float to the top.
  const terms = query.toLowerCase().split(/\s+/).filter((t) => t !== '');
  if (terms.length === 0) return [];
  const scored: Array<{ page: DocsPage; score: number; snippet?: string; index: number }> = [];
  pages.forEach((page, index) => {
    let score = 0;
    let snippet: string | undefined;
    const titleLower = page.title.toLowerCase();
    const hrefLower = page.href.toLowerCase();
    const lines = page.content.split('\n');
    const linesLower = lines.map((l) => l.toLowerCase());
    // The snippet is the line matching the most distinct query terms
    // (first such line wins ties) — the densest context, not just the
    // first hit.
    let bestSnippetTerms = 0;
    for (let li = 0; li < lines.length; li++) {
      const matched = terms.filter((t) => linesLower[li]!.includes(t));
      if (matched.length > bestSnippetTerms) {
        bestSnippetTerms = matched.length;
        const idx = linesLower[li]!.indexOf(matched[0]!);
        snippet = makeSnippet(lines[li]!, idx, matched[0]!.length);
      }
    }
    for (const term of terms) {
      if (titleLower.includes(term)) score += 10;
      if (hrefLower.includes(term)) score += 4;
      let bodyHits = 0;
      for (let li = 0; li < lines.length; li++) {
        const idx = linesLower[li]!.indexOf(term);
        if (idx === -1) continue;
        bodyHits++;
        if (bodyHits >= 5) break; // cap per-term body contribution
      }
      score += Math.min(bodyHits, 5);
    }
    if (score > 0) scored.push({ page, score, ...(snippet ? { snippet } : {}), index });
  });
  scored.sort((a, b) => b.score - a.score || a.index - b.index);
  return scored.map(({ page, snippet }) => ({
    href: page.href,
    title: page.title,
    ...(snippet ? { snippet } : {}),
  }));
}

/**
 * Candidate companion hrefs for a query, mirroring the CLI's
 * normalization (docs_llms.rs `candidates`): strip `./`, `/`, and a
 * trailing `/`; map `.qmd`/`.html` to `.md`; extensionless paths try
 * `<q>.md` then `<q>/index.md`.
 */
function hrefCandidates(query: string): string[] {
  let q = query.trim().replaceAll('\\', '/');
  while (q.startsWith('./')) q = q.slice(2);
  while (q.startsWith('/')) q = q.slice(1);
  if (q.endsWith('/')) q = q.slice(0, -1);
  if (q.endsWith('.md')) return [q];
  for (const ext of ['.qmd', '.html']) {
    if (q.endsWith(ext)) return [`${q.slice(0, -ext.length)}.md`];
  }
  return [`${q}.md`, `${q}/index.md`];
}

export type FindPageResult =
  | { ok: true; page: DocsPage }
  | { ok: false; suggestions: string[] };

/** Locate one page by any accepted spelling; a miss suggests nearby hrefs. */
export function findPage(pages: DocsPage[], query: string): FindPageResult {
  const candidates = hrefCandidates(query);
  const byHref = new Map(pages.map((p) => [p.href, p]));
  for (const candidate of candidates) {
    const page = byHref.get(candidate);
    if (page) return { ok: true, page };
  }
  // Suggest: pages whose stem contains the query stem, else same-directory pages.
  const stem = (candidates[0] ?? query).replace(/\.md$/, '').split('/').pop()!.toLowerCase();
  const suggestions = pages
    .filter((p) => p.href.toLowerCase().includes(stem))
    .map((p) => p.href)
    .slice(0, 5);
  return { ok: false, suggestions };
}

// ---------------------------------------------------------------------------
// Corpus loading (the q2 subprocess seam)
// ---------------------------------------------------------------------------

/** How the corpus load can fail, surfaced as an actionable tool error. */
export class DocsUnavailableError extends Error {
  override readonly name = 'DocsUnavailableError';
}

const CORPUS_TIMEOUT_MS = 30_000;

/** The process-lifetime corpus cache, keyed by the resolved q2 path. */
const corpusCache = new Map<string, Promise<DocsPage[]>>();

/** Test seam: drop the cache so a new QUARTO_Q2_PATH takes effect. */
export function resetDocsCorpusForTests(): void {
  corpusCache.clear();
}

function fetchFullCorpus(q2Path: string): Promise<string> {
  return new Promise((resolvePromise, reject) => {
    let child;
    try {
      child = spawn(q2Path, ['docs', 'llms', '--full'], { stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (err) {
      reject(err);
      return;
    }
    let stdout = '';
    let stderr = '';
    const timer = setTimeout(() => {
      child.kill('SIGTERM');
      reject(new DocsUnavailableError(`\`q2 docs llms --full\` did not answer within 30 s`));
    }, CORPUS_TIMEOUT_MS);
    timer.unref();
    child.stdout.on('data', (c: Buffer) => (stdout += c.toString('utf8')));
    child.stderr.on('data', (c: Buffer) => (stderr += c.toString('utf8')));
    child.on('error', (err) => {
      clearTimeout(timer);
      reject(err);
    });
    child.on('close', (code) => {
      clearTimeout(timer);
      if (code === 0) {
        resolvePromise(stdout);
      } else {
        reject(new DocsUnavailableError(stderr.trim() || `q2 docs llms --full exited ${code}`));
      }
    });
  });
}

/**
 * The documentation corpus, fetched once per process from the resolved
 * q2 (`QUARTO_Q2_PATH`, else `q2` on PATH) and cached for the lifetime
 * of the server.
 */
export function loadDocsPages(): Promise<DocsPage[]> {
  const q2Path = process.env['QUARTO_Q2_PATH'] ?? 'q2';
  let cached = corpusCache.get(q2Path);
  if (!cached) {
    cached = (async () => {
      try {
        return parseFullCorpus(await fetchFullCorpus(q2Path));
      } catch (err) {
        if (err instanceof DocsUnavailableError) throw err;
        const detail = err instanceof Error ? err.message : String(err);
        throw new DocsUnavailableError(
          `could not read the embedded documentation from q2 ("${q2Path}"): ${detail}. ` +
            'Under `q2 mcp` the launcher injects QUARTO_Q2_PATH; standalone, set QUARTO_Q2_PATH ' +
            'or put q2 on PATH.',
        );
      }
    })();
    // A failed load is not cached — the next call retries.
    cached.catch(() => corpusCache.delete(q2Path));
    corpusCache.set(q2Path, cached);
  }
  return cached;
}

// ---------------------------------------------------------------------------
// Output schemas (registered on the tool; validated in tests)
// ---------------------------------------------------------------------------

export const outDocsQuery = z.object({
  results: z.array(
    z.object({
      href: z.string(),
      title: z.string(),
      snippet: z.string().optional(),
    }),
  ),
  total_matches: z.number(),
  truncated: z.boolean(),
});

export const outDocsPage = z.object({
  href: z.string(),
  title: z.string(),
  markdown: z.string(),
});
