/*
 * test_lua_constructors.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for missing Lua element constructors (Phase 1 of Pandoc Lua API port).
 */

// Tests require the lua-filter feature
#![cfg(feature = "lua-filter")]

use pampa::lua::apply_lua_filters;
use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{Block, Inline, Pandoc, Paragraph, Str};
use std::io::Write;
use tempfile::NamedTempFile;

/// Helper to create a simple Pandoc document with a paragraph
fn create_test_doc(content: Vec<Inline>) -> Pandoc {
    Pandoc {
        meta: Default::default(),
        blocks: vec![Block::Paragraph(Paragraph {
            content,
            source_info: quarto_source_map::SourceInfo::for_test(),
        })],
    }
}

/// Helper to run a filter and assert success
async fn run_filter(filter_code: &str, doc: Pandoc) -> (Pandoc, ASTContext) {
    let mut filter_file = NamedTempFile::new().expect("Failed to create temp file");
    filter_file
        .write_all(filter_code.as_bytes())
        .expect("Failed to write filter");

    let context = ASTContext::anonymous();
    let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
    let result = apply_lua_filters(
        doc,
        context,
        &[filter_file.path().to_path_buf()],
        "html",
        runtime,
        None,
    )
    .await;
    let output = result.expect("Filter failed");
    let (pandoc, context) = (output.pandoc, output.context);
    (pandoc, context)
}

/// Helper to run a filter and return the collected diagnostics.
async fn run_filter_diagnostics(
    filter_code: &str,
    doc: Pandoc,
) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    let mut filter_file = NamedTempFile::new().expect("Failed to create temp file");
    filter_file
        .write_all(filter_code.as_bytes())
        .expect("Failed to write filter");

    let context = ASTContext::anonymous();
    let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
    apply_lua_filters(
        doc,
        context,
        &[filter_file.path().to_path_buf()],
        "html",
        runtime,
        None,
    )
    .await
    .expect("Filter failed")
    .diagnostics
}

/// Helper to run a filter that is expected to fail; returns the error text.
async fn run_filter_expect_error(filter_code: &str, doc: Pandoc) -> String {
    let mut filter_file = NamedTempFile::new().expect("Failed to create temp file");
    filter_file
        .write_all(filter_code.as_bytes())
        .expect("Failed to write filter");

    let context = ASTContext::anonymous();
    let runtime: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
    let result = apply_lua_filters(
        doc,
        context,
        &[filter_file.path().to_path_buf()],
        "html",
        runtime,
        None,
    )
    .await;
    match result {
        Ok(_) => panic!("expected the filter to fail, but it succeeded"),
        Err(e) => e.to_string(),
    }
}

// ============================================================================
// Cite and Citation constructor tests
// ============================================================================

#[tokio::test]
async fn test_cite_constructor() {
    // Test pandoc.Cite(content, citations) constructor (Pandoc arg order)
    let filter_code = r#"
function Para(elem)
    -- Create a citation
    local citation = pandoc.Citation(
        "knuth1984",           -- id
        "NormalCitation"       -- mode
    )

    -- Create a Cite inline with the citation
    local cite = pandoc.Cite(
        {pandoc.Str("Knuth")}, -- content
        {citation}             -- citations list
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_citation_constructor_all_args() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

// ============================================================================
// DefinitionList constructor tests
// ============================================================================

#[tokio::test]
async fn test_definition_list_constructor() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

// ============================================================================
// LineBlock constructor tests
// ============================================================================

#[tokio::test]
async fn test_line_block_constructor() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

// ============================================================================
// Figure and Caption constructor tests
// ============================================================================

#[tokio::test]
async fn test_caption_constructor() {
    // Test pandoc.Caption constructor
    let filter_code = r#"
function Para(elem)
    -- Create a caption with short and long forms
    local caption = pandoc.Caption(
        {pandoc.Para{pandoc.Str("Long"), pandoc.Space(), pandoc.Str("caption")}},  -- long
        {pandoc.Str("Short")}  -- short
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_figure_constructor() {
    // Test pandoc.Figure constructor
    let filter_code = r#"
function Para(elem)
    -- Create a figure with caption
    local caption = pandoc.Caption(
        {pandoc.Para{pandoc.Str("Figure caption")}}  -- long; no short caption
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

// ============================================================================
// Table constructor tests
// ============================================================================

#[tokio::test]
async fn test_cell_constructor() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_row_constructor() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_table_head_constructor() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_table_body_constructor() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_table_foot_constructor() {
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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_table_constructor() {
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

    local caption = pandoc.Caption({pandoc.Para{pandoc.Str("Table caption")}})

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
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

// ============================================================================
// ListAttributes constructor tests
// ============================================================================

#[tokio::test]
async fn test_list_attributes_constructor() {
    // Test pandoc.ListAttributes constructor
    let filter_code = r#"
function Para(elem)
    -- Create list attributes with custom start, style, and delimiter
    -- (typed userdata with named properties, matching Pandoc)
    local attr = pandoc.ListAttributes(5, "Decimal", "Period")

    if attr.start ~= 5 then
        error("Expected start 5, got " .. attr.start)
    end

    if attr.style ~= "Decimal" then
        error("Expected style 'Decimal', got " .. tostring(attr.style))
    end

    if attr.delimiter ~= "Period" then
        error("Expected delim 'Period', got " .. tostring(attr.delimiter))
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

#[tokio::test]
async fn test_ordered_list_with_list_attributes() {
    // Test that pandoc.OrderedList properly uses ListAttributes
    let filter_code = r#"
function Para(elem)
    local items = {{pandoc.Para{pandoc.Str("Item 1")}}}
    local attr = pandoc.ListAttributes(10, "UpperAlpha", "TwoParens")
    local list = pandoc.OrderedList(items, attr)

    if list.tag ~= "OrderedList" then
        error("Expected OrderedList tag, got " .. tostring(list.tag))
    end

    -- The list must carry the attributes we specified (bd-0xghpvij:
    -- the constructor used to silently discard its second argument)
    if list.start ~= 10 then
        error("Expected start 10, got " .. tostring(list.start))
    end
    if list.style ~= "UpperAlpha" then
        error("Expected style UpperAlpha, got " .. tostring(list.style))
    end

    return elem
end
"#;

    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);

    run_filter(filter_code, doc).await;
}

// ============================================================================
// SimpleTable: deliberate divergence (bd-d4wd6r3i, epic-plan Decision 6).
// q2 does not implement the legacy pre-pandoc-2.10 simple-table API; all
// three entry points raise an actionable Q-11-2 error pointing at
// pandoc.Table. Registry: crates/pampa/tests/lua-conformance/divergences.md
// ============================================================================

fn simpletable_doc() -> Pandoc {
    create_test_doc(vec![Inline::Str(Str {
        text: "test".to_string(),
        source_info: quarto_source_map::SourceInfo::for_test(),
    })])
}

fn assert_simpletable_divergence_error(err: &str, entry_point: &str) {
    assert!(
        err.contains("Q-11-2"),
        "{entry_point}: expected Q-11-2 in error, got: {err}"
    );
    assert!(
        err.contains("pandoc.Table"),
        "{entry_point}: expected pointer to pandoc.Table in error, got: {err}"
    );
}

#[tokio::test]
async fn test_simpletable_constructor_raises_divergence_error() {
    let filter_code = r#"
function Para(elem)
    pandoc.SimpleTable({}, {}, {}, {}, {})
    return elem
end
"#;
    let err = run_filter_expect_error(filter_code, simpletable_doc()).await;
    assert_simpletable_divergence_error(&err, "pandoc.SimpleTable");
}

#[tokio::test]
async fn test_utils_to_simple_table_raises_divergence_error() {
    let filter_code = r#"
function Para(elem)
    pandoc.utils.to_simple_table(pandoc.Table(
        {long = {}},
        {{pandoc.AlignDefault, nil}},
        pandoc.TableHead(),
        {},
        pandoc.TableFoot()
    ))
    return elem
end
"#;
    let err = run_filter_expect_error(filter_code, simpletable_doc()).await;
    assert_simpletable_divergence_error(&err, "pandoc.utils.to_simple_table");
}

#[tokio::test]
async fn test_utils_from_simple_table_raises_divergence_error() {
    let filter_code = r#"
function Para(elem)
    pandoc.utils.from_simple_table({})
    return elem
end
"#;
    let err = run_filter_expect_error(filter_code, simpletable_doc()).await;
    assert_simpletable_divergence_error(&err, "pandoc.utils.from_simple_table");
}

// ============================================================================
// Marshaling error contract (bd-9p2686pc): granular Q-codes.
// Q-11-3 invalid argument, Q-11-4 invalid filter return,
// Q-11-5 invalid property assignment.
// ============================================================================

#[tokio::test]
async fn test_filter_return_error_is_q_coded() {
    let filter_code = r#"
function Str(elem)
    return 5
end
"#;
    let err = run_filter_expect_error(filter_code, simpletable_doc()).await;
    assert!(err.contains("Q-11-4"), "expected Q-11-4 in: {err}");
    assert!(err.contains("'Str'"), "expected filter fn name in: {err}");
    assert!(err.contains("got number"), "expected got-type in: {err}");
    assert!(
        !err.contains("got number, got number"),
        "got-type stated twice in: {err}"
    );
}

#[tokio::test]
async fn test_doc_level_handler_emits_unimplemented_warning() {
    // bd-2llqjsms / bd-a9g50za2 are still open: doc-level filter
    // functions (Pandoc/Meta/Doc) are collected but never invoked.
    // Until they are implemented, defining one must produce a loud
    // Q-11-6 warning instead of a silent no-op.
    for handler in ["Meta", "Pandoc", "Doc"] {
        let filter_code = format!(
            r#"
function Str(elem)
    return elem
end
function {handler}(x)
    return x
end
"#
        );
        let doc = create_test_doc(vec![Inline::Str(Str {
            text: "hi".to_string(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })]);
        let diags = run_filter_diagnostics(&filter_code, doc).await;
        let warning = diags
            .iter()
            .find(|d| d.code.as_deref() == Some("Q-11-6"))
            .unwrap_or_else(|| panic!("no Q-11-6 diagnostic for '{handler}' in: {diags:?}"));
        assert!(
            warning.title.contains(&format!("'{handler}'")),
            "{handler}: title does not name the handler: {}",
            warning.title
        );
        assert!(
            matches!(
                warning.kind,
                quarto_error_reporting::DiagnosticKind::Warning
            ),
            "{handler}: expected a warning, got {:?}",
            warning.kind
        );
    }
}

#[tokio::test]
async fn test_element_only_filter_has_no_unimplemented_warning() {
    let filter_code = r#"
function Str(elem)
    return elem
end
"#;
    let doc = create_test_doc(vec![Inline::Str(Str {
        text: "hi".to_string(),
        source_info: quarto_source_map::SourceInfo::for_test(),
    })]);
    let diags = run_filter_diagnostics(filter_code, doc).await;
    assert!(
        diags.iter().all(|d| d.code.as_deref() != Some("Q-11-6")),
        "unexpected Q-11-6 diagnostic: {diags:?}"
    );
}

#[tokio::test]
async fn test_readonly_field_assignment_is_q_coded() {
    let filter_code = r#"
function Para(elem)
    elem.tag = "Div"
    return elem
end
"#;
    let err = run_filter_expect_error(filter_code, simpletable_doc()).await;
    assert!(err.contains("Q-11-5"), "expected Q-11-5 in: {err}");
    assert!(err.contains("read-only"), "expected read-only in: {err}");
}

#[tokio::test]
async fn test_unknown_field_assignment_is_q_coded() {
    let filter_code = r#"
function Para(elem)
    elem.bogus_field = 1
    return elem
end
"#;
    let err = run_filter_expect_error(filter_code, simpletable_doc()).await;
    assert!(err.contains("Q-11-5"), "expected Q-11-5 in: {err}");
    assert!(err.contains("bogus_field"), "expected field name in: {err}");
}
