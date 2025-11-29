//! Generate an HTML report of CSL conformance test status.
//!
//! Usage:
//!   cargo run --bin csl_conformance_report > report.html           # Enabled tests only
//!   cargo run --bin csl_conformance_report -- --all > report.html  # All tests

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;

use quarto_citeproc::output::{
    move_punctuation_inside_quotes, render_blocks_to_csl_html, render_inlines_to_csl_html,
};
use quarto_citeproc::{Citation, CitationItem, Processor, Reference};
use quarto_csl::parse_csl;
use similar::{ChangeTag, TextDiff};

fn main() {
    let args: Vec<String> = env::args().collect();
    let run_all = args.iter().any(|a| a == "--all");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let test_dir = Path::new(manifest_dir).join("test-data/csl-suite");
    let enabled_file = Path::new(manifest_dir).join("tests/enabled_tests.txt");

    // Load enabled tests
    let enabled_tests = load_enabled_tests(&enabled_file);

    // Collect all test files
    let mut test_files: Vec<_> = fs::read_dir(&test_dir)
        .expect("Failed to read test-data/csl-suite")
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "txt")
                .unwrap_or(false)
        })
        .collect();

    test_files.sort_by_key(|e| e.path());

    let total_tests = test_files.len();
    let mut passing = Vec::new();
    let mut failing = Vec::new();
    let mut skipped = Vec::new();

    // Run each test
    for entry in &test_files {
        let path = entry.path();
        let file_name = path.file_stem().unwrap().to_str().unwrap();

        // Check if enabled (or run all)
        let is_enabled = enabled_tests.contains(&file_name.to_lowercase());

        if !run_all && !is_enabled {
            skipped.push(file_name.to_string());
            continue;
        }

        // Parse and run the test
        let content = fs::read_to_string(&path).expect("Failed to read test file");
        match CslTest::parse(file_name, &content) {
            Ok(test) => match run_csl_test(&test) {
                Ok(()) => passing.push(file_name.to_string()),
                Err(result) => failing.push(FailedTest {
                    name: file_name.to_string(),
                    expected: result.expected,
                    actual: result.actual,
                    mode: test.mode,
                }),
            },
            Err(e) => failing.push(FailedTest {
                name: file_name.to_string(),
                expected: String::new(),
                actual: format!("Parse error: {}", e),
                mode: String::new(),
            }),
        }
    }

    // Generate HTML report
    print!("{}", generate_html_report(
        total_tests,
        &passing,
        &failing,
        &skipped,
        run_all,
    ));
}

struct FailedTest {
    name: String,
    expected: String,
    actual: String,
    mode: String,
}

fn generate_html_report(
    total: usize,
    passing: &[String],
    failing: &[FailedTest],
    skipped: &[String],
    run_all: bool,
) -> String {
    let mut html = String::new();

    let title = if run_all {
        "CSL Conformance Test Report (All Tests)"
    } else {
        "CSL Conformance Test Report (Enabled Tests)"
    };

    html.push_str(&format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title}</title>
    <style>
        :root {{
            --bg: #1a1a2e;
            --card-bg: #16213e;
            --text: #eee;
            --text-muted: #888;
            --pass: #4ade80;
            --fail: #f87171;
            --skip: #facc15;
            --border: #333;
        }}
        * {{ box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: var(--bg);
            color: var(--text);
            margin: 0;
            padding: 20px;
            line-height: 1.6;
        }}
        h1 {{ margin-top: 0; }}
        .summary {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
            gap: 16px;
            margin-bottom: 32px;
        }}
        .stat-card {{
            background: var(--card-bg);
            padding: 20px;
            border-radius: 8px;
            text-align: center;
        }}
        .stat-card .number {{
            font-size: 2.5em;
            font-weight: bold;
        }}
        .stat-card .label {{
            color: var(--text-muted);
            font-size: 0.9em;
        }}
        .stat-card.pass .number {{ color: var(--pass); }}
        .stat-card.fail .number {{ color: var(--fail); }}
        .stat-card.skip .number {{ color: var(--skip); }}
        .progress-bar {{
            background: var(--card-bg);
            border-radius: 8px;
            height: 24px;
            overflow: hidden;
            margin-bottom: 32px;
            display: flex;
        }}
        .progress-bar .segment {{
            height: 100%;
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 0.8em;
            font-weight: bold;
        }}
        .progress-bar .pass {{ background: var(--pass); color: #000; }}
        .progress-bar .fail {{ background: var(--fail); color: #000; }}
        .progress-bar .skip {{ background: var(--skip); color: #000; }}
        h2 {{ margin-top: 40px; border-bottom: 1px solid var(--border); padding-bottom: 8px; }}
        .test-list {{
            display: grid;
            grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
            gap: 8px;
        }}
        .test-item {{
            background: var(--card-bg);
            padding: 8px 12px;
            border-radius: 4px;
            font-family: monospace;
            font-size: 0.85em;
        }}
        .failed-test {{
            background: var(--card-bg);
            border-radius: 8px;
            margin-bottom: 16px;
            overflow: hidden;
        }}
        .failed-test-header {{
            padding: 12px 16px;
            font-weight: bold;
            background: rgba(248, 113, 113, 0.1);
            border-left: 4px solid var(--fail);
        }}
        .failed-test .content {{ padding: 16px; }}
        pre.diff {{
            background: #0d1117;
            padding: 12px;
            border-radius: 4px;
            overflow-x: auto;
            font-size: 0.85em;
            margin: 0;
            line-height: 1.4;
        }}
        .diff-del {{
            display: block;
            background: rgba(248, 81, 73, 0.2);
            color: #f85149;
        }}
        .diff-ins {{
            display: block;
            background: rgba(63, 185, 80, 0.2);
            color: #3fb950;
        }}
        .diff-eq {{
            display: block;
            color: var(--text-muted);
        }}
        .mode-badge {{
            display: inline-block;
            padding: 2px 8px;
            border-radius: 4px;
            font-size: 0.75em;
            margin-left: 8px;
            background: var(--border);
        }}
        .collapsible-section summary {{
            cursor: pointer;
            padding: 8px 0;
        }}
    </style>
</head>
<body>
    <h1>{title}</h1>
"#));

    // Summary stats
    let pass_pct = if total > 0 { passing.len() * 100 / total } else { 0 };
    let fail_pct = if total > 0 { failing.len() * 100 / total } else { 0 };
    let skip_pct = if total > 0 { skipped.len() * 100 / total } else { 0 };

    html.push_str(&format!(r#"
    <div class="summary">
        <div class="stat-card">
            <div class="number">{}</div>
            <div class="label">Total Tests</div>
        </div>
        <div class="stat-card pass">
            <div class="number">{}</div>
            <div class="label">Passing</div>
        </div>
        <div class="stat-card fail">
            <div class="number">{}</div>
            <div class="label">Failing</div>
        </div>
        <div class="stat-card skip">
            <div class="number">{}</div>
            <div class="label">Skipped</div>
        </div>
    </div>

    <div class="progress-bar">
        <div class="segment pass" style="width: {}%">{}</div>
        <div class="segment fail" style="width: {}%">{}</div>
        <div class="segment skip" style="width: {}%">{}</div>
    </div>
"#,
        total,
        passing.len(),
        failing.len(),
        skipped.len(),
        pass_pct, passing.len(),
        fail_pct, failing.len(),
        skip_pct, skipped.len(),
    ));

    // Failing tests section
    if !failing.is_empty() {
        html.push_str(&format!("<h2>Failing Tests ({})</h2>\n", failing.len()));
        for test in failing {
            let diff_html = generate_diff_html(&test.expected, &test.actual);
            html.push_str(&format!(r#"
    <div class="failed-test">
        <div class="failed-test-header">{}<span class="mode-badge">{}</span></div>
        <div class="content">
            <pre class="diff">{}</pre>
        </div>
    </div>
"#,
                html_escape(&test.name),
                html_escape(&test.mode),
                diff_html,
            ));
        }
    }

    // Passing tests section (collapsed)
    if !passing.is_empty() {
        html.push_str(&format!(r#"
    <details class="collapsible-section">
        <summary><h2 style="display: inline">Passing Tests ({})</h2></summary>
        <div class="test-list">
"#, passing.len()));
        for name in passing {
            html.push_str(&format!("            <div class=\"test-item\">{}</div>\n", html_escape(name)));
        }
        html.push_str("        </div>\n    </details>\n");
    }

    // Skipped tests section (collapsed)
    if !skipped.is_empty() {
        html.push_str(&format!(r#"
    <details class="collapsible-section">
        <summary><h2 style="display: inline">Skipped Tests ({})</h2></summary>
        <div class="test-list">
"#, skipped.len()));
        for name in skipped {
            html.push_str(&format!("            <div class=\"test-item\">{}</div>\n", html_escape(name)));
        }
        html.push_str("        </div>\n    </details>\n");
    }

    html.push_str("</body>\n</html>\n");
    html
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn generate_diff_html(expected: &str, actual: &str) -> String {
    let diff = TextDiff::from_lines(expected, actual);
    let mut result = String::new();

    for change in diff.iter_all_changes() {
        let (class, prefix) = match change.tag() {
            ChangeTag::Delete => ("diff-del", "-"),
            ChangeTag::Insert => ("diff-ins", "+"),
            ChangeTag::Equal => ("diff-eq", " "),
        };
        let line = html_escape(change.value().trim_end_matches('\n'));
        result.push_str(&format!(
            "<span class=\"{}\">{}{}</span>\n",
            class, prefix, line
        ));
    }

    result
}

fn load_enabled_tests(path: &Path) -> HashSet<String> {
    if !path.exists() {
        return HashSet::new();
    }

    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|s| s.to_lowercase())
        .collect()
}

// ============================================================================
// Test infrastructure (copied from csl_conformance.rs)
// ============================================================================

use std::collections::HashMap;

#[derive(Debug, Clone)]
struct CslTest {
    #[allow(dead_code)]
    name: String,
    mode: String,
    result: String,
    csl: String,
    input: String,
    citation_items: Option<String>,
    citations: Option<String>,
}

impl CslTest {
    fn parse(name: &str, content: &str) -> Result<Self, String> {
        let sections = parse_sections(content);

        let mode = sections
            .get("mode")
            .ok_or("Missing MODE section")?
            .trim()
            .to_string();

        let result = sections
            .get("result")
            .ok_or("Missing RESULT section")?
            .to_string();

        let csl = sections
            .get("csl")
            .ok_or("Missing CSL section")?
            .to_string();

        let input = sections
            .get("input")
            .ok_or("Missing INPUT section")?
            .to_string();

        Ok(CslTest {
            name: name.to_string(),
            mode,
            result,
            csl,
            input,
            citation_items: sections.get("citation-items").cloned(),
            citations: sections.get("citations").cloned(),
        })
    }
}

fn parse_sections(content: &str) -> HashMap<String, String> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);

    let mut sections = HashMap::new();
    let mut current_section: Option<String> = None;
    let mut current_content = String::new();

    for line in content.lines() {
        if line.starts_with(">>") && line.contains("==") && line.ends_with(">>") {
            if let Some(ref name) = current_section {
                sections.insert(name.clone(), current_content.trim_end().to_string());
            }
            let name = extract_section_name(line);
            current_section = Some(name.to_lowercase());
            current_content = String::new();
        } else if line.starts_with("<<") && line.contains("==") && line.ends_with("<<") {
            if let Some(ref name) = current_section {
                sections.insert(name.clone(), current_content.trim_end().to_string());
            }
            current_section = None;
            current_content = String::new();
        } else if current_section.is_some() {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    sections
}

fn extract_section_name(line: &str) -> String {
    let trimmed = line
        .trim_start_matches('>')
        .trim_end_matches('>')
        .trim_start_matches('<')
        .trim_end_matches('<');

    let parts: Vec<&str> = trimmed.split('=').collect();
    for part in parts {
        let part = part.trim();
        if !part.is_empty() {
            return part.to_string();
        }
    }
    String::new()
}

struct TestResult {
    expected: String,
    actual: String,
}

fn run_csl_test(test: &CslTest) -> Result<(), TestResult> {
    let style = parse_csl(&test.csl).map_err(|e| TestResult {
        expected: test.result.clone(),
        actual: format!("CSL parse error: {:?}", e),
    })?;

    let mut processor = Processor::new(style);

    let references: Vec<Reference> = serde_json::from_str(&test.input).map_err(|e| TestResult {
        expected: test.result.clone(),
        actual: format!("Input JSON error: {}", e),
    })?;

    for reference in references.iter() {
        processor.add_reference(reference.clone());
    }

    let citations = build_citations(test, &references).map_err(|e| TestResult {
        expected: test.result.clone(),
        actual: e,
    })?;

    let punct_in_quote = processor.punctuation_in_quote();
    let is_incremental = test.citations.is_some() && is_complex_citations_format(test);

    let actual = match test.mode.as_str() {
        "citation" => {
            let output_asts = processor
                .process_citations_with_disambiguation_to_outputs(&citations)
                .map_err(|e| TestResult {
                    expected: test.result.clone(),
                    actual: format!("Citation error: {:?}", e),
                })?;

            let outputs: Vec<String> = output_asts
                .into_iter()
                .map(|output_ast| {
                    let processed = if punct_in_quote {
                        move_punctuation_inside_quotes(output_ast)
                    } else {
                        output_ast
                    };
                    let inlines = processed.to_inlines();
                    render_inlines_to_csl_html(&inlines)
                })
                .collect();

            if is_incremental {
                format_incremental_output(&outputs)
            } else {
                outputs.join("\n")
            }
        }
        "bibliography" => {
            if !citations.is_empty() {
                let _ = processor.process_citations_with_disambiguation_to_outputs(&citations);
            }

            let entries = processor.generate_bibliography_to_outputs().map_err(|e| TestResult {
                expected: test.result.clone(),
                actual: format!("Bibliography error: {:?}", e),
            })?;

            let outputs: Vec<String> = entries
                .into_iter()
                .map(|(_, output)| {
                    let processed = if punct_in_quote {
                        move_punctuation_inside_quotes(output)
                    } else {
                        output
                    };
                    let blocks = processed.to_blocks();
                    let html = render_blocks_to_csl_html(&blocks);
                    format_bib_entry(&html)
                })
                .collect();
            format_bibliography(&outputs)
        }
        other => {
            return Err(TestResult {
                expected: test.result.clone(),
                actual: format!("Unknown mode: {}", other),
            });
        }
    };

    if actual == test.result {
        Ok(())
    } else {
        Err(TestResult {
            expected: test.result.clone(),
            actual,
        })
    }
}

fn build_citations(test: &CslTest, references: &[Reference]) -> Result<Vec<Citation>, String> {
    if let Some(ref citations_json) = test.citations {
        let raw: serde_json::Value = serde_json::from_str(citations_json)
            .map_err(|e| format!("Citations JSON error: {}", e))?;

        if let Some(outer_array) = raw.as_array() {
            let is_complex_format = outer_array.first().map_or(false, |first| {
                if let Some(arr) = first.as_array() {
                    arr.first()
                        .and_then(|obj| obj.get("citationItems"))
                        .is_some()
                } else {
                    false
                }
            });

            if is_complex_format {
                return parse_complex_citations_format(outer_array);
            } else {
                return parse_simple_citations_format(outer_array);
            }
        }

        return Err("Citations must be an array".to_string());
    }

    if let Some(ref items_json) = test.citation_items {
        let raw: Vec<Vec<serde_json::Value>> = serde_json::from_str(items_json)
            .map_err(|e| format!("Citation-items JSON error: {}", e))?;

        let mut citations = Vec::new();
        for cite_group in raw {
            let items: Vec<CitationItem> = cite_group
                .iter()
                .filter_map(|v| parse_citation_item(v))
                .collect();

            if !items.is_empty() {
                citations.push(Citation {
                    items,
                    ..Default::default()
                });
            }
        }

        return Ok(citations);
    }

    let items: Vec<CitationItem> = references
        .iter()
        .map(|r| CitationItem {
            id: r.id.clone(),
            ..Default::default()
        })
        .collect();

    Ok(vec![Citation {
        items,
        ..Default::default()
    }])
}

fn parse_complex_citations_format(outer_array: &[serde_json::Value]) -> Result<Vec<Citation>, String> {
    let mut citations = Vec::new();

    for entry in outer_array {
        let entry_array = entry.as_array().ok_or("CITATIONS entry must be an array")?;
        let citation_obj = entry_array.first().ok_or("CITATIONS entry must have citation object")?;

        let citation_id = citation_obj
            .get("citationID")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let note_number = citation_obj
            .get("properties")
            .and_then(|p| p.get("noteIndex"))
            .and_then(|n| n.as_i64())
            .map(|n| n as i32);

        let citation_items_array = citation_obj
            .get("citationItems")
            .and_then(|v| v.as_array())
            .ok_or("CITATIONS entry must have citationItems array")?;

        let items: Vec<CitationItem> = citation_items_array
            .iter()
            .filter_map(|v| parse_citation_item(v))
            .collect();

        if !items.is_empty() {
            citations.push(Citation {
                id: citation_id,
                note_number,
                items,
            });
        }
    }

    Ok(citations)
}

fn parse_simple_citations_format(outer_array: &[serde_json::Value]) -> Result<Vec<Citation>, String> {
    let mut citations = Vec::new();

    for cite_group in outer_array {
        let group_array = cite_group.as_array().ok_or("Citation group must be an array")?;

        let items: Vec<CitationItem> = group_array
            .iter()
            .filter_map(|v| parse_citation_item(v))
            .collect();

        if !items.is_empty() {
            citations.push(Citation {
                items,
                ..Default::default()
            });
        }
    }

    Ok(citations)
}

fn is_complex_citations_format(test: &CslTest) -> bool {
    if let Some(ref citations_json) = test.citations {
        if let Ok(raw) = serde_json::from_str::<serde_json::Value>(citations_json) {
            if let Some(outer_array) = raw.as_array() {
                return outer_array.first().map_or(false, |first| {
                    if let Some(arr) = first.as_array() {
                        arr.first()
                            .and_then(|obj| obj.get("citationItems"))
                            .is_some()
                    } else {
                        false
                    }
                });
            }
        }
    }
    false
}

fn format_incremental_output(outputs: &[String]) -> String {
    if outputs.is_empty() {
        return String::new();
    }

    let last_idx = outputs.len() - 1;
    outputs
        .iter()
        .enumerate()
        .map(|(i, output)| {
            let marker = if i == last_idx { ">>" } else { ".." };
            format!("{}[{}] {}", marker, i, output)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_citation_item(v: &serde_json::Value) -> Option<CitationItem> {
    let id = match v.get("id")? {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };

    Some(CitationItem {
        id,
        locator: v.get("locator").and_then(|l| l.as_str()).map(|s| s.to_string()),
        label: v.get("label").and_then(|l| l.as_str()).map(|s| s.to_string()),
        prefix: v.get("prefix").and_then(|p| p.as_str()).map(|s| s.to_string()),
        suffix: v.get("suffix").and_then(|s| s.as_str()).map(|s| s.to_string()),
        position: v.get("position").and_then(|p| p.as_i64()).map(|n| n as i32),
        ..Default::default()
    })
}

fn format_bib_entry(entry: &str) -> String {
    if entry.contains("class=\"csl-left-margin\"") || entry.contains("class=\"csl-right-inline\"") {
        format!("  <div class=\"csl-entry\">\n    {}\n  </div>", entry)
    } else if entry.contains("class=\"csl-indent\"") || entry.contains("class=\"csl-block\"") {
        format!("  <div class=\"csl-entry\">{}\n  </div>", entry)
    } else {
        format!("  <div class=\"csl-entry\">{}</div>", entry)
    }
}

fn format_bibliography(entries: &[String]) -> String {
    let mut output = String::from("<div class=\"csl-bib-body\">\n");
    for entry in entries {
        output.push_str(entry);
        output.push('\n');
    }
    output.push_str("</div>");
    output
}
