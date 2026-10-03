import type { RequestFile, ShareTree } from './types.ts';

/** The share-tree entry (relative to `request.share_tree_path`) that pandoc runs in every Lua state, before Quarto's own init code. */
export const INIT_LUA = 'pandoc/datadir/init.lua';

const enc = new TextEncoder();

/** A Lua string literal for `s`: UTF-8 bytes, printable ASCII verbatim, everything else (and `"` and `\`) as a 3-digit decimal escape. */
function luaString(s: string): string {
  let out = '"';
  for (const b of enc.encode(s)) out += b >= 0x20 && b < 0x7f && b !== 0x22 && b !== 0x5c ? String.fromCharCode(b) : `\\${String(b).padStart(3, '0')}`;
  return `${out}"`;
}

/**
 * The Lua line that makes `os.getenv` answer from `env` and return `nil` for every other key, as a warm instance
 * needs: the WASI environment of a warm instance is read once, at instantiation, so a per-request environment can only
 * reach Lua this way. One line with no trailing newline and no top-level `local` (it is wrapped in `do ... end`), so
 * that putting it in front of a file shifts no line number.
 */
export function buildEnvPreamble(env: Readonly<Record<string, string>>): string {
  const entries = Object.entries(env)
    .map(([k, v]) => `[${luaString(k)}]=${luaString(v)}`)
    .join(',');
  return `do local e={${entries}} os.getenv=function(k) return e[k] end end`;
}

const concat = (a: Uint8Array, b: Uint8Array): Uint8Array => {
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
};

/**
 * A share tree whose `pandoc/datadir/init.lua` starts with the env preamble, joined to the first original line with a
 * space (so `init.lua:N` in errors and tracebacks is unchanged). The bytes are concatenated, never decoded and
 * re-encoded, so a BOM or invalid UTF-8 in the original survive. The input tree and its entry are not mutated and
 * `share_tree_version` stays that of the original: validation and the version hash see the original tree.
 */
export function withPreamble(shareTree: ShareTree, env: Readonly<Record<string, string>>): ShareTree {
  const i = shareTree.files.findIndex((f) => f.path === INIT_LUA);
  if (i < 0) throw new Error(`the share tree has no ${INIT_LUA}, so the environment cannot reach Lua`);
  const original = shareTree.files[i];
  const files: RequestFile[] = shareTree.files.slice();
  files[i] = { path: original.path, bytes: concat(enc.encode(`${buildEnvPreamble(env)} `), original.bytes) };
  return { ...shareTree, files };
}
