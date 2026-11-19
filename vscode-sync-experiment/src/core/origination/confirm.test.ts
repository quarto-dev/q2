import { describe, expect, it } from 'vitest';
import { PUBLIC_SERVER, buildConfirm, formatBytes, looksSecret, type ConfirmInput } from './confirm';

const base: ConfirmInput = {
  server: PUBLIC_SERVER,
  projectName: 'docs-site',
  kind: 'git',
  branch: 'main',
  fileCount: 142,
  totalBytes: 18.4 * 1024 * 1024,
  skipped: [{ path: 'a.bin', size: 11 << 20 }, { path: 'b.bin', size: 12 << 20 }, { path: 'c.bin', size: 13 << 20 }],
  dirty: ['index.qmd', 'guides/intro.qmd', 'guides/setup.qmd', 'x.qmd'],
  published: ['index.qmd'],
};

describe('buildConfirm', () => {
  it('the public server, a dirty git clone: the exact text', () => {
    const c = buildConfirm(base);
    expect(c.title).toBe('Share this folder on wss://sync.automerge.org?');
    expect(c.body).toBe(
      [
        "This publishes a copy of your project to a public server. Anyone who gets the link can read and edit it, and it can't be unpublished.",
        '',
        '  Project:        docs-site  (git repo, branch main)',
        '  Files:          142 tracked files, 18.4 MB',
        '  Skipped:        3 files over 10 MB (listed in the Quarto Hub output)',
        '  Uncommitted:    4 files will be shared as they are on disk:',
        '                    index.qmd',
        '                    guides/intro.qmd',
        '                    guides/setup.qmd',
        '                    and 1 more',
      ].join('\n'),
    );
    expect(c.confirmLabel).toBe('Share');
  });

  it('a custom server drops the irrevocable line and says who can read it', () => {
    const c = buildConfirm({ ...base, server: 'ws://127.0.0.1:3030', skipped: [], dirty: [] });
    expect(c.title).toBe('Share this folder on ws://127.0.0.1:3030?');
    expect(c.body.split('\n')[0]).toBe('This publishes a copy of your project to that server. Anyone with access to that server and the project link can read and edit it.');
    expect(c.body).not.toContain("can't be unpublished");
    expect(c.body).toContain('  Uncommitted:    none');
    expect(c.body).not.toContain('Skipped:');
  });

  it('three or fewer dirty files are all named, with no "and N more"', () => {
    const c = buildConfirm({ ...base, dirty: ['a.qmd', 'b.qmd'], skipped: [] });
    expect(c.body).toContain('  Uncommitted:    2 files will be shared as they are on disk:\n                    a.qmd\n                    b.qmd');
    expect(c.body).not.toContain('more');
  });

  it('one skipped file and one dirty file use the singular', () => {
    const c = buildConfirm({ ...base, skipped: [{ path: 'a.bin', size: 11 << 20 }], dirty: ['a.qmd'] });
    expect(c.body).toContain('1 file over 10 MB');
    expect(c.body).toContain('1 file will be shared as it is on disk:');
  });

  it('a plain folder says so and has no Uncommitted line', () => {
    const c = buildConfirm({ ...base, kind: 'plain', branch: null, dirty: [] });
    expect(c.body).toContain('  Project:        docs-site  (plain folder)');
    expect(c.body).toContain('  Files:          142 files, 18.4 MB');
    expect(c.body).not.toContain('Uncommitted');
  });

  it('lists up to three secret-looking published files', () => {
    const c = buildConfirm({ ...base, published: ['.env', 'keys/server.pem', 'my-secret.txt', 'api-token.json', 'index.qmd'] });
    expect(c.body).toContain('  Secret-looking files:\n                    .env\n                    keys/server.pem\n                    my-secret.txt');
    expect(c.body).not.toContain('api-token.json');
  });
});

describe('looksSecret', () => {
  it.each(['.env', 'a/.Renviron', 'x.pem', 'dir/my-Secret.yml', 'github_token.txt'])('%s looks secret', (p) => expect(looksSecret(p)).toBe(true));
  it.each(['index.qmd', 'environment.qmd', 'tokenizer-notes.qmd'.replace('tokenizer', 'x'), 'a.pdf'])('%s does not', (p) => expect(looksSecret(p)).toBe(false));
});

describe('formatBytes', () => {
  it('uses one decimal for MB and rounds small sizes up to KB', () => {
    expect(formatBytes(0)).toBe('0 KB');
    expect(formatBytes(1500)).toBe('2 KB');
    expect(formatBytes(18.4 * 1024 * 1024)).toBe('18.4 MB');
  });
});
