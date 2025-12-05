#![feature(trim_prefix_suffix)]
#![allow(dead_code)]

/*
 * main.rs
 * Copyright (c) 2025 Posit, PBC
 */

use clap::Parser;
use quarto_error_reporting::DiagnosticMessageBuilder;
use std::io::{self, Read, Write};

mod citeproc_filter;
mod errors;
mod filter_context;
mod filters;
#[cfg(feature = "json-filter")]
mod json_filter;
#[cfg(feature = "lua-filter")]
mod lua;
mod options;
mod pandoc;
mod readers;
mod template;
mod traversals;
mod unified_filter;
mod utils;
mod writers;
use template::{
    TemplateBundle,
    builtin::{BUILTIN_TEMPLATE_NAMES, get_builtin_template, is_builtin_template},
    render::{BodyFormat, render_with_bundle},
};
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

    #[arg(long = "no-prune-errors")]
    no_prune_errors: bool,

    #[arg(long = "json-source-location", value_parser = ["full"])]
    json_source_location: Option<String>,

    #[arg(short = 'o', long = "output")]
    output: Option<String>,

    #[arg(
        long = "_internal-report-error-state",
        hide = true,
        default_value_t = false
    )]
    _internal_report_error_state: bool,

    /// Apply a filter to the document (can be specified multiple times).
    /// Filter type is determined automatically:
    /// - "citeproc": built-in citation processor
    /// - *.lua: Lua filter
    /// - anything else: JSON filter (external executable)
    #[arg(short = 'F', long = "filter", action = clap::ArgAction::Append)]
    filters: Vec<String>,

    /// Use a template (built-in name like 'html5' or file path)
    #[cfg(feature = "template-fs")]
    #[arg(long = "template")]
    template: Option<String>,

    /// Use a template bundle JSON file
    #[arg(long = "template-bundle")]
    template_bundle: Option<std::path::PathBuf>,

    /// Export a built-in template as JSON and exit
    #[arg(long = "export-template", value_name = "NAME")]
    export_template: Option<String>,
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

    // Handle --export-template early (like --help)
    if let Some(template_name) = &args.export_template {
        match get_builtin_template(template_name) {
            Some(bundle) => match bundle.to_json() {
                Ok(json) => {
                    println!("{}", json);
                    return;
                }
                Err(e) => {
                    eprintln!("Error serializing template: {}", e);
                    std::process::exit(1);
                }
            },
            None => {
                eprintln!(
                    "Unknown built-in template: '{}'. Available: {}",
                    template_name,
                    BUILTIN_TEMPLATE_NAMES.join(", ")
                );
                std::process::exit(1);
            }
        }
    }

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
        let warning = DiagnosticMessageBuilder::warning("Missing Newline at End of File")
            .with_code("Q-7-1")
            .problem(format!(
                "File `{}` does not end with a newline",
                input_filename
            ))
            .add_info("A newline will be added automatically")
            .build();

        if args.json_errors {
            eprintln!("{}", warning.to_json());
        } else {
            eprintln!("{}", warning.to_text(None));
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
                !args.no_prune_errors, // prune_errors = !no_prune_errors
                None,
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
                        // Text format (default) - pass source_context for Ariadne rendering
                        for warning in warnings {
                            eprintln!("{}", warning.to_text(Some(&context.source_context)));
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
                        // Build a minimal source context for Ariadne rendering
                        let mut source_context = quarto_source_map::SourceContext::new();
                        source_context.add_file(input_filename.to_string(), Some(input.clone()));

                        for diagnostic in diagnostics {
                            eprintln!("{}", diagnostic.to_text(Some(&source_context)));
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

    // Apply filters in order
    let (pandoc, context) = if args.filters.is_empty() {
        (pandoc, context)
    } else {
        // Parse filter specifications
        let filter_specs: Vec<unified_filter::FilterSpec> = args
            .filters
            .iter()
            .map(|s| unified_filter::FilterSpec::parse(s))
            .collect();

        match unified_filter::apply_filters(pandoc, context, &filter_specs, &args.to) {
            Ok((filtered_pandoc, filtered_context, diagnostics)) => {
                // Output any diagnostics from filters
                if !diagnostics.is_empty() {
                    if args.json_errors {
                        for diagnostic in &diagnostics {
                            eprintln!("{}", diagnostic.to_json());
                        }
                    } else {
                        for diagnostic in &diagnostics {
                            eprintln!(
                                "{}",
                                diagnostic.to_text(Some(&filtered_context.source_context))
                            );
                        }
                    }
                }
                (filtered_pandoc, filtered_context)
            }
            Err(e) => {
                if args.json_errors {
                    let error_json = serde_json::json!({
                        "title": "Filter Error",
                        "message": e.to_string()
                    });
                    eprintln!("{}", error_json);
                } else {
                    eprintln!("Filter error: {}", e);
                }
                std::process::exit(1);
            }
        }
    };

    // Load template if specified
    #[cfg(feature = "template-fs")]
    let template_bundle: Option<TemplateBundle> = {
        if let Some(bundle_path) = &args.template_bundle {
            let bundle_json = std::fs::read_to_string(bundle_path).unwrap_or_else(|e| {
                eprintln!(
                    "Failed to read template bundle '{}': {}",
                    bundle_path.display(),
                    e
                );
                std::process::exit(1);
            });
            match TemplateBundle::from_json(&bundle_json) {
                Ok(bundle) => Some(bundle),
                Err(e) => {
                    eprintln!("Failed to parse template bundle: {}", e);
                    std::process::exit(1);
                }
            }
        } else if let Some(template_arg) = &args.template {
            if is_builtin_template(template_arg) {
                get_builtin_template(template_arg)
            } else {
                let template_path = std::path::Path::new(template_arg);
                let template_source = std::fs::read_to_string(template_path).unwrap_or_else(|e| {
                    eprintln!("Failed to read template file '{}': {}", template_arg, e);
                    std::process::exit(1);
                });
                Some(TemplateBundle::new(template_source))
            }
        } else {
            None
        }
    };

    #[cfg(not(feature = "template-fs"))]
    let template_bundle: Option<TemplateBundle> = {
        if let Some(bundle_path) = &args.template_bundle {
            let bundle_json = std::fs::read_to_string(bundle_path).unwrap_or_else(|e| {
                eprintln!(
                    "Failed to read template bundle '{}': {}",
                    bundle_path.display(),
                    e
                );
                std::process::exit(1);
            });
            match TemplateBundle::from_json(&bundle_json) {
                Ok(bundle) => Some(bundle),
                Err(e) => {
                    eprintln!("Failed to parse template bundle: {}", e);
                    std::process::exit(1);
                }
            }
        } else {
            None
        }
    };

    let mut buf = Vec::new();
    let writer_result = if let Some(bundle) = template_bundle {
        // Determine body format from --to
        let body_format = match args.to.as_str() {
            "html" => BodyFormat::Html,
            "plaintext" | "plain" => BodyFormat::Plaintext,
            other => {
                eprintln!(
                    "Template rendering requires --to html or --to plaintext, got '{}'",
                    other
                );
                std::process::exit(1);
            }
        };

        match render_with_bundle(&pandoc, &context, &bundle, body_format) {
            Ok((output, diagnostics)) => {
                buf.extend_from_slice(output.as_bytes());
                // Output any diagnostics (warnings)
                if !diagnostics.is_empty() {
                    if args.json_errors {
                        for diagnostic in &diagnostics {
                            eprintln!("{}", diagnostic.to_json());
                        }
                    } else {
                        for diagnostic in &diagnostics {
                            eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
                        }
                    }
                }
                Ok(())
            }
            Err(e) => Err(vec![
                quarto_error_reporting::DiagnosticMessageBuilder::error("Template render error")
                    .with_code("Q-3-2")
                    .problem(format!("Failed to render template: {}", e))
                    .build(),
            ]),
        }
    } else {
        // No template - use regular writers
        match args.to.as_str() {
            "json" => {
                let json_config = writers::json::JsonConfig {
                    include_inline_locations: args
                        .json_source_location
                        .as_ref()
                        .map(|s| s == "full")
                        .unwrap_or(false),
                };
                writers::json::write_with_config(&pandoc, &context, &mut buf, &json_config)
            }
            "native" => writers::native::write(&pandoc, &context, &mut buf),
            "markdown" | "qmd" => writers::qmd::write(&pandoc, &mut buf),
            "html" => writers::html::write(&pandoc, &mut buf).map_err(|e| {
                vec![
                    quarto_error_reporting::DiagnosticMessageBuilder::error(
                        "IO error during write",
                    )
                    .with_code("Q-3-1")
                    .problem(format!("Failed to write HTML output: {}", e))
                    .build(),
                ]
            }),
            "plaintext" | "plain" => {
                let (output, diagnostics) = writers::plaintext::blocks_to_string(&pandoc.blocks);
                buf.extend_from_slice(output.as_bytes());
                // Plaintext diagnostics are warnings (dropped nodes), not errors
                // Output them but don't fail
                if !diagnostics.is_empty() {
                    if args.json_errors {
                        for diagnostic in &diagnostics {
                            eprintln!("{}", diagnostic.to_json());
                        }
                    } else {
                        for diagnostic in &diagnostics {
                            eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
                        }
                    }
                }
                Ok(())
            }
            #[cfg(feature = "terminal-support")]
            "ansi" => writers::ansi::write(&pandoc, &mut buf),
            _ => {
                eprintln!("Unknown output format: {}", args.to);
                std::process::exit(1);
            }
        }
    };

    if let Err(diagnostics) = writer_result {
        // Format and output writer errors
        if args.json_errors {
            for diagnostic in diagnostics {
                eprintln!("{}", diagnostic.to_json());
            }
        } else {
            for diagnostic in diagnostics {
                eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
            }
        }
        std::process::exit(1);
    }

    // Write output to file or stdout
    if let Some(output_path) = args.output {
        std::fs::write(&output_path, &buf)
            .expect(&format!("Failed to write output to file: {}", output_path));
    } else {
        let output = String::from_utf8(buf).expect("Invalid UTF-8 in output");
        print!("{}", output);
    }
}
