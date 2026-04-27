// Same shape as upstream tree-sitter-language's build.rs: publish
// wasm-headers and wasm-src metadata so downstream grammar crates
// (tree-sitter-lua, tree-sitter-css) that read
// DEP_TREE_SITTER_LANGUAGE_WASM_HEADERS / _WASM_SRC in their own
// build.rs will find our paths instead of upstream's.
//
// Our wasm/include/ mirrors upstream exactly. Our wasm/src/*.c files
// are intentionally empty — c_shim.rs in wasm-quarto-hub-client
// provides the stdio/stdlib/string symbols for the whole binary.

fn main() {
    if std::env::var("TARGET")
        .unwrap_or_default()
        .starts_with("wasm32-unknown")
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let wasm_headers = std::path::Path::new(&manifest_dir).join("wasm/include");
        let wasm_src = std::path::Path::new(&manifest_dir).join("wasm/src");

        println!("cargo::metadata=wasm-headers={}", wasm_headers.display());
        println!("cargo::metadata=wasm-src={}", wasm_src.display());
    }
}
