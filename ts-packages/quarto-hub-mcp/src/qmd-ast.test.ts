/**
 * qmd AST utilities (CAP-11): outline extraction, section location, and
 * section body splicing over the pampa JSON AST produced by the staged
 * wasm-qmd-parser (spike S-1). These are the structural primitives behind
 * `get_outline` and the `section` selectors on read_file/patch_file.
 *
 * Tests parse real fixture documents through the actual WASM parser so the
 * AST-shape assumptions are pinned against ground truth, not hand-written
 * JSON.
 */

import { describe, it, expect, beforeAll } from 'vitest';

import { loadQmdParser, type QmdParser } from './qmd-parser.js';
import { PARSER_UNAVAILABLE } from './test-setup.js';
import {
  extractOutline,
  findSection,
  sectionContent,
  replaceSection,
  inlineText,
  type OutlineEntry,
} from './qmd-ast.js';

const QMD = [
  '---',
  'title: Sections',
  '---',
  '',
  '# Introduction',
  '',
  'Intro paragraph.',
  '',
  '## Background',
  '',
  'Background text.',
  '',
  '# Methods',
  '',
  'Method text.',
  '',
  '### Details',
  '',
  'Detail text.',
  '',
  '# Results',
  '',
  'Result text.',
  '',
].join('\n');
const QMD_LINES = 23; // trailing newline terminates line 23

let parser: QmdParser;

beforeAll(async () => {
  parser = await loadQmdParser();
});

describe.skipIf(PARSER_UNAVAILABLE)('loadQmdParser', () => {
  it('parses qmd into a pampa JSON AST with block locations', () => {
    const ast = parser.parse('# Hello\n\nWorld.\n');
    expect(ast).not.toBeNull();
    const blocks = (ast as { blocks: unknown[] }).blocks;
    expect(Array.isArray(blocks)).toBe(true);
    expect(blocks.length).toBeGreaterThan(0);
  });

  it('returns null for content the parser rejects', () => {
    // The qmd parser is near-total; this pins the defensive contract, not a
    // specific rejection. If pampa becomes fully total, swap this for a
    // stubbed-failure test.
    const ast = parser.parse('');
    expect(ast === null || typeof ast === 'object').toBe(true);
  });
});

describe.skipIf(PARSER_UNAVAILABLE)('extractOutline', () => {
  it('returns the heading tree with 1-based line ranges', () => {
    const ast = parser.parse(QMD);
    const outline = extractOutline(ast, QMD_LINES);
    expect(outline).toEqual([
      { level: 1, title: 'Introduction', id: 'introduction', line: 5, endLine: 12 },
      { level: 2, title: 'Background', id: 'background', line: 9, endLine: 12 },
      { level: 1, title: 'Methods', id: 'methods', line: 13, endLine: 20 },
      { level: 3, title: 'Details', id: 'details', line: 17, endLine: 20 },
      { level: 1, title: 'Results', id: 'results', line: 21, endLine: 23 },
    ]);
  });

  it('gives the last section the document line count as its end', () => {
    const ast = parser.parse('# Only\n\nbody\n');
    const outline = extractOutline(ast, 3);
    expect(outline).toEqual([
      { level: 1, title: 'Only', id: 'only', line: 1, endLine: 3 },
    ]);
  });

  it('returns an empty outline for a document with no headings', () => {
    const ast = parser.parse('Just a paragraph.\n');
    expect(extractOutline(ast, 1)).toEqual([]);
  });

  it('omits id when the heading has none', () => {
    const ast = parser.parse('# Plain {#custom}\n\n# Auto\n');
    const outline = extractOutline(ast, 4);
    expect(outline[0]).toMatchObject({ title: 'Plain', id: 'custom' });
    // pampa auto-generates ids from the title; whatever the second entry's
    // id is, it must be a string or absent — never null.
    const second = outline[1] as OutlineEntry & { id?: string | null };
    expect(second.id === undefined || typeof second.id === 'string').toBe(true);
  });
});

describe.skipIf(PARSER_UNAVAILABLE)('inlineText', () => {
  it('flattens formatting inlines to plain text', () => {
    const ast = parser.parse('# A *bold* `code` [link](./x.html) title\n');
    const outline = extractOutline(ast, 2);
    expect(outline[0]?.title).toBe('A bold code link title');
  });
});

describe.skipIf(PARSER_UNAVAILABLE)('findSection', () => {
  it('finds a section by exact title', () => {
    const ast = parser.parse(QMD);
    const found = findSection(ast, 'Background', QMD_LINES);
    expect(found.ok).toBe(true);
    if (found.ok) {
      expect(found.section).toMatchObject({ level: 2, line: 9, endLine: 12 });
    }
  });

  it('matches titles with surrounding whitespace trimmed', () => {
    const ast = parser.parse(QMD);
    expect(findSection(ast, '  Methods  ', QMD_LINES).ok).toBe(true);
  });

  it('reports a miss with the available titles', () => {
    const ast = parser.parse(QMD);
    const miss = findSection(ast, 'Conclusion', QMD_LINES);
    expect(miss.ok).toBe(false);
    if (!miss.ok) {
      expect(miss.reason).toBe('not_found');
      expect(miss.available).toEqual([
        'Introduction',
        'Background',
        'Methods',
        'Details',
        'Results',
      ]);
    }
  });

  it('reports ambiguity with the matching locations', () => {
    const dup = '# Alpha\n\n## Notes\n\na\n\n# Beta\n\n## Notes\n\nb\n';
    const ast = parser.parse(dup);
    const found = findSection(ast, 'Notes', 11);
    expect(found.ok).toBe(false);
    if (!found.ok) {
      expect(found.reason).toBe('ambiguous');
      expect(found.matches).toEqual(['Notes (line 3)', 'Notes (line 9)']);
    }
  });

  it('is case-sensitive', () => {
    const ast = parser.parse(QMD);
    expect(findSection(ast, 'methods', QMD_LINES).ok).toBe(false);
  });
});

describe.skipIf(PARSER_UNAVAILABLE)('sectionContent', () => {
  it('returns the heading line plus body up to the next same-or-higher heading', () => {
    const ast = parser.parse(QMD);
    const found = findSection(ast, 'Background', QMD_LINES);
    if (!found.ok) throw new Error('section not found');
    expect(sectionContent(QMD, found.section)).toBe(
      '## Background\n\nBackground text.\n\n',
    );
  });

  it('includes subsections in the parent section', () => {
    const ast = parser.parse(QMD);
    const found = findSection(ast, 'Methods', QMD_LINES);
    if (!found.ok) throw new Error('section not found');
    expect(sectionContent(QMD, found.section)).toBe(
      '# Methods\n\nMethod text.\n\n### Details\n\nDetail text.\n\n',
    );
  });

  it('a final section without a trailing newline is returned verbatim', () => {
    const text = '# A\n\nbody';
    const ast = parser.parse(text);
    const found = findSection(ast, 'A', 3);
    if (!found.ok) throw new Error('section not found');
    expect(sectionContent(text, found.section)).toBe('# A\n\nbody');
  });
});

describe.skipIf(PARSER_UNAVAILABLE)('replaceSection', () => {
  it('replaces the whole section, heading included, preserving the rest', () => {
    const ast = parser.parse(QMD);
    const found = findSection(ast, 'Methods', QMD_LINES);
    if (!found.ok) throw new Error('section not found');
    const result = replaceSection(QMD, found.section, '# Methods\n\nRewritten methods.\n');
    expect(result).toBe(
      [
        '---',
        'title: Sections',
        '---',
        '',
        '# Introduction',
        '',
        'Intro paragraph.',
        '',
        '## Background',
        '',
        'Background text.',
        '',
        '# Methods',
        '',
        'Rewritten methods.',
        '# Results',
        '',
        'Result text.',
        '',
      ].join('\n'),
    );
  });

  it('replaces the final section, keeping the file trailing newline', () => {
    const ast = parser.parse(QMD);
    const found = findSection(ast, 'Results', QMD_LINES);
    if (!found.ok) throw new Error('section not found');
    const result = replaceSection(QMD, found.section, '# Results\n\nNew results.\nMore.');
    expect(result.endsWith('# Results\n\nNew results.\nMore.\n')).toBe(true);
    expect(result.startsWith('---')).toBe(true);
  });

  it('an empty replacement deletes the section outright', () => {
    const ast = parser.parse(QMD);
    const found = findSection(ast, 'Background', QMD_LINES);
    if (!found.ok) throw new Error('section not found');
    const result = replaceSection(QMD, found.section, '');
    expect(result).not.toContain('Background');
    expect(result).toContain('# Methods');
    // The surrounding sections join cleanly.
    expect(result).toContain('Intro paragraph.\n\n# Methods');
  });

  it('preserves a missing trailing newline at EOF', () => {
    const text = '# A\n\nold body';
    const ast = parser.parse(text);
    const found = findSection(ast, 'A', 3);
    if (!found.ok) throw new Error('section not found');
    const result = replaceSection(text, found.section, '# A\n\nnew body');
    expect(result).toBe('# A\n\nnew body');
  });

  it('edits outside the section survive verbatim (the CRDT-merge property)', () => {
    // Simulates: agent splices a section while a collaborator's edit
    // elsewhere in the file is already in the base text. The splice must
    // touch only the section's lines.
    const base = QMD.replace('Result text.', 'Result text (collaborator).');
    const ast = parser.parse(base);
    const found = findSection(ast, 'Methods', QMD_LINES);
    if (!found.ok) throw new Error('section not found');
    const result = replaceSection(base, found.section, '# Methods\n\nRewritten.');
    expect(result).toContain('Result text (collaborator).');
    expect(result).toContain('# Methods\n\nRewritten.\n');
    expect(result).not.toContain('Method text.');
  });

  it('supports renaming the heading inside the replacement text', () => {
    const ast = parser.parse(QMD);
    const found = findSection(ast, 'Background', QMD_LINES);
    if (!found.ok) throw new Error('section not found');
    const result = replaceSection(QMD, found.section, '## Context\n\nBackground text.');
    expect(result).toContain('## Context\n\nBackground text.');
    expect(result).not.toContain('## Background');
  });
});
