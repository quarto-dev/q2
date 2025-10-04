use quarto_markdown_pandoc::readers::json;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_read_all_json_files_in_tests_readers() {
    let test_dir = PathBuf::from("tests/readers");

    if !test_dir.exists() {
        eprintln!("Warning: tests/readers directory does not exist, skipping test");
        return;
    }

    let mut json_files = Vec::new();
    collect_json_files(&test_dir, &mut json_files);

    if json_files.is_empty() {
        eprintln!("Warning: No JSON files found in tests/readers directory");
        return;
    }

    for json_file in json_files {
        println!("Testing JSON reader with: {}", json_file.display());

        let mut file = fs::File::open(&json_file)
            .expect(&format!("Failed to open file: {}", json_file.display()));

        match json::read(&mut file) {
            Ok(pandoc) => {
                println!("  ✓ Successfully read {}", json_file.display());
                // Basic validation - ensure we got some content
                assert!(
                    !pandoc.blocks.is_empty() || !pandoc.meta.is_empty(),
                    "File {} produced empty document",
                    json_file.display()
                );
            }
            Err(e) => {
                panic!("Failed to read JSON file {}: {}", json_file.display(), e);
            }
        }
    }
}

fn collect_json_files(dir: &PathBuf, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_dir() {
                    collect_json_files(&path, files);
                } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    files.push(path);
                }
            }
        }
    }
}

#[test]
fn test_manybullets_json_specifically() {
    let json_file = PathBuf::from("tests/readers/json/manybullets.json");

    if !json_file.exists() {
        eprintln!("Warning: manybullets.json not found, skipping test");
        return;
    }

    let mut file = fs::File::open(&json_file).expect("Failed to open manybullets.json");

    let pandoc = json::read(&mut file).expect("Failed to read manybullets.json");

    // Verify the content matches what we expect
    assert_eq!(pandoc.blocks.len(), 1, "Should have exactly one block");

    match &pandoc.blocks[0] {
        quarto_markdown_pandoc::pandoc::Block::OrderedList(list) => {
            assert_eq!(list.content.len(), 12, "Should have 12 list items");
            assert_eq!(list.attr.0, 1, "List should start at 1");
        }
        _ => panic!("Expected OrderedList block"),
    }
}
