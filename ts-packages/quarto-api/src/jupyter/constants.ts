/**
 * @quarto/api/jupyter — constants
 *
 * Pure data: MIME-type constants used by the Jupyter display-data / markdown
 * emission machinery, plus the language → comment-char table used when
 * round-tripping cell options as comments.
 *
 * Ported from (values only — no imports; see External Sources Policy):
 *   - external-sources/quarto-cli/src/core/mime.ts
 *   - external-sources/quarto-cli/src/core/jupyter/jupyter.ts:187 (kQuartoMimeType)
 *   - external-sources/quarto-cli/src/core/lib/partition-cell-options.ts
 *     (~line 310) — the CANONICAL `kLangCommentChars` table. Do NOT use the
 *     stale, non-exported duplicate in jupyter.ts (~line 1208); the two
 *     tables have diverged (see constants.test.ts for a discriminating case).
 */

/** Key Quarto injects into a widget's JSON payload to record its real MIME type. */
export const kQuartoMimeType = "quarto_mimetype";

// ─── MIME type constants ───────────────────────────────────────────────────

export const kTextMarkdown = "text/markdown";
export const kTextHtml = "text/html";
export const kTextPlain = "text/plain";
export const kTextLatex = "text/latex";
export const kImageSvg = "image/svg+xml";
export const kImagePng = "image/png";
export const kImageJpeg = "image/jpeg";
export const kApplicationPdf = "application/pdf";
export const kApplicationJavascript = "application/javascript";
export const kApplicationJupyterWidgetState =
  "application/vnd.jupyter.widget-state+json";
export const kApplicationJupyterWidgetView =
  "application/vnd.jupyter.widget-view+json";

// ─── Language comment-char table ───────────────────────────────────────────

/**
 * Maps a Jupyter kernel/cell language to the comment marker(s) used when
 * writing cell options back into source as comments. Most languages use a
 * single line-comment prefix (`"#"`, `"//"`, ...); some require a
 * block-comment `[open, close]` pair (e.g. `c`, `css`, `ocaml`).
 */
export const kLangCommentChars: Record<string, string | [string, string]> = {
  r: "#",
  python: "#",
  julia: "#",
  scala: "//",
  matlab: "%",
  csharp: "//",
  fsharp: "//",
  c: ["/*", "*/"],
  css: ["/*", "*/"],
  sas: ["*", ";"],
  powershell: "#",
  bash: "#",
  sql: "--",
  mysql: "--",
  psql: "--",
  lua: "--",
  cpp: "//",
  cc: "//",
  stan: "#",
  octave: "#",
  fortran: "!",
  fortran95: "!",
  awk: "#",
  gawk: "#",
  stata: "*",
  java: "//",
  groovy: "//",
  kotlin: "//",
  sed: "#",
  perl: "#",
  prql: "#",
  ruby: "#",
  tikz: "%",
  js: "//",
  d3: "//",
  node: "//",
  sass: "//",
  scss: "//",
  coffee: "#",
  go: "//",
  asy: "//",
  haskell: "--",
  dot: "//",
  ojs: "//",
  apl: "⍝",
  ocaml: ["(*", "*)"],
  q: "/",
  rust: "//",
};
