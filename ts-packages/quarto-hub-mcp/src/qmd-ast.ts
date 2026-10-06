/**
 * AST utilities for the qmd structure tools (CAP-11): outline extraction,
 * section location, and section body splicing over the pampa JSON AST
 * produced by {@link loadQmdParser}.
 *
 * Wire shape relied on (pinned by qmd-ast.test.ts against the real parser):
 * top-level `{ blocks: Block[] }`; a Header block is
 * `{ t: "Header", c: [level, [id, classes, kvs], inlines], l: Loc }` with
 * `Loc = { b: { c, l, o }, e: { c, l, o }, f }` (1-based line `l`, byte
 * offset `o`). All section math is done in *lines*, never byte offsets —
 * automerge text is a JS (UTF-16) string and byte↔index conversion is a
 * needless hazard.
 *
 * A "section" is a heading plus everything up to (not including) the next
 * heading of the same or higher level — subsections included. read_file's
 * `section` selector shows exactly that range and patch_file's replaces
 * exactly its body, so the two always agree.
 */

/** One heading in a document outline. Lines are 1-based, inclusive. */
export interface OutlineEntry {
  level: number;
  title: string;
  /** The heading's id attribute; omitted when the parser assigns none. */
  id?: string;
  /** Line of the heading itself. */
  line: number;
  /** Line of the section's last line (before the next same-or-higher heading, or EOF). */
  endLine: number;
}

export interface SectionMatch {
  ok: true;
  section: OutlineEntry;
}

export interface SectionMiss {
  ok: false;
  reason: 'not_found' | 'ambiguous';
  /** Ambiguous only: the matching titles with their line numbers. */
  matches: string[];
  /** All outline titles in document order (for error messages). */
  available: string[];
}

interface AstLoc {
  b?: { l?: unknown };
}

interface AstNode {
  t?: unknown;
  c?: unknown;
  a?: unknown;
  l?: AstLoc;
}

/** Same semantics as tools.ts's splitLines: "a\n" is one line, "" is zero. */
function splitLines(text: string): string[] {
  if (text === '') return [];
  const lines = text.split('\n');
  if (lines[lines.length - 1] === '') lines.pop();
  return lines;
}

function isInlineList(x: unknown): x is AstNode[] {
  return (
    Array.isArray(x) &&
    x.every((el) => el !== null && typeof el === 'object' && 't' in (el as object))
  );
}

function inlineNodeText(node: AstNode): string {
  switch (node.t) {
    case 'Str':
      return typeof node.c === 'string' ? node.c : '';
    case 'Space':
    case 'SoftBreak':
      return ' ';
    case 'LineBreak':
      return '\n';
    case 'Code':
    case 'Math':
      // [attr, text] / [mathType, text]
      return Array.isArray(node.c) && typeof node.c[1] === 'string' ? node.c[1] : '';
    default: {
      // Containers (Strong, Emph, Link, Quoted, Span, …): their inlines are
      // either `c` directly or one array slot of a `c` tuple.
      if (isInlineList(node.c)) return inlineText(node.c);
      if (Array.isArray(node.c)) {
        let s = '';
        for (const part of node.c) {
          if (isInlineList(part)) s += inlineText(part);
        }
        return s;
      }
      return '';
    }
  }
}

/** Flatten an inline sequence to plain text (Str/Space/Code/Math + containers). */
export function inlineText(inlines: unknown): string {
  if (!isInlineList(inlines)) return '';
  let out = '';
  for (const node of inlines) out += inlineNodeText(node);
  return out;
}

/**
 * Extract the document outline: every top-level Header block with its
 * 1-based line and section end line. `totalLines` is the document's line
 * count (the last section runs to EOF, which the AST alone cannot see —
 * trailing blank lines belong to the final section).
 */
export function extractOutline(ast: unknown, totalLines: number): OutlineEntry[] {
  const blocks = (ast as { blocks?: unknown } | null)?.blocks;
  if (!Array.isArray(blocks)) return [];
  const entries: OutlineEntry[] = [];
  for (const block of blocks as AstNode[]) {
    if (block?.t !== 'Header' || !Array.isArray(block.c)) continue;
    const [levelRaw, attrRaw, inlines] = block.c as [unknown, unknown, unknown];
    const level = typeof levelRaw === 'number' ? levelRaw : 0;
    const line = typeof block.l?.b?.l === 'number' ? block.l.b.l : 0;
    if (level < 1 || line < 1) continue;
    const attr = Array.isArray(attrRaw) ? attrRaw : [];
    const id = typeof attr[0] === 'string' && attr[0] !== '' ? attr[0] : undefined;
    entries.push({ level, title: inlineText(inlines), ...(id ? { id } : {}), line, endLine: 0 });
  }
  for (let i = 0; i < entries.length; i++) {
    let end = totalLines;
    for (let j = i + 1; j < entries.length; j++) {
      if (entries[j].level <= entries[i].level) {
        end = entries[j].line - 1;
        break;
      }
    }
    entries[i].endLine = end;
  }
  return entries;
}

/**
 * Locate one section by exact (whitespace-trimmed, case-sensitive) heading
 * title. Reports `not_found` with all available titles, or `ambiguous`
 * with the matching titles and their line numbers.
 */
export function findSection(
  ast: unknown,
  selector: string,
  totalLines: number,
): SectionMatch | SectionMiss {
  const outline = extractOutline(ast, totalLines);
  const wanted = selector.trim();
  const hits = outline.filter((e) => e.title === wanted);
  if (hits.length === 1) return { ok: true, section: hits[0] };
  if (hits.length > 1) {
    return {
      ok: false,
      reason: 'ambiguous',
      matches: hits.map((e) => `${e.title} (line ${e.line})`),
      available: outline.map((e) => e.title),
    };
  }
  return { ok: false, reason: 'not_found', matches: [], available: outline.map((e) => e.title) };
}

/**
 * The section's source text: heading line plus body, preserving the
 * file's own line termination (a final unterminated line stays so).
 */
export function sectionContent(text: string, section: OutlineEntry): string {
  const lines = splitLines(text);
  const body = lines.slice(section.line - 1, section.endLine).join('\n');
  return text.endsWith('\n') || section.endLine < lines.length ? body + '\n' : body;
}

/**
 * Replace the section's entire source range — heading line included —
 * with `newText`, preserving every line outside the section, including a
 * collaborator's concurrent edits there (the caller applies the result
 * through automerge's updateText, whose diff touches only the spliced
 * range). The file's trailing-newline state is preserved.
 *
 * The range is exactly what `sectionContent` returns, so read_file with a
 * `section` selector always shows precisely what patch_file will replace.
 * Replacing with '' deletes the section outright; editing the heading
 * line inside `newText` renames it.
 */
export function replaceSection(
  text: string,
  section: OutlineEntry,
  newText: string,
): string {
  const lines = splitLines(text);
  const before = lines.slice(0, section.line - 1);
  const after = lines.slice(section.endLine);
  const middle = splitLines(newText);
  const joined = [...before, ...middle, ...after].join('\n');
  if (joined === '') return '';
  return text.endsWith('\n') ? joined + '\n' : joined;
}
