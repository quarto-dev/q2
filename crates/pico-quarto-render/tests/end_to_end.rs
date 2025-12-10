/*
 * end_to_end.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * End-to-end tests for pico-quarto-render HTML output.
 */

use std::path::Path;

/// Helper to get the path to test fixtures
fn fixture_path(name: &str) -> std::path::PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("tests/fixtures").join(name)
}

/// Render a QMD file to HTML using the full pipeline.
fn render_qmd_to_html(fixture_name: &str) -> String {
    use quarto_doctemplate::Template;
    use std::fs;

    // These are the same modules used in main.rs, but we access them via the crate
    // Since they're private, we'll replicate the minimal logic here for testing

    let qmd_path = fixture_path(fixture_name);
    let input_content = fs::read(&qmd_path).expect("Failed to read fixture");

    // Parse QMD
    let mut output_stream = std::io::sink();
    let (mut pandoc, _context, _warnings) = pampa::readers::qmd::read(
        &input_content,
        false,
        qmd_path.to_str().unwrap(),
        &mut output_stream,
        true,
        None,
    )
    .expect("Failed to parse QMD");

    // Prepare template metadata (adds pagetitle from title)
    prepare_template_metadata(&mut pandoc);

    // Load template from embedded resources
    // For tests, we'll compile a minimal template inline
    let template_source = r#"<!DOCTYPE html>
<html>
<head>
<title>$pagetitle$</title>
</head>
<body>
$if(title)$
<h1 class="title">$title$</h1>
$endif$
$body$
</body>
</html>"#;

    let template = Template::compile(template_source).expect("Failed to compile template");

    // Convert metadata and render
    let writers = HtmlWriters;
    render_with_template(&pandoc, &template, &writers).expect("Failed to render")
}

// Re-implement the minimal functions needed for testing
// (In a real scenario, these would be exposed from the crate)

use pampa::pandoc::{MetaMapEntry, MetaValueWithSourceInfo, Pandoc};

fn prepare_template_metadata(pandoc: &mut Pandoc) {
    let MetaValueWithSourceInfo::MetaMap {
        entries,
        source_info,
    } = &mut pandoc.meta
    else {
        return;
    };

    let has_pagetitle = entries.iter().any(|e| e.key == "pagetitle");
    if has_pagetitle {
        return;
    }

    let title_entry = entries.iter().find(|e| e.key == "title");
    if let Some(entry) = title_entry {
        let plain_text = match &entry.value {
            MetaValueWithSourceInfo::MetaString { value, .. } => value.clone(),
            MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                let (text, _) = pampa::writers::plaintext::inlines_to_string(content);
                text
            }
            MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
                let (text, _) = pampa::writers::plaintext::blocks_to_string(content);
                text
            }
            _ => return,
        };

        entries.push(MetaMapEntry {
            key: "pagetitle".to_string(),
            key_source: source_info.clone(),
            value: MetaValueWithSourceInfo::MetaString {
                value: plain_text,
                source_info: source_info.clone(),
            },
        });
    }
}

use pampa::pandoc::block::Block;
use pampa::pandoc::inline::Inlines;
use quarto_doctemplate::{Template, TemplateContext, TemplateValue};
use std::collections::HashMap;

struct HtmlWriters;

impl HtmlWriters {
    fn write_blocks(&self, blocks: &[Block]) -> anyhow::Result<String> {
        let mut buf = Vec::new();
        pampa::writers::html::write_blocks(blocks, &mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn write_inlines(&self, inlines: &Inlines) -> anyhow::Result<String> {
        let mut buf = Vec::new();
        pampa::writers::html::write_inlines(inlines, &mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

fn meta_to_template_value(
    meta: &MetaValueWithSourceInfo,
    writers: &HtmlWriters,
) -> anyhow::Result<TemplateValue> {
    Ok(match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => TemplateValue::String(value.clone()),
        MetaValueWithSourceInfo::MetaBool { value, .. } => TemplateValue::Bool(*value),
        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
            TemplateValue::String(writers.write_inlines(content)?)
        }
        MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
            TemplateValue::String(writers.write_blocks(content)?)
        }
        MetaValueWithSourceInfo::MetaList { items, .. } => {
            let values: anyhow::Result<Vec<_>> = items
                .iter()
                .map(|item| meta_to_template_value(item, writers))
                .collect();
            TemplateValue::List(values?)
        }
        MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            let mut map = HashMap::new();
            for entry in entries {
                map.insert(
                    entry.key.clone(),
                    meta_to_template_value(&entry.value, writers)?,
                );
            }
            TemplateValue::Map(map)
        }
    })
}

fn render_with_template(
    pandoc: &Pandoc,
    template: &Template,
    writers: &HtmlWriters,
) -> anyhow::Result<String> {
    let meta_value = meta_to_template_value(&pandoc.meta, writers)?;

    let mut context = TemplateContext::new();
    if let TemplateValue::Map(map) = meta_value {
        for (key, value) in map {
            context.insert(key, value);
        }
    }

    let body = writers.write_blocks(&pandoc.blocks)?;
    context.insert("body", TemplateValue::String(body));

    let output = template
        .render(&context)
        .map_err(|e| anyhow::anyhow!("Template error: {:?}", e))?;

    Ok(output)
}

#[test]
fn test_simple_document() {
    let html = render_qmd_to_html("simple.qmd");

    // Check document structure
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("<title>Simple Test</title>"));
    assert!(html.contains("<h1 class=\"title\">Simple Test</h1>"));
    assert!(html.contains("This is a simple test document."));
}

#[test]
fn test_document_with_formatting() {
    let html = render_qmd_to_html("with-formatting.qmd");

    // Check title
    assert!(html.contains("<title>Formatting Test</title>"));

    // Check inline formatting
    assert!(html.contains("<em>emphasis</em>"));
    assert!(html.contains("<strong>strong</strong>"));

    // Check heading
    assert!(html.contains("<h2"));
    assert!(html.contains("A Heading"));
}

#[test]
fn test_document_without_title() {
    let html = render_qmd_to_html("no-title.qmd");

    // Should still produce valid HTML
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("<title></title>")); // Empty title

    // Should NOT have title block
    assert!(!html.contains("<h1 class=\"title\">"));

    // Should have content
    assert!(html.contains("Just some content"));
}
