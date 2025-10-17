use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod conversions;
mod diagnostics;
mod problem;
mod utils;

use conversions::definition_lists::DefinitionListConverter;
use conversions::div_whitespace::DivWhitespaceConverter;
use conversions::grid_tables::GridTableConverter;
use diagnostics::syntax_check::SyntaxChecker;
use utils::glob_expand::expand_globs;

#[derive(Parser)]
#[command(name = "qmd-syntax-helper")]
#[command(about = "Helper tool for converting and fixing Quarto Markdown syntax")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert grid tables to list-table format
    UngridTables {
        /// Input files (can be multiple files or glob patterns like "docs/**/*.qmd")
        #[arg(required = true)]
        files: Vec<String>,

        /// Edit files in place
        #[arg(short, long)]
        in_place: bool,

        /// Check mode: show what would be changed without modifying files
        #[arg(short, long)]
        check: bool,

        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Convert definition lists to div-based format
    UndefLists {
        /// Input files (can be multiple files or glob patterns like "docs/**/*.qmd")
        #[arg(required = true)]
        files: Vec<String>,

        /// Edit files in place
        #[arg(short, long)]
        in_place: bool,

        /// Check mode: show what would be changed without modifying files
        #[arg(short, long)]
        check: bool,

        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Fix div fences missing whitespace (:::{ -> ::: {)
    FixDivWhitespace {
        /// Input files (can be multiple files or glob patterns like "docs/**/*.qmd")
        #[arg(required = true)]
        files: Vec<String>,

        /// Edit files in place
        #[arg(short, long)]
        in_place: bool,

        /// Check mode: show what would be changed without modifying files
        #[arg(short, long)]
        check: bool,

        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Check syntax of files and report errors
    Check {
        /// Input files (can be multiple files or glob patterns like "docs/**/*.qmd")
        #[arg(required = true)]
        files: Vec<String>,

        /// Show verbose output (each file as processed)
        #[arg(short, long)]
        verbose: bool,

        /// Output results as JSONL
        #[arg(long)]
        json: bool,

        /// Save detailed results to file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::UngridTables {
            files,
            in_place,
            check,
            verbose,
        } => {
            let converter = GridTableConverter::new()?;
            let file_paths = expand_globs(&files)?;

            for file_path in file_paths {
                if verbose {
                    println!("Processing: {}", file_path.display());
                }

                converter.process_file(&file_path, in_place, check, verbose)?;
            }

            Ok(())
        }
        Commands::UndefLists {
            files,
            in_place,
            check,
            verbose,
        } => {
            let converter = DefinitionListConverter::new()?;
            let file_paths = expand_globs(&files)?;

            for file_path in file_paths {
                if verbose {
                    println!("Processing: {}", file_path.display());
                }

                converter.process_file(&file_path, in_place, check, verbose)?;
            }

            Ok(())
        }
        Commands::FixDivWhitespace {
            files,
            in_place,
            check,
            verbose,
        } => {
            let converter = DivWhitespaceConverter::new()?;
            let file_paths = expand_globs(&files)?;

            for file_path in file_paths {
                if verbose {
                    println!("Processing: {}", file_path.display());
                }

                converter.process_file(&file_path, in_place, check, verbose)?;
            }

            Ok(())
        }
        Commands::Check {
            files,
            verbose,
            json,
            output,
        } => {
            let mut checker = SyntaxChecker::new();
            let file_paths = expand_globs(&files)?;

            for file_path in file_paths {
                checker.check_file(&file_path, verbose)?;
            }

            // Print summary if not JSON mode
            if !json {
                checker.print_summary();
            }

            // Save to output file if specified
            if let Some(output_path) = output {
                checker.export_jsonl(&output_path)?;
                if !json {
                    println!("\nDetailed results written to: {}", output_path.display());
                }
            }

            // Print JSON to stdout if requested
            if json {
                for result in &checker.results {
                    println!("{}", serde_json::to_string(result)?);
                }
            }

            Ok(())
        }
    }
}
