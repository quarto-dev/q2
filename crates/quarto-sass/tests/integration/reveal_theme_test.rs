//! RevealJS theme assembly + compilation tests (Stage A of bd-r9mkybwl).
//!
//! Verifies that `assemble_reveal_scss` produces a SCSS bundle that:
//! - compiles cleanly through `grass` (our native SCSS compiler),
//! - emits the reveal `--r-*` custom properties carrying Quarto's overridden
//!   values (not reveal's defaults), and
//! - includes the look-fixing rules that distinguish a Quarto deck from a stock
//!   reveal deck (left-aligned slides, non-uppercase headings, Quarto title slide).

use quarto_sass::bundle::{assemble_reveal_scss, load_quarto_reveal_layer, load_reveal_framework};

/// Compile assembled reveal SCSS through grass (expanded output for assertions).
fn compile(scss: &str) -> String {
    let options = grass::Options::default();
    grass::from_string(scss, &options).unwrap_or_else(|e| panic!("reveal SCSS should compile: {e}"))
}

#[test]
fn reveal_framework_layer_loads() {
    let fw = load_reveal_framework().unwrap();
    assert!(fw.uses.contains("sass:color"), "framework uses sass:color");
    assert!(
        fw.defaults.contains("$heading-text-transform"),
        "framework defaults declare reveal vars"
    );
    assert!(
        fw.rules.contains("--r-main-color"),
        "framework rules include the :root --r-* emitter"
    );
    assert!(
        fw.rules.contains("var(--r-main-color)"),
        "framework rules include the theme rule set"
    );
    assert!(
        fw.mixins.contains("dark-bg-text-color"),
        "framework mixins include reveal mixins"
    );
}

#[test]
fn quarto_reveal_layer_parses() {
    let q = load_quarto_reveal_layer().unwrap();
    assert!(
        q.defaults.contains("$presentation-slide-text-align"),
        "quarto layer declares the presentation vocabulary"
    );
    assert!(
        q.rules.contains("#title-slide"),
        "quarto layer rules include title-slide layout"
    );
}

#[test]
fn reveal_theme_compiles() {
    let scss = assemble_reveal_scss(None).unwrap();
    let css = compile(&scss);
    assert!(!css.is_empty(), "compiled reveal CSS is non-empty");
    // sanity: the reveal base rule set made it through
    assert!(
        css.contains(".reveal-viewport"),
        "includes reveal base rules"
    );
}

#[test]
fn quarto_values_flow_into_custom_properties() {
    let css = compile(&assemble_reveal_scss(None).unwrap());

    // Quarto's body color (#222) must win over reveal's default (#eee).
    assert!(
        css.contains("--r-main-color: #222"),
        "Quarto body color should drive --r-main-color\n{css}"
    );
    // Quarto's background (white) must win over reveal's default (#2b2b2b / #bbb).
    assert!(
        css.contains("--r-background-color: #fff"),
        "Quarto body-bg should drive --r-background-color"
    );
    // Collision case: reveal's own $link-color is set from Quarto's $primary.
    assert!(
        css.contains("--r-link-color: #2a76dd"),
        "Quarto primary should drive --r-link-color"
    );
}

#[test]
fn headings_are_not_uppercased() {
    let css = compile(&assemble_reveal_scss(None).unwrap());
    // Quarto turns OFF reveal's default uppercase headings.
    assert!(
        css.contains("--r-heading-text-transform: none"),
        "heading text-transform should be none"
    );
    assert!(
        !css.contains("uppercase"),
        "no uppercase anywhere in the compiled Quarto reveal theme\n{css}"
    );
}

#[test]
fn slides_are_left_aligned() {
    let css = compile(&assemble_reveal_scss(None).unwrap());
    // The Quarto rule `.reveal .slides { text-align: left }` must be present.
    assert!(
        css.contains("text-align: left"),
        "slides should be left aligned\n{css}"
    );
}

#[test]
fn title_slide_is_centered_and_resized() {
    let css = compile(&assemble_reveal_scss(None).unwrap());
    assert!(css.contains("#title-slide"), "title-slide rules present");
    assert!(
        css.contains("text-align: center"),
        "title slide centered\n{css}"
    );
}
