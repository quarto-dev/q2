/**
 * @quarto/api/jupyter — assets
 *
 * Computes (and creates) the figure/supporting directories for a Jupyter
 * notebook render. **Host-dependent** (P3-2): unlike most of this
 * namespace's pure path computation, `assets` performs FS I/O
 * (`host.fs.ensureDir` + `host.fs.walk`) so that the `figures_dir` it
 * returns is guaranteed to exist before `to-markdown.ts` (Task 8) writes
 * figures into it.
 *
 * Ported (REWRITE, not extract) from:
 *   external-sources/quarto-cli/src/core/jupyter/jupyter.ts:665-696 (`jupyterAssets`)
 *   external-sources/quarto-cli/src/core/render.ts:13-16 (`inputFilesDir`)
 *   external-sources/quarto-cli/src/core/render.ts:20-26 (`figuresDir`)
 *
 * Q1-FAITHFUL CWD COUPLING (do NOT "fix"): the returned `figures_dir` (and
 * `files_dir`/`supporting_dir`) are RELATIVE + forward-slashed
 * (`pathWithForwardSlashes(relative(base_dir, figures_dir))`,
 * `jupyter.ts:690-694`), while `host.fs.ensureDir` is called on the ABSOLUTE
 * path. `to-markdown.ts`'s figure-write step re-joins `assets.base_dir` +
 * `assets.figures_dir` to land in the same directory this function created —
 * the write only lands correctly when the two directories agree, which they
 * do because both are built from the same `base_dir`/`files_dir` values.
 * This mirrors Q1 exactly; porting it "correctly" (e.g. returning the
 * absolute figures_dir) would break that agreement.
 */

import type { JupyterNotebookAssetPaths } from "@quarto/types";
import type { PlatformHost } from "../platform/index.js";
import { dirAndStem, inputFilesDir, toForwardSlashes } from "../path/index.js";

/**
 * rmarkdown-derived figures dir.
 * Mirrors Q1 `core/render.ts:20-26` (`figuresDir`):
 *   - `html4` normalizes to `html`
 *   - any `+…`/`-…` suffix on `to` is stripped
 *   - defaults to `html` when `to` is undefined
 */
function figuresDir(to?: string): string {
  let pandocTo = to;
  if (pandocTo === "html4") {
    pandocTo = "html";
  }
  pandocTo = (pandocTo || "html").replace(/[+-].*$/, "");
  return "figure-" + pandocTo;
}

/** Join a base dir and a relative segment without `node:path`. */
function joinPath(base: string, rel: string): string {
  if (!base) {
    return rel;
  }
  return base.endsWith("/") ? base + rel : base + "/" + rel;
}

/**
 * Return `target` relative to `base`, assuming `target` was built by
 * joining segments onto `base` (i.e. `base` is always a prefix of
 * `target`). No `node:path` — plain string-prefix stripping is sufficient
 * for the paths this module constructs.
 */
function relativeTo(base: string, target: string): string {
  if (target === base) {
    return ".";
  }
  const prefix = base.endsWith("/") ? base : base + "/";
  if (target.startsWith(prefix)) {
    return target.slice(prefix.length);
  }
  return target;
}

/**
 * Compute (and create) the asset directories for a Jupyter notebook render.
 * Mirrors Q1 `jupyterAssets` (`core/jupyter/jupyter.ts:665-696`).
 *
 * @param host  - Minimal fs-only host slice (`ensureDir` + `walk`).
 * @param input - Absolute input file path.
 * @param to    - Pandoc `to` format (optional; defaults to `html`).
 */
export function assets(
  host: Pick<PlatformHost, "fs">,
  input: string,
  to?: string,
): JupyterNotebookAssetPaths {
  const [base_dir] = dirAndStem(input);

  const files_dir = joinPath(base_dir, inputFilesDir(input));
  const figures_dir = joinPath(files_dir, figuresDir(to));

  // Create the figures dir eagerly — this is the dir the figure-write step
  // (Task 8) writes into. Runs on the ABSOLUTE path (see module doc above).
  host.fs.ensureDir(figures_dir);

  // Determine supporting_dir: if there are no other subdirs under files_dir
  // besides figures_dir, supporting_dir === files_dir; otherwise it's just
  // the figures_dir. (Q1's walkSync check, jupyter.ts:680-687.)
  let supporting_dir = files_dir;
  for (const entry of host.fs.walk(files_dir, { maxDepth: 1 })) {
    if (entry.path !== files_dir && entry.path !== figures_dir) {
      supporting_dir = figures_dir;
      break;
    }
  }

  return {
    base_dir,
    files_dir: toForwardSlashes(relativeTo(base_dir, files_dir)),
    figures_dir: toForwardSlashes(relativeTo(base_dir, figures_dir)),
    supporting_dir: toForwardSlashes(relativeTo(base_dir, supporting_dir)),
  };
}
