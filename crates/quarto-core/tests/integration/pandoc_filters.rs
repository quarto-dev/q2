use std::path::Path;

/// T1.1: All import() lines in embedded filters/main.lua resolve
#[test]
fn test_embedded_filters_closure_is_complete() {
    use quarto_core::pandoc_filters::FILTERS_DIR;

    let main_lua = FILTERS_DIR
        .get_file("main.lua")
        .expect("main.lua not found in FILTERS_DIR");
    let content = std::str::from_utf8(main_lua.contents()).expect("main.lua is not valid UTF-8");

    // Parse all import("./X") lines
    let mut imports = Vec::new();
    let mut unresolved = Vec::new();

    for line in content.lines() {
        if let Some(path_str) = line
            .trim_start()
            .strip_prefix("import(")
            .and_then(|s| s.strip_prefix("\"./"))
            .and_then(|s| s.strip_suffix("\")"))
        {
            imports.push(path_str.to_string());

            // Check if this path exists in the embedded directory
            if FILTERS_DIR.get_file(path_str).is_none() {
                unresolved.push(path_str.to_string());
            }
        }
    }

    // T1.1 revert hunk: delete or comment out the include_dir! for filters
    // That would make imports.len() == 0 and this assertion RED.
    //
    // 171, not the original 170: Task 8 added one import line
    // (`import("./quarto2-shim.lua")`) — a deliberate edit, per this test's
    // own vacuity note that the literal count must move with real changes.
    assert_eq!(
        imports.len(),
        171,
        "Expected 171 import lines, got {}",
        imports.len()
    );
    assert_eq!(
        unresolved,
        Vec::<String>::new(),
        "Unresolved imports: {:?}",
        unresolved
    );
}

/// T1.2: All require'd modules in embedded pandoc/datadir/init.lua resolve
#[test]
fn test_embedded_datadir_requires_resolve() {
    use quarto_core::pandoc_filters::DATADIR_DIR;

    let init_lua = DATADIR_DIR
        .get_file("init.lua")
        .expect("init.lua not found in DATADIR_DIR");
    let _content = std::str::from_utf8(init_lua.contents()).expect("init.lua is not valid UTF-8");

    // The modules required in init.lua:149-154 are:
    // _format, _base64, _json, _utils, logging
    let required_modules = vec![
        "_format.lua",
        "_base64.lua",
        "_json.lua",
        "_utils.lua",
        "logging.lua",
    ];

    for module in required_modules {
        assert!(
            DATADIR_DIR.get_file(module).is_some(),
            "Required module {} not found in DATADIR_DIR",
            module
        );
    }

    // T1.2 revert hunk: delete resources/pandoc-filters/pandoc/datadir/_format.lua
    // That would make the _format.lua assertion RED
    assert!(DATADIR_DIR.get_file("_format.lua").is_some());
}

/// T1.3: README records the pins correctly in ## Source section
#[test]
fn test_readme_records_the_pins() {
    use quarto_core::pandoc_filters::{PANDOC_PIN, QUARTO_CLI_PIN};

    let readme_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("resources/pandoc-filters/README.md");

    let content = std::fs::read_to_string(&readme_path)
        .unwrap_or_else(|_| panic!("Could not read README at {:?}", readme_path));

    // Parse the ## Source section to extract the recorded pins
    let mut readme_tag = None;
    let mut readme_pandoc = None;

    for line in content.lines() {
        if line.contains("quarto-cli") && line.contains("v1.11.3") {
            readme_tag = Some("v1.11.3");
        }
        if line.contains("pandoc") && line.contains("3.10") {
            readme_pandoc = Some("3.10");
        }
    }

    let readme_tag = readme_tag.expect("README does not record quarto-cli tag v1.11.3");
    let readme_pandoc = readme_pandoc.expect("README does not record pandoc version 3.10");

    // T1.3 revert hunk: change QUARTO_CLI_PIN to something else or remove tag line from README
    // That would make this assertion RED
    assert_eq!(readme_tag, QUARTO_CLI_PIN);
    assert_eq!(readme_pandoc, PANDOC_PIN);
}

/// Returns the line index (0-based) of the first line in `content` whose
/// trimmed text equals `needle` exactly, or the first line *containing*
/// `needle` if no exact match exists — used below to find both a bare
/// `tappend(...)` statement and an `import(...)` line by their full text.
fn find_line_index(content: &str, needle: &str) -> Option<usize> {
    content
        .lines()
        .position(|line| line.trim() == needle)
        .or_else(|| content.lines().position(|line| line.contains(needle)))
}

/// T8.1: the shim's `tappend` splice sits strictly between
/// `quarto_init_filters` and `quarto_normalize_filters` in the compiled-in
/// `main.lua`, and all three anchors are found exactly once.
///
/// Revert hunk: moving the splice line to after
/// `tappend(quarto_filter_list, quarto_normalize_filters)` makes
/// `init_idx < shim_idx && shim_idx < normalize_idx` RED. Presence alone
/// does not discriminate this — see the plan's Task 8 vacuity-check note.
#[test]
fn test_shim_splice_position() {
    use quarto_core::pandoc_filters::FILTERS_DIR;

    let main_lua = FILTERS_DIR
        .get_file("main.lua")
        .expect("main.lua not found in FILTERS_DIR");
    let content = std::str::from_utf8(main_lua.contents()).expect("main.lua is not valid UTF-8");

    let init_line = "tappend(quarto_filter_list, quarto_init_filters)";
    let shim_line = "tappend(quarto_filter_list, quarto_pandoc_shim_filters)";
    let normalize_line = "tappend(quarto_filter_list, quarto_normalize_filters)";

    let init_count = content.lines().filter(|l| l.trim() == init_line).count();
    let shim_count = content.lines().filter(|l| l.trim() == shim_line).count();
    let normalize_count = content
        .lines()
        .filter(|l| l.trim() == normalize_line)
        .count();
    assert_eq!(init_count, 1, "expected exactly one {init_line:?}");
    assert_eq!(shim_count, 1, "expected exactly one {shim_line:?}");
    assert_eq!(
        normalize_count, 1,
        "expected exactly one {normalize_line:?}"
    );

    let init_idx = find_line_index(content, init_line).unwrap();
    let shim_idx = find_line_index(content, shim_line).unwrap();
    let normalize_idx = find_line_index(content, normalize_line).unwrap();

    assert!(
        init_idx < shim_idx && shim_idx < normalize_idx,
        "expected init ({init_idx}) < shim ({shim_idx}) < normalize ({normalize_idx})"
    );
}

/// T8.2: the shim's `import` line sits after `ast/customnodes.lua`'s import
/// and before the last `import(` line in the file.
///
/// Revert hunk: moving `import("./quarto2-shim.lua")` to the top of the
/// import block (before `ast/customnodes.lua`) makes
/// `shim_import_idx > customnodes_import_idx` RED.
#[test]
fn test_shim_import_order() {
    use quarto_core::pandoc_filters::FILTERS_DIR;

    let main_lua = FILTERS_DIR
        .get_file("main.lua")
        .expect("main.lua not found in FILTERS_DIR");
    let content = std::str::from_utf8(main_lua.contents()).expect("main.lua is not valid UTF-8");

    let customnodes_import_idx = find_line_index(content, "import(\"./ast/customnodes.lua\")")
        .expect("ast/customnodes.lua import not found");
    let shim_import_idx = find_line_index(content, "import(\"./quarto2-shim.lua\")")
        .expect("quarto2-shim.lua import not found");
    let last_import_idx = content
        .lines()
        .enumerate()
        .filter(|(_, l)| l.trim_start().starts_with("import("))
        .map(|(idx, _)| idx)
        .last()
        .expect("at least one import( line expected");

    assert!(
        shim_import_idx > customnodes_import_idx,
        "expected shim import ({shim_import_idx}) after customnodes import ({customnodes_import_idx})"
    );
    assert!(
        shim_import_idx <= last_import_idx,
        "expected shim import ({shim_import_idx}) at or before the last import line ({last_import_idx})"
    );
}

/// T8.3 (positive half) + T8.5: the embedded placeholder shim defines
/// `quarto_pandoc_shim_filters` with at least one entry carrying `name` and
/// `filter`, contains no `traverse = 'topdown'`, and both patched regions
/// in `main.lua` carry the `QUARTO2-PATCH` marker naming both group
/// boundaries.
///
/// Revert hunks: adding `traverse = 'topdown'` to the shim group makes the
/// negative assertion RED; removing the marker comment from either patched
/// region in `main.lua` makes the marker-naming assertions RED.
#[test]
fn test_shim_group_shape_and_patch_markers() {
    use quarto_core::pandoc_filters::FILTERS_DIR;

    let shim_file = FILTERS_DIR
        .get_file("quarto2-shim.lua")
        .expect("quarto2-shim.lua not found in FILTERS_DIR");
    let shim_src =
        std::str::from_utf8(shim_file.contents()).expect("quarto2-shim.lua is not valid UTF-8");

    assert!(shim_src.contains("quarto_pandoc_shim_filters = {"));
    assert!(shim_src.contains("name = "));
    assert!(shim_src.contains("filter = "));
    assert!(
        !shim_src.contains("traverse = 'topdown'"),
        "shim group must stay bottom-up (no traverse = 'topdown')"
    );

    let main_lua = FILTERS_DIR
        .get_file("main.lua")
        .expect("main.lua not found in FILTERS_DIR");
    let content = std::str::from_utf8(main_lua.contents()).expect("main.lua is not valid UTF-8");

    let marker_count = content.matches("QUARTO2-PATCH").count();
    assert_eq!(
        marker_count, 2,
        "expected exactly two QUARTO2-PATCH markers (import region + tappend region), got {marker_count}"
    );
    // Each marker's surrounding comment block must name both group
    // boundaries, not a line number -- group *contents* have been
    // refactored twice in two years while the top-level group *order* has
    // been stable since 2023.
    let lines: Vec<&str> = content.lines().collect();
    let marker_line_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("QUARTO2-PATCH"))
        .map(|(idx, _)| idx)
        .collect();
    assert_eq!(marker_line_indices.len(), 2);

    for &marker_idx in &marker_line_indices {
        // Collect this comment line and every subsequent comment line
        // (stopping at the first non-comment line, e.g. the actual
        // `import(...)`/`tappend(...)` statement the marker documents).
        let comment_block: String = lines[marker_idx..]
            .iter()
            .take_while(|l| l.trim_start().starts_with("--"))
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            comment_block.contains("quarto_init_filters")
                && comment_block.contains("quarto_normalize_filters"),
            "QUARTO2-PATCH comment block at line {marker_idx} does not name both group \
             boundaries:\n{comment_block}"
        );
    }
}
