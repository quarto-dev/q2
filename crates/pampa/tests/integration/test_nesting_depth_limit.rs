/*
 * test_nesting_depth_limit.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The qmd reader rejects documents whose concrete syntax tree is nested too
 * deeply. The guard exists so that fuzzer-style inputs produce a diagnostic
 * instead of overflowing the stack in the recursive code that runs after
 * conversion (filters, writers, recursive `Drop`).
 *
 * These tests pin the exact acceptance threshold (a document is accepted iff
 * its deepest CST node is at depth <= 99, counting the root as 1) against an
 * independent, test-local depth computation, so that the guard can move
 * between implementations without shifting the boundary (bd-t7i6oanu).
 */

use pampa::readers;
use quarto_error_reporting::DiagnosticMessage;
use tree_sitter_qmd::MarkdownParser;

/// Deepest node depth permitted by the reader, counting the root as 1.
const MAX_ACCEPTED_DEPTH: usize = 99;

/// Depth of the deepest node in the CST of `input`, counting the root as 1.
///
/// A straight cursor walk, deliberately independent of the reader's own
/// implementation of the guard.
fn reference_cst_depth(input: &str) -> usize {
    let mut parser = MarkdownParser::default();
    let tree = parser.parse(input.as_bytes(), None).expect("parse");
    let mut cursor = tree.walk_cursor();
    let (mut depth, mut max) = (1usize, 1usize);
    loop {
        if cursor.goto_first_child() {
            depth += 1;
            max = max.max(depth);
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return max;
            }
            depth -= 1;
        }
    }
}

fn nested_blockquotes(levels: usize) -> String {
    format!("{} x\n", ">".repeat(levels))
}

/// `levels` nested bracketed spans. Unlike blockquotes and lists, which the
/// grammar stops accepting after a couple hundred levels, spans nest to any
/// depth without a parse error, so they reach the depth guard.
fn nested_spans(levels: usize) -> String {
    format!("{}x{}\n", "[".repeat(levels), "]{.c}".repeat(levels))
}

fn read(input: &str) -> Result<(), Vec<DiagnosticMessage>> {
    let mut sink = std::io::sink();
    readers::qmd::read(input.as_bytes(), false, "test.qmd", &mut sink, true, None).map(|_| ())
}

fn assert_rejected_as_too_deep(result: Result<(), Vec<DiagnosticMessage>>, what: &str) {
    let errors = match result {
        Ok(()) => panic!("{what}: expected the depth guard to reject the document"),
        Err(errors) => errors,
    };
    assert_eq!(errors.len(), 1, "{what}: {errors:#?}");
    let text = errors[0].to_text(None);
    assert!(
        text.contains("too deeply nested"),
        "{what}: unexpected diagnostic: {text}"
    );
}

/// The largest number of nested blockquotes whose CST depth is accepted,
/// found with the reference walk rather than hard-coded, so the test keeps
/// pinning the depth threshold if the grammar's node shape changes.
fn deepest_accepted_blockquote_nesting() -> usize {
    let levels = (1..=MAX_ACCEPTED_DEPTH)
        .take_while(|&n| reference_cst_depth(&nested_blockquotes(n)) <= MAX_ACCEPTED_DEPTH)
        .last()
        .expect("a single blockquote is shallow enough");
    // Each `>` adds exactly one node on the deepest path, so both sides of
    // the threshold are reachable exactly.
    assert_eq!(
        reference_cst_depth(&nested_blockquotes(levels)),
        MAX_ACCEPTED_DEPTH
    );
    assert_eq!(
        reference_cst_depth(&nested_blockquotes(levels + 1)),
        MAX_ACCEPTED_DEPTH + 1
    );
    levels
}

#[test]
fn deepest_accepted_document_parses() {
    let levels = deepest_accepted_blockquote_nesting();
    let input = nested_blockquotes(levels);
    if let Err(errors) = read(&input) {
        panic!("depth {MAX_ACCEPTED_DEPTH} must be accepted: {errors:#?}");
    }
}

#[test]
fn one_level_deeper_is_rejected() {
    let levels = deepest_accepted_blockquote_nesting() + 1;
    assert_rejected_as_too_deep(
        read(&nested_blockquotes(levels)),
        &format!("depth {}", MAX_ACCEPTED_DEPTH + 1),
    );
}

#[test]
fn rejection_message_states_the_limit() {
    let levels = deepest_accepted_blockquote_nesting() + 1;
    let errors = read(&nested_blockquotes(levels)).expect_err("must be rejected");
    let text = errors[0].to_text(None);
    assert!(
        text.contains(&format!("more than {MAX_ACCEPTED_DEPTH} levels")),
        "diagnostic should state the enforced limit: {text}"
    );
}

/// A pathologically deep document must come back as a diagnostic, not a
/// stack overflow. Runs on a thread with an explicit stack size so the result
/// does not depend on the test harness's default.
#[test]
fn pathologically_deep_document_is_rejected_without_overflow() {
    let input = nested_spans(5_000);
    assert!(reference_cst_depth(&input) > 100 * MAX_ACCEPTED_DEPTH);
    let result = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || read(&input))
        .expect("spawn")
        .join()
        .expect("reader thread must not crash");
    assert_rejected_as_too_deep(result, "5k nested spans");
}
