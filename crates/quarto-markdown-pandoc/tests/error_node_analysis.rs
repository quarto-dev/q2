/*
 * error_node_analysis.rs
 * Temporary test to analyze ERROR nodes in tree-sitter parse tree
 */

use quarto_markdown_pandoc::utils::tree_sitter_log_observer::TreeSitterLogObserverTrait;
use std::collections::BTreeMap;
use tree_sitter_qmd::MarkdownParser;

#[test]
fn analyze_categorical_predictors_errors() {
    let input_path = std::path::Path::new(&std::env::var("HOME").unwrap())
        .join("today/categorical-predictors.qmd");

    if !input_path.exists() {
        println!("Skipping test - file not found: {:?}", input_path);
        return;
    }

    let input_bytes = std::fs::read(&input_path).expect("Failed to read file");
    let mut parser = MarkdownParser::default();
    let tree = parser.parse(&input_bytes, None).expect("Failed to parse");

    // Collect all ERROR nodes
    let mut error_nodes: Vec<(usize, usize, usize, usize)> = Vec::new();
    collect_error_nodes(&mut tree.walk(), &mut error_nodes);

    println!("\n=== ERROR NODE ANALYSIS ===");
    println!("Total ERROR nodes found: {}", error_nodes.len());

    // Find outer (non-nested) ERROR nodes
    let mut outer_errors: Vec<usize> = Vec::new();
    for i in 0..error_nodes.len() {
        let (start_i, end_i, _, _) = error_nodes[i];
        let mut is_outer = true;
        for j in 0..error_nodes.len() {
            if i == j {
                continue;
            }
            let (start_j, end_j, _, _) = error_nodes[j];
            // Check if node i is contained within node j
            if start_i >= start_j && end_i <= end_j {
                is_outer = false;
                break;
            }
        }
        if is_outer {
            outer_errors.push(i);
        }
    }

    println!("\nOuter (non-nested) ERROR nodes: {}", outer_errors.len());
    for outer_idx in &outer_errors {
        let (start, end, row, col) = error_nodes[*outer_idx];
        println!(
            "  {}. byte range: [{}, {}) at row:{} col:{} (size: {} bytes)",
            outer_idx + 1,
            start,
            end,
            row,
            col,
            end - start
        );
    }

    println!("\nAll ERROR node byte ranges:");
    for (i, (start, end, row, col)) in error_nodes.iter().enumerate() {
        let is_outer = outer_errors.contains(&i);
        let marker = if is_outer { " [OUTER]" } else { "" };
        println!(
            "  {}. byte range: [{}, {}) at row:{} col:{} (size: {} bytes){}",
            i + 1,
            start,
            end,
            row,
            col,
            end - start,
            marker
        );
    }

    // Now run the actual error diagnostic generation to compare
    let mut log_observer =
        quarto_markdown_pandoc::utils::tree_sitter_log_observer::TreeSitterLogObserver::default();
    parser.parser.set_logger(Some(Box::new(|log_type, message| {
        if let tree_sitter::LogType::Parse = log_type {
            log_observer.log(log_type, message);
        }
    })));
    parser.parse(&input_bytes, None).expect("Failed to parse");

    if log_observer.had_errors() {
        let filename = "categorical-predictors.qmd";
        let source_context = quarto_source_map::SourceContext::new();
        let diagnostics =
            quarto_markdown_pandoc::readers::qmd_error_messages::produce_diagnostic_messages(
                &input_bytes,
                &log_observer,
                filename,
                &source_context,
            );

        println!("\n=== DIAGNOSTIC MESSAGE ANALYSIS ===");
        println!("Total diagnostics generated: {}", diagnostics.len());

        // Group diagnostics by ERROR node
        let mut diagnostics_by_error_node: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut diagnostics_outside_errors = Vec::new();

        for (diag_idx, diag) in diagnostics.iter().enumerate() {
            if let Some(location) = &diag.location {
                let diag_start = location.start_offset();
                let diag_end = location.end_offset();

                // Find which ERROR node this diagnostic falls into
                let mut found = false;
                for (error_idx, (err_start, err_end, _, _)) in error_nodes.iter().enumerate() {
                    // Check if diagnostic overlaps with error node
                    if diag_start < *err_end && diag_end > *err_start {
                        diagnostics_by_error_node
                            .entry(error_idx)
                            .or_insert(Vec::new())
                            .push(diag_idx);
                        found = true;
                        break;
                    }
                }

                if !found {
                    diagnostics_outside_errors.push(diag_idx);
                }
            }
        }

        println!("\nDiagnostics per ERROR node:");
        for (error_idx, diag_indices) in diagnostics_by_error_node.iter() {
            let (start, end, row, col) = error_nodes[*error_idx];
            let is_outer = outer_errors.contains(error_idx);
            let marker = if is_outer { " [OUTER]" } else { "" };
            println!(
                "  ERROR node {} (byte [{}, {}) at row:{} col:{}): {} diagnostics{}",
                error_idx + 1,
                start,
                end,
                row,
                col,
                diag_indices.len(),
                marker
            );
        }

        println!(
            "\nDiagnostics outside ERROR nodes: {}",
            diagnostics_outside_errors.len()
        );

        // Now analyze using only OUTER error nodes
        let mut diagnostics_by_outer_error: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut diagnostics_outside_outer_errors = Vec::new();

        for (diag_idx, diag) in diagnostics.iter().enumerate() {
            if let Some(location) = &diag.location {
                let diag_start = location.start_offset();
                let diag_end = location.end_offset();

                // Find which OUTER ERROR node this diagnostic falls into
                let mut found = false;
                for outer_idx in &outer_errors {
                    let (err_start, err_end, _, _) = error_nodes[*outer_idx];
                    // Check if diagnostic overlaps with error node
                    if diag_start < err_end && diag_end > err_start {
                        diagnostics_by_outer_error
                            .entry(*outer_idx)
                            .or_insert(Vec::new())
                            .push(diag_idx);
                        found = true;
                        break;
                    }
                }

                if !found {
                    diagnostics_outside_outer_errors.push(diag_idx);
                }
            }
        }

        println!("\n=== ANALYSIS USING OUTER ERROR NODES ===");
        println!("Diagnostics per OUTER ERROR node:");
        for (error_idx, diag_indices) in diagnostics_by_outer_error.iter() {
            let (start, end, row, col) = error_nodes[*error_idx];
            println!(
                "  OUTER ERROR node {} (byte [{}, {}) at row:{} col:{}): {} diagnostics",
                error_idx + 1,
                start,
                end,
                row,
                col,
                diag_indices.len()
            );
        }

        println!(
            "\nDiagnostics outside OUTER ERROR nodes: {}",
            diagnostics_outside_outer_errors.len()
        );

        // Show pruning opportunities using OUTER error nodes
        println!("\n=== PRUNING OPPORTUNITIES (using OUTER ERROR nodes) ===");
        let mut total_kept = 0;
        let mut total_pruned = 0;
        for (error_idx, diag_indices) in diagnostics_by_outer_error.iter() {
            if diag_indices.len() > 1 {
                let (start, end, row, col) = error_nodes[*error_idx];
                println!(
                    "  OUTER ERROR node {} (byte [{}, {}) at row:{} col:{}): {} diagnostics -> keep 1, prune {}",
                    error_idx + 1,
                    start,
                    end,
                    row,
                    col,
                    diag_indices.len(),
                    diag_indices.len() - 1
                );
                total_kept += 1;
                total_pruned += diag_indices.len() - 1;
            } else if diag_indices.len() == 1 {
                total_kept += 1;
            }
        }

        println!("\nSummary (using OUTER ERROR nodes):");
        println!("  Total diagnostics before pruning: {}", diagnostics.len());
        println!(
            "  Diagnostics to keep (1 per OUTER ERROR node): {}",
            total_kept
        );
        println!("  Diagnostics to prune: {}", total_pruned);
        println!(
            "  Diagnostics outside OUTER ERROR nodes: {}",
            diagnostics_outside_outer_errors.len()
        );
        println!("  Expected final count: {}", total_kept);
    }
}

fn collect_error_nodes(
    cursor: &mut tree_sitter_qmd::MarkdownCursor,
    errors: &mut Vec<(usize, usize, usize, usize)>,
) {
    let node = cursor.node();

    if node.kind() == "ERROR" {
        let start = node.start_byte();
        let end = node.end_byte();
        let pos = node.start_position();
        errors.push((start, end, pos.row, pos.column));
    }

    // Recurse to children
    if cursor.goto_first_child() {
        loop {
            collect_error_nodes(cursor, errors);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}
