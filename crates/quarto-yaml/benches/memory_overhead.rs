//! Memory overhead benchmark for YamlWithSourceInfo vs raw Yaml
//!
//! This benchmark measures the actual memory overhead of our owned data approach
//! compared to using yaml-rust2::Yaml directly.
//!
//! Run with: cargo bench --bench memory_overhead

use quarto_yaml::parse;
use std::mem;
use yaml_rust2::YamlLoader;

/// Calculate approximate memory usage of a Yaml tree
fn estimate_yaml_memory(yaml: &yaml_rust2::Yaml) -> usize {
    let mut size = mem::size_of::<yaml_rust2::Yaml>();

    match yaml {
        yaml_rust2::Yaml::Real(s) | yaml_rust2::Yaml::String(s) => {
            size += s.capacity();
        }
        yaml_rust2::Yaml::Array(arr) => {
            size += arr.capacity() * mem::size_of::<yaml_rust2::Yaml>();
            for item in arr {
                size += estimate_yaml_memory(item);
            }
        }
        yaml_rust2::Yaml::Hash(hash) => {
            // HashMap overhead is complex, approximate
            size += hash.capacity() * (mem::size_of::<yaml_rust2::Yaml>() * 2);
            for (k, v) in hash {
                size += estimate_yaml_memory(k);
                size += estimate_yaml_memory(v);
            }
        }
        _ => {}
    }

    size
}

/// Calculate approximate memory usage of a YamlWithSourceInfo tree
fn estimate_yaml_with_source_memory(yaml: &quarto_yaml::YamlWithSourceInfo) -> usize {
    let mut size = mem::size_of::<quarto_yaml::YamlWithSourceInfo>();

    // Add the underlying Yaml
    size += estimate_yaml_memory(&yaml.yaml);

    // Add SourceInfo
    // Note: SourceInfo size is already included in sizeof(YamlWithSourceInfo)
    // For basic parsing, SourceInfo uses Original variant with FileId (just a usize)

    // Add children
    if let Some(children) = yaml.as_array() {
        // Note: using len() not capacity() since we only have a slice
        size += children.len() * mem::size_of::<quarto_yaml::YamlWithSourceInfo>();
        for child in children {
            size += estimate_yaml_with_source_memory(child);
        }
    } else if let Some(entries) = yaml.as_hash() {
        // Note: using len() not capacity() since we only have a slice
        size += entries.len() * mem::size_of::<quarto_yaml::YamlHashEntry>();
        for entry in entries {
            size += estimate_yaml_with_source_memory(&entry.key);
            size += estimate_yaml_with_source_memory(&entry.value);
            // Add the 3 SourceInfo structs in YamlHashEntry
            size += mem::size_of::<quarto_yaml::SourceInfo>() * 3;
        }
    }

    size
}

/// Test case with name, YAML content, and description
struct TestCase {
    name: &'static str,
    yaml: &'static str,
    description: &'static str,
}

const TEST_CASES: &[TestCase] = &[
    TestCase {
        name: "simple_scalar",
        yaml: "hello world",
        description: "Single scalar value",
    },
    TestCase {
        name: "small_hash",
        yaml: r#"
title: My Document
author: John Doe
date: 2024-01-01
"#,
        description: "Small hash with 3 string values",
    },
    TestCase {
        name: "small_array",
        yaml: r#"
- item1
- item2
- item3
- item4
- item5
"#,
        description: "Small array with 5 items",
    },
    TestCase {
        name: "nested_structure",
        yaml: r#"
project:
  title: My Project
  version: 1.0.0
  authors:
    - name: Alice
      email: alice@example.com
    - name: Bob
      email: bob@example.com
  config:
    port: 8080
    debug: true
    features:
      - feature1
      - feature2
      - feature3
"#,
        description: "Nested structure with arrays and hashes",
    },
    TestCase {
        name: "quarto_document",
        yaml: r#"
title: "My Research Paper"
author: "Jane Smith"
date: "2024-01-01"
format:
  html:
    theme: cosmo
    toc: true
    toc-depth: 3
    code-fold: true
  pdf:
    documentclass: article
    margin-left: 1in
    margin-right: 1in
execute:
  echo: true
  warning: false
  error: false
bibliography: references.bib
csl: apa.csl
"#,
        description: "Typical Quarto document metadata",
    },
    TestCase {
        name: "quarto_project",
        yaml: r#"
project:
  type: website
  output-dir: _site

website:
  title: "My Website"
  navbar:
    left:
      - text: "Home"
        href: index.qmd
      - text: "About"
        href: about.qmd
      - text: "Blog"
        href: blog/index.qmd
    right:
      - icon: github
        href: https://github.com/user/repo

format:
  html:
    theme:
      light: flatly
      dark: darkly
    css: styles.css
    toc: true

execute:
  freeze: auto
"#,
        description: "Quarto project configuration",
    },
];

fn main() {
    println!("Memory Overhead Analysis: YamlWithSourceInfo vs raw Yaml");
    println!("==========================================================\n");

    println!("Size of base types:");
    println!("  yaml_rust2::Yaml:           {} bytes", mem::size_of::<yaml_rust2::Yaml>());
    println!("  YamlWithSourceInfo:         {} bytes", mem::size_of::<quarto_yaml::YamlWithSourceInfo>());
    println!("  SourceInfo:                 {} bytes", mem::size_of::<quarto_yaml::SourceInfo>());
    println!("  YamlHashEntry:              {} bytes", mem::size_of::<quarto_yaml::YamlHashEntry>());
    println!();

    let mut total_raw = 0usize;
    let mut total_tracked = 0usize;

    for test in TEST_CASES {
        println!("Test: {} - {}", test.name, test.description);
        println!("{}", "-".repeat(60));

        // Parse with yaml-rust2
        let raw_docs = YamlLoader::load_from_str(test.yaml)
            .expect("Failed to parse YAML");
        let raw_yaml = &raw_docs[0];
        let raw_size = estimate_yaml_memory(raw_yaml);

        // Parse with YamlWithSourceInfo
        let tracked_yaml = parse(test.yaml)
            .expect("Failed to parse YAML with source tracking");
        let tracked_size = estimate_yaml_with_source_memory(&tracked_yaml);

        let overhead = tracked_size as f64 / raw_size as f64;
        let diff = tracked_size - raw_size;

        println!("  Raw Yaml size:              {:>8} bytes", raw_size);
        println!("  YamlWithSourceInfo size:    {:>8} bytes", tracked_size);
        println!("  Overhead:                   {:>8} bytes ({:.2}x)", diff, overhead);
        println!();

        total_raw += raw_size;
        total_tracked += tracked_size;
    }

    println!("==========================================================");
    println!("TOTALS across all test cases:");
    println!("  Total raw:                  {:>8} bytes", total_raw);
    println!("  Total tracked:              {:>8} bytes", total_tracked);
    let total_overhead = total_tracked as f64 / total_raw as f64;
    println!("  Average overhead:           {:.2}x", total_overhead);
    println!();

    // Analysis
    println!("Analysis:");
    if total_overhead < 2.0 {
        println!("  ✅ Overhead is better than expected (<2x)");
    } else if total_overhead < 3.0 {
        println!("  ✅ Overhead is within expected range (2-3x)");
    } else if total_overhead < 4.0 {
        println!("  ⚠️  Overhead is slightly higher than expected (3-4x)");
    } else {
        println!("  ❌ Overhead is significantly higher than expected (>4x)");
    }

    println!();
    println!("Notes:");
    println!("  - These are estimates based on size_of and capacity");
    println!("  - Actual memory usage may differ due to allocator overhead");
    println!("  - For typical Quarto configs (<10KB raw), overhead is acceptable");
    println!("  - The overhead provides precise error reporting and LSP support");
}
