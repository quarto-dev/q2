use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

mod conversions;
mod diagnostics;
mod rule;
mod utils;

use rule::{CheckResult, Rule, RuleRegistry};
use std::collections::HashSet;
use utils::glob_expand::expand_globs;
use utils::parse_probe::probe_file;

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
    /// Check files for known problems
    Check {
        /// Input files (can be multiple files or glob patterns like "docs/**/*.qmd")
        #[arg(required = true)]
        files: Vec<String>,

        /// Rules to check (defaults to "all")
        #[arg(short = 'r', long = "rule", default_values_t = vec!["all".to_string()])]
        rule: Vec<String>,

        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,

        /// Output results as JSONL
        #[arg(long)]
        json: bool,

        /// Save detailed results to file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Convert/fix problems in files
    Convert {
        /// Input files (can be multiple files or glob patterns like "docs/**/*.qmd")
        #[arg(required = true)]
        files: Vec<String>,

        /// Rules to apply (defaults to "all")
        #[arg(short = 'r', long = "rule", default_values_t = vec!["all".to_string()])]
        rule: Vec<String>,

        /// Edit files in place
        #[arg(short, long)]
        in_place: bool,

        /// Check mode: show what would be changed without modifying files
        #[arg(short, long)]
        check: bool,

        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,

        /// Maximum iterations for fixing (default: 10)
        #[arg(long, default_value = "10")]
        max_iterations: usize,

        /// Disable iterative fixing (run each rule once, like old behavior)
        #[arg(long)]
        no_iteration: bool,
    },

    /// List all available rules
    ListRules,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let registry = RuleRegistry::new()?;

    match cli.command {
        Commands::Check {
            files,
            rule: rule_names,
            verbose,
            json,
            output,
        } => {
            let file_paths = expand_globs(&files)?;
            let total_files_checked = file_paths.len();
            let rules = resolve_rules(&registry, &rule_names, RuleUse::Check)?;

            // Rules that walk the AST are skipped on files that fail to
            // parse; such a file is accounted "unanalyzable", never clean
            // (bd-syntax-helper-parse-masking-w88mhedp). Sorted because the
            // registry iterates a HashMap.
            let mut requires_parse_rules: Vec<String> = rules
                .iter()
                .filter(|r| r.requires_parse())
                .map(|r| r.name().to_string())
                .collect();
            requires_parse_rules.sort();

            let mut all_results = Vec::new();
            // Files where a rule (or the parse probe) failed outright — not
            // checked, so never counted clean.
            let mut error_files: HashSet<String> = HashSet::new();

            for file_path in file_paths {
                // Print filename first in verbose mode
                if verbose && !json {
                    println!("Checking: {}", file_path.display());
                }

                let path_str = file_path.to_string_lossy().to_string();

                // Probe once per file, only when an AST-requiring rule was
                // requested.
                let parse_failure = if requires_parse_rules.is_empty() {
                    None
                } else {
                    match probe_file(&file_path) {
                        Ok(failure) => failure,
                        Err(e) => {
                            // Unreadable: every rule would fail identically.
                            error_files.insert(path_str);
                            if !json {
                                if !verbose {
                                    println!("{}", file_path.display());
                                }
                                eprintln!("  {} Error reading file: {}", "✗".red(), e);
                            }
                            continue;
                        }
                    }
                };

                // Collect results for this file
                let mut file_results = Vec::new();

                for rule in &rules {
                    if parse_failure.is_some() && rule.requires_parse() {
                        continue;
                    }
                    match rule.check(&file_path, verbose && !json) {
                        Ok(results) => {
                            file_results.extend(results);
                        }
                        Err(e) => {
                            error_files.insert(path_str.clone());
                            if !json {
                                // For errors, print filename first if not verbose
                                if !verbose {
                                    println!("{}", file_path.display());
                                }
                                eprintln!("  {} Error checking {}: {}", "✗".red(), rule.name(), e);
                            }
                        }
                    }
                }

                // One synthesized record per unparseable file: the skipped
                // rules never saw an AST, so "no findings" from them would
                // read as clean when the file was in fact not checked.
                if let Some(failure) = &parse_failure {
                    file_results.push(CheckResult {
                        rule_name: "unanalyzable".to_string(),
                        file_path: path_str.clone(),
                        message: Some(format!(
                            "{failure}; {} rule(s) not applied: {}",
                            requires_parse_rules.len(),
                            requires_parse_rules.join(", ")
                        )),
                        error_codes: if failure.error_codes.is_empty() {
                            None
                        } else {
                            Some(failure.error_codes.clone())
                        },
                        unanalyzable: true,
                        skipped_rules: Some(requires_parse_rules.clone()),
                        ..Default::default()
                    });
                }

                // Print results based on mode
                if !json {
                    if verbose {
                        // Verbose: filename already printed, just print issues
                        print_file_results(&file_results);
                    } else {
                        // Non-verbose: only print filename if there is
                        // something to say
                        let printable = file_results.iter().any(|r| r.has_issue || r.unanalyzable);
                        if printable {
                            println!("{}", file_path.display());
                            print_file_results(&file_results);
                            println!(); // Blank line between files
                        }
                    }
                }

                // Add to overall results
                all_results.extend(file_results);
            }

            // Print summary if not in JSON mode
            if !json {
                print_check_summary(&all_results, total_files_checked, &error_files);
            }

            // Output handling
            if json {
                for result in &all_results {
                    println!("{}", serde_json::to_string(result)?);
                }
            }

            if let Some(output_path) = output {
                let mut output_str = String::new();
                for result in &all_results {
                    output_str.push_str(&serde_json::to_string(result)?);
                    output_str.push('\n');
                }
                std::fs::write(output_path, output_str)?;
            }

            Ok(())
        }

        Commands::Convert {
            files,
            rule: rule_names,
            in_place,
            check: check_mode,
            verbose,
            max_iterations,
            no_iteration,
        } => {
            let file_paths = expand_globs(&files)?;
            let rules = resolve_rules(&registry, &rule_names, RuleUse::Convert)?;
            let max_iter = if no_iteration { 1 } else { max_iterations };

            // Rules that walk the AST are skipped while the working copy
            // fails to parse. Re-probed every iteration: an earlier rule in
            // the same run (e.g. apostrophe-quotes) may repair the parse
            // error, after which the skipped rules apply normally. Only if
            // the file *still* fails to parse at convergence is the skip
            // reported (bd-syntax-helper-parse-masking-w88mhedp).
            let mut requires_parse_rules: Vec<String> = rules
                .iter()
                .filter(|r| r.requires_parse())
                .map(|r| r.name().to_string())
                .collect();
            requires_parse_rules.sort();

            for file_path in file_paths {
                if verbose {
                    println!("Processing: {}", file_path.display());
                }

                // Create temporary working copy
                let temp_file = create_temp_copy(&file_path)?;
                let temp_path = temp_file.path().to_path_buf();

                // Iteration loop
                let mut iteration = 0;
                let mut total_fixes_for_file = 0;
                let mut prev_fixes = 0;
                let mut oscillation_count = 0;
                let mut show_iteration_details = false;

                loop {
                    iteration += 1;
                    let mut fixes_this_iteration = 0;

                    // Show iteration header if we're showing details
                    if show_iteration_details && verbose {
                        println!("  Iteration {}:", iteration);
                    }

                    // Probe the working copy, only when an AST-requiring rule
                    // was requested.
                    let parse_failure = if requires_parse_rules.is_empty() {
                        None
                    } else {
                        probe_file(&temp_path)?
                    };

                    // Apply all rules to temp file (always in_place=true, check_mode=false on temp)
                    // We always write to temp since it's temporary; finalize_temp_file handles check_mode
                    for rule in &rules {
                        if parse_failure.is_some() && rule.requires_parse() {
                            continue;
                        }
                        match rule.convert(&temp_path, true, false, verbose) {
                            Ok(mut result) => {
                                if result.fixes_applied > 0 {
                                    fixes_this_iteration += result.fixes_applied;
                                    total_fixes_for_file += result.fixes_applied;

                                    // Override file_path in result for user-facing reporting
                                    result.file_path = file_path.to_string_lossy().to_string();

                                    // Show rule progress
                                    if verbose || check_mode {
                                        let prefix =
                                            if show_iteration_details { "    " } else { "  " };
                                        println!(
                                            "{}{} {} - {}",
                                            prefix,
                                            if check_mode { "Would fix" } else { "Fixed" },
                                            rule.name(),
                                            result.message.clone().unwrap_or_default()
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "  {} Error converting {}: {}",
                                    "✗".red(),
                                    rule.name(),
                                    e
                                );
                                drop(temp_file); // Clean up temp
                                return Err(e);
                            }
                        }
                    }

                    // Check for convergence
                    if fixes_this_iteration == 0 {
                        if verbose && show_iteration_details {
                            println!(
                                "  Converged after {} iteration(s) ({} total fixes)",
                                iteration, total_fixes_for_file
                            );
                        }
                        break;
                    }

                    // Oscillation detection - only trigger if we've been stuck for many iterations
                    // Making steady progress (same # of fixes each time) is OK
                    if fixes_this_iteration == prev_fixes && iteration > 5 {
                        oscillation_count += 1;
                        if oscillation_count >= 3 {
                            eprintln!(
                                "  {} Warning: Possible oscillation detected (same fix count for {} consecutive iterations)",
                                "⚠".yellow(),
                                oscillation_count + 1
                            );
                            eprintln!("  Stopping iteration to prevent infinite loop");
                            break;
                        }
                    } else {
                        oscillation_count = 0;
                    }
                    prev_fixes = fixes_this_iteration;

                    // Check max iterations
                    if iteration >= max_iter {
                        if !no_iteration {
                            eprintln!(
                                "  {} Warning: Reached max iterations ({}), but file may still have issues",
                                "⚠".yellow(),
                                max_iter
                            );
                        }
                        break;
                    }

                    // From iteration 2 onwards, show detailed iteration info
                    if iteration == 1 && !no_iteration && fixes_this_iteration > 0 {
                        show_iteration_details = true;
                    }
                }

                // If the file still fails to parse now that the run has
                // settled, the AST-requiring rules were never applied —
                // refuse loudly (per file; the sweep continues).
                if !requires_parse_rules.is_empty()
                    && let Some(failure) = probe_file(&temp_path)?
                {
                    eprintln!(
                        "  {} {}: {}; rule(s) not applied: {}",
                        "⚠".yellow(),
                        file_path.display(),
                        failure,
                        requires_parse_rules.join(", ")
                    );
                }

                // Finalize: copy temp to original or print to stdout
                finalize_temp_file(temp_file, &file_path, in_place, check_mode)?;
            }

            Ok(())
        }

        Commands::ListRules => {
            println!("{}", "Available rules:".bold());
            let mut any_opt_in = false;
            let mut any_requires_parse = false;
            for name in registry.list_names() {
                let rule = registry.get(&name)?;
                let mut marker = String::new();
                if rule.opt_in_only() {
                    any_opt_in = true;
                    marker.push_str(" *");
                }
                if rule.requires_parse() {
                    any_requires_parse = true;
                    marker.push_str(" †");
                }
                println!("  {}{} - {}", name.cyan(), marker, rule.description());
            }
            if any_opt_in || any_requires_parse {
                println!();
            }
            if any_opt_in {
                println!(
                    "  {} not applied by `convert -r all`; \
                     name it explicitly with `-r <rule>` to opt in.",
                    "*".cyan()
                );
            }
            if any_requires_parse {
                println!(
                    "  {} needs the file to parse; on files that don't, `check` \
                     counts the file unanalyzable and `convert` skips the rule \
                     (fix parse errors first, e.g. `convert -r apostrophe-quotes`).",
                    "†".cyan()
                );
            }
            Ok(())
        }
    }
}

/// What the resolved rules are going to be used for.
///
/// `all` means "every rule" when reporting and "every rule safe to apply
/// unasked" when editing, because some rules make edits that cannot later be
/// distinguished from the author's intent (see [`Rule::opt_in_only`]).
/// Naming such a rule explicitly always applies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleUse {
    Check,
    Convert,
}

fn resolve_rules(
    registry: &RuleRegistry,
    names: &[String],
    rule_use: RuleUse,
) -> Result<Vec<std::sync::Arc<dyn Rule + Send + Sync>>> {
    if names.len() == 1 && names[0] == "all" {
        return Ok(match rule_use {
            RuleUse::Check => registry.all(),
            RuleUse::Convert => registry.all_auto_convertible(),
        });
    }

    let mut rules = Vec::new();
    for name in names {
        rules.push(registry.get(name)?);
    }
    Ok(rules)
}

/// Create a temporary copy of a file in the same directory
fn create_temp_copy(file_path: &Path) -> Result<NamedTempFile> {
    // Create temp file in same directory as original (enables atomic rename)
    let parent = file_path.parent().unwrap_or(Path::new("."));
    let temp = tempfile::Builder::new()
        .prefix(".qmd-syntax-helper.")
        .suffix(".tmp")
        .tempfile_in(parent)?;

    // Copy original content to temp
    let original_content = std::fs::read_to_string(file_path)?;
    std::fs::write(temp.path(), original_content)?;

    Ok(temp)
}

/// Finalize the temp file based on mode
fn finalize_temp_file(
    temp: NamedTempFile,
    original_path: &Path,
    in_place: bool,
    check_mode: bool,
) -> Result<()> {
    if check_mode {
        // Check mode: just drop temp (auto-deleted)
        drop(temp);
        return Ok(());
    }

    if in_place {
        // Preserve original permissions before persisting
        let metadata = std::fs::metadata(original_path)?;
        let permissions = metadata.permissions();
        std::fs::set_permissions(temp.path(), permissions)?;

        // Atomic rename temp → original
        temp.persist(original_path)?;
    } else {
        // Print final content to stdout
        let final_content = std::fs::read_to_string(temp.path())?;
        print!("{}", final_content);

        // Temp auto-deleted on drop
        drop(temp);
    }

    Ok(())
}

/// Print one file's check results: `✗` per issue, `⚠` for the synthesized
/// unanalyzable record.
fn print_file_results(file_results: &[rule::CheckResult]) {
    for result in file_results {
        if result.has_issue {
            println!(
                "  {} {}",
                "✗".red(),
                result.message.as_ref().unwrap_or(&String::new())
            );
        } else if result.unanalyzable {
            println!(
                "  {} {}",
                "⚠".yellow(),
                result.message.as_ref().unwrap_or(&String::new())
            );
        }
    }
}

fn print_check_summary(
    results: &[rule::CheckResult],
    total_files: usize,
    error_files: &HashSet<String>,
) {
    use std::collections::HashMap;

    // Count files with issues (at least one result with has_issue=true)
    let mut files_with_issues = HashSet::new();
    // Files where requires-parse rules were skipped: not checked, not clean.
    let mut files_unanalyzable = HashSet::new();
    let mut total_issues = 0;

    // Track issues by rule type
    let mut issues_by_rule: HashMap<String, usize> = HashMap::new();
    let mut files_by_rule: HashMap<String, HashSet<String>> = HashMap::new();

    for result in results {
        if result.unanalyzable {
            files_unanalyzable.insert(&result.file_path);
        }
        if result.has_issue {
            files_with_issues.insert(&result.file_path);
            total_issues += result.issue_count;

            // Track by rule
            *issues_by_rule.entry(result.rule_name.clone()).or_insert(0) += result.issue_count;
            files_by_rule
                .entry(result.rule_name.clone())
                .or_default()
                .insert(result.file_path.clone());
        }
    }

    let files_with_issues_count = files_with_issues.len();

    // Clean means: every requested rule ran and none found anything. A file
    // can be in more than one non-clean bucket (e.g. a parse-rule issue plus
    // skipped AST rules), so clean is the complement of the union.
    let mut not_clean: HashSet<&str> = HashSet::new();
    not_clean.extend(files_with_issues.iter().map(|s| s.as_str()));
    not_clean.extend(files_unanalyzable.iter().map(|s| s.as_str()));
    not_clean.extend(error_files.iter().map(|s| s.as_str()));
    let files_clean = total_files - not_clean.len();

    println!("\n{}", "=== Summary ===".bold());
    println!("Total files:         {}", total_files);
    println!(
        "Files with issues:   {} {}",
        files_with_issues_count,
        if files_with_issues_count > 0 {
            "✗".red()
        } else {
            "✓".green()
        }
    );
    if !files_unanalyzable.is_empty() {
        println!(
            "Unanalyzable files:  {} {}",
            files_unanalyzable.len(),
            "⚠".yellow()
        );
    }
    if !error_files.is_empty() {
        println!(
            "Files with errors:   {} {}",
            error_files.len(),
            "⚠".yellow()
        );
    }
    println!("Clean files:         {} {}", files_clean, "✓".green());

    if !issues_by_rule.is_empty() {
        println!("\n{}", "Issues by rule:".bold());
        let mut rule_names: Vec<_> = issues_by_rule.keys().collect();
        rule_names.sort();

        for rule_name in rule_names {
            let count = issues_by_rule[rule_name];
            let file_count = files_by_rule[rule_name].len();
            println!(
                "  {}: {} issue(s) in {} file(s)",
                rule_name.cyan(),
                count,
                file_count
            );
        }
    }

    println!("\nTotal issues found:  {}", total_issues);

    if total_files > 0 {
        let success_rate = (files_clean as f64 / total_files as f64) * 100.0;
        println!("Success rate:        {:.1}%", success_rate);
    }
}
