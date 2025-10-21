#![feature(trim_prefix_suffix)]
#![allow(dead_code)]

/*
 * main.rs
 * Copyright (c) 2025 Posit, PBC
 */

use clap::Parser;
use std::io::{self, Read, Write};

mod errors;
mod filters;
mod pandoc;
mod readers;
mod traversals;
mod utils;
mod writers;
use utils::output::VerboseOutput;

#[derive(Parser, Debug)]
#[command(name = "quarto-markdown-pandoc")]
#[command(about = "Convert Quarto markdown to various output formats")]
struct Args {
    #[arg(short = 'f', long = "from", default_value = "markdown")]
    from: String,

    #[arg(short = 't', long = "to", default_value = "native")]
    to: String,

    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

    #[arg(short = 'i', long = "input", default_value = "-")]
    input: String,

    #[arg(long = "loose")]
    loose: bool,

    #[arg(long = "json-errors")]
    json_errors: bool,

    #[arg(short = 'o', long = "output")]
    output: Option<String>,

    #[arg(
        long = "_internal-report-error-state",
        hide = true,
        default_value_t = false
    )]
    _internal_report_error_state: bool,
}

fn print_whole_tree<T: Write>(cursor: &mut tree_sitter_qmd::MarkdownCursor, buf: &mut T) {
    let mut depth = 0;
    traversals::topdown_traverse_concrete_tree(cursor, &mut |node, phase| {
        if phase == traversals::TraversePhase::Enter {
            writeln!(buf, "{}{}: {:?}", "  ".repeat(depth), node.kind(), node).unwrap();
            depth += 1;
        } else {
            depth -= 1;
        }
        true // continue traversing
    });
}

fn main() {
    let args = Args::parse();

    let mut input_filename = "<stdin>";
    let mut input = String::new();
    let mut output_stream = if args.verbose {
        VerboseOutput::Stderr(io::stderr())
    } else {
        VerboseOutput::Sink(io::sink())
    };
    if args.input == "-" {
        // Read from stdin
        io::stdin()
            .read_to_string(&mut input)
            .expect("Failed to read from stdin");
    } else {
        // Read from file
        input_filename = &args.input;
        std::fs::File::open(&args.input)
            .expect("Failed to open input file")
            .read_to_string(&mut input)
            .expect("Failed to read input file");
    }

    if !input.ends_with("\n") {
        if args.json_errors {
            // Output as JSON to stderr
            let warning_json = serde_json::json!({
                "title": "Warning",
                "message": "Adding missing newline to end of input"
            });
            eprintln!("{}", warning_json);
        } else {
            // Output as plain text to stderr
            eprintln!("(Warning) Adding missing newline to end of input.");
        }
        input.push('\n'); // ensure the input ends with a newline
    }

    if args._internal_report_error_state {
        let error_messages = readers::qmd::read_bad_qmd_for_error_message(input.as_bytes());
        for msg in error_messages {
            println!("{}", msg);
        }
        return;
    }

    let (pandoc, context) = match args.from.as_str() {
        "markdown" | "qmd" => {
            let result = readers::qmd::read(
                input.as_bytes(),
                args.loose,
                input_filename,
                &mut output_stream,
            );
            match result {
                Ok((pandoc, context, warnings)) => {
                    // Output warnings to stderr
                    if args.json_errors {
                        // JSON format
                        for warning in warnings {
                            eprintln!("{}", warning.to_json());
                        }
                    } else {
                        // Text format (default)
                        for warning in warnings {
                            eprintln!("{}", warning.to_text(None));
                        }
                    }
                    (pandoc, context)
                }
                Err(diagnostics) => {
                    // Output errors
                    if args.json_errors {
                        // For JSON errors, print to stdout as a JSON array
                        for diagnostic in diagnostics {
                            println!("{}", diagnostic.to_json());
                        }
                    } else {
                        for diagnostic in diagnostics {
                            eprintln!("{}", diagnostic.to_text(None));
                        }
                    }
                    std::process::exit(1);
                }
            }
        }
        "json" => {
            let result = readers::json::read(&mut input.as_bytes());
            match result {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Error reading JSON: {}", e);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("Unknown input format: {}", args.from);
            std::process::exit(1);
        }
    };

    let mut buf = Vec::new();
    match args.to.as_str() {
        "json" => writers::json::write(&pandoc, &context, &mut buf),
        "native" => writers::native::write(&pandoc, &mut buf),
        "markdown" | "qmd" => writers::qmd::write(&pandoc, &mut buf),
        "html" => writers::html::write(&pandoc, &mut buf),
        _ => {
            eprintln!("Unknown output format: {}", args.to);
            return;
        }
    }
    .unwrap();

    // Write output to file or stdout
    if let Some(output_path) = args.output {
        std::fs::write(&output_path, &buf)
            .expect(&format!("Failed to write output to file: {}", output_path));
    } else {
        let output = String::from_utf8(buf).expect("Invalid UTF-8 in output");
        print!("{}", output);
    }
}
