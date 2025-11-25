/*
 * build.rs
 * Copyright (c) 2025 Posit, PBC
 */

fn main() {
    let grammar_dir = std::path::Path::new("grammar").join("src");

    let mut c_config = cc::Build::new();
    c_config.std("c11").include(&grammar_dir);

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    let parser_path = grammar_dir.join("parser.c");
    c_config.file(&parser_path);
    println!("cargo:rerun-if-changed={}", parser_path.to_str().unwrap());

    // Include external scanner if present
    let scanner_path = grammar_dir.join("scanner.c");
    if scanner_path.exists() {
        c_config.file(&scanner_path);
        println!("cargo:rerun-if-changed={}", scanner_path.to_str().unwrap());
    }

    c_config.compile("tree-sitter-doctemplate");
}
