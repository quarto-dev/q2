//! Scaling analysis: verify overhead grows linearly with data size
//!
//! This benchmark tests whether memory overhead grows linearly (O(n)) or
//! superlinearly (O(n²), O(n log n), etc.) with increasing YAML data size.
//!
//! If overhead ratio stays constant as size increases → Linear (good!)
//! If overhead ratio increases as size increases → Superlinear (bad!)
//!
//! Run with: cargo bench --bench scaling_overhead

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

    size += estimate_yaml_memory(&yaml.yaml);
    // Note: SourceInfo size is already included in sizeof(YamlWithSourceInfo)
    // For basic parsing, SourceInfo uses Original variant with FileId (just a usize)

    if let Some(children) = yaml.as_array() {
        size += children.len() * mem::size_of::<quarto_yaml::YamlWithSourceInfo>();
        for child in children {
            size += estimate_yaml_with_source_memory(child);
        }
    } else if let Some(entries) = yaml.as_hash() {
        size += entries.len() * mem::size_of::<quarto_yaml::YamlHashEntry>();
        for entry in entries {
            size += estimate_yaml_with_source_memory(&entry.key);
            size += estimate_yaml_with_source_memory(&entry.value);
            size += mem::size_of::<quarto_yaml::SourceInfo>() * 3;
        }
    }

    size
}

struct ScalingResult {
    size: usize,
    raw_bytes: usize,
    tracked_bytes: usize,
    overhead_ratio: f64,
}

/// Generate a flat array of N string items
fn generate_flat_array(n: usize) -> String {
    let mut yaml = String::from("[\n");
    for i in 0..n {
        yaml.push_str(&format!("  \"item_{}\",\n", i));
    }
    yaml.push_str("]\n");
    yaml
}

/// Generate a flat hash with N key-value pairs
fn generate_flat_hash(n: usize) -> String {
    let mut yaml = String::new();
    for i in 0..n {
        yaml.push_str(&format!("key_{}: \"value_{}\"\n", i, i));
    }
    yaml
}

/// Generate a nested structure with depth D and breadth B
/// (D levels deep, B children at each level)
fn generate_nested_structure(depth: usize, breadth: usize) -> String {
    fn generate_level(
        current_depth: usize,
        max_depth: usize,
        breadth: usize,
        indent: usize,
    ) -> String {
        let ind = "  ".repeat(indent);

        if current_depth >= max_depth {
            return format!("{}value\n", ind);
        }

        let mut yaml = String::new();
        for i in 0..breadth {
            yaml.push_str(&format!("{}child_{}:\n", ind, i));
            yaml.push_str(&generate_level(
                current_depth + 1,
                max_depth,
                breadth,
                indent + 1,
            ));
        }
        yaml
    }

    generate_level(0, depth, breadth, 0)
}

/// Generate a mixed structure: top-level hash with N keys, each having a small nested structure
fn generate_mixed_structure(n: usize) -> String {
    let mut yaml = String::new();
    for i in 0..n {
        yaml.push_str(&format!(
            "section_{}:\n  title: \"Section {}\"\n  enabled: true\n  items:\n    - item1\n    - item2\n    - item3\n",
            i, i
        ));
    }
    yaml
}

fn test_scaling(name: &str, generator: impl Fn(usize) -> String, sizes: &[usize]) {
    println!("\n{}", "=".repeat(70));
    println!("Scaling Test: {}", name);
    println!("{}", "=".repeat(70));
    println!(
        "{:>6} {:>12} {:>12} {:>12} {:>8}",
        "Size", "Raw (bytes)", "Tracked", "Overhead", "Ratio"
    );
    println!("{}", "-".repeat(70));

    let mut results = Vec::new();

    for &size in sizes {
        let yaml_content = generator(size);

        // Parse with yaml-rust2
        let raw_docs = YamlLoader::load_from_str(&yaml_content).expect("Failed to parse YAML");
        let raw_yaml = &raw_docs[0];
        let raw_bytes = estimate_yaml_memory(raw_yaml);

        // Parse with YamlWithSourceInfo
        let tracked_yaml = parse(&yaml_content).expect("Failed to parse YAML with source tracking");
        let tracked_bytes = estimate_yaml_with_source_memory(&tracked_yaml);

        let overhead = tracked_bytes - raw_bytes;
        let ratio = tracked_bytes as f64 / raw_bytes as f64;

        println!(
            "{:>6} {:>12} {:>12} {:>12} {:>8.2}x",
            size, raw_bytes, tracked_bytes, overhead, ratio
        );

        results.push(ScalingResult {
            size,
            raw_bytes,
            tracked_bytes,
            overhead_ratio: ratio,
        });
    }

    // Analyze scaling behavior
    println!("\nScaling Analysis:");

    if results.len() >= 2 {
        let first = &results[0];
        let last = &results[results.len() - 1];

        let size_ratio = last.size as f64 / first.size as f64;
        let raw_ratio = last.raw_bytes as f64 / first.raw_bytes as f64;
        let tracked_ratio = last.tracked_bytes as f64 / first.tracked_bytes as f64;

        println!("  Size increased:         {:.1}x", size_ratio);
        println!("  Raw memory increased:   {:.1}x", raw_ratio);
        println!("  Tracked memory increased: {:.1}x", tracked_ratio);

        // Check if overhead ratio is stable
        let ratio_change = (last.overhead_ratio - first.overhead_ratio).abs();
        let ratio_change_pct = (ratio_change / first.overhead_ratio) * 100.0;

        println!(
            "\n  Overhead ratio change: {:.2}x → {:.2}x (Δ{:.1}%)",
            first.overhead_ratio, last.overhead_ratio, ratio_change_pct
        );

        if ratio_change_pct < 10.0 {
            println!("  ✅ Overhead is STABLE - scales linearly!");
        } else if ratio_change_pct < 25.0 {
            println!("  ⚠️  Overhead grows slightly - possibly O(n log n)");
        } else {
            println!("  ❌ Overhead grows significantly - possibly superlinear!");
        }

        // Check raw and tracked growth rates
        let raw_per_item = last.raw_bytes as f64 / last.size as f64;
        let tracked_per_item = last.tracked_bytes as f64 / last.size as f64;

        println!("\n  At largest size:");
        println!("    Raw bytes per item:     {:.1} bytes", raw_per_item);
        println!("    Tracked bytes per item: {:.1} bytes", tracked_per_item);
        println!(
            "    Overhead per item:      {:.1} bytes",
            tracked_per_item - raw_per_item
        );
    }
}

fn main() {
    println!("Scaling Overhead Analysis: YamlWithSourceInfo");
    println!("=============================================================");
    println!("Testing whether overhead grows linearly with data size");
    println!();

    // Test 1: Flat arrays
    let array_sizes = vec![10, 50, 100, 250, 500, 1000];
    test_scaling("Flat Array", generate_flat_array, &array_sizes);

    // Test 2: Flat hashes
    let hash_sizes = vec![10, 50, 100, 250, 500, 1000];
    test_scaling("Flat Hash", generate_flat_hash, &hash_sizes);

    // Test 3: Mixed structures (realistic Quarto configs)
    let mixed_sizes = vec![5, 10, 20, 50, 100];
    test_scaling("Mixed Structure", generate_mixed_structure, &mixed_sizes);

    // Test 4: Nested structures (depth=5, varying breadth)
    println!("\n{}", "=".repeat(70));
    println!("Nested Structure Scaling (depth=5, varying breadth)");
    println!("{}", "=".repeat(70));
    println!(
        "{:>8} {:>12} {:>12} {:>12} {:>8}",
        "Breadth", "Raw (bytes)", "Tracked", "Overhead", "Ratio"
    );
    println!("{}", "-".repeat(70));

    let breadths = vec![2, 3, 4, 5];
    let mut nested_results = Vec::new();

    for breadth in &breadths {
        let yaml_content = generate_nested_structure(5, *breadth);

        let raw_docs = YamlLoader::load_from_str(&yaml_content).expect("Failed to parse YAML");
        let raw_yaml = &raw_docs[0];
        let raw_bytes = estimate_yaml_memory(raw_yaml);

        let tracked_yaml = parse(&yaml_content).expect("Failed to parse YAML with source tracking");
        let tracked_bytes = estimate_yaml_with_source_memory(&tracked_yaml);

        let overhead = tracked_bytes - raw_bytes;
        let ratio = tracked_bytes as f64 / raw_bytes as f64;

        println!(
            "{:>8} {:>12} {:>12} {:>12} {:>8.2}x",
            breadth, raw_bytes, tracked_bytes, overhead, ratio
        );

        nested_results.push((breadth, raw_bytes, tracked_bytes, ratio));
    }

    println!("\nNested Structure Analysis:");
    if nested_results.len() >= 2 {
        let first = nested_results.first().unwrap();
        let last = nested_results.last().unwrap();

        let total_nodes_first = first.0.pow(5); // breadth^depth
        let total_nodes_last = last.0.pow(5);

        println!(
            "  Total nodes: {} → {}",
            total_nodes_first, total_nodes_last
        );
        println!("  Overhead ratio: {:.2}x → {:.2}x", first.3, last.3);

        let ratio_change_pct = ((last.3 - first.3) / first.3) * 100.0;
        if ratio_change_pct.abs() < 10.0 {
            println!("  ✅ Overhead is STABLE even with deep nesting!");
        } else {
            println!("  ⚠️  Overhead changes with nesting depth");
        }
    }

    // Final summary
    println!("\n{}", "=".repeat(70));
    println!("CONCLUSION");
    println!("{}", "=".repeat(70));
    println!("If overhead ratios stay roughly constant (within 10-25%)");
    println!("across all tests, then overhead scales linearly O(n).");
    println!();
    println!("This means larger configs use proportionally more memory,");
    println!("but don't suffer from superlinear growth.");
}
