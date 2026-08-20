/*
 * tests/integration/conditional_content_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Conditional content e2e (bd-fu16z22k, Phase 4).
 */

//! `.content-visible` / `.content-hidden` with `when-`/`unless-` ×
//! `format` / `profile` / `meta`, driven through the real `q2`
//! binary. Semantics ported from Quarto 1's `content-hidden.lua`:
//! condition kinds AND together, comma-separated values within one
//! condition OR (a q2 extension — Q1 only ever matches a single
//! value), `unless-*` negates, a bare `.content-hidden` always
//! hides, and surviving nodes keep their classes but lose the
//! condition attributes.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// Render `doc.qmd` (written from `body`, optional `front` matter)
/// inside a fresh default project; return the HTML.
fn render(body: &str, front: &str, extra: &[&str]) -> String {
    let dir = TempDir::new().unwrap();
    render_in(dir.path(), body, front, extra)
}

fn render_in(root: &Path, body: &str, front: &str, extra: &[&str]) -> String {
    std::fs::write(root.join("_quarto.yml"), "project:\n  type: default\n").unwrap();
    let doc = if front.is_empty() {
        body.to_string()
    } else {
        format!("---\n{front}---\n\n{body}")
    };
    std::fs::write(root.join("doc.qmd"), doc).unwrap();
    let out = Command::new(Q2_BIN)
        .arg("render")
        .arg(root.join("doc.qmd"))
        .arg("--quiet")
        .args(extra)
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(
        out.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read_to_string(root.join("doc.html")).expect("output exists")
}

// ── when-profile / unless-profile ───────────────────────────────────

#[test]
fn when_profile_div_shows_only_under_profile() {
    let body = "always-there\n\n\
                ::: {.content-visible when-profile=\"advanced\"}\nADVANCED-ONLY\n:::\n";
    let html = render(body, "", &["--profile", "advanced"]);
    assert!(
        html.contains("ADVANCED-ONLY"),
        "visible under the profile: {html}"
    );

    let html = render(body, "", &[]);
    assert!(html.contains("always-there"));
    assert!(
        !html.contains("ADVANCED-ONLY"),
        "hidden without the profile: {html}"
    );
}

#[test]
fn unless_profile_div_inverts() {
    let body = "::: {.content-visible unless-profile=\"advanced\"}\nBASIC-ONLY\n:::\n";
    let html = render(body, "", &[]);
    assert!(html.contains("BASIC-ONLY"));
    let html = render(body, "", &["--profile", "advanced"]);
    assert!(!html.contains("BASIC-ONLY"));
}

#[test]
fn content_hidden_when_profile() {
    let body = "::: {.content-hidden when-profile=\"advanced\"}\nSECRET\n:::\n";
    let html = render(body, "", &["--profile", "advanced"]);
    assert!(!html.contains("SECRET"), "hidden under the profile: {html}");
    let html = render(body, "", &[]);
    assert!(html.contains("SECRET"), "shown without it: {html}");
}

#[test]
fn when_profile_span_works_inline() {
    let body = "Text [secret words]{.content-visible when-profile=\"advanced\"} tail.\n";
    let html = render(body, "", &[]);
    assert!(!html.contains("secret words"), "span removed: {html}");
    assert!(html.contains("tail"), "surrounding text survives");
    let html = render(body, "", &["--profile", "advanced"]);
    assert!(html.contains("secret words"));
}

#[test]
fn comma_values_or_within_one_condition() {
    // q2 extension: comma-separated values OR (Q1 matches literally).
    let body = "::: {.content-visible when-profile=\"a,b\"}\nEITHER\n:::\n";
    let html = render(body, "", &["--profile", "b"]);
    assert!(html.contains("EITHER"));
    let html = render(body, "", &["--profile", "c"]);
    assert!(!html.contains("EITHER"));
}

// ── when-format / unless-format ─────────────────────────────────────

#[test]
fn when_format_matches_via_alias_table() {
    // `html` is an alias family: the concrete `html` target matches;
    // `pdf` does not.
    let body = "::: {.content-visible when-format=\"html\"}\nHTML-ONLY\n:::\n\
                ::: {.content-visible when-format=\"pdf\"}\nPDF-ONLY\n:::\n";
    let html = render(body, "", &[]);
    assert!(html.contains("HTML-ONLY"), "{html}");
    assert!(!html.contains("PDF-ONLY"), "{html}");
}

#[test]
fn unless_format_inverts() {
    let body = "::: {.content-hidden unless-format=\"pdf\"}\nPDF-BOUND\n:::\n";
    let html = render(body, "", &[]);
    assert!(!html.contains("PDF-BOUND"), "hidden under html: {html}");
}

// ── when-meta / unless-meta ─────────────────────────────────────────

#[test]
fn when_meta_dotted_path_truthiness() {
    let body = "::: {.content-visible when-meta=\"features.beta\"}\nBETA\n:::\n";
    let html = render(body, "features:\n  beta: true\n", &[]);
    assert!(html.contains("BETA"), "truthy meta shows: {html}");
    let html = render(body, "features:\n  beta: false\n", &[]);
    assert!(!html.contains("BETA"), "explicit false hides: {html}");
    let html = render(body, "", &[]);
    assert!(!html.contains("BETA"), "missing meta hides: {html}");
}

#[test]
fn when_meta_sees_profile_overlay_metadata() {
    // The documented Q1 pattern: profiles set metadata, when-meta
    // reads it — so profiles control content through config.
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(root.join("_quarto-beta.yml"), "features:\n  beta: true\n").unwrap();
    let body = "::: {.content-visible when-meta=\"features.beta\"}\nBETA\n:::\n";
    let html = render_in(root, body, "", &["--profile", "beta"]);
    assert!(html.contains("BETA"), "{html}");
}

// ── composition + structure ─────────────────────────────────────────

#[test]
fn conditions_of_different_kinds_and_together() {
    let body = "::: {.content-visible when-format=\"html\" when-profile=\"advanced\"}\nBOTH\n:::\n";
    let html = render(body, "", &["--profile", "advanced"]);
    assert!(html.contains("BOTH"), "both conditions hold: {html}");
    let html = render(body, "", &[]);
    assert!(!html.contains("BOTH"), "profile condition fails: {html}");
}

#[test]
fn bare_content_hidden_always_hides() {
    let body = "::: {.content-hidden}\nNEVER\n:::\nvisible-text\n";
    let html = render(body, "", &[]);
    assert!(!html.contains("NEVER"));
    assert!(html.contains("visible-text"));
}

#[test]
fn surviving_div_loses_condition_attributes() {
    let body = "::: {.content-visible when-profile=\"advanced\"}\nKEPT\n:::\n";
    let html = render(body, "", &["--profile", "advanced"]);
    assert!(html.contains("KEPT"));
    assert!(
        !html.contains("when-profile"),
        "condition attributes must not leak into HTML: {html}"
    );
}

#[test]
fn hidden_float_does_not_consume_a_crossref_number() {
    let body = "::: {.content-hidden when-profile=\"prod\"}\n\
                ::: {#fig-first}\nhidden content\n\nHidden caption\n:::\n\
                :::\n\n\
                ::: {#fig-second}\nvisible content\n\nVisible caption\n:::\n\n\
                See @fig-second.\n";
    let html = render(body, "", &["--profile", "prod"]);
    assert!(
        html.contains("Figure&nbsp;1") || html.contains("Figure 1"),
        "the only visible figure must be number 1: {html}"
    );
    assert!(!html.contains("Hidden caption"), "{html}");
}

#[test]
fn nested_conditionals_compose() {
    let body = "::: {.content-visible when-format=\"html\"}\nouter\n\n\
                ::: {.content-visible when-profile=\"advanced\"}\ninner\n:::\n\
                :::\n";
    let html = render(body, "", &[]);
    assert!(html.contains("outer"));
    assert!(!html.contains("inner"));
    let html = render(body, "", &["--profile", "advanced"]);
    assert!(html.contains("outer") && html.contains("inner"));
}

#[test]
fn misspelled_condition_attribute_warns() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(root.join("_quarto.yml"), "project:\n  type: default\n").unwrap();
    std::fs::write(
        root.join("doc.qmd"),
        "::: {.content-visible when-profil=\"x\"}\nBODY\n:::\n",
    )
    .unwrap();
    let out = Command::new(Q2_BIN)
        .arg("render")
        .arg(root.join("doc.qmd"))
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(out.status.success(), "a typo warns, it does not abort");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Q-2-42") && stderr.contains("when-profil"),
        "must warn about the unknown condition attribute: {stderr}"
    );
}
