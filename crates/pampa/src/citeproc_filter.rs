/*
 * citeproc_filter.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Built-in citeproc filter for citation processing.
 *
 * This filter processes citations in the document using quarto-citeproc,
 * replacing Cite inlines with rendered citation text and optionally
 * appending a bibliography section.
 */

use std::path::Path;

use quarto_citeproc::{Citation, CitationItem, Processor, Reference};
use quarto_csl::parse_csl;
use quarto_error_reporting::DiagnosticMessage;

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::{Block, Div, Inline, Pandoc};
use crate::unified_filter::CiteprocFilterError;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind};

/// Default CSL style (Chicago Manual of Style, author-date format).
const DEFAULT_CSL_STYLE: &str = include_str!("../resources/csl/chicago-author-date.csl");

/// Configuration for the citeproc filter, extracted from document metadata.
#[derive(Debug)]
pub struct CiteprocConfig {
    /// Path to CSL style file.
    pub csl: Option<String>,
    /// Paths to bibliography files (CSL-JSON format).
    pub bibliography: Vec<String>,
    /// Inline references from document metadata.
    pub references: Vec<Reference>,
    /// Document language for locale selection.
    pub lang: Option<String>,
    /// Whether to wrap citations in hyperlinks to bibliography.
    pub link_citations: bool,
    /// Whether to add URLs/DOIs as links in bibliography.
    pub link_bibliography: bool,
    /// Reference IDs to include in bibliography without citing.
    pub nocite: Vec<String>,
    /// Whether to suppress bibliography output.
    pub suppress_bibliography: bool,
}

impl Default for CiteprocConfig {
    fn default() -> Self {
        Self {
            csl: None,
            bibliography: Vec::new(),
            references: Vec::new(),
            lang: None,
            link_citations: false,
            link_bibliography: true, // Default to true (Pandoc's behavior)
            nocite: Vec::new(),
            suppress_bibliography: false,
        }
    }
}

/// Apply the citeproc filter to a document.
///
/// This is the main entry point for citation processing. It:
/// 1. Extracts configuration from document metadata
/// 2. Loads the CSL style and bibliography references
/// 3. Processes all Cite inlines in the document
/// 4. Appends a bibliography section (unless suppressed)
pub fn apply_citeproc_filter(
    pandoc: Pandoc,
    context: ASTContext,
    _target_format: &str,
) -> Result<(Pandoc, ASTContext, Vec<DiagnosticMessage>), CiteprocFilterError> {
    // Extract configuration from document metadata
    let config = extract_config(&pandoc);

    // If no bibliography or references are specified, pass through unchanged
    if config.bibliography.is_empty() && config.references.is_empty() {
        return Ok((pandoc, context, vec![]));
    }

    // Load CSL style
    let style = load_csl_style(&config)?;

    // Create processor
    let mut processor = Processor::new(style);

    // Add inline references from metadata
    if !config.references.is_empty() {
        processor.add_references(config.references.clone());
    }

    // Load bibliography references from files
    for bib_path in &config.bibliography {
        let references = load_bibliography(bib_path)?;
        processor.add_references(references);
    }

    // Collect all citations from the document
    let citations = collect_citations(&pandoc);

    // Process citations with disambiguation
    let rendered_citations = processor
        .process_citations_with_disambiguation(&citations)
        .map_err(|e| CiteprocFilterError::ProcessingError(e.to_string()))?;

    // Build a map from citation index to rendered output
    let citation_outputs: Vec<_> = citations.iter().zip(rendered_citations.iter()).collect();

    // Transform the document, replacing Cite inlines with rendered content
    let mut pandoc = pandoc;
    let mut citation_index = 0;
    transform_blocks(
        &mut pandoc.blocks,
        &citation_outputs,
        &mut citation_index,
        &processor,
    );

    // Generate bibliography if not suppressed
    if !config.suppress_bibliography {
        let bib_blocks = generate_bibliography(&mut processor)?;
        if !bib_blocks.is_empty() {
            insert_bibliography(&mut pandoc.blocks, bib_blocks);
        }
    }

    Ok((pandoc, context, vec![]))
}

/// Load the CSL style from file or use the default.
fn load_csl_style(config: &CiteprocConfig) -> Result<quarto_csl::Style, CiteprocFilterError> {
    let csl_content = if let Some(ref csl_path) = config.csl {
        let path = Path::new(csl_path);
        std::fs::read_to_string(path)
            .map_err(|e| CiteprocFilterError::StyleNotFound(path.to_owned(), e))?
    } else {
        DEFAULT_CSL_STYLE.to_string()
    };

    parse_csl(&csl_content).map_err(|e| {
        let path = config.csl.as_ref().map_or_else(
            || Path::new("<default>").to_owned(),
            |s| Path::new(s).to_owned(),
        );
        CiteprocFilterError::StyleParseError(path, e.to_string())
    })
}

/// Load bibliography references from a CSL-JSON file.
fn load_bibliography(path: &str) -> Result<Vec<Reference>, CiteprocFilterError> {
    let path = Path::new(path);
    let content = std::fs::read_to_string(path)
        .map_err(|e| CiteprocFilterError::BibliographyNotFound(path.to_owned(), e))?;

    // Parse as JSON array of references
    let references: Vec<Reference> = serde_json::from_str(&content)
        .map_err(|e| CiteprocFilterError::BibliographyParseError(path.to_owned(), e.to_string()))?;

    Ok(references)
}

/// Collect all citations from the document.
fn collect_citations(pandoc: &Pandoc) -> Vec<Citation> {
    let mut citations = Vec::new();
    let mut note_number = 1;

    for block in &pandoc.blocks {
        collect_citations_from_block(block, &mut citations, &mut note_number);
    }

    citations
}

/// Collect citations from a block.
fn collect_citations_from_block(
    block: &Block,
    citations: &mut Vec<Citation>,
    note_number: &mut i32,
) {
    match block {
        Block::Paragraph(p) => {
            collect_citations_from_inlines(&p.content, citations, note_number);
        }
        Block::Plain(p) => {
            collect_citations_from_inlines(&p.content, citations, note_number);
        }
        Block::Header(h) => {
            collect_citations_from_inlines(&h.content, citations, note_number);
        }
        Block::BlockQuote(bq) => {
            for b in &bq.content {
                collect_citations_from_block(b, citations, note_number);
            }
        }
        Block::OrderedList(ol) => {
            for item in &ol.content {
                for b in item {
                    collect_citations_from_block(b, citations, note_number);
                }
            }
        }
        Block::BulletList(bl) => {
            for item in &bl.content {
                for b in item {
                    collect_citations_from_block(b, citations, note_number);
                }
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &dl.content {
                collect_citations_from_inlines(term, citations, note_number);
                for def in defs {
                    for b in def {
                        collect_citations_from_block(b, citations, note_number);
                    }
                }
            }
        }
        Block::Div(d) => {
            for b in &d.content {
                collect_citations_from_block(b, citations, note_number);
            }
        }
        Block::Figure(f) => {
            if let Some(ref short) = f.caption.short {
                collect_citations_from_inlines(short, citations, note_number);
            }
            if let Some(ref long) = f.caption.long {
                for b in long {
                    collect_citations_from_block(b, citations, note_number);
                }
            }
            for b in &f.content {
                collect_citations_from_block(b, citations, note_number);
            }
        }
        Block::Table(t) => {
            // Table caption
            if let Some(ref short) = t.caption.short {
                collect_citations_from_inlines(short, citations, note_number);
            }
            if let Some(ref long) = t.caption.long {
                for b in long {
                    collect_citations_from_block(b, citations, note_number);
                }
            }
            // Table cells
            for row in &t.head.rows {
                for cell in &row.cells {
                    for b in &cell.content {
                        collect_citations_from_block(b, citations, note_number);
                    }
                }
            }
            for body in &t.bodies {
                for row in &body.body {
                    for cell in &row.cells {
                        for b in &cell.content {
                            collect_citations_from_block(b, citations, note_number);
                        }
                    }
                }
            }
            for row in &t.foot.rows {
                for cell in &row.cells {
                    for b in &cell.content {
                        collect_citations_from_block(b, citations, note_number);
                    }
                }
            }
        }
        Block::LineBlock(lb) => {
            for line in &lb.content {
                collect_citations_from_inlines(line, citations, note_number);
            }
        }
        _ => {}
    }
}

/// Collect citations from inlines.
fn collect_citations_from_inlines(
    inlines: &[Inline],
    citations: &mut Vec<Citation>,
    note_number: &mut i32,
) {
    for inline in inlines {
        match inline {
            Inline::Cite(cite) => {
                // Convert our Citation type to quarto_citeproc's Citation
                let items: Vec<CitationItem> = cite
                    .citations
                    .iter()
                    .map(|c| CitationItem {
                        id: c.id.clone(),
                        locator: None, // TODO: Extract locator from suffix
                        label: None,
                        prefix: if c.prefix.is_empty() {
                            None
                        } else {
                            Some(inlines_to_text(&c.prefix))
                        },
                        suffix: if c.suffix.is_empty() {
                            None
                        } else {
                            Some(inlines_to_text(&c.suffix))
                        },
                        suppress_author: Some(matches!(
                            c.mode,
                            crate::pandoc::CitationMode::SuppressAuthor
                        )),
                        author_only: Some(matches!(
                            c.mode,
                            crate::pandoc::CitationMode::AuthorInText
                        )),
                        position: None,
                    })
                    .collect();

                citations.push(Citation {
                    id: None,
                    note_number: Some(*note_number),
                    items,
                });
                *note_number += 1;
            }
            Inline::Emph(e) => collect_citations_from_inlines(&e.content, citations, note_number),
            Inline::Strong(s) => collect_citations_from_inlines(&s.content, citations, note_number),
            Inline::Underline(u) => {
                collect_citations_from_inlines(&u.content, citations, note_number)
            }
            Inline::Strikeout(s) => {
                collect_citations_from_inlines(&s.content, citations, note_number)
            }
            Inline::Superscript(s) => {
                collect_citations_from_inlines(&s.content, citations, note_number)
            }
            Inline::Subscript(s) => {
                collect_citations_from_inlines(&s.content, citations, note_number)
            }
            Inline::SmallCaps(s) => {
                collect_citations_from_inlines(&s.content, citations, note_number)
            }
            Inline::Quoted(q) => collect_citations_from_inlines(&q.content, citations, note_number),
            Inline::Link(l) => collect_citations_from_inlines(&l.content, citations, note_number),
            Inline::Span(s) => collect_citations_from_inlines(&s.content, citations, note_number),
            Inline::Note(n) => {
                for b in &n.content {
                    collect_citations_from_block(b, citations, note_number);
                }
            }
            _ => {}
        }
    }
}

/// Transform blocks, replacing Cite inlines with rendered content.
fn transform_blocks(
    blocks: &mut Vec<Block>,
    citation_outputs: &[(&Citation, &String)],
    citation_index: &mut usize,
    processor: &Processor,
) {
    for block in blocks.iter_mut() {
        transform_block(block, citation_outputs, citation_index, processor);
    }
}

/// Transform a single block.
fn transform_block(
    block: &mut Block,
    citation_outputs: &[(&Citation, &String)],
    citation_index: &mut usize,
    processor: &Processor,
) {
    match block {
        Block::Paragraph(p) => {
            transform_inlines(&mut p.content, citation_outputs, citation_index, processor);
        }
        Block::Plain(p) => {
            transform_inlines(&mut p.content, citation_outputs, citation_index, processor);
        }
        Block::Header(h) => {
            transform_inlines(&mut h.content, citation_outputs, citation_index, processor);
        }
        Block::BlockQuote(bq) => {
            transform_blocks(&mut bq.content, citation_outputs, citation_index, processor);
        }
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                transform_blocks(item, citation_outputs, citation_index, processor);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                transform_blocks(item, citation_outputs, citation_index, processor);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &mut dl.content {
                transform_inlines(term, citation_outputs, citation_index, processor);
                for def in defs {
                    transform_blocks(def, citation_outputs, citation_index, processor);
                }
            }
        }
        Block::Div(d) => {
            transform_blocks(&mut d.content, citation_outputs, citation_index, processor);
        }
        Block::Figure(f) => {
            if let Some(ref mut short) = f.caption.short {
                transform_inlines(short, citation_outputs, citation_index, processor);
            }
            if let Some(ref mut long) = f.caption.long {
                transform_blocks(long, citation_outputs, citation_index, processor);
            }
            transform_blocks(&mut f.content, citation_outputs, citation_index, processor);
        }
        Block::Table(t) => {
            if let Some(ref mut short) = t.caption.short {
                transform_inlines(short, citation_outputs, citation_index, processor);
            }
            if let Some(ref mut long) = t.caption.long {
                transform_blocks(long, citation_outputs, citation_index, processor);
            }
            for row in &mut t.head.rows {
                for cell in &mut row.cells {
                    transform_blocks(
                        &mut cell.content,
                        citation_outputs,
                        citation_index,
                        processor,
                    );
                }
            }
            for body in &mut t.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        transform_blocks(
                            &mut cell.content,
                            citation_outputs,
                            citation_index,
                            processor,
                        );
                    }
                }
            }
            for row in &mut t.foot.rows {
                for cell in &mut row.cells {
                    transform_blocks(
                        &mut cell.content,
                        citation_outputs,
                        citation_index,
                        processor,
                    );
                }
            }
        }
        Block::LineBlock(lb) => {
            for line in &mut lb.content {
                transform_inlines(line, citation_outputs, citation_index, processor);
            }
        }
        _ => {}
    }
}

/// Transform inlines, replacing Cite with rendered content.
fn transform_inlines(
    inlines: &mut Vec<Inline>,
    citation_outputs: &[(&Citation, &String)],
    citation_index: &mut usize,
    processor: &Processor,
) {
    let mut i = 0;
    while i < inlines.len() {
        match &mut inlines[i] {
            Inline::Cite(_) => {
                if *citation_index < citation_outputs.len() {
                    let (citation, rendered) = citation_outputs[*citation_index];
                    *citation_index += 1;

                    // Get the Output AST for this citation to convert to Inlines
                    // For now, we use the rendered string and create a simple Str inline
                    // TODO: Use processor.process_citation_to_output() for proper Inline conversion
                    let _ = citation; // silence unused warning
                    let _ = processor; // silence unused warning

                    let replacement = Inline::Str(crate::pandoc::Str {
                        text: rendered.clone(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    });
                    inlines[i] = replacement;
                }
                i += 1;
            }
            Inline::Emph(e) => {
                transform_inlines(&mut e.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            Inline::Strong(s) => {
                transform_inlines(&mut s.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            Inline::Underline(u) => {
                transform_inlines(&mut u.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            Inline::Strikeout(s) => {
                transform_inlines(&mut s.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            Inline::Superscript(s) => {
                transform_inlines(&mut s.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            Inline::Subscript(s) => {
                transform_inlines(&mut s.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            Inline::SmallCaps(s) => {
                transform_inlines(&mut s.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            Inline::Quoted(q) => {
                transform_inlines(&mut q.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            Inline::Link(l) => {
                transform_inlines(&mut l.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            Inline::Span(s) => {
                transform_inlines(&mut s.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            Inline::Note(n) => {
                transform_blocks(&mut n.content, citation_outputs, citation_index, processor);
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Generate bibliography blocks.
fn generate_bibliography(processor: &mut Processor) -> Result<Vec<Block>, CiteprocFilterError> {
    let entries = processor
        .generate_bibliography_to_outputs()
        .map_err(|e| CiteprocFilterError::ProcessingError(e.to_string()))?;

    if entries.is_empty() {
        return Ok(vec![]);
    }

    // Convert each entry to blocks
    let mut bib_blocks = Vec::new();
    for (id, output) in entries {
        let blocks = output.to_blocks();
        if !blocks.is_empty() {
            // Wrap each entry in a Div with the reference ID
            // Attr is a tuple: (id, classes, attributes)
            let entry_div = Block::Div(Div {
                attr: (
                    format!("ref-{}", id),
                    vec!["csl-entry".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                content: blocks,
                source_info: quarto_source_map::SourceInfo::default(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
            });
            bib_blocks.push(entry_div);
        }
    }

    Ok(bib_blocks)
}

/// Insert bibliography into the document.
///
/// If a Div with id="refs" exists, replace its contents.
/// Otherwise, append a new Div at the end of the document.
fn insert_bibliography(blocks: &mut Vec<Block>, bib_blocks: Vec<Block>) {
    // Look for existing #refs div
    // Attr is a tuple: (id, classes, attributes)
    for block in blocks.iter_mut() {
        if let Block::Div(d) = block
            && d.attr.0 == "refs"
        {
            // Replace contents of existing #refs div
            d.content = bib_blocks;
            // Add required classes if not present
            if !d.attr.1.contains(&"references".to_string()) {
                d.attr.1.push("references".to_string());
            }
            if !d.attr.1.contains(&"csl-bib-body".to_string()) {
                d.attr.1.push("csl-bib-body".to_string());
            }
            return;
        }
    }

    // No #refs div found, create one at the end
    let refs_div = Block::Div(Div {
        attr: (
            "refs".to_string(),
            vec!["references".to_string(), "csl-bib-body".to_string()],
            hashlink::LinkedHashMap::new(),
        ),
        content: bib_blocks,
        source_info: quarto_source_map::SourceInfo::default(),
        attr_source: crate::pandoc::AttrSourceInfo::empty(),
    });
    blocks.push(refs_div);
}

/// Extract citeproc configuration from document metadata.
fn extract_config(pandoc: &Pandoc) -> CiteprocConfig {
    let meta = &pandoc.meta;
    let mut config = CiteprocConfig::default();

    // Helper to get a string value from metadata
    fn get_meta_string(meta: &ConfigValue, key: &str) -> Option<String> {
        if let ConfigValueKind::Map(entries) = &meta.value {
            for entry in entries {
                if entry.key == key {
                    // Try as string first
                    if let Some(s) = entry.value.as_str() {
                        return Some(s.to_string());
                    }
                    // Try as inlines
                    if let ConfigValueKind::PandocInlines(content) = &entry.value.value {
                        return Some(inlines_to_text(content));
                    }
                    return None;
                }
            }
        }
        None
    }

    // Helper to get a boolean value from metadata
    fn get_meta_bool(meta: &ConfigValue, key: &str) -> Option<bool> {
        if let ConfigValueKind::Map(entries) = &meta.value {
            for entry in entries {
                if entry.key == key {
                    if let ConfigValueKind::Scalar(yaml_rust2::Yaml::Boolean(value)) =
                        &entry.value.value
                    {
                        return Some(*value);
                    }
                    return None;
                }
            }
        }
        None
    }

    // Helper to get a string list from metadata
    fn get_meta_string_list(meta: &ConfigValue, key: &str) -> Vec<String> {
        if let ConfigValueKind::Map(entries) = &meta.value {
            for entry in entries {
                if entry.key == key {
                    // Try as string first
                    if let Some(s) = entry.value.as_str() {
                        return vec![s.to_string()];
                    }
                    // Try as inlines
                    if let ConfigValueKind::PandocInlines(content) = &entry.value.value {
                        return vec![inlines_to_text(content)];
                    }
                    // Try as array
                    if let ConfigValueKind::Array(items) = &entry.value.value {
                        return items
                            .iter()
                            .filter_map(|item| {
                                if let Some(s) = item.as_str() {
                                    return Some(s.to_string());
                                }
                                if let ConfigValueKind::PandocInlines(content) = &item.value {
                                    return Some(inlines_to_text(content));
                                }
                                None
                            })
                            .collect();
                    }
                    return vec![];
                }
            }
        }
        vec![]
    }

    // Extract configuration values
    config.csl = get_meta_string(meta, "csl");
    config.bibliography = get_meta_string_list(meta, "bibliography");
    config.lang = get_meta_string(meta, "lang");
    config.link_citations = get_meta_bool(meta, "link-citations").unwrap_or(false);
    config.link_bibliography = get_meta_bool(meta, "link-bibliography").unwrap_or(true);
    config.nocite = get_meta_string_list(meta, "nocite");
    config.suppress_bibliography = get_meta_bool(meta, "suppress-bibliography").unwrap_or(false);

    // Extract inline references from metadata
    config.references = extract_references(meta);

    config
}

/// Extract inline references from the 'references' metadata field.
fn extract_references(meta: &ConfigValue) -> Vec<Reference> {
    let ConfigValueKind::Map(entries) = &meta.value else {
        return vec![];
    };

    let references_list = entries
        .iter()
        .find(|e| e.key == "references")
        .and_then(|e| {
            if let ConfigValueKind::Array(items) = &e.value.value {
                Some(items)
            } else {
                None
            }
        });

    let Some(items) = references_list else {
        return vec![];
    };

    items.iter().filter_map(meta_to_reference).collect()
}

/// Convert a metadata map to a Reference.
fn meta_to_reference(meta: &ConfigValue) -> Option<Reference> {
    use quarto_citeproc::reference::StringOrNumber;

    let ConfigValueKind::Map(entries) = &meta.value else {
        return None;
    };

    // Helper to get a string from an entry
    let get_string = |key: &str| -> Option<String> {
        entries.iter().find(|e| e.key == key).and_then(|e| {
            if let Some(s) = e.value.as_str() {
                return Some(s.to_string());
            }
            if let ConfigValueKind::PandocInlines(content) = &e.value.value {
                return Some(inlines_to_text(content));
            }
            None
        })
    };

    // Get the required ID field
    let id = get_string("id")?;

    // Get the type field (defaults to "article")
    let ref_type = get_string("type").unwrap_or_else(|| "article".to_string());

    // Build the reference with direct field assignment
    let reference = Reference {
        id,
        ref_type,
        title: get_string("title"),
        title_short: get_string("title-short"),
        container_title: get_string("container-title"),
        container_title_short: get_string("container-title-short"),
        collection_title: get_string("collection-title"),
        publisher: get_string("publisher"),
        publisher_place: get_string("publisher-place"),
        edition: get_string("edition").map(StringOrNumber::String),
        volume: get_string("volume").map(StringOrNumber::String),
        issue: get_string("issue").map(StringOrNumber::String),
        page: get_string("page"),
        page_first: get_string("page-first"),
        number_of_pages: get_string("number-of-pages").map(StringOrNumber::String),
        chapter: get_string("chapter").map(StringOrNumber::String),
        abstract_: get_string("abstract"),
        doi: get_string("DOI"),
        isbn: get_string("ISBN"),
        issn: get_string("ISSN"),
        url: get_string("URL"),
        note: get_string("note"),
        language: get_string("language"),
        source: get_string("source"),
        author: extract_names(entries, "author"),
        editor: extract_names(entries, "editor"),
        translator: extract_names(entries, "translator"),
        container_author: extract_names(entries, "container-author"),
        collection_editor: extract_names(entries, "collection-editor"),
        director: extract_names(entries, "director"),
        interviewer: extract_names(entries, "interviewer"),
        recipient: extract_names(entries, "recipient"),
        reviewed_author: extract_names(entries, "reviewed-author"),
        composer: extract_names(entries, "composer"),
        issued: extract_date(entries, "issued"),
        accessed: extract_date(entries, "accessed"),
        event_date: extract_date(entries, "event-date"),
        original_date: extract_date(entries, "original-date"),
        submitted: extract_date(entries, "submitted"),
        other: hashlink::LinkedHashMap::new(),
        disambiguation: None,
    };

    Some(reference)
}

/// Extract names from a metadata entry.
fn extract_names(
    entries: &[ConfigMapEntry],
    key: &str,
) -> Option<Vec<quarto_citeproc::reference::Name>> {
    use quarto_citeproc::reference::Name;

    let entry = entries.iter().find(|e| e.key == key)?;

    let ConfigValueKind::Array(names_list) = &entry.value.value else {
        return None;
    };

    let names: Vec<Name> = names_list
        .iter()
        .filter_map(|item| {
            let ConfigValueKind::Map(name_entries) = &item.value else {
                return None;
            };

            let get_name_part = |key: &str| -> Option<String> {
                name_entries.iter().find(|e| e.key == key).and_then(|e| {
                    if let Some(s) = e.value.as_str() {
                        return Some(s.to_string());
                    }
                    if let ConfigValueKind::PandocInlines(content) = &e.value.value {
                        return Some(inlines_to_text(content));
                    }
                    None
                })
            };

            let family = get_name_part("family");
            let given = get_name_part("given");
            let literal = get_name_part("literal");

            if family.is_some() || given.is_some() || literal.is_some() {
                Some(Name {
                    family,
                    given,
                    literal,
                    dropping_particle: get_name_part("dropping-particle"),
                    non_dropping_particle: get_name_part("non-dropping-particle"),
                    suffix: get_name_part("suffix"),
                    comma_suffix: None,
                    static_ordering: None,
                    parse_names: None,
                })
            } else {
                None
            }
        })
        .collect();

    if names.is_empty() { None } else { Some(names) }
}

/// Extract a date from a metadata entry.
fn extract_date(
    entries: &[ConfigMapEntry],
    key: &str,
) -> Option<quarto_citeproc::reference::DateVariable> {
    use quarto_citeproc::reference::DateVariable;

    let entry = entries.iter().find(|e| e.key == key)?;

    let ConfigValueKind::Map(date_entries) = &entry.value.value else {
        return None;
    };

    // Look for date-parts
    let date_parts_entry = date_entries.iter().find(|e| e.key == "date-parts")?;
    let ConfigValueKind::Array(outer_list) = &date_parts_entry.value.value else {
        return None;
    };

    // date-parts is a list of lists: [[year, month, day], [end_year, end_month, end_day]]
    let date_parts: Vec<Vec<i32>> = outer_list
        .iter()
        .filter_map(|inner| {
            let ConfigValueKind::Array(parts_list) = &inner.value else {
                return None;
            };

            let nums: Vec<i32> = parts_list
                .iter()
                .filter_map(|p| {
                    // Try integer first (years like 2019 are parsed as integers by YAML)
                    if let ConfigValueKind::Scalar(yaml_rust2::Yaml::Integer(i)) = &p.value {
                        return i32::try_from(*i).ok();
                    }
                    if let Some(s) = p.as_str() {
                        return s.parse().ok();
                    }
                    if let ConfigValueKind::PandocInlines(content) = &p.value {
                        return inlines_to_text(content).parse().ok();
                    }
                    None
                })
                .collect();

            if nums.is_empty() { None } else { Some(nums) }
        })
        .collect();

    if date_parts.is_empty() {
        None
    } else {
        Some(DateVariable {
            date_parts: Some(date_parts),
            literal: None,
            raw: None,
            season: None,
            circa: None,
        })
    }
}

/// Convert inlines to plain text (for metadata extraction).
fn inlines_to_text(inlines: &[crate::pandoc::Inline]) -> String {
    use crate::pandoc::Inline;

    let mut result = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(s) => result.push_str(&s.text),
            Inline::Space(_) => result.push(' '),
            Inline::SoftBreak(_) => result.push(' '),
            Inline::LineBreak(_) => result.push('\n'),
            Inline::Emph(e) => result.push_str(&inlines_to_text(&e.content)),
            Inline::Strong(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Underline(u) => result.push_str(&inlines_to_text(&u.content)),
            Inline::Strikeout(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Superscript(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Subscript(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::SmallCaps(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Quoted(q) => result.push_str(&inlines_to_text(&q.content)),
            Inline::Link(l) => result.push_str(&inlines_to_text(&l.content)),
            Inline::Span(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Code(c) => result.push_str(&c.text),
            Inline::Math(m) => result.push_str(&m.text),
            Inline::RawInline(r) => result.push_str(&r.text),
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pandoc::{
        Code, Emph, LineBreak, Math, MathType, QuoteType, Quoted, RawInline, SmallCaps, SoftBreak,
        Space, Strikeout, Strong, Subscript, Superscript, Underline,
    };

    // Helper to create a default SourceInfo for tests
    fn si() -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::default()
    }

    // Helper to create a Str inline
    fn str_inline(text: &str) -> Inline {
        Inline::Str(crate::pandoc::Str {
            text: text.to_string(),
            source_info: si(),
        })
    }

    #[test]
    fn test_default_config() {
        let config = CiteprocConfig::default();
        assert!(config.csl.is_none());
        assert!(config.bibliography.is_empty());
        assert!(!config.link_citations);
        assert!(config.link_bibliography);
        assert!(!config.suppress_bibliography);
    }

    #[test]
    fn test_default_csl_style_loads() {
        // Verify the embedded CSL style can be parsed
        let style = parse_csl(DEFAULT_CSL_STYLE);
        assert!(
            style.is_ok(),
            "Failed to parse default CSL style: {:?}",
            style.err()
        );
    }

    // Tests for inlines_to_text function
    #[test]
    fn test_inlines_to_text_str() {
        let inlines = vec![str_inline("Hello World")];
        assert_eq!(inlines_to_text(&inlines), "Hello World");
    }

    #[test]
    fn test_inlines_to_text_space() {
        let inlines = vec![
            str_inline("Hello"),
            Inline::Space(Space { source_info: si() }),
            str_inline("World"),
        ];
        assert_eq!(inlines_to_text(&inlines), "Hello World");
    }

    #[test]
    fn test_inlines_to_text_soft_break() {
        let inlines = vec![
            str_inline("Line1"),
            Inline::SoftBreak(SoftBreak { source_info: si() }),
            str_inline("Line2"),
        ];
        assert_eq!(inlines_to_text(&inlines), "Line1 Line2");
    }

    #[test]
    fn test_inlines_to_text_line_break() {
        let inlines = vec![
            str_inline("Line1"),
            Inline::LineBreak(LineBreak { source_info: si() }),
            str_inline("Line2"),
        ];
        assert_eq!(inlines_to_text(&inlines), "Line1\nLine2");
    }

    #[test]
    fn test_inlines_to_text_emph() {
        let inlines = vec![Inline::Emph(Emph {
            content: vec![str_inline("emphasized")],
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "emphasized");
    }

    #[test]
    fn test_inlines_to_text_strong() {
        let inlines = vec![Inline::Strong(Strong {
            content: vec![str_inline("bold")],
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "bold");
    }

    #[test]
    fn test_inlines_to_text_underline() {
        let inlines = vec![Inline::Underline(Underline {
            content: vec![str_inline("underlined")],
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "underlined");
    }

    #[test]
    fn test_inlines_to_text_strikeout() {
        let inlines = vec![Inline::Strikeout(Strikeout {
            content: vec![str_inline("struck")],
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "struck");
    }

    #[test]
    fn test_inlines_to_text_superscript() {
        let inlines = vec![Inline::Superscript(Superscript {
            content: vec![str_inline("2")],
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "2");
    }

    #[test]
    fn test_inlines_to_text_subscript() {
        let inlines = vec![Inline::Subscript(Subscript {
            content: vec![str_inline("i")],
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "i");
    }

    #[test]
    fn test_inlines_to_text_smallcaps() {
        let inlines = vec![Inline::SmallCaps(SmallCaps {
            content: vec![str_inline("text")],
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "text");
    }

    #[test]
    fn test_inlines_to_text_quoted() {
        let inlines = vec![Inline::Quoted(Quoted {
            quote_type: QuoteType::DoubleQuote,
            content: vec![str_inline("quoted")],
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "quoted");
    }

    #[test]
    fn test_inlines_to_text_code() {
        let inlines = vec![Inline::Code(Code {
            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
            text: "println!".to_string(),
            source_info: si(),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
        })];
        assert_eq!(inlines_to_text(&inlines), "println!");
    }

    #[test]
    fn test_inlines_to_text_math() {
        let inlines = vec![Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "x^2".to_string(),
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "x^2");
    }

    #[test]
    fn test_inlines_to_text_raw_inline() {
        let inlines = vec![Inline::RawInline(RawInline {
            format: "html".to_string(),
            text: "<b>raw</b>".to_string(),
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "<b>raw</b>");
    }

    #[test]
    fn test_inlines_to_text_link() {
        let inlines = vec![Inline::Link(crate::pandoc::Link {
            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![str_inline("link text")],
            target: ("https://example.com".to_string(), "".to_string()),
            source_info: si(),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            target_source: crate::pandoc::TargetSourceInfo::empty(),
        })];
        assert_eq!(inlines_to_text(&inlines), "link text");
    }

    #[test]
    fn test_inlines_to_text_span() {
        let inlines = vec![Inline::Span(crate::pandoc::Span {
            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![str_inline("span content")],
            source_info: si(),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
        })];
        assert_eq!(inlines_to_text(&inlines), "span content");
    }

    #[test]
    fn test_inlines_to_text_nested() {
        // Test nested formatting: Strong inside Emph
        let inlines = vec![Inline::Emph(Emph {
            content: vec![
                str_inline("italic "),
                Inline::Strong(Strong {
                    content: vec![str_inline("and bold")],
                    source_info: si(),
                }),
            ],
            source_info: si(),
        })];
        assert_eq!(inlines_to_text(&inlines), "italic and bold");
    }

    #[test]
    fn test_inlines_to_text_complex() {
        // Complex example with multiple inline types
        let inlines = vec![
            str_inline("Hello"),
            Inline::Space(Space { source_info: si() }),
            Inline::Emph(Emph {
                content: vec![str_inline("world")],
                source_info: si(),
            }),
            str_inline("!"),
            Inline::Space(Space { source_info: si() }),
            Inline::Code(Code {
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                text: "code".to_string(),
                source_info: si(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
            }),
        ];
        assert_eq!(inlines_to_text(&inlines), "Hello world! code");
    }

    #[test]
    fn test_inlines_to_text_empty() {
        let inlines: Vec<Inline> = vec![];
        assert_eq!(inlines_to_text(&inlines), "");
    }

    // Helper to create a ConfigValue with a Map containing entries
    fn meta_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(
                entries
                    .into_iter()
                    .map(|(key, value)| ConfigMapEntry {
                        key: key.to_string(),
                        key_source: quarto_source_map::SourceInfo::default(),
                        value,
                    })
                    .collect(),
            ),
            source_info: quarto_source_map::SourceInfo::default(),
            merge_op: quarto_pandoc_types::config_value::MergeOp::default(),
        }
    }

    // Helper to create a string ConfigValue
    fn meta_string(s: &str) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Scalar(yaml_rust2::Yaml::String(s.to_string())),
            source_info: quarto_source_map::SourceInfo::default(),
            merge_op: quarto_pandoc_types::config_value::MergeOp::default(),
        }
    }

    // Helper to create a boolean ConfigValue
    fn meta_bool(b: bool) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Scalar(yaml_rust2::Yaml::Boolean(b)),
            source_info: quarto_source_map::SourceInfo::default(),
            merge_op: quarto_pandoc_types::config_value::MergeOp::default(),
        }
    }

    // Helper to create an array ConfigValue
    fn meta_array(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: quarto_source_map::SourceInfo::default(),
            merge_op: quarto_pandoc_types::config_value::MergeOp::default(),
        }
    }

    // Helper to create a Pandoc document with metadata
    fn pandoc_with_meta(meta: ConfigValue) -> Pandoc {
        Pandoc {
            meta,
            blocks: vec![],
        }
    }

    // Tests for extract_config function
    #[test]
    fn test_extract_config_empty() {
        let pandoc = pandoc_with_meta(meta_map(vec![]));
        let config = extract_config(&pandoc);
        assert!(config.csl.is_none());
        assert!(config.bibliography.is_empty());
        assert!(config.lang.is_none());
        assert!(!config.link_citations);
        assert!(config.link_bibliography);
        assert!(!config.suppress_bibliography);
    }

    #[test]
    fn test_extract_config_csl() {
        let pandoc = pandoc_with_meta(meta_map(vec![("csl", meta_string("my-style.csl"))]));
        let config = extract_config(&pandoc);
        assert_eq!(config.csl, Some("my-style.csl".to_string()));
    }

    #[test]
    fn test_extract_config_bibliography_single() {
        let pandoc = pandoc_with_meta(meta_map(vec![("bibliography", meta_string("refs.bib"))]));
        let config = extract_config(&pandoc);
        assert_eq!(config.bibliography, vec!["refs.bib".to_string()]);
    }

    #[test]
    fn test_extract_config_bibliography_array() {
        let pandoc = pandoc_with_meta(meta_map(vec![(
            "bibliography",
            meta_array(vec![meta_string("refs1.bib"), meta_string("refs2.bib")]),
        )]));
        let config = extract_config(&pandoc);
        assert_eq!(
            config.bibliography,
            vec!["refs1.bib".to_string(), "refs2.bib".to_string()]
        );
    }

    #[test]
    fn test_extract_config_lang() {
        let pandoc = pandoc_with_meta(meta_map(vec![("lang", meta_string("en-US"))]));
        let config = extract_config(&pandoc);
        assert_eq!(config.lang, Some("en-US".to_string()));
    }

    #[test]
    fn test_extract_config_link_citations_true() {
        let pandoc = pandoc_with_meta(meta_map(vec![("link-citations", meta_bool(true))]));
        let config = extract_config(&pandoc);
        assert!(config.link_citations);
    }

    #[test]
    fn test_extract_config_link_citations_false() {
        let pandoc = pandoc_with_meta(meta_map(vec![("link-citations", meta_bool(false))]));
        let config = extract_config(&pandoc);
        assert!(!config.link_citations);
    }

    #[test]
    fn test_extract_config_link_bibliography_false() {
        let pandoc = pandoc_with_meta(meta_map(vec![("link-bibliography", meta_bool(false))]));
        let config = extract_config(&pandoc);
        assert!(!config.link_bibliography);
    }

    #[test]
    fn test_extract_config_suppress_bibliography() {
        let pandoc = pandoc_with_meta(meta_map(vec![("suppress-bibliography", meta_bool(true))]));
        let config = extract_config(&pandoc);
        assert!(config.suppress_bibliography);
    }

    #[test]
    fn test_extract_config_nocite_single() {
        let pandoc = pandoc_with_meta(meta_map(vec![("nocite", meta_string("@*"))]));
        let config = extract_config(&pandoc);
        assert_eq!(config.nocite, vec!["@*".to_string()]);
    }

    #[test]
    fn test_extract_config_nocite_array() {
        let pandoc = pandoc_with_meta(meta_map(vec![(
            "nocite",
            meta_array(vec![meta_string("@smith2020"), meta_string("@jones2021")]),
        )]));
        let config = extract_config(&pandoc);
        assert_eq!(
            config.nocite,
            vec!["@smith2020".to_string(), "@jones2021".to_string()]
        );
    }

    #[test]
    fn test_extract_config_complete() {
        let pandoc = pandoc_with_meta(meta_map(vec![
            ("csl", meta_string("apa.csl")),
            (
                "bibliography",
                meta_array(vec![meta_string("main.bib"), meta_string("extra.bib")]),
            ),
            ("lang", meta_string("de-DE")),
            ("link-citations", meta_bool(true)),
            ("link-bibliography", meta_bool(false)),
            ("suppress-bibliography", meta_bool(false)),
            ("nocite", meta_string("@*")),
        ]));
        let config = extract_config(&pandoc);
        assert_eq!(config.csl, Some("apa.csl".to_string()));
        assert_eq!(
            config.bibliography,
            vec!["main.bib".to_string(), "extra.bib".to_string()]
        );
        assert_eq!(config.lang, Some("de-DE".to_string()));
        assert!(config.link_citations);
        assert!(!config.link_bibliography);
        assert!(!config.suppress_bibliography);
        assert_eq!(config.nocite, vec!["@*".to_string()]);
    }

    // Helper to create an integer ConfigValue
    fn meta_int(i: i64) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Scalar(yaml_rust2::Yaml::Integer(i)),
            source_info: quarto_source_map::SourceInfo::default(),
            merge_op: quarto_pandoc_types::config_value::MergeOp::default(),
        }
    }

    // Tests for extract_references
    #[test]
    fn test_extract_references_empty_meta() {
        let meta = meta_map(vec![]);
        let refs = extract_references(&meta);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_references_no_references_field() {
        let meta = meta_map(vec![("title", meta_string("My Document"))]);
        let refs = extract_references(&meta);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_references_with_one_reference() {
        let reference = meta_map(vec![
            ("id", meta_string("smith2020")),
            ("type", meta_string("article-journal")),
            ("title", meta_string("A Great Paper")),
        ]);
        let meta = meta_map(vec![("references", meta_array(vec![reference]))]);
        let refs = extract_references(&meta);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].id, "smith2020");
        assert_eq!(refs[0].ref_type, "article-journal");
        assert_eq!(refs[0].title, Some("A Great Paper".to_string()));
    }

    #[test]
    fn test_extract_references_with_multiple_references() {
        let ref1 = meta_map(vec![
            ("id", meta_string("smith2020")),
            ("title", meta_string("First Paper")),
        ]);
        let ref2 = meta_map(vec![
            ("id", meta_string("jones2021")),
            ("title", meta_string("Second Paper")),
        ]);
        let meta = meta_map(vec![("references", meta_array(vec![ref1, ref2]))]);
        let refs = extract_references(&meta);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].id, "smith2020");
        assert_eq!(refs[1].id, "jones2021");
    }

    // Tests for meta_to_reference
    #[test]
    fn test_meta_to_reference_missing_id() {
        let meta = meta_map(vec![("title", meta_string("No ID Paper"))]);
        let reference = meta_to_reference(&meta);
        assert!(reference.is_none());
    }

    #[test]
    fn test_meta_to_reference_minimal() {
        let meta = meta_map(vec![("id", meta_string("test2020"))]);
        let reference = meta_to_reference(&meta).unwrap();
        assert_eq!(reference.id, "test2020");
        assert_eq!(reference.ref_type, "article"); // default type
        assert!(reference.title.is_none());
    }

    #[test]
    fn test_meta_to_reference_with_all_string_fields() {
        let meta = meta_map(vec![
            ("id", meta_string("complete2020")),
            ("type", meta_string("book")),
            ("title", meta_string("Complete Book")),
            ("title-short", meta_string("CB")),
            ("container-title", meta_string("Book Series")),
            ("publisher", meta_string("Academic Press")),
            ("publisher-place", meta_string("New York")),
            ("edition", meta_string("2nd")),
            ("volume", meta_string("3")),
            ("issue", meta_string("4")),
            ("page", meta_string("100-200")),
            ("DOI", meta_string("10.1234/test")),
            ("ISBN", meta_string("978-3-16-148410-0")),
            ("URL", meta_string("https://example.com")),
            ("note", meta_string("A note")),
            ("language", meta_string("en")),
        ]);
        let reference = meta_to_reference(&meta).unwrap();
        assert_eq!(reference.id, "complete2020");
        assert_eq!(reference.ref_type, "book");
        assert_eq!(reference.title, Some("Complete Book".to_string()));
        assert_eq!(reference.title_short, Some("CB".to_string()));
        assert_eq!(reference.publisher, Some("Academic Press".to_string()));
        assert_eq!(reference.doi, Some("10.1234/test".to_string()));
        assert_eq!(reference.isbn, Some("978-3-16-148410-0".to_string()));
        assert_eq!(reference.url, Some("https://example.com".to_string()));
    }

    // Tests for extract_names
    #[test]
    fn test_extract_names_no_author_field() {
        let entries: Vec<ConfigMapEntry> = vec![];
        let names = extract_names(&entries, "author");
        assert!(names.is_none());
    }

    #[test]
    fn test_extract_names_with_single_author() {
        let author = meta_map(vec![
            ("family", meta_string("Smith")),
            ("given", meta_string("John")),
        ]);
        let entries = vec![ConfigMapEntry {
            key: "author".to_string(),
            key_source: quarto_source_map::SourceInfo::default(),
            value: meta_array(vec![author]),
        }];
        let names = extract_names(&entries, "author").unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].family, Some("Smith".to_string()));
        assert_eq!(names[0].given, Some("John".to_string()));
    }

    #[test]
    fn test_extract_names_with_multiple_authors() {
        let author1 = meta_map(vec![
            ("family", meta_string("Smith")),
            ("given", meta_string("John")),
        ]);
        let author2 = meta_map(vec![
            ("family", meta_string("Jones")),
            ("given", meta_string("Jane")),
        ]);
        let entries = vec![ConfigMapEntry {
            key: "author".to_string(),
            key_source: quarto_source_map::SourceInfo::default(),
            value: meta_array(vec![author1, author2]),
        }];
        let names = extract_names(&entries, "author").unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].family, Some("Smith".to_string()));
        assert_eq!(names[1].family, Some("Jones".to_string()));
    }

    #[test]
    fn test_extract_names_with_literal_name() {
        let author = meta_map(vec![("literal", meta_string("World Health Organization"))]);
        let entries = vec![ConfigMapEntry {
            key: "author".to_string(),
            key_source: quarto_source_map::SourceInfo::default(),
            value: meta_array(vec![author]),
        }];
        let names = extract_names(&entries, "author").unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(
            names[0].literal,
            Some("World Health Organization".to_string())
        );
        assert!(names[0].family.is_none());
    }

    #[test]
    fn test_extract_names_with_particles() {
        let author = meta_map(vec![
            ("family", meta_string("Beethoven")),
            ("given", meta_string("Ludwig")),
            ("non-dropping-particle", meta_string("van")),
        ]);
        let entries = vec![ConfigMapEntry {
            key: "author".to_string(),
            key_source: quarto_source_map::SourceInfo::default(),
            value: meta_array(vec![author]),
        }];
        let names = extract_names(&entries, "author").unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].family, Some("Beethoven".to_string()));
        assert_eq!(names[0].given, Some("Ludwig".to_string()));
        assert_eq!(names[0].non_dropping_particle, Some("van".to_string()));
    }

    #[test]
    fn test_extract_names_empty_names_returns_none() {
        // Names with no family, given, or literal should be filtered out
        let author = meta_map(vec![("suffix", meta_string("Jr."))]); // Only suffix, no name
        let entries = vec![ConfigMapEntry {
            key: "author".to_string(),
            key_source: quarto_source_map::SourceInfo::default(),
            value: meta_array(vec![author]),
        }];
        let names = extract_names(&entries, "author");
        assert!(names.is_none()); // Empty vec becomes None
    }

    // Tests for extract_date
    #[test]
    fn test_extract_date_no_date_field() {
        let entries: Vec<ConfigMapEntry> = vec![];
        let date = extract_date(&entries, "issued");
        assert!(date.is_none());
    }

    #[test]
    fn test_extract_date_with_year_only() {
        let date_parts = meta_array(vec![meta_array(vec![meta_int(2020)])]);
        let date_map = meta_map(vec![("date-parts", date_parts)]);
        let entries = vec![ConfigMapEntry {
            key: "issued".to_string(),
            key_source: quarto_source_map::SourceInfo::default(),
            value: date_map,
        }];
        let date = extract_date(&entries, "issued").unwrap();
        assert!(date.date_parts.is_some());
        let parts = date.date_parts.unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], vec![2020]);
    }

    #[test]
    fn test_extract_date_with_year_month_day() {
        let date_parts = meta_array(vec![meta_array(vec![
            meta_int(2020),
            meta_int(6),
            meta_int(15),
        ])]);
        let date_map = meta_map(vec![("date-parts", date_parts)]);
        let entries = vec![ConfigMapEntry {
            key: "issued".to_string(),
            key_source: quarto_source_map::SourceInfo::default(),
            value: date_map,
        }];
        let date = extract_date(&entries, "issued").unwrap();
        let parts = date.date_parts.unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], vec![2020, 6, 15]);
    }

    #[test]
    fn test_extract_date_with_date_range() {
        let date_parts = meta_array(vec![
            meta_array(vec![meta_int(2020), meta_int(1)]),
            meta_array(vec![meta_int(2020), meta_int(12)]),
        ]);
        let date_map = meta_map(vec![("date-parts", date_parts)]);
        let entries = vec![ConfigMapEntry {
            key: "issued".to_string(),
            key_source: quarto_source_map::SourceInfo::default(),
            value: date_map,
        }];
        let date = extract_date(&entries, "issued").unwrap();
        let parts = date.date_parts.unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], vec![2020, 1]);
        assert_eq!(parts[1], vec![2020, 12]);
    }

    // Tests for collect_citations
    #[test]
    fn test_collect_citations_empty_document() {
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![],
        };
        let citations = collect_citations(&pandoc);
        assert!(citations.is_empty());
    }

    #[test]
    fn test_collect_citations_no_citations() {
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![str_inline("Just plain text")],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert!(citations.is_empty());
    }

    #[test]
    fn test_collect_citations_single_citation() {
        let cite = Inline::Cite(crate::pandoc::Cite {
            citations: vec![crate::pandoc::Citation {
                id: "smith2020".to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: crate::pandoc::CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![cite],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items.len(), 1);
        assert_eq!(citations[0].items[0].id, "smith2020");
    }

    #[test]
    fn test_collect_citations_in_emphasis() {
        let cite = Inline::Cite(crate::pandoc::Cite {
            citations: vec![crate::pandoc::Citation {
                id: "jones2021".to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: crate::pandoc::CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![],
            source_info: si(),
        });
        let emph = Inline::Emph(Emph {
            content: vec![cite],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![emph],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "jones2021");
    }

    #[test]
    fn test_collect_citations_in_block_quote() {
        let cite = Inline::Cite(crate::pandoc::Cite {
            citations: vec![crate::pandoc::Citation {
                id: "quoted2020".to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: crate::pandoc::CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::BlockQuote(crate::pandoc::BlockQuote {
                content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![cite],
                    source_info: si(),
                })],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "quoted2020");
    }

    #[test]
    fn test_collect_citations_in_div() {
        let cite = Inline::Cite(crate::pandoc::Cite {
            citations: vec![crate::pandoc::Citation {
                id: "div2020".to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: crate::pandoc::CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Div(crate::pandoc::Div {
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![cite],
                    source_info: si(),
                })],
                source_info: si(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "div2020");
    }

    // Tests for insert_bibliography
    #[test]
    fn test_insert_bibliography_empty_blocks() {
        let mut blocks: Vec<Block> = vec![];
        let bib_blocks = vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![str_inline("Bibliography entry")],
            source_info: si(),
        })];
        insert_bibliography(&mut blocks, bib_blocks);
        // Should add a refs div at the end
        assert_eq!(blocks.len(), 1);
        if let Block::Div(d) = &blocks[0] {
            assert_eq!(d.attr.0, "refs");
            assert!(d.attr.1.contains(&"references".to_string()));
            assert!(d.attr.1.contains(&"csl-bib-body".to_string()));
        } else {
            panic!("Expected Div block");
        }
    }

    #[test]
    fn test_insert_bibliography_replaces_existing_refs_div() {
        let mut blocks = vec![Block::Div(crate::pandoc::Div {
            attr: ("refs".to_string(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![], // Empty initially
            source_info: si(),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
        })];
        let bib_blocks = vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![str_inline("New bibliography")],
            source_info: si(),
        })];
        insert_bibliography(&mut blocks, bib_blocks);
        // Should replace contents of existing refs div
        assert_eq!(blocks.len(), 1);
        if let Block::Div(d) = &blocks[0] {
            assert_eq!(d.attr.0, "refs");
            assert_eq!(d.content.len(), 1); // Now has content
            assert!(d.attr.1.contains(&"references".to_string()));
        } else {
            panic!("Expected Div block");
        }
    }

    // Helper to create a citation inline
    fn make_cite(id: &str) -> Inline {
        Inline::Cite(crate::pandoc::Cite {
            citations: vec![crate::pandoc::Citation {
                id: id.to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: crate::pandoc::CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![],
            source_info: si(),
        })
    }

    // Tests for collect_citations in various block types
    #[test]
    fn test_collect_citations_in_plain_block() {
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Plain(crate::pandoc::Plain {
                content: vec![make_cite("plain2020")],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "plain2020");
    }

    #[test]
    fn test_collect_citations_in_header_block() {
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Header(crate::pandoc::Header {
                level: 1,
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                content: vec![str_inline("Title "), make_cite("header2020")],
                source_info: si(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "header2020");
    }

    #[test]
    fn test_collect_citations_in_ordered_list() {
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::OrderedList(crate::pandoc::OrderedList {
                attr: (
                    1,
                    crate::pandoc::ListNumberStyle::Decimal,
                    crate::pandoc::ListNumberDelim::Period,
                ),
                content: vec![vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![make_cite("orderedlist2020")],
                    source_info: si(),
                })]],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "orderedlist2020");
    }

    #[test]
    fn test_collect_citations_in_bullet_list() {
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::BulletList(crate::pandoc::BulletList {
                content: vec![vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![make_cite("bulletlist2020")],
                    source_info: si(),
                })]],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "bulletlist2020");
    }

    #[test]
    fn test_collect_citations_in_definition_list() {
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::DefinitionList(crate::pandoc::DefinitionList {
                content: vec![(
                    vec![str_inline("Term "), make_cite("defterm2020")],
                    vec![vec![Block::Paragraph(crate::pandoc::Paragraph {
                        content: vec![make_cite("defbody2020")],
                        source_info: si(),
                    })]],
                )],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].items[0].id, "defterm2020");
        assert_eq!(citations[1].items[0].id, "defbody2020");
    }

    #[test]
    fn test_collect_citations_in_figure() {
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Figure(crate::pandoc::Figure {
                attr: ("fig1".to_string(), vec![], hashlink::LinkedHashMap::new()),
                caption: crate::pandoc::Caption {
                    short: Some(vec![make_cite("figshort2020")]),
                    long: Some(vec![Block::Paragraph(crate::pandoc::Paragraph {
                        content: vec![make_cite("figlong2020")],
                        source_info: si(),
                    })]),
                    source_info: si(),
                },
                content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![make_cite("figcontent2020")],
                    source_info: si(),
                })],
                source_info: si(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 3);
        assert_eq!(citations[0].items[0].id, "figshort2020");
        assert_eq!(citations[1].items[0].id, "figlong2020");
        assert_eq!(citations[2].items[0].id, "figcontent2020");
    }

    #[test]
    fn test_collect_citations_in_table() {
        use crate::pandoc::{
            Alignment, Caption, Cell, Row, Table, TableBody, TableFoot, TableHead,
        };

        // Create a table with citations in caption and cells
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Table(Table {
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                caption: Caption {
                    short: Some(vec![make_cite("tableshort2020")]),
                    long: Some(vec![Block::Paragraph(crate::pandoc::Paragraph {
                        content: vec![make_cite("tablelong2020")],
                        source_info: si(),
                    })]),
                    source_info: si(),
                },
                colspec: vec![(Alignment::Default, crate::pandoc::ColWidth::Default)],
                head: TableHead {
                    attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                    rows: vec![Row {
                        attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                        cells: vec![Cell {
                            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                            alignment: Alignment::Default,
                            row_span: 1,
                            col_span: 1,
                            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                                content: vec![make_cite("tablehead2020")],
                                source_info: si(),
                            })],
                            source_info: si(),
                            attr_source: crate::pandoc::AttrSourceInfo::empty(),
                        }],
                        source_info: si(),
                        attr_source: crate::pandoc::AttrSourceInfo::empty(),
                    }],
                    source_info: si(),
                    attr_source: crate::pandoc::AttrSourceInfo::empty(),
                },
                bodies: vec![TableBody {
                    attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                    rowhead_columns: 0,
                    head: vec![],
                    body: vec![Row {
                        attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                        cells: vec![Cell {
                            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                            alignment: Alignment::Default,
                            row_span: 1,
                            col_span: 1,
                            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                                content: vec![make_cite("tablebody2020")],
                                source_info: si(),
                            })],
                            source_info: si(),
                            attr_source: crate::pandoc::AttrSourceInfo::empty(),
                        }],
                        source_info: si(),
                        attr_source: crate::pandoc::AttrSourceInfo::empty(),
                    }],
                    source_info: si(),
                    attr_source: crate::pandoc::AttrSourceInfo::empty(),
                }],
                foot: TableFoot {
                    attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                    rows: vec![Row {
                        attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                        cells: vec![Cell {
                            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                            alignment: Alignment::Default,
                            row_span: 1,
                            col_span: 1,
                            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                                content: vec![make_cite("tablefoot2020")],
                                source_info: si(),
                            })],
                            source_info: si(),
                            attr_source: crate::pandoc::AttrSourceInfo::empty(),
                        }],
                        source_info: si(),
                        attr_source: crate::pandoc::AttrSourceInfo::empty(),
                    }],
                    source_info: si(),
                    attr_source: crate::pandoc::AttrSourceInfo::empty(),
                },
                source_info: si(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 5);
        assert_eq!(citations[0].items[0].id, "tableshort2020");
        assert_eq!(citations[1].items[0].id, "tablelong2020");
        assert_eq!(citations[2].items[0].id, "tablehead2020");
        assert_eq!(citations[3].items[0].id, "tablebody2020");
        assert_eq!(citations[4].items[0].id, "tablefoot2020");
    }

    #[test]
    fn test_collect_citations_in_line_block() {
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::LineBlock(crate::pandoc::LineBlock {
                content: vec![vec![str_inline("Line 1 "), make_cite("lineblock2020")]],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "lineblock2020");
    }

    // Tests for collect_citations in various inline types
    #[test]
    fn test_collect_citations_in_strong() {
        let cite = make_cite("strong2020");
        let strong = Inline::Strong(Strong {
            content: vec![cite],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![strong],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "strong2020");
    }

    #[test]
    fn test_collect_citations_in_underline() {
        let cite = make_cite("underline2020");
        let underline = Inline::Underline(Underline {
            content: vec![cite],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![underline],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "underline2020");
    }

    #[test]
    fn test_collect_citations_in_strikeout() {
        let cite = make_cite("strikeout2020");
        let strikeout = Inline::Strikeout(Strikeout {
            content: vec![cite],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![strikeout],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "strikeout2020");
    }

    #[test]
    fn test_collect_citations_in_superscript() {
        let cite = make_cite("superscript2020");
        let superscript = Inline::Superscript(Superscript {
            content: vec![cite],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![superscript],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "superscript2020");
    }

    #[test]
    fn test_collect_citations_in_subscript() {
        let cite = make_cite("subscript2020");
        let subscript = Inline::Subscript(Subscript {
            content: vec![cite],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![subscript],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "subscript2020");
    }

    #[test]
    fn test_collect_citations_in_smallcaps() {
        let cite = make_cite("smallcaps2020");
        let smallcaps = Inline::SmallCaps(SmallCaps {
            content: vec![cite],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![smallcaps],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "smallcaps2020");
    }

    #[test]
    fn test_collect_citations_in_quoted() {
        let cite = make_cite("quoted2020");
        let quoted = Inline::Quoted(Quoted {
            quote_type: QuoteType::DoubleQuote,
            content: vec![cite],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![quoted],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "quoted2020");
    }

    #[test]
    fn test_collect_citations_in_link() {
        let cite = make_cite("link2020");
        let link = Inline::Link(crate::pandoc::Link {
            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![cite],
            target: ("https://example.com".to_string(), "".to_string()),
            source_info: si(),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
            target_source: crate::pandoc::TargetSourceInfo::empty(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![link],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "link2020");
    }

    #[test]
    fn test_collect_citations_in_span() {
        let cite = make_cite("span2020");
        let span = Inline::Span(crate::pandoc::Span {
            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![cite],
            source_info: si(),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![span],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "span2020");
    }

    #[test]
    fn test_collect_citations_in_note() {
        let cite = make_cite("note2020");
        let note = Inline::Note(crate::pandoc::Note {
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![cite],
                source_info: si(),
            })],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![note],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].id, "note2020");
    }

    // Test citation with prefix and suffix
    #[test]
    fn test_collect_citations_with_prefix_suffix() {
        let cite = Inline::Cite(crate::pandoc::Cite {
            citations: vec![crate::pandoc::Citation {
                id: "prefixsuffix2020".to_string(),
                prefix: vec![str_inline("see ")],
                suffix: vec![str_inline(", p. 42")],
                mode: crate::pandoc::CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![cite],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].prefix, Some("see ".to_string()));
        assert_eq!(citations[0].items[0].suffix, Some(", p. 42".to_string()));
    }

    // Test citation modes
    #[test]
    fn test_collect_citations_suppress_author_mode() {
        let cite = Inline::Cite(crate::pandoc::Cite {
            citations: vec![crate::pandoc::Citation {
                id: "suppress2020".to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: crate::pandoc::CitationMode::SuppressAuthor,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![cite],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].suppress_author, Some(true));
        assert_eq!(citations[0].items[0].author_only, Some(false));
    }

    #[test]
    fn test_collect_citations_author_in_text_mode() {
        let cite = Inline::Cite(crate::pandoc::Cite {
            citations: vec![crate::pandoc::Citation {
                id: "authoronly2020".to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: crate::pandoc::CitationMode::AuthorInText,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![],
            source_info: si(),
        });
        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![cite],
                source_info: si(),
            })],
        };
        let citations = collect_citations(&pandoc);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].items[0].suppress_author, Some(false));
        assert_eq!(citations[0].items[0].author_only, Some(true));
    }

    // Tests for transform functions
    #[test]
    fn test_transform_blocks_basic() {
        use quarto_citeproc::Citation as CpCitation;

        let cite = make_cite("smith2020");
        let mut blocks = vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![cite],
            source_info: si(),
        })];

        // Create a mock citation output
        let citations = vec![CpCitation {
            id: None,
            note_number: Some(1),
            items: vec![quarto_citeproc::CitationItem {
                id: "smith2020".to_string(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: Some(false),
                author_only: Some(false),
                position: None,
            }],
        }];
        let rendered = vec!["(Smith 2020)".to_string()];
        let citation_outputs: Vec<_> = citations.iter().zip(rendered.iter()).collect();

        // Create a minimal processor
        let style = quarto_csl::parse_csl(DEFAULT_CSL_STYLE).unwrap();
        let processor = quarto_citeproc::Processor::new(style);

        let mut citation_index = 0;
        transform_blocks(
            &mut blocks,
            &citation_outputs,
            &mut citation_index,
            &processor,
        );

        // After transformation, the cite should be replaced with a Str
        assert_eq!(blocks.len(), 1);
        if let Block::Paragraph(p) = &blocks[0] {
            assert_eq!(p.content.len(), 1);
            if let Inline::Str(s) = &p.content[0] {
                assert_eq!(s.text, "(Smith 2020)");
            } else {
                panic!("Expected Str inline");
            }
        } else {
            panic!("Expected Paragraph block");
        }
    }

    #[test]
    fn test_transform_inlines_with_nested_cite() {
        use quarto_citeproc::Citation as CpCitation;

        let cite = make_cite("nested2020");
        let mut inlines = vec![Inline::Emph(Emph {
            content: vec![cite],
            source_info: si(),
        })];

        let citations = vec![CpCitation {
            id: None,
            note_number: Some(1),
            items: vec![quarto_citeproc::CitationItem {
                id: "nested2020".to_string(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: Some(false),
                author_only: Some(false),
                position: None,
            }],
        }];
        let rendered = vec!["(Nested 2020)".to_string()];
        let citation_outputs: Vec<_> = citations.iter().zip(rendered.iter()).collect();

        let style = quarto_csl::parse_csl(DEFAULT_CSL_STYLE).unwrap();
        let processor = quarto_citeproc::Processor::new(style);

        let mut citation_index = 0;
        transform_inlines(
            &mut inlines,
            &citation_outputs,
            &mut citation_index,
            &processor,
        );

        // The Emph should now contain a Str instead of Cite
        if let Inline::Emph(e) = &inlines[0] {
            if let Inline::Str(s) = &e.content[0] {
                assert_eq!(s.text, "(Nested 2020)");
            } else {
                panic!("Expected Str inside Emph");
            }
        } else {
            panic!("Expected Emph");
        }
    }

    #[test]
    fn test_transform_blocks_in_various_containers() {
        use quarto_citeproc::Citation as CpCitation;

        let cite = make_cite("container2020");
        let mut blocks = vec![
            // Test in BlockQuote
            Block::BlockQuote(crate::pandoc::BlockQuote {
                content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![cite.clone()],
                    source_info: si(),
                })],
                source_info: si(),
            }),
            // Test in Div
            Block::Div(crate::pandoc::Div {
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![make_cite("div2020")],
                    source_info: si(),
                })],
                source_info: si(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
            }),
        ];

        let citations = vec![
            CpCitation {
                id: None,
                note_number: Some(1),
                items: vec![quarto_citeproc::CitationItem {
                    id: "container2020".to_string(),
                    locator: None,
                    label: None,
                    prefix: None,
                    suffix: None,
                    suppress_author: Some(false),
                    author_only: Some(false),
                    position: None,
                }],
            },
            CpCitation {
                id: None,
                note_number: Some(2),
                items: vec![quarto_citeproc::CitationItem {
                    id: "div2020".to_string(),
                    locator: None,
                    label: None,
                    prefix: None,
                    suffix: None,
                    suppress_author: Some(false),
                    author_only: Some(false),
                    position: None,
                }],
            },
        ];
        let rendered = vec!["(Container 2020)".to_string(), "(Div 2020)".to_string()];
        let citation_outputs: Vec<_> = citations.iter().zip(rendered.iter()).collect();

        let style = quarto_csl::parse_csl(DEFAULT_CSL_STYLE).unwrap();
        let processor = quarto_citeproc::Processor::new(style);

        let mut citation_index = 0;
        transform_blocks(
            &mut blocks,
            &citation_outputs,
            &mut citation_index,
            &processor,
        );

        // Check BlockQuote
        if let Block::BlockQuote(bq) = &blocks[0] {
            if let Block::Paragraph(p) = &bq.content[0] {
                if let Inline::Str(s) = &p.content[0] {
                    assert_eq!(s.text, "(Container 2020)");
                }
            }
        }

        // Check Div
        if let Block::Div(d) = &blocks[1] {
            if let Block::Paragraph(p) = &d.content[0] {
                if let Inline::Str(s) = &p.content[0] {
                    assert_eq!(s.text, "(Div 2020)");
                }
            }
        }
    }

    #[test]
    fn test_transform_blocks_in_lists() {
        use quarto_citeproc::Citation as CpCitation;

        let mut blocks = vec![
            // Test in OrderedList
            Block::OrderedList(crate::pandoc::OrderedList {
                attr: (
                    1,
                    crate::pandoc::ListNumberStyle::Decimal,
                    crate::pandoc::ListNumberDelim::Period,
                ),
                content: vec![vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![make_cite("ordered2020")],
                    source_info: si(),
                })]],
                source_info: si(),
            }),
            // Test in BulletList
            Block::BulletList(crate::pandoc::BulletList {
                content: vec![vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![make_cite("bullet2020")],
                    source_info: si(),
                })]],
                source_info: si(),
            }),
        ];

        let citations = vec![
            CpCitation {
                id: None,
                note_number: Some(1),
                items: vec![quarto_citeproc::CitationItem {
                    id: "ordered2020".to_string(),
                    locator: None,
                    label: None,
                    prefix: None,
                    suffix: None,
                    suppress_author: Some(false),
                    author_only: Some(false),
                    position: None,
                }],
            },
            CpCitation {
                id: None,
                note_number: Some(2),
                items: vec![quarto_citeproc::CitationItem {
                    id: "bullet2020".to_string(),
                    locator: None,
                    label: None,
                    prefix: None,
                    suffix: None,
                    suppress_author: Some(false),
                    author_only: Some(false),
                    position: None,
                }],
            },
        ];
        let rendered = vec!["(Ordered 2020)".to_string(), "(Bullet 2020)".to_string()];
        let citation_outputs: Vec<_> = citations.iter().zip(rendered.iter()).collect();

        let style = quarto_csl::parse_csl(DEFAULT_CSL_STYLE).unwrap();
        let processor = quarto_citeproc::Processor::new(style);

        let mut citation_index = 0;
        transform_blocks(
            &mut blocks,
            &citation_outputs,
            &mut citation_index,
            &processor,
        );

        // Check OrderedList
        if let Block::OrderedList(ol) = &blocks[0] {
            if let Block::Paragraph(p) = &ol.content[0][0] {
                if let Inline::Str(s) = &p.content[0] {
                    assert_eq!(s.text, "(Ordered 2020)");
                }
            }
        }

        // Check BulletList
        if let Block::BulletList(bl) = &blocks[1] {
            if let Block::Paragraph(p) = &bl.content[0][0] {
                if let Inline::Str(s) = &p.content[0] {
                    assert_eq!(s.text, "(Bullet 2020)");
                }
            }
        }
    }

    #[test]
    fn test_transform_inlines_various_types() {
        use quarto_citeproc::Citation as CpCitation;

        let mut inlines = vec![
            // Strong
            Inline::Strong(Strong {
                content: vec![make_cite("strong2020")],
                source_info: si(),
            }),
            // Underline
            Inline::Underline(Underline {
                content: vec![make_cite("underline2020")],
                source_info: si(),
            }),
            // Strikeout
            Inline::Strikeout(Strikeout {
                content: vec![make_cite("strikeout2020")],
                source_info: si(),
            }),
            // Superscript
            Inline::Superscript(Superscript {
                content: vec![make_cite("super2020")],
                source_info: si(),
            }),
            // Subscript
            Inline::Subscript(Subscript {
                content: vec![make_cite("sub2020")],
                source_info: si(),
            }),
            // SmallCaps
            Inline::SmallCaps(SmallCaps {
                content: vec![make_cite("small2020")],
                source_info: si(),
            }),
            // Quoted
            Inline::Quoted(Quoted {
                quote_type: QuoteType::DoubleQuote,
                content: vec![make_cite("quoted2020")],
                source_info: si(),
            }),
            // Link
            Inline::Link(crate::pandoc::Link {
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                content: vec![make_cite("link2020")],
                target: ("https://example.com".to_string(), "".to_string()),
                source_info: si(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                target_source: crate::pandoc::TargetSourceInfo::empty(),
            }),
            // Span
            Inline::Span(crate::pandoc::Span {
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                content: vec![make_cite("span2020")],
                source_info: si(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
            }),
        ];

        let ids = [
            "strong2020",
            "underline2020",
            "strikeout2020",
            "super2020",
            "sub2020",
            "small2020",
            "quoted2020",
            "link2020",
            "span2020",
        ];
        let citations: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| CpCitation {
                id: None,
                note_number: Some(i as i32 + 1),
                items: vec![quarto_citeproc::CitationItem {
                    id: id.to_string(),
                    locator: None,
                    label: None,
                    prefix: None,
                    suffix: None,
                    suppress_author: Some(false),
                    author_only: Some(false),
                    position: None,
                }],
            })
            .collect();
        let rendered: Vec<_> = ids.iter().map(|id| format!("({})", id)).collect();
        let citation_outputs: Vec<_> = citations.iter().zip(rendered.iter()).collect();

        let style = quarto_csl::parse_csl(DEFAULT_CSL_STYLE).unwrap();
        let processor = quarto_citeproc::Processor::new(style);

        let mut citation_index = 0;
        transform_inlines(
            &mut inlines,
            &citation_outputs,
            &mut citation_index,
            &processor,
        );

        // Check that all citations were transformed
        assert_eq!(citation_index, 9);

        // Verify Strong transformation
        if let Inline::Strong(s) = &inlines[0] {
            if let Inline::Str(str) = &s.content[0] {
                assert_eq!(str.text, "(strong2020)");
            }
        }
    }

    #[test]
    fn test_transform_blocks_in_note() {
        use quarto_citeproc::Citation as CpCitation;

        let mut inlines = vec![Inline::Note(crate::pandoc::Note {
            content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![make_cite("note2020")],
                source_info: si(),
            })],
            source_info: si(),
        })];

        let citations = vec![CpCitation {
            id: None,
            note_number: Some(1),
            items: vec![quarto_citeproc::CitationItem {
                id: "note2020".to_string(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: Some(false),
                author_only: Some(false),
                position: None,
            }],
        }];
        let rendered = vec!["(Note 2020)".to_string()];
        let citation_outputs: Vec<_> = citations.iter().zip(rendered.iter()).collect();

        let style = quarto_csl::parse_csl(DEFAULT_CSL_STYLE).unwrap();
        let processor = quarto_citeproc::Processor::new(style);

        let mut citation_index = 0;
        transform_inlines(
            &mut inlines,
            &citation_outputs,
            &mut citation_index,
            &processor,
        );

        // Check Note transformation
        if let Inline::Note(n) = &inlines[0] {
            if let Block::Paragraph(p) = &n.content[0] {
                if let Inline::Str(s) = &p.content[0] {
                    assert_eq!(s.text, "(Note 2020)");
                }
            }
        }
    }

    #[test]
    fn test_transform_blocks_plain_and_header() {
        use quarto_citeproc::Citation as CpCitation;

        let mut blocks = vec![
            Block::Plain(crate::pandoc::Plain {
                content: vec![make_cite("plain2020")],
                source_info: si(),
            }),
            Block::Header(crate::pandoc::Header {
                level: 1,
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                content: vec![make_cite("header2020")],
                source_info: si(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
            }),
        ];

        let citations = vec![
            CpCitation {
                id: None,
                note_number: Some(1),
                items: vec![quarto_citeproc::CitationItem {
                    id: "plain2020".to_string(),
                    locator: None,
                    label: None,
                    prefix: None,
                    suffix: None,
                    suppress_author: Some(false),
                    author_only: Some(false),
                    position: None,
                }],
            },
            CpCitation {
                id: None,
                note_number: Some(2),
                items: vec![quarto_citeproc::CitationItem {
                    id: "header2020".to_string(),
                    locator: None,
                    label: None,
                    prefix: None,
                    suffix: None,
                    suppress_author: Some(false),
                    author_only: Some(false),
                    position: None,
                }],
            },
        ];
        let rendered = vec!["(Plain 2020)".to_string(), "(Header 2020)".to_string()];
        let citation_outputs: Vec<_> = citations.iter().zip(rendered.iter()).collect();

        let style = quarto_csl::parse_csl(DEFAULT_CSL_STYLE).unwrap();
        let processor = quarto_citeproc::Processor::new(style);

        let mut citation_index = 0;
        transform_blocks(
            &mut blocks,
            &citation_outputs,
            &mut citation_index,
            &processor,
        );

        // Check Plain
        if let Block::Plain(p) = &blocks[0] {
            if let Inline::Str(s) = &p.content[0] {
                assert_eq!(s.text, "(Plain 2020)");
            }
        }

        // Check Header
        if let Block::Header(h) = &blocks[1] {
            if let Inline::Str(s) = &h.content[0] {
                assert_eq!(s.text, "(Header 2020)");
            }
        }
    }

    #[test]
    fn test_transform_blocks_definition_list() {
        use quarto_citeproc::Citation as CpCitation;

        let mut blocks = vec![Block::DefinitionList(crate::pandoc::DefinitionList {
            content: vec![(
                vec![make_cite("term2020")],
                vec![vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![make_cite("def2020")],
                    source_info: si(),
                })]],
            )],
            source_info: si(),
        })];

        let citations = vec![
            CpCitation {
                id: None,
                note_number: Some(1),
                items: vec![quarto_citeproc::CitationItem {
                    id: "term2020".to_string(),
                    locator: None,
                    label: None,
                    prefix: None,
                    suffix: None,
                    suppress_author: Some(false),
                    author_only: Some(false),
                    position: None,
                }],
            },
            CpCitation {
                id: None,
                note_number: Some(2),
                items: vec![quarto_citeproc::CitationItem {
                    id: "def2020".to_string(),
                    locator: None,
                    label: None,
                    prefix: None,
                    suffix: None,
                    suppress_author: Some(false),
                    author_only: Some(false),
                    position: None,
                }],
            },
        ];
        let rendered = vec!["(Term 2020)".to_string(), "(Def 2020)".to_string()];
        let citation_outputs: Vec<_> = citations.iter().zip(rendered.iter()).collect();

        let style = quarto_csl::parse_csl(DEFAULT_CSL_STYLE).unwrap();
        let processor = quarto_citeproc::Processor::new(style);

        let mut citation_index = 0;
        transform_blocks(
            &mut blocks,
            &citation_outputs,
            &mut citation_index,
            &processor,
        );

        // Check DefinitionList
        if let Block::DefinitionList(dl) = &blocks[0] {
            let (term, defs) = &dl.content[0];
            if let Inline::Str(s) = &term[0] {
                assert_eq!(s.text, "(Term 2020)");
            }
            if let Block::Paragraph(p) = &defs[0][0] {
                if let Inline::Str(s) = &p.content[0] {
                    assert_eq!(s.text, "(Def 2020)");
                }
            }
        }
    }

    #[test]
    fn test_transform_blocks_line_block() {
        use quarto_citeproc::Citation as CpCitation;

        let mut blocks = vec![Block::LineBlock(crate::pandoc::LineBlock {
            content: vec![vec![make_cite("line2020")]],
            source_info: si(),
        })];

        let citations = vec![CpCitation {
            id: None,
            note_number: Some(1),
            items: vec![quarto_citeproc::CitationItem {
                id: "line2020".to_string(),
                locator: None,
                label: None,
                prefix: None,
                suffix: None,
                suppress_author: Some(false),
                author_only: Some(false),
                position: None,
            }],
        }];
        let rendered = vec!["(Line 2020)".to_string()];
        let citation_outputs: Vec<_> = citations.iter().zip(rendered.iter()).collect();

        let style = quarto_csl::parse_csl(DEFAULT_CSL_STYLE).unwrap();
        let processor = quarto_citeproc::Processor::new(style);

        let mut citation_index = 0;
        transform_blocks(
            &mut blocks,
            &citation_outputs,
            &mut citation_index,
            &processor,
        );

        // Check LineBlock
        if let Block::LineBlock(lb) = &blocks[0] {
            if let Inline::Str(s) = &lb.content[0][0] {
                assert_eq!(s.text, "(Line 2020)");
            }
        }
    }

    // Test apply_citeproc_filter with inline references
    #[test]
    fn test_apply_citeproc_filter_no_bibliography() {
        use crate::pandoc::ast_context::ASTContext;

        let pandoc = Pandoc {
            meta: meta_map(vec![]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![str_inline("No citations here")],
                source_info: si(),
            })],
        };
        let context = ASTContext::new();

        let result = apply_citeproc_filter(pandoc.clone(), context, "html");
        assert!(result.is_ok());
        let (result_pandoc, _, _) = result.unwrap();
        // Should pass through unchanged since no bibliography
        assert_eq!(result_pandoc.blocks.len(), 1);
    }

    #[test]
    fn test_apply_citeproc_filter_with_inline_references() {
        use crate::pandoc::ast_context::ASTContext;

        // Create a reference in metadata
        let _reference = meta_map(vec![
            ("id", meta_string("test2020")),
            ("type", meta_string("book")),
            ("title", meta_string("Test Book")),
        ]);
        let _author = meta_map(vec![
            ("family", meta_string("Author")),
            ("given", meta_string("Test")),
        ]);
        let date_parts = meta_array(vec![meta_array(vec![meta_int(2020)])]);
        let date_map = meta_map(vec![("date-parts", date_parts)]);
        let full_ref = meta_map(vec![
            ("id", meta_string("test2020")),
            ("type", meta_string("book")),
            ("title", meta_string("Test Book")),
            (
                "author",
                meta_array(vec![meta_map(vec![
                    ("family", meta_string("Author")),
                    ("given", meta_string("Test")),
                ])]),
            ),
            ("issued", date_map),
        ]);

        // Create a citation
        let cite = Inline::Cite(crate::pandoc::Cite {
            citations: vec![crate::pandoc::Citation {
                id: "test2020".to_string(),
                prefix: vec![],
                suffix: vec![],
                mode: crate::pandoc::CitationMode::NormalCitation,
                note_num: 0,
                hash: 0,
                id_source: None,
            }],
            content: vec![],
            source_info: si(),
        });

        let pandoc = Pandoc {
            meta: meta_map(vec![("references", meta_array(vec![full_ref]))]),
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![cite],
                source_info: si(),
            })],
        };
        let context = ASTContext::new();

        let result = apply_citeproc_filter(pandoc, context, "html");
        assert!(result.is_ok());
        let (result_pandoc, _, _) = result.unwrap();

        // Should have bibliography at the end (refs div)
        assert!(result_pandoc.blocks.len() >= 1);
        // The citation should be replaced
        if let Block::Paragraph(p) = &result_pandoc.blocks[0] {
            // Citation should be replaced with rendered text
            assert!(!p.content.is_empty());
        }
    }

    #[test]
    fn test_load_csl_style_default() {
        let config = CiteprocConfig::default();
        let result = load_csl_style(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_csl_style_invalid_path() {
        let config = CiteprocConfig {
            csl: Some("/nonexistent/path/style.csl".to_string()),
            ..Default::default()
        };
        let result = load_csl_style(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_insert_bibliography_appends_when_no_refs_div() {
        let mut blocks = vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![str_inline("Content")],
            source_info: si(),
        })];
        let bib_blocks = vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![str_inline("Ref 1")],
            source_info: si(),
        })];

        insert_bibliography(&mut blocks, bib_blocks);

        assert_eq!(blocks.len(), 2);
        if let Block::Div(d) = &blocks[1] {
            assert_eq!(d.attr.0, "refs");
            assert!(d.attr.1.contains(&"references".to_string()));
            assert!(d.attr.1.contains(&"csl-bib-body".to_string()));
        } else {
            panic!("Expected Div at end");
        }
    }

    #[test]
    fn test_insert_bibliography_adds_classes_to_existing_refs_div() {
        let mut blocks = vec![Block::Div(crate::pandoc::Div {
            attr: (
                "refs".to_string(),
                vec![], // No classes initially
                hashlink::LinkedHashMap::new(),
            ),
            content: vec![],
            source_info: si(),
            attr_source: crate::pandoc::AttrSourceInfo::empty(),
        })];
        let bib_blocks = vec![Block::Paragraph(crate::pandoc::Paragraph {
            content: vec![str_inline("Ref 1")],
            source_info: si(),
        })];

        insert_bibliography(&mut blocks, bib_blocks);

        assert_eq!(blocks.len(), 1);
        if let Block::Div(d) = &blocks[0] {
            // Should have added the classes
            assert!(d.attr.1.contains(&"references".to_string()));
            assert!(d.attr.1.contains(&"csl-bib-body".to_string()));
        }
    }
}
