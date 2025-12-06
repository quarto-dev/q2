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
        let path = config
            .csl
            .as_ref()
            .map(|s| Path::new(s).to_owned())
            .unwrap_or_else(|| Path::new("<default>").to_owned());
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
        if let Block::Div(d) = block {
            if d.attr.0 == "refs" {
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
    use crate::pandoc::MetaValueWithSourceInfo;

    let mut config = CiteprocConfig::default();

    // Helper to get a string value from metadata
    fn get_meta_string(meta: &MetaValueWithSourceInfo, key: &str) -> Option<String> {
        if let MetaValueWithSourceInfo::MetaMap { entries, .. } = meta {
            for entry in entries {
                if entry.key == key {
                    return match &entry.value {
                        MetaValueWithSourceInfo::MetaString { value, .. } => Some(value.clone()),
                        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                            Some(inlines_to_text(content))
                        }
                        _ => None,
                    };
                }
            }
        }
        None
    }

    // Helper to get a boolean value from metadata
    fn get_meta_bool(meta: &MetaValueWithSourceInfo, key: &str) -> Option<bool> {
        if let MetaValueWithSourceInfo::MetaMap { entries, .. } = meta {
            for entry in entries {
                if entry.key == key {
                    return match &entry.value {
                        MetaValueWithSourceInfo::MetaBool { value, .. } => Some(*value),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    // Helper to get a string list from metadata
    fn get_meta_string_list(meta: &MetaValueWithSourceInfo, key: &str) -> Vec<String> {
        if let MetaValueWithSourceInfo::MetaMap { entries, .. } = meta {
            for entry in entries {
                if entry.key == key {
                    return match &entry.value {
                        MetaValueWithSourceInfo::MetaString { value, .. } => vec![value.clone()],
                        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                            vec![inlines_to_text(content)]
                        }
                        MetaValueWithSourceInfo::MetaList { items, .. } => items
                            .iter()
                            .filter_map(|item| match item {
                                MetaValueWithSourceInfo::MetaString { value, .. } => {
                                    Some(value.clone())
                                }
                                MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                                    Some(inlines_to_text(content))
                                }
                                _ => None,
                            })
                            .collect(),
                        _ => vec![],
                    };
                }
            }
        }
        vec![]
    }

    // Extract configuration values
    config.csl = get_meta_string(&pandoc.meta, "csl");
    config.bibliography = get_meta_string_list(&pandoc.meta, "bibliography");
    config.lang = get_meta_string(&pandoc.meta, "lang");
    config.link_citations = get_meta_bool(&pandoc.meta, "link-citations").unwrap_or(false);
    config.link_bibliography = get_meta_bool(&pandoc.meta, "link-bibliography").unwrap_or(true);
    config.nocite = get_meta_string_list(&pandoc.meta, "nocite");
    config.suppress_bibliography =
        get_meta_bool(&pandoc.meta, "suppress-bibliography").unwrap_or(false);

    // Extract inline references from metadata
    config.references = extract_references(&pandoc.meta);

    config
}

/// Extract inline references from the 'references' metadata field.
fn extract_references(meta: &crate::pandoc::MetaValueWithSourceInfo) -> Vec<Reference> {
    use crate::pandoc::MetaValueWithSourceInfo;

    let references_list = if let MetaValueWithSourceInfo::MetaMap { entries, .. } = meta {
        entries
            .iter()
            .find(|e| e.key == "references")
            .and_then(|e| {
                if let MetaValueWithSourceInfo::MetaList { items, .. } = &e.value {
                    Some(items)
                } else {
                    None
                }
            })
    } else {
        None
    };

    let Some(items) = references_list else {
        return vec![];
    };

    items
        .iter()
        .filter_map(|item| meta_to_reference(item))
        .collect()
}

/// Convert a metadata map to a Reference.
fn meta_to_reference(meta: &crate::pandoc::MetaValueWithSourceInfo) -> Option<Reference> {
    use crate::pandoc::MetaValueWithSourceInfo;
    use quarto_citeproc::reference::StringOrNumber;

    let MetaValueWithSourceInfo::MetaMap { entries, .. } = meta else {
        return None;
    };

    // Helper to get a string from an entry
    let get_string = |key: &str| -> Option<String> {
        entries
            .iter()
            .find(|e| e.key == key)
            .and_then(|e| match &e.value {
                MetaValueWithSourceInfo::MetaString { value, .. } => Some(value.clone()),
                MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                    Some(inlines_to_text(content))
                }
                _ => None,
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
        other: std::collections::HashMap::new(),
        disambiguation: None,
    };

    Some(reference)
}

/// Extract names from a metadata entry.
fn extract_names(
    entries: &[crate::pandoc::MetaMapEntry],
    key: &str,
) -> Option<Vec<quarto_citeproc::reference::Name>> {
    use crate::pandoc::MetaValueWithSourceInfo;
    use quarto_citeproc::reference::Name;

    let entry = entries.iter().find(|e| e.key == key)?;

    let names_list = match &entry.value {
        MetaValueWithSourceInfo::MetaList { items, .. } => items,
        _ => return None,
    };

    let names: Vec<Name> = names_list
        .iter()
        .filter_map(|item| {
            let MetaValueWithSourceInfo::MetaMap {
                entries: name_entries,
                ..
            } = item
            else {
                return None;
            };

            let get_name_part = |key: &str| -> Option<String> {
                name_entries
                    .iter()
                    .find(|e| e.key == key)
                    .and_then(|e| match &e.value {
                        MetaValueWithSourceInfo::MetaString { value, .. } => Some(value.clone()),
                        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                            Some(inlines_to_text(content))
                        }
                        _ => None,
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
    entries: &[crate::pandoc::MetaMapEntry],
    key: &str,
) -> Option<quarto_citeproc::reference::DateVariable> {
    use crate::pandoc::MetaValueWithSourceInfo;
    use quarto_citeproc::reference::DateVariable;

    let entry = entries.iter().find(|e| e.key == key)?;

    let MetaValueWithSourceInfo::MetaMap {
        entries: date_entries,
        ..
    } = &entry.value
    else {
        return None;
    };

    // Look for date-parts
    let date_parts_entry = date_entries.iter().find(|e| e.key == "date-parts")?;
    let MetaValueWithSourceInfo::MetaList {
        items: outer_list, ..
    } = &date_parts_entry.value
    else {
        return None;
    };

    // date-parts is a list of lists: [[year, month, day], [end_year, end_month, end_day]]
    let date_parts: Vec<Vec<i32>> = outer_list
        .iter()
        .filter_map(|inner| {
            let MetaValueWithSourceInfo::MetaList {
                items: parts_list, ..
            } = inner
            else {
                return None;
            };

            let nums: Vec<i32> = parts_list
                .iter()
                .filter_map(|p| match p {
                    MetaValueWithSourceInfo::MetaString { value, .. } => value.parse().ok(),
                    MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                        inlines_to_text(content).parse().ok()
                    }
                    _ => None,
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
}
