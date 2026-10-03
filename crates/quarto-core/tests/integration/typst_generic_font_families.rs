//! Typst has no CSS generic font families, so `typst_css.lua` resolves each
//! generic keyword (`sans-serif`, `monospace`, `system-ui`, ...) to the first
//! *available* font from a curated candidate list instead of passing the
//! keyword through (which made every compile warn `unknown font family`).
//!
//! The resolver is a pure function (`resolve_font_families`), exercised here
//! with synthetic availability sets through a standalone `pandoc lua` run, so
//! the assertions don't depend on which fonts the host machine has.

use quarto_core::pandoc_filters::harness::assert_pandoc_available;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn typst_css_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/pandoc-filters/filters/modules/typst_css.lua")
}

/// Runs `body` with `resolve(families, available)` in scope, where
/// `families` is a list of names and `available` a list of font names (or
/// `nil` for "unknown"). `resolve` returns the result joined by `|`.
fn run_lua(body: &str) -> String {
    assert_pandoc_available();
    let path = typst_css_path().to_string_lossy().replace('\\', "\\\\");
    let script = format!(
        r#"
param = function() return nil end
quarto = {{}}
local css = dofile("{path}")
local function resolve(families, available)
  local set = nil
  if available then
    set = {{}}
    for _, n in ipairs(available) do set[n:lower()] = true end
  end
  return table.concat(css.resolve_font_families(families, set), '|')
end
{body}
"#
    );
    let mut f = tempfile::Builder::new().suffix(".lua").tempfile().unwrap();
    f.write_all(script.as_bytes()).unwrap();
    let out = Command::new("pandoc")
        .arg("lua")
        .arg(f.path())
        .output()
        .expect("failed to execute pandoc lua");
    assert!(
        out.status.success(),
        "lua failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn first_available_candidate_wins_in_list_order() {
    // Both Arial and Helvetica present (macOS): the earlier candidate wins.
    let out = run_lua("print(resolve({'sans-serif'}, {'Helvetica', 'Arial'}))");
    assert_eq!(out, "Arial");
    let out = run_lua("print(resolve({'sans-serif'}, {'Helvetica', 'DejaVu Sans'}))");
    assert_eq!(out, "Helvetica");
}

#[test]
fn keyword_is_replaced_in_place_mid_list() {
    let out = run_lua(
        "print(resolve({'Roboto', 'sans-serif', 'Georgia'}, {'Roboto', 'Arial', 'Georgia'}))",
    );
    assert_eq!(out, "Roboto|Arial|Georgia");
}

#[test]
fn keyword_never_emitted_even_when_nothing_matches() {
    // sans-serif has no built-in target: with nothing available it is dropped
    // and the terminal fallback (Typst's default text font) is used, because
    // Typst rejects an empty font list.
    let out = run_lua("print(resolve({'sans-serif'}, {'Libertinus Serif'}))");
    assert_eq!(out, "Libertinus Serif");
    // Unknown availability (no typst-available-fonts param): also dropped.
    let out = run_lua("print(resolve({'Roboto', 'monospace'}, nil))");
    assert_eq!(out, "Roboto");
}

#[test]
fn builtin_targets_always_resolve() {
    let builtins = "{'DejaVu Sans Mono', 'Libertinus Serif', 'New Computer Modern', 'New Computer Modern Math'}";
    let out = run_lua(&format!("print(resolve({{'monospace'}}, {builtins}))"));
    assert_eq!(out, "DejaVu Sans Mono");
    let out = run_lua(&format!("print(resolve({{'serif'}}, {builtins}))"));
    assert_eq!(out, "Libertinus Serif");
    let out = run_lua(&format!("print(resolve({{'math'}}, {builtins}))"));
    assert_eq!(out, "New Computer Modern Math");
}

#[test]
fn matching_is_case_insensitive() {
    let out = run_lua("print(resolve({'SANS-SERIF'}, {'arial'}))");
    assert_eq!(out, "Arial");
}

#[test]
fn system_ui_and_ui_variants_resolve() {
    let out = run_lua("print(resolve({'system-ui'}, {'Segoe UI', 'Arial'}))");
    assert_eq!(out, "Segoe UI");
    // system-ui falls through to the plain sans-serif candidates.
    let out = run_lua("print(resolve({'system-ui'}, {'Arial'}))");
    assert_eq!(out, "Arial");
    let out = run_lua("print(resolve({'ui-monospace'}, {'SF Mono', 'Menlo'}))");
    assert_eq!(out, "SF Mono");
    let out = run_lua("print(resolve({'ui-serif'}, {'Times New Roman'}))");
    assert_eq!(out, "Times New Roman");
}

#[test]
fn script_specific_generics_are_dropped() {
    let out = run_lua("print(resolve({'generic(kai)', 'Arial'}, {'Arial'}))");
    assert_eq!(out, "Arial");
}

#[test]
fn duplicates_are_collapsed() {
    let out = run_lua("print(resolve({'Arial', 'sans-serif'}, {'Arial'}))");
    assert_eq!(out, "Arial");
}

#[test]
fn unavailable_named_fonts_still_filtered_or_kept_as_before() {
    // Available named fonts survive, unavailable ones drop.
    let out = run_lua("print(resolve({'Nope', 'Arial'}, {'Arial'}))");
    assert_eq!(out, "Arial");
    // Nothing available at all: keep the user's names (Typst will warn about
    // them, which is the honest signal), rather than invent a font.
    let out = run_lua("print(resolve({'Nope'}, {'Arial'}))");
    assert_eq!(out, "Nope");
}
