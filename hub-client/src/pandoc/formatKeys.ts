/**
 * The hub wasm's key for "typst compiled to a PDF" (menu row, download and preview artifact). It is not a
 * document's `format:` value: `format: pdf` means LaTeX, which the browser can't run.
 */
export const TYPST_PDF_KEY = 'typst-pdf' as const;
