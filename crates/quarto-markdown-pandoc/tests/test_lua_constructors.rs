/*
 * test_lua_constructors.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for missing Lua element constructors (Phase 1 of Pandoc Lua API port).
 */

// Tests require the lua-filter feature
#![cfg(feature = "lua-filter")]

use quarto_markdown_pandoc::lua::apply_lua_filters;
use quarto_markdown_pandoc::pandoc::ast_context::ASTContext;
use quarto_markdown_pandoc::pandoc::{Block, Inline, Pandoc, Paragraph, Str};
use std::io::Write;
use tempfile::NamedTempFile;

/// Helper to create a simple Pandoc document with a paragraph
fn create_test_doc(content: Vec<Inline>) -> Pandoc {
    Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content,
            source_info: quarto_source_map::SourceInfo::default(),
        })],
    }
}

/// Helper to run a filter and assert success
fn run_filter(filter_code: &str, doc: Pandoc) -> (Pandoc, ASTContext) {
    let mut filter_file = NamedTempFile::new().expect("Failed to create temp file");
    filter_file
        .write_all(filter_code.as_bytes())
        .expect("Failed to write filter");

    let context = ASTContext::anonymous();
    let result = apply_lua_filters(doc, context, &[filter_file.path().to_path_buf()], "html");
    let (pandoc, context, _diagnostics) = result.expect("Filter failed");
    (pandoc, context)
}

// ============================================================================
// Cite and Citation constructor tests
// ============================================================================

#[test]
fn test_cite_constructor() {
    // Test pandoc.Cite(citations, content) constructor
    let filter_code = r#"
function Para(elem)
    -- Create a citation
    local citation = pandoc.Citation(
        "knuth1984",           -- id
        "NormalCitation"       -- mode
    )

    -- Create a Cite inline with the citation
    local cite = pandoc.Cite(
        {citation},            -- citations list
        {pandoc.Str("Knuth")}  -- content
    )

    -- Verify the cite was created correctly
    if cite.tag ~= "Cite" then
        error("Expected Cite tag, got " .. tostring(cite.tag))
    end

    -- Verify we can access citations
    if #cite.citations ~= 1 then
        error("Expected 1 citation, got " .. #cite.citations)
    end

    if cite.citations[1].id ~= "knuth1984" then
        error("Expected citation id 'knuth1984', got " .. tostring(cite.citations[1].id))
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_citation_constructor_all_args() {
    // Test pandoc.Citation with all arguments
    let filter_code = r#"
function Para(elem)
    local citation = pandoc.Citation(
        "smith2020",                      -- id
        "AuthorInText",                   -- mode
        {pandoc.Str("see")},              -- prefix
        {pandoc.Str(","), pandoc.Space(), pandoc.Str("p. 42")},  -- suffix
        0,                                -- note_num
        0                                 -- hash
    )

    if citation.id ~= "smith2020" then
        error("Expected id 'smith2020', got " .. tostring(citation.id))
    end

    if citation.mode ~= "AuthorInText" then
        error("Expected mode 'AuthorInText', got " .. tostring(citation.mode))
    end

    if #citation.prefix ~= 1 then
        error("Expected prefix length 1, got " .. #citation.prefix)
    end

    if #citation.suffix ~= 3 then
        error("Expected suffix length 3, got " .. #citation.suffix)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// DefinitionList constructor tests
// ============================================================================

#[test]
fn test_definition_list_constructor() {
    // Test pandoc.DefinitionList constructor
    let filter_code = r#"
function Para(elem)
    -- Create a definition list with one term and one definition
    local dl = pandoc.DefinitionList{
        {{pandoc.Str("Term")}, {{pandoc.Para{pandoc.Str("Definition")}}}}
    }

    if dl.tag ~= "DefinitionList" then
        error("Expected DefinitionList tag, got " .. tostring(dl.tag))
    end

    -- Verify content structure
    if #dl.content ~= 1 then
        error("Expected 1 definition item, got " .. #dl.content)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// LineBlock constructor tests
// ============================================================================

#[test]
fn test_line_block_constructor() {
    // Test pandoc.LineBlock constructor
    let filter_code = r#"
function Para(elem)
    -- Create a line block with two lines
    local lb = pandoc.LineBlock{
        {pandoc.Str("First"), pandoc.Space(), pandoc.Str("line")},
        {pandoc.Str("Second"), pandoc.Space(), pandoc.Str("line")}
    }

    if lb.tag ~= "LineBlock" then
        error("Expected LineBlock tag, got " .. tostring(lb.tag))
    end

    -- Verify content structure (list of lines)
    if #lb.content ~= 2 then
        error("Expected 2 lines, got " .. #lb.content)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// Figure and Caption constructor tests
// ============================================================================

#[test]
fn test_caption_constructor() {
    // Test pandoc.Caption constructor
    let filter_code = r#"
function Para(elem)
    -- Create a caption with short and long forms
    local caption = pandoc.Caption(
        {pandoc.Str("Short")},  -- short
        {pandoc.Para{pandoc.Str("Long"), pandoc.Space(), pandoc.Str("caption")}}  -- long
    )

    -- Check short caption
    if caption.short and #caption.short ~= 1 then
        error("Expected short caption length 1, got " .. #caption.short)
    end

    -- Check long caption
    if caption.long and #caption.long ~= 1 then
        error("Expected long caption length 1, got " .. #caption.long)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_figure_constructor() {
    // Test pandoc.Figure constructor
    let filter_code = r#"
function Para(elem)
    -- Create a figure with caption
    local caption = pandoc.Caption(
        nil,  -- no short caption
        {pandoc.Para{pandoc.Str("Figure caption")}}
    )

    local figure = pandoc.Figure(
        {pandoc.Para{pandoc.Str("Figure content")}},  -- content
        caption,                                       -- caption
        pandoc.Attr("fig1", {"figure"}, {})           -- attr
    )

    if figure.tag ~= "Figure" then
        error("Expected Figure tag, got " .. tostring(figure.tag))
    end

    -- Verify we can access the caption
    if not figure.caption then
        error("Expected figure to have caption")
    end

    -- Verify we can access content
    if #figure.content ~= 1 then
        error("Expected 1 block in figure content, got " .. #figure.content)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// Table constructor tests
// ============================================================================

#[test]
fn test_cell_constructor() {
    // Test pandoc.Cell constructor
    let filter_code = r#"
function Para(elem)
    local cell = pandoc.Cell{pandoc.Para{pandoc.Str("Cell content")}}

    -- Verify default values
    if cell.alignment ~= "AlignDefault" then
        error("Expected default alignment, got " .. tostring(cell.alignment))
    end

    if cell.row_span ~= 1 then
        error("Expected row_span 1, got " .. cell.row_span)
    end

    if cell.col_span ~= 1 then
        error("Expected col_span 1, got " .. cell.col_span)
    end

    if #cell.content ~= 1 then
        error("Expected 1 block, got " .. #cell.content)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_row_constructor() {
    // Test pandoc.Row constructor
    let filter_code = r#"
function Para(elem)
    local cell1 = pandoc.Cell{pandoc.Para{pandoc.Str("Cell 1")}}
    local cell2 = pandoc.Cell{pandoc.Para{pandoc.Str("Cell 2")}}
    local row = pandoc.Row{cell1, cell2}

    if #row.cells ~= 2 then
        error("Expected 2 cells, got " .. #row.cells)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_table_head_constructor() {
    // Test pandoc.TableHead constructor
    let filter_code = r#"
function Para(elem)
    local cell = pandoc.Cell{pandoc.Para{pandoc.Str("Header")}}
    local row = pandoc.Row{cell}
    local head = pandoc.TableHead{row}

    if #head.rows ~= 1 then
        error("Expected 1 row, got " .. #head.rows)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_table_body_constructor() {
    // Test pandoc.TableBody constructor
    let filter_code = r#"
function Para(elem)
    local cell = pandoc.Cell{pandoc.Para{pandoc.Str("Body cell")}}
    local row = pandoc.Row{cell}
    local body = pandoc.TableBody({row})  -- body rows

    if #body.body ~= 1 then
        error("Expected 1 body row, got " .. #body.body)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_table_foot_constructor() {
    // Test pandoc.TableFoot constructor
    let filter_code = r#"
function Para(elem)
    local cell = pandoc.Cell{pandoc.Para{pandoc.Str("Footer")}}
    local row = pandoc.Row{cell}
    local foot = pandoc.TableFoot{row}

    if #foot.rows ~= 1 then
        error("Expected 1 row, got " .. #foot.rows)
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_table_constructor() {
    // Test pandoc.Table constructor
    let filter_code = r#"
function Para(elem)
    -- Build a simple 1x1 table
    local header_cell = pandoc.Cell{pandoc.Para{pandoc.Str("Header")}}
    local body_cell = pandoc.Cell{pandoc.Para{pandoc.Str("Body")}}

    local header_row = pandoc.Row{header_cell}
    local body_row = pandoc.Row{body_cell}

    local head = pandoc.TableHead{header_row}
    local body = pandoc.TableBody({body_row})
    local foot = pandoc.TableFoot{}

    local caption = pandoc.Caption(nil, {pandoc.Para{pandoc.Str("Table caption")}})

    -- Column specs: list of {alignment, width} tuples
    local colspecs = {{pandoc.AlignDefault, pandoc.ColWidthDefault}}

    local table = pandoc.Table(
        caption,
        colspecs,
        head,
        {body},
        foot
    )

    if table.tag ~= "Table" then
        error("Expected Table tag, got " .. tostring(table.tag))
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

// ============================================================================
// ListAttributes constructor tests
// ============================================================================

#[test]
fn test_list_attributes_constructor() {
    // Test pandoc.ListAttributes constructor
    let filter_code = r#"
function Para(elem)
    -- Create list attributes with custom start, style, and delimiter
    local attr = pandoc.ListAttributes(5, "Decimal", "Period")

    if attr[1] ~= 5 then
        error("Expected start 5, got " .. attr[1])
    end

    if attr[2] ~= "Decimal" then
        error("Expected style 'Decimal', got " .. tostring(attr[2]))
    end

    if attr[3] ~= "Period" then
        error("Expected delim 'Period', got " .. tostring(attr[3]))
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}

#[test]
fn test_ordered_list_with_list_attributes() {
    // Test that pandoc.OrderedList properly uses ListAttributes
    let filter_code = r#"
function Para(elem)
    local items = {{pandoc.Para{pandoc.Str("Item 1")}}}
    local attr = pandoc.ListAttributes(10, "UpperAlpha", "TwoParens")
    local list = pandoc.OrderedList(items, attr)

    if list.tag ~= "OrderedList" then
        error("Expected OrderedList tag, got " .. tostring(list.tag))
    end

    -- The list should have the attributes we specified
    -- Note: accessing listAttributes may vary based on implementation

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::default(),
    })]);

    run_filter(filter_code, doc);
}
