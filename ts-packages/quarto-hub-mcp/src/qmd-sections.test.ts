/**
 * Section selectors on read_file / patch_file (CAP-11): structural reads
 * and edits of one qmd section by heading title, instead of fragile string
 * surgery. A section is a heading plus everything up to the next heading of
 * the same or higher level (its subsections included); the ranges agree
 * with get_outline.
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';
import { PARSER_UNAVAILABLE } from './test-setup.js';

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

interface SectionRead {
  path: string;
  hash: string;
  content: string;
  section: { name: string; level: number; heading_line: number; end_line: number };
}

function structured<T>(result: unknown): T {
  const sc = (result as { structuredContent?: unknown }).structuredContent;
  if (sc === undefined || sc === null || typeof sc !== 'object') {
    throw new Error(`expected structuredContent, got: ${JSON.stringify(result)}`);
  }
  return sc as T;
}

function errorText(result: unknown): string {
  const content = (result as { content?: Array<{ type: string; text?: string }> }).content;
  return content?.map((b) => b.text ?? '').join('\n') ?? '';
}

async function seed(f: InMemoryMcpFixture, content = QMD): Promise<string> {
  const s = await seedProject(f, [{ path: 'paper.qmd', content }]);
  return s.indexDocId;
}

describe.skipIf(PARSER_UNAVAILABLE)('read_file with a section selector (CAP-11)', () => {
  it('returns exactly the section, heading line included, with its range', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      const result = await callTool(f, 'read_file', {
        project,
        path: 'paper.qmd',
        section: 'Background',
      });
      expect(result.isError).not.toBe(true);
      const read = structured<SectionRead>(result);
      expect(read.content).toBe('## Background\n\nBackground text.\n\n');
      expect(read.section).toEqual({ name: 'Background', level: 2, heading_line: 9, end_line: 12 });
      expect(read.hash).toMatch(/^sha256:/);
    } finally {
      await f.close();
    }
  });

  it('a parent section includes its subsections', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      const read = structured<SectionRead>(
        await callTool(f, 'read_file', { project, path: 'paper.qmd', section: 'Methods' }),
      );
      expect(read.content).toBe('# Methods\n\nMethod text.\n\n### Details\n\nDetail text.\n\n');
      expect(read.section.end_line).toBe(20);
    } finally {
      await f.close();
    }
  });

  it('the final section runs to EOF', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      const read = structured<SectionRead>(
        await callTool(f, 'read_file', { project, path: 'paper.qmd', section: 'Results' }),
      );
      expect(read.content).toBe('# Results\n\nResult text.\n');
      expect(read.section.end_line).toBe(23);
    } finally {
      await f.close();
    }
  });

  it('an unknown section names the available headings', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      const result = await callTool(f, 'read_file', {
        project,
        path: 'paper.qmd',
        section: 'Conclusion',
      });
      expect(result.isError).toBe(true);
      const text = errorText(result);
      expect(text).toMatch(/Conclusion/);
      expect(text).toMatch(/Introduction/);
      expect(text).toMatch(/Results/);
      expect(text).toMatch(/get_outline/);
    } finally {
      await f.close();
    }
  });

  it('an ambiguous section names the matches with line numbers', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(
        f,
        '# Alpha\n\n## Notes\n\na\n\n# Beta\n\n## Notes\n\nb\n',
      );
      const result = await callTool(f, 'read_file', {
        project,
        path: 'paper.qmd',
        section: 'Notes',
      });
      expect(result.isError).toBe(true);
      const text = errorText(result);
      expect(text).toMatch(/line 3/);
      expect(text).toMatch(/line 9/);
    } finally {
      await f.close();
    }
  });

  it('section and offset/limit are mutually exclusive', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      const result = await callTool(f, 'read_file', {
        project,
        path: 'paper.qmd',
        section: 'Methods',
        offset: 2,
      });
      expect(result.isError).toBe(true);
      expect(errorText(result)).toMatch(/section/);
      expect(errorText(result)).toMatch(/offset/);
    } finally {
      await f.close();
    }
  });

  it('errors on a non-qmd file, naming the .qmd requirement', async () => {
    const f = await startInMemoryMcp();
    try {
      const s = await seedProject(f, [{ path: 'notes.txt', content: '# Not qmd\n' }]);
      const result = await callTool(f, 'read_file', {
        project: s.indexDocId,
        path: 'notes.txt',
        section: 'Not qmd',
      });
      expect(result.isError).toBe(true);
      expect(errorText(result)).toMatch(/\.qmd/);
    } finally {
      await f.close();
    }
  });
});

describe.skipIf(PARSER_UNAVAILABLE)('patch_file with a section selector (CAP-11)', () => {
  it('replaces exactly the section read_file shows, heading included', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      const result = await callTool(f, 'patch_file', {
        project,
        path: 'paper.qmd',
        section: 'Methods',
        new_string: '# Methods\n\nRewritten methods.\n\n',
      });
      expect(result.isError).not.toBe(true);
      const patched = structured<{ hash: string; section: { name: string } }>(result);
      expect(patched.hash).toMatch(/^sha256:/);
      expect(patched.section.name).toBe('Methods');

      const after = structured<{ content: string }>(
        await callTool(f, 'read_file', { project, path: 'paper.qmd' }),
      );
      expect(after.content).toBe(
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
          '',
          '# Results',
          '',
          'Result text.',
          '',
        ].join('\n'),
      );
    } finally {
      await f.close();
    }
  });

  it('replacing a section replaces its subsections too (read/patch ranges agree)', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      const result = await callTool(f, 'patch_file', {
        project,
        path: 'paper.qmd',
        section: 'Methods',
        new_string: '# Methods\n\nAll new.',
      });
      expect(result.isError).not.toBe(true);
      const after = structured<{ content: string }>(
        await callTool(f, 'read_file', { project, path: 'paper.qmd' }),
      );
      expect(after.content).not.toContain('### Details');
      expect(after.content).not.toContain('Detail text.');
      expect(after.content).toContain('# Methods\n\nAll new.');
    } finally {
      await f.close();
    }
  });

  it('concurrent edits outside the section are preserved (CRDT merge)', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      // The agent reads the section it means to replace.
      const before = structured<SectionRead>(
        await callTool(f, 'read_file', { project, path: 'paper.qmd', section: 'Methods' }),
      );
      expect(before.section.name).toBe('Methods');
      // A collaborator edits a different section before the agent's patch
      // lands. (Sequential changes through the same updateText path the
      // CRDT merges — the splice must be computed against current text.)
      const collab = await callTool(f, 'patch_file', {
        project,
        path: 'paper.qmd',
        old_string: 'Result text.',
        new_string: 'Result text (collaborator).',
      });
      expect(collab.isError).not.toBe(true);

      const mine = await callTool(f, 'patch_file', {
        project,
        path: 'paper.qmd',
        section: 'Methods',
        new_string: '# Methods\n\nRewritten methods.',
      });
      expect(mine.isError).not.toBe(true);

      const after = structured<{ content: string }>(
        await callTool(f, 'read_file', { project, path: 'paper.qmd' }),
      );
      expect(after.content).toContain('Result text (collaborator).');
      expect(after.content).toContain('# Methods\n\nRewritten methods.\n');
      expect(after.content).not.toContain('Method text.');
    } finally {
      await f.close();
    }
  });

  it('an empty new_string deletes the section outright', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      const result = await callTool(f, 'patch_file', {
        project,
        path: 'paper.qmd',
        section: 'Background',
        new_string: '',
      });
      expect(result.isError).not.toBe(true);
      const after = structured<{ content: string }>(
        await callTool(f, 'read_file', { project, path: 'paper.qmd' }),
      );
      expect(after.content).not.toContain('Background');
      expect(after.content).toContain('Intro paragraph.\n\n# Methods');
    } finally {
      await f.close();
    }
  });

  it('section and old_string are mutually exclusive; one of them is required', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      const both = await callTool(f, 'patch_file', {
        project,
        path: 'paper.qmd',
        section: 'Methods',
        old_string: 'Method text.',
        new_string: 'x',
      });
      expect(both.isError).toBe(true);
      expect(errorText(both)).toMatch(/section/);
      expect(errorText(both)).toMatch(/old_string/);

      const neither = await callTool(f, 'patch_file', {
        project,
        path: 'paper.qmd',
        new_string: 'x',
      });
      expect(neither.isError).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('expected_hash still guards section patches (stale hash refused)', async () => {
    const f = await startInMemoryMcp();
    try {
      const project = await seed(f);
      const stale = structured<{ hash: string }>(
        await callTool(f, 'read_file', { project, path: 'paper.qmd' }),
      );
      const collab = await callTool(f, 'patch_file', {
        project,
        path: 'paper.qmd',
        old_string: 'Result text.',
        new_string: 'Changed.',
      });
      expect(collab.isError).not.toBe(true);

      const refused = await callTool(f, 'patch_file', {
        project,
        path: 'paper.qmd',
        section: 'Methods',
        new_string: 'x',
        expected_hash: stale.hash,
      });
      expect(refused.isError).toBe(true);
      const after = structured<{ content: string }>(
        await callTool(f, 'read_file', { project, path: 'paper.qmd' }),
      );
      expect(after.content).toContain('Method text.');
    } finally {
      await f.close();
    }
  });
});
