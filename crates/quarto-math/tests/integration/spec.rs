//! The q2-owned command spec (`spec/commands.json`).
//!
//! The file is generated (`cargo xtask gen-math-spec`) from mitex's spec dump
//! plus `spec/overrides.json`; these tests keep it honest: no drift from a
//! regeneration, full coverage of the upstream names, valid symbol text, and
//! a reviewable inventory of what is still unsupported. The last test is the
//! acceptance bar for the overrides: every command the fixture corpus uses
//! (outside deliberate errors and fixture-local macros) must have writer
//! semantics.

use std::collections::BTreeSet;

use quarto_math::spec::generator::generate;
use quarto_math::spec::{Semantics, Spec, SpecFile};

use crate::fixture_corpus::load_all;

const UPSTREAM: &str = include_str!("../../spec/upstream/mitex-default-spec.json");
const OVERRIDES: &str = include_str!("../../spec/overrides.json");
const COMMITTED: &str = include_str!("../../spec/commands.json");

fn committed() -> SpecFile {
    serde_json::from_str(COMMITTED).expect("committed commands.json parses")
}

#[test]
fn committed_spec_matches_a_regeneration() {
    let regenerated = generate(UPSTREAM, OVERRIDES).expect("generation succeeds");
    assert!(
        regenerated == COMMITTED,
        "spec/commands.json is stale; run `cargo xtask gen-math-spec`"
    );
}

#[test]
fn every_upstream_name_has_a_row() {
    let upstream: serde_json::Value = serde_json::from_str(UPSTREAM).unwrap();
    let file = committed();
    let missing: Vec<&str> = upstream["commands"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| !file.commands.contains_key(k.as_str()))
        .map(String::as_str)
        .collect();
    assert!(
        missing.is_empty(),
        "upstream names without a row: {missing:?}"
    );
    assert!(file.commands.len() >= 995);
}

#[test]
fn symbol_texts_are_non_empty_and_have_no_ascii_letters_where_literal() {
    for (name, row) in &committed().commands {
        if let Some(text) = row.sem.text() {
            assert!(!text.is_empty(), "{name}: empty symbol text");
            assert!(
                text.chars().count() <= 4,
                "{name}: symbol text {text:?} looks like a Typst expression, not a symbol"
            );
        }
        if let Semantics::Space { em } = row.sem {
            assert!(em.abs() <= 4.0, "{name}: implausible space {em}em");
        }
    }
}

#[test]
fn builtin_spec_is_the_committed_file() {
    let spec = Spec::builtin();
    assert_eq!(spec.len(), committed().commands.len());
    assert!(matches!(
        spec.semantics("frac"),
        Some(Semantics::Frac { .. })
    ));
    assert!(matches!(spec.semantics("alpha"), Some(Semantics::Sym { text }) if text == "α"));
    assert!(matches!(spec.semantics("sum"), Some(Semantics::Nary { text, .. }) if text == "∑"));
    assert!(matches!(
        spec.semantics("aligned"),
        Some(Semantics::Env { .. })
    ));
}

/// Reviewable inventory of rows without writer semantics. A change here is
/// either an override landing (good) or a regression (not good).
#[test]
fn unsupported_inventory() {
    let file = committed();
    let mut by_reason: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (name, row) in &file.commands {
        if let Semantics::Unsupported { why } = &row.sem {
            by_reason.entry(why.clone()).or_default().push(name.clone());
        }
    }
    let rendered: Vec<String> = by_reason
        .iter()
        .map(|(why, names)| format!("{why} ({}): {}", names.len(), names.join(" ")))
        .collect();
    insta::assert_snapshot!("unsupported_inventory", rendered.join("\n"));
}

/// Commands the corpus uses. Fixture-local macros (`\newcommand{\pr}…`) and
/// the `errors/` group (unknown commands on purpose) are excluded.
fn corpus_commands() -> BTreeSet<String> {
    let mut used = BTreeSet::new();
    for fx in load_all() {
        if fx.group == "errors" {
            continue;
        }
        let defined: BTreeSet<String> = find_defined_macros(&fx.text);
        for name in find_commands(&fx.text) {
            // `\begin`/`\end`/`\left`/`\right` are lexer-level syntax in
            // mitex, never spec rows.
            if matches!(name.as_str(), "begin" | "end" | "left" | "right") {
                continue;
            }
            if !defined.contains(&name) {
                used.insert(name);
            }
        }
    }
    used
}

fn find_commands(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
                end += 1;
            }
            if end > start {
                // Starred forms are their own spec rows.
                let mut name = text[start..end].to_string();
                if end < bytes.len() && bytes[end] == b'*' {
                    name.push('*');
                    i = end + 1;
                } else {
                    i = end;
                }
                out.push(name);
                continue;
            }
            // Control symbols: `\,` `\;` `\!` `\ ` `\{` `\|` etc.
            if start < bytes.len() {
                let c = bytes[start] as char;
                if !c.is_ascii_alphabetic() && c != '\\' {
                    out.push(c.to_string());
                }
                i = start + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn find_defined_macros(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for marker in [
        "\\newcommand{\\",
        "\\newcommand*{\\",
        "\\renewcommand{\\",
        "\\def\\",
    ] {
        let mut rest = text;
        while let Some(pos) = rest.find(marker) {
            let after = &rest[pos + marker.len()..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            if !name.is_empty() {
                out.insert(name);
            }
            rest = after;
        }
    }
    out
}

#[test]
fn every_corpus_command_has_writer_semantics() {
    let spec = Spec::builtin();
    let mut missing = Vec::new();
    let mut unsupported = Vec::new();
    for name in corpus_commands() {
        match spec.semantics(&name) {
            None => missing.push(name),
            Some(Semantics::Unsupported { why }) => unsupported.push(format!("{name} ({why})")),
            Some(_) => {}
        }
    }
    assert!(
        missing.is_empty() && unsupported.is_empty(),
        "corpus commands without semantics:\n  missing: {missing:?}\n  unsupported: {unsupported:?}"
    );
}
