/*
 * test_wasm_entrypoints.rs
 * Copyright (c) 2025 Posit, PBC
 */

#[test]
fn test_wasm_read_entrypoint() {
    let input = "# hello _world_.\n";
    let result = quarto_markdown_pandoc::wasm_entry_points::parse_qmd(input.as_bytes(), true);
    eprintln!("result: {}", result);
}
