// Most types/utils are consumed via sub-path imports
// (`@quarto/preview-renderer/types/<module>`,
// `@quarto/preview-renderer/utils/<module>`), declared in
// `package.json`'s `exports` map. This keeps the API surface
// stable as files are added, and avoids name collisions between
// modules that legitimately export the same identifier (the
// deprecated `Diagnostic` in `types/diagnostic` vs the LSP-style
// `Diagnostic` in `types/intelligence`).
//
// Top-level barrel exports are added here as Phases 3–4 move
// components whose public-API names don't collide.
export {};
