/*
 * ast_reconcile.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Utility binary to compute and report AST reconciliation between two qmd files.
 */
#![feature(trim_prefix_suffix)]

use clap::Parser;
use pampa::readers;
use quarto_pandoc_types::reconcile::{ReconciliationPlan, compute_reconciliation};
use serde::Serialize;
use std::io;

#[derive(Parser, Debug)]
#[command(name = "ast-reconcile")]
#[command(about = "Compute and report AST reconciliation between two qmd files")]
struct Args {
    /// The pre-engine qmd file (as authored)
    #[arg(short = 'b', long = "before")]
    before: String,

    /// The post-engine qmd file (after execution)
    #[arg(short = 'a', long = "after")]
    after: String,

    /// Output format (json or summary)
    #[arg(short = 'f', long = "format", default_value = "json")]
    format: String,

    /// Pretty-print JSON output
    #[arg(long = "pretty")]
    pretty: bool,
}

/// Report structure for JSON output
#[derive(Serialize)]
struct ReconciliationReport {
    /// Source files
    before_file: String,
    after_file: String,

    /// The reconciliation plan
    plan: ReconciliationPlan,

    /// Summary statistics
    summary: Summary,
}

#[derive(Serialize)]
struct Summary {
    total_blocks_in_result: usize,
    blocks_kept_from_before: usize,
    blocks_used_from_after: usize,
    blocks_with_recursion: usize,
    preservation_rate: f64,
}

fn main() {
    let args = Args::parse();

    // Read before file
    let before_content = std::fs::read_to_string(&args.before).unwrap_or_else(|e| {
        eprintln!("Error reading before file '{}': {}", args.before, e);
        std::process::exit(1);
    });

    // Read after file
    let after_content = std::fs::read_to_string(&args.after).unwrap_or_else(|e| {
        eprintln!("Error reading after file '{}': {}", args.after, e);
        std::process::exit(1);
    });

    // Ensure files end with newline
    let before_content = if before_content.ends_with('\n') {
        before_content
    } else {
        format!("{}\n", before_content)
    };
    let after_content = if after_content.ends_with('\n') {
        after_content
    } else {
        format!("{}\n", after_content)
    };

    // Parse before file
    let mut sink = io::sink();
    let (before_ast, _, _) = match readers::qmd::read(
        before_content.as_bytes(),
        false,
        &args.before,
        &mut sink,
        true,
        None,
    ) {
        Ok(result) => result,
        Err(diagnostics) => {
            eprintln!("Error parsing before file '{}':", args.before);
            for diag in diagnostics {
                eprintln!("  {}", diag.to_text(None));
            }
            std::process::exit(1);
        }
    };

    // Parse after file
    let (after_ast, _, _) = match readers::qmd::read(
        after_content.as_bytes(),
        false,
        &args.after,
        &mut sink,
        true,
        None,
    ) {
        Ok(result) => result,
        Err(diagnostics) => {
            eprintln!("Error parsing after file '{}':", args.after);
            for diag in diagnostics {
                eprintln!("  {}", diag.to_text(None));
            }
            std::process::exit(1);
        }
    };

    // Compute reconciliation plan
    let plan = compute_reconciliation(&before_ast, &after_ast);

    // Calculate summary
    let total_blocks = plan.block_alignments.len();
    let blocks_kept = plan.stats.blocks_kept;
    let blocks_replaced = plan.stats.blocks_replaced;
    let blocks_recursed = plan.stats.blocks_recursed;
    let preservation_rate = if total_blocks > 0 {
        (blocks_kept as f64 + blocks_recursed as f64) / total_blocks as f64
    } else {
        1.0
    };

    match args.format.as_str() {
        "json" => {
            let report = ReconciliationReport {
                before_file: args.before,
                after_file: args.after,
                plan,
                summary: Summary {
                    total_blocks_in_result: total_blocks,
                    blocks_kept_from_before: blocks_kept,
                    blocks_used_from_after: blocks_replaced,
                    blocks_with_recursion: blocks_recursed,
                    preservation_rate,
                },
            };

            let json = if args.pretty {
                serde_json::to_string_pretty(&report)
            } else {
                serde_json::to_string(&report)
            };

            match json {
                Ok(s) => println!("{}", s),
                Err(e) => {
                    eprintln!("Error serializing to JSON: {}", e);
                    std::process::exit(1);
                }
            }
        }
        "summary" => {
            println!("AST Reconciliation Report");
            println!("=========================");
            println!("Before: {}", args.before);
            println!("After:  {}", args.after);
            println!();
            println!("Statistics:");
            println!("  Total blocks in result:   {}", total_blocks);
            println!(
                "  Blocks kept from before:  {} ({:.1}%)",
                blocks_kept,
                if total_blocks > 0 {
                    blocks_kept as f64 / total_blocks as f64 * 100.0
                } else {
                    0.0
                }
            );
            println!(
                "  Blocks used from after:   {} ({:.1}%)",
                blocks_replaced,
                if total_blocks > 0 {
                    blocks_replaced as f64 / total_blocks as f64 * 100.0
                } else {
                    0.0
                }
            );
            println!(
                "  Blocks with recursion:    {} ({:.1}%)",
                blocks_recursed,
                if total_blocks > 0 {
                    blocks_recursed as f64 / total_blocks as f64 * 100.0
                } else {
                    0.0
                }
            );
            println!();
            println!(
                "  Preservation rate:        {:.1}%",
                preservation_rate * 100.0
            );
            println!();
            println!("Block Alignments:");
            for (i, alignment) in plan.block_alignments.iter().enumerate() {
                let desc = match alignment {
                    quarto_pandoc_types::reconcile::BlockAlignment::KeepBefore(idx) => {
                        format!("KEEP before[{}]", idx)
                    }
                    quarto_pandoc_types::reconcile::BlockAlignment::UseAfter(idx) => {
                        format!("USE after[{}]", idx)
                    }
                    quarto_pandoc_types::reconcile::BlockAlignment::RecurseIntoContainer {
                        before_idx,
                        after_idx,
                    } => {
                        format!("RECURSE before[{}] <-> after[{}]", before_idx, after_idx)
                    }
                };
                println!("  [{}] {}", i, desc);
            }
        }
        _ => {
            eprintln!("Unknown format: {}. Use 'json' or 'summary'.", args.format);
            std::process::exit(1);
        }
    }
}
