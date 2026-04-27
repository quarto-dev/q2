// Intentionally empty.
//
// Upstream tree-sitter-language ships its own snprintf / vsnprintf /
// fclose / fdopen / fputc / fputs / fwrite / fprintf stubs here. In our
// build, those symbols are provided by wasm-quarto-hub-client's
// c_shim.rs, which is a superset of upstream's behavior plus the
// format-specifier coverage our Lua runtime needs. We neutralize the
// upstream file by compiling this empty one instead.
//
// See ../../../../claude-notes/plans/2026-04-20-wasm-shim-merge.md
