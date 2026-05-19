/**
 * Discovery of user tree-sitter grammars under a project's
 * `_quarto/grammars/<name>/` directories — Phase 4.5 of
 * `claude-notes/plans/2026-04-21-syntax-highlighting-phase-4.md`.
 *
 * Mirrors the native rule at
 * `crates/quarto-highlight/src/user_grammar.rs:load_all_from_parent`:
 *
 * - The scan happens one level deep: `_quarto/grammars/<name>/…`.
 * - A subdirectory qualifies iff it contains **exactly one** `*.wasm`
 *   file and a `highlights.scm`. Extra files (`injections.scm`,
 *   `locals.scm`, `PROVENANCE.md`, README, etc.) are tolerated.
 * - Multiple `*.wasm` files in the same subdirectory disqualify the
 *   grammar (ambiguous).
 * - The registered class name is the `.wasm` file's stem, not the
 *   directory name, so `my-grammar/toml.wasm` registers as `toml`
 *   (match native).
 *
 * The scan is pure: it takes a list of file paths and returns the
 * descriptors it would load. Byte-loading happens elsewhere (the
 * cache layer consumes this output and fetches binary content on
 * demand).
 */

const GRAMMARS_PREFIX = '_quarto/grammars/';
const HIGHLIGHTS_FILE = 'highlights.scm';

export interface GrammarDescriptor {
  /** Registered language class — the `.wasm` file's stem. */
  readonly class: string;
  /** Project-relative path to the grammar `.wasm` file. */
  readonly wasmPath: string;
  /** Project-relative path to the `highlights.scm`. */
  readonly highlightsPath: string;
}

/**
 * Walk `paths` and return one [`GrammarDescriptor`] per valid
 * `_quarto/grammars/<name>/` subdirectory. Output is sorted by class
 * name for deterministic ordering.
 *
 * `paths` must use project-relative form with no leading slash (the
 * convention established by `validateProjectPath` and reflected in
 * `FileEntry.path`). Paths with a leading slash are ignored — they
 * indicate a caller passing WASM-side `/project/…` paths by mistake,
 * which should be caught explicitly rather than silently coerced.
 */
export function discoverUserGrammars(paths: readonly string[]): GrammarDescriptor[] {
  // Group the children of each `<name>/` subdirectory.
  type DirContents = { wasms: string[]; hasHighlights: boolean };
  const dirs = new Map<string, DirContents>();

  for (const path of paths) {
    if (path.startsWith('/')) continue;
    if (!path.startsWith(GRAMMARS_PREFIX)) continue;
    const rest = path.slice(GRAMMARS_PREFIX.length);
    const firstSlash = rest.indexOf('/');
    if (firstSlash < 0) continue; // top-level file under _quarto/grammars/, not in a subdir
    const subdir = rest.slice(0, firstSlash);
    if (subdir.length === 0) continue;
    const childPath = rest.slice(firstSlash + 1);
    // Only one level deep: if there's another `/` inside `childPath`,
    // the file is in a nested subdirectory (e.g.
    // `_quarto/grammars/foo/bar/baz.wasm`) — not a grammar.
    if (childPath.includes('/')) continue;

    let entry = dirs.get(subdir);
    if (!entry) {
      entry = { wasms: [], hasHighlights: false };
      dirs.set(subdir, entry);
    }
    if (childPath === HIGHLIGHTS_FILE) {
      entry.hasHighlights = true;
    } else if (childPath.endsWith('.wasm')) {
      entry.wasms.push(path);
    }
  }

  const grammars: GrammarDescriptor[] = [];
  for (const [subdir, entry] of dirs) {
    if (!entry.hasHighlights) continue;
    if (entry.wasms.length !== 1) continue;
    const wasmPath = entry.wasms[0];
    const stem = wasmFileStem(wasmPath);
    if (!stem) continue;
    grammars.push({
      class: stem,
      wasmPath,
      highlightsPath: `${GRAMMARS_PREFIX}${subdir}/${HIGHLIGHTS_FILE}`,
    });
  }

  grammars.sort((a, b) => a.class.localeCompare(b.class));
  return grammars;
}

/**
 * Extract the `.wasm` file's stem — i.e. `toml` for
 * `_quarto/grammars/my-grammar/toml.wasm`. Returns `null` for paths
 * that don't have a `.wasm` extension or have an empty stem.
 */
function wasmFileStem(path: string): string | null {
  const lastSlash = path.lastIndexOf('/');
  const filename = lastSlash < 0 ? path : path.slice(lastSlash + 1);
  if (!filename.endsWith('.wasm')) return null;
  const stem = filename.slice(0, -'.wasm'.length);
  return stem.length > 0 ? stem : null;
}
