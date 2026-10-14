/*
 * build.rs
 * Copyright (c) 2026 Posit, PBC
 */

fn main() {
    let grammar_dir = std::path::Path::new("grammar").join("src");

    let mut c_config = cc::Build::new();
    c_config.std("c11").include(&grammar_dir);

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    for name in ["parser.c", "scanner.c"] {
        let path = grammar_dir.join(name);
        c_config.file(&path);
        println!("cargo:rerun-if-changed={}", path.to_str().unwrap());
    }

    c_config.compile("tree-sitter-lmd");
}
