/**
 * VFS path for the compiled theme CSS artifact. Mirrors
 * `DEFAULT_CSS_ARTIFACT_PATH` in `crates/quarto-core/src/pipeline.rs:85`.
 *
 * Consumer: parent-side `Q2PreviewIframe`, which reads the VFS bytes,
 * mints a blob URL, and posts the URL via `UPDATE_THEME`. The iframe
 * entry never imports this constant — it only handles whatever CSS
 * URL the parent posts.
 *
 * Sync convention: when the Rust constant changes, update this file
 * and re-run hub-client tests. Matches the `types/diagnostic.ts` ↔
 * `DiagnosticMessage` pattern.
 */
export const DEFAULT_CSS_ARTIFACT_PATH = '/.quarto/project-artifacts/styles.css';
