import { describe, expect, it } from 'vitest';
import { ARGV_ALLOWLIST_VERSION, UnsupportedArgv, argvToDefaults } from './argv.ts';
import { bytes, readRepoJson } from './golden.test-util.ts';

const A = (...a: string[]) => ['pandoc', ...a];
const unsupported = (argv: string[], files = [] as { path: string; bytes: Uint8Array }[]) => {
  try {
    argvToDefaults(argv, files);
  } catch (e) {
    expect(e).toBeInstanceOf(UnsupportedArgv);
    return e as UnsupportedArgv;
  }
  throw new Error('expected UnsupportedArgv');
};

describe('argvToDefaults', () => {
  it('maps the typst grammar one to one', () => {
    const o = argvToDefaults(
      A('-f', 'json', '-t', 'typst', '--data-dir', '/s/datadir', '-L', '/s/main.lua', '--standalone', '--wrap', 'none', '--default-image-extension', 'svg', '--resource-path', '/d', '--shift-heading-level-by', '-1', '--template', '/t/template.typ', '-o', '/o/doc.typ', '/tmp/in.json'),
    );
    expect(o).toEqual({
      from: 'json',
      to: 'typst',
      'data-dir': '/s/datadir',
      filters: [{ type: 'lua', path: '/s/main.lua' }],
      standalone: true,
      wrap: 'none',
      'default-image-extension': 'svg',
      'resource-path': ['/d'],
      'shift-heading-level-by': -1,
      template: '/t/template.typ',
      'output-file': '/o/doc.typ',
      'input-files': ['/tmp/in.json'],
    });
  });

  it('translates the recorded typst argv', () => {
    const argv = readRepoJson('crates/quarto-core/tests/fixtures/pandoc-recordings/recordings/callouts-typst/argv.json');
    const o = argvToDefaults(argv);
    expect(o.from).toBe('json');
    expect(o.to).toBe('typst');
    expect(o['input-files']).toHaveLength(1);
  });

  it('folds -V as pandoc does: split at the first : or =, bare means true, repeats make an array', () => {
    expect(argvToDefaults(A('-V', 'a:b')).variables).toEqual({ a: 'b' });
    expect(argvToDefaults(A('-V', 'a')).variables).toEqual({ a: 'true' });
    expect(argvToDefaults(A('-V', 'k=v', '-V', 'k=w', '-V', 'k:x')).variables).toEqual({ k: ['v', 'w', 'x'] });
    const defs = '#let EndLine() = {\n  x: 1\n}\n';
    expect(argvToDefaults(A('-V', `highlighting-definitions=${defs}`)).variables).toEqual({ 'highlighting-definitions': defs });
  });

  it('colon-splits --resource-path and lets a repeat prepend', () => {
    expect(argvToDefaults(A('--resource-path', '/a:/b', '--resource-path', '/c'))['resource-path']).toEqual(['/c', '/a', '/b']);
    expect(argvToDefaults(A('--resource-path', '/a::/b'))['resource-path']).toEqual(['/a', '.', '/b']);
  });

  it('inlines a --defaults file of key: scalar lines and rejects anything else', () => {
    const ok = [{ path: '/tmp/d.yaml', bytes: bytes('toc: true\ntoc-depth: 17\n') }];
    expect(argvToDefaults(A('--defaults', '/tmp/d.yaml'), ok)).toMatchObject({ toc: true, 'toc-depth': 17 });
    expect(argvToDefaults(A('--defaults=/tmp/d.yaml'), ok)).toMatchObject({ toc: true });
    expect(unsupported(A('--defaults', '/missing.yaml'), ok).reason).toMatch(/not among/);
    for (const text of ['toc:\n  - a\n', 'filters: [a]\n', 'a: |\n  text\n', 'just text\n']) {
      unsupported(A('--defaults', '/tmp/d.yaml'), [{ path: '/tmp/d.yaml', bytes: bytes(text) }]);
    }
    // a defaults key may not shadow an option given on the command line
    unsupported(A('-t', 'typst', '--defaults', '/tmp/d.yaml'), [{ path: '/tmp/d.yaml', bytes: bytes('to: html\n') }]);
  });

  it('drops +RTS ... -RTS and never takes it for an input file', () => {
    expect(argvToDefaults(A('+RTS', '-M5m', '-RTS', '-t', 'typst', '/in.json'))).toEqual({ to: 'typst', 'input-files': ['/in.json'] });
    expect(argvToDefaults(A('-t', 'typst', '+RTS', '-M5m'))).toEqual({ to: 'typst' });
  });

  it('accepts -t typst-citations and the = form of long flags', () => {
    expect(argvToDefaults(A('-t', 'typst-citations', '--syntax-highlighting=idiomatic', '--top-level-division=chapter', '--reference-doc', '/r.docx'))).toEqual({
      to: 'typst-citations',
      'syntax-highlighting': 'idiomatic',
      'top-level-division': 'chapter',
      'reference-doc': '/r.docx',
    });
  });

  it('rejects a repeated scalar flag, an unknown flag, a missing value and a non-integer shift', () => {
    expect(unsupported(A('--shift-heading-level-by', '1', '--shift-heading-level-by', '2')).reason).toMatch(/repeated/);
    expect(unsupported(A('--template', '/a', '--template', '/b')).reason).toMatch(/repeated/);
    expect(unsupported(A('--no-such-flag')).reason).toMatch(/unknown/);
    expect(unsupported(A('-t')).reason).toMatch(/no value/);
    expect(unsupported(A('--shift-heading-level-by', 'x')).reason).toMatch(/integer/);
    expect(unsupported(A('--standalone=1')).reason).toMatch(/switch/);
  });

  it('keeps several -L filters in order and several inputs', () => {
    const o = argvToDefaults(A('-L', '/a.lua', '-L', '/b.lua', '/x.json', '/y.json'));
    expect(o.filters?.map((f) => f.path)).toEqual(['/a.lua', '/b.lua']);
    expect(o['input-files']).toEqual(['/x.json', '/y.json']);
  });
});

describe('the allowlist file', () => {
  const file = readRepoJson('ts-packages/pandoc-host/src/typst-argv-flags.json') as { version: number; flags: string[] };
  it('has the version of the translator', () => expect(file.version).toBe(ARGV_ALLOWLIST_VERSION));
  it('lists only flags the translator accepts', () => {
    for (const flag of file.flags) {
      const value = flag === '--standalone' ? [] : flag === '-V' ? ['k=v'] : flag === '--shift-heading-level-by' ? ['1'] : flag === '--defaults' ? ['/d.yaml'] : ['v'];
      const files = flag === '--defaults' ? [{ path: '/d.yaml', bytes: bytes('toc: true\n') }] : [];
      expect(() => argvToDefaults(['pandoc', flag, ...value], files), flag).not.toThrow();
    }
  });
});
