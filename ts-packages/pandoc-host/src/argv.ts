import type { RequestFile } from './types.ts';

/**
 * Version of the typst argv grammar this translator handles. `src/typst-argv-flags.json` carries the same number
 * and the flag names; `crates/quarto-core/tests/integration/pandoc_typst_argv_guard.rs` fails when the builder
 * emits a flag that the file does not list, and a unit test fails when the translator does not accept every
 * listed flag, so drift in either direction is caught.
 */
export const ARGV_ALLOWLIST_VERSION = 1;

/** The options object the module's `convert` export takes: pandoc defaults-file keys (not argv). */
export interface ConvertOptions {
  from?: string;
  to?: string;
  'data-dir'?: string;
  filters?: { type: 'lua'; path: string }[];
  standalone?: boolean;
  wrap?: string;
  'default-image-extension'?: string;
  'resource-path'?: string[];
  'shift-heading-level-by'?: number;
  template?: string;
  'reference-doc'?: string;
  'top-level-division'?: string;
  'syntax-highlighting'?: string;
  variables?: Record<string, string | string[]>;
  'output-file'?: string;
  'input-files'?: string[];
  /** Keys inlined from a `--defaults` file (`toc`, `toc-depth`, ...). */
  [key: string]: unknown;
}

/** The argv has a flag, a repeat or a defaults-file line the translator does not handle: the request runs on the fresh path. */
export class UnsupportedArgv extends Error {
  readonly reason: string;
  readonly token?: string;
  constructor(reason: string, token?: string) {
    super(`unsupported pandoc argv${token === undefined ? '' : ` at \`${token}\``}: ${reason}`);
    this.name = 'UnsupportedArgv';
    this.reason = reason;
    this.token = token;
  }
}

/** Flags that take one value and may appear once. Each maps to a defaults key. */
const SCALARS: Record<string, string> = {
  '-f': 'from',
  '-t': 'to',
  '--data-dir': 'data-dir',
  '--wrap': 'wrap',
  '--default-image-extension': 'default-image-extension',
  '--shift-heading-level-by': 'shift-heading-level-by',
  '--template': 'template',
  '--reference-doc': 'reference-doc',
  '--top-level-division': 'top-level-division',
  '--syntax-highlighting': 'syntax-highlighting',
  '-o': 'output-file',
};
/** Value-less flags that may appear once. */
const SWITCHES: Record<string, string> = { '--standalone': 'standalone' };

const dec = new TextDecoder();

/** `+RTS ... -RTS` is dropped: the warm instance's RTS options were fixed at init, and the options are never input files. */
function stripRts(args: string[]): string[] {
  const out: string[] = [];
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '+RTS') {
      while (i < args.length && args[i] !== '-RTS') i++;
      continue;
    }
    out.push(args[i]);
  }
  return out;
}

/** pandoc's `splitSearchPath` on POSIX: colon-separated, an empty entry means the current directory. */
const splitSearchPath = (s: string) => s.split(':').map((p) => (p === '' ? '.' : p));

/** A `--defaults` line value: a boolean, an integer, or the text. */
function scalar(text: string): string | number | boolean {
  if (text === 'true') return true;
  if (text === 'false') return false;
  if (/^-?\d+$/.test(text)) return Number(text);
  return text.replace(/^(['"])(.*)\1$/, '$2');
}

/** The `key: scalar` lines of a defaults file; anything else (a list, a map, a block scalar, a second document) is unsupported. */
function readDefaults(path: string, files: readonly RequestFile[]): Record<string, string | number | boolean> {
  const file = files.find((f) => f.path === path);
  if (!file) throw new UnsupportedArgv('the defaults file is not among the request files', path);
  const out: Record<string, string | number | boolean> = {};
  for (const raw of dec.decode(file.bytes).split(/\r?\n/)) {
    const line = raw.trim();
    if (line === '' || line.startsWith('#')) continue;
    const m = /^([A-Za-z][\w-]*):\s+(\S.*)$/.exec(line);
    if (!m || /^[[{|>&*!]/.test(m[2])) throw new UnsupportedArgv(`the defaults file line \`${line}\` is not \`key: scalar\``, path);
    out[m[1]] = scalar(m[2]);
  }
  return out;
}

/**
 * Translate a pandoc argv (program name first) into the options `convert` takes, for the typst argv the Rust builder
 * emits (see `src/typst-argv-flags.json`). `files` is the request's file map; it is read only for `--defaults <path>`,
 * whose keys are inlined because `convert` rejects a `defaults` key. Semantics follow pandoc's own folding: `-V` splits at
 * the first `:` or `=`, a bare `-V key` means `"true"`, a repeated key becomes an array; `--resource-path` is
 * colon-split and a repeat prepends. Anything else throws `UnsupportedArgv`, which the warm executor answers by running
 * the request on the fresh path.
 */
export function argvToDefaults(argv: readonly string[], files: readonly RequestFile[] = []): ConvertOptions {
  const args = stripRts(argv.slice(1));
  const o: ConvertOptions = {};
  const seen = new Set<string>();
  const once = (key: string, token: string) => {
    if (seen.has(key)) throw new UnsupportedArgv('a repeated flag', token);
    seen.add(key);
  };
  const variables: Record<string, string | string[]> = {};
  const filters: { type: 'lua'; path: string }[] = [];
  let resourcePath: string[] = [];
  const inputs: string[] = [];

  for (let i = 0; i < args.length; i++) {
    let token = args[i];
    let inline: string | undefined;
    const eq = token.startsWith('--') ? token.indexOf('=') : -1;
    if (eq > 0) {
      inline = token.slice(eq + 1);
      token = token.slice(0, eq);
    }
    const value = () => {
      if (inline !== undefined) return inline;
      if (i + 1 >= args.length) throw new UnsupportedArgv('a flag with no value', token);
      return args[++i];
    };
    if (token in SWITCHES) {
      if (inline !== undefined) throw new UnsupportedArgv('a switch with a value', token);
      once(SWITCHES[token], token);
      o[SWITCHES[token]] = true;
    } else if (token in SCALARS) {
      const key = SCALARS[token];
      once(key, token);
      const v = value();
      if (key === 'shift-heading-level-by') {
        const n = Number(v);
        if (v.trim() === '' || !Number.isInteger(n)) throw new UnsupportedArgv('not an integer', `${token} ${v}`);
        o[key] = n;
      } else o[key] = v;
    } else if (token === '-L') {
      filters.push({ type: 'lua', path: value() });
    } else if (token === '-V') {
      const v = value();
      const at = v.search(/[:=]/);
      const key = at < 0 ? v : v.slice(0, at);
      const val = at < 0 ? 'true' : v.slice(at + 1);
      const prev = variables[key];
      variables[key] = prev === undefined ? val : Array.isArray(prev) ? [...prev, val] : [prev, val];
    } else if (token === '--resource-path') {
      resourcePath = [...splitSearchPath(value()), ...resourcePath];
    } else if (token === '--defaults') {
      const path = value();
      for (const [k, v] of Object.entries(readDefaults(path, files))) {
        if (k in o || k === 'defaults') throw new UnsupportedArgv(`the defaults key \`${k}\` collides with an option`, path);
        o[k] = v;
      }
    } else if (token.startsWith('-') && token !== '-') {
      throw new UnsupportedArgv('an unknown flag', token);
    } else inputs.push(token);
  }
  if (filters.length) o.filters = filters;
  if (Object.keys(variables).length) o.variables = variables;
  if (resourcePath.length) o['resource-path'] = resourcePath;
  if (inputs.length) o['input-files'] = inputs;
  return o;
}
