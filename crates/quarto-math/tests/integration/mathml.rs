//! `MathAst` → MathML Core (bd-9z83tcv0).
//!
//! Every fixture is snapshotted, and the whole corpus is checked for
//! *validity* the way the Typst corpus is checked by compiling: each
//! output must be well-formed XML, use only elements MathML Core defines
//! (plus `menclose`, decision 3 of the plan), give every fixed-arity
//! element exactly the children it takes, and never set `mathvariant` to
//! anything but `normal`. Then the syntax choices are asserted directly.

use std::collections::BTreeSet;

use quick_xml::Reader;
use quick_xml::events::Event;

use quarto_math::mathml::to_mathml;
use quarto_math::normalize::{Mode, normalize};
use quarto_math::spec::Spec;

use crate::fixture_corpus::{Fixture, load_all};

fn mml(text: &str) -> String {
    to_mathml(&normalize(text, Mode::Inline, Spec::builtin()))
}

fn mml_display(text: &str) -> String {
    to_mathml(&normalize(text, Mode::Display, Spec::builtin()))
}

/// The body between `<semantics>` and `<annotation`, so assertions read
/// the structure without the boilerplate.
fn body(text: &str) -> String {
    let full = mml(text);
    inner(&full)
}

fn body_display(text: &str) -> String {
    inner(&mml_display(text))
}

fn inner(full: &str) -> String {
    let start = full.find("<semantics>").expect("semantics") + "<semantics>".len();
    let end = full.find("<annotation").expect("annotation");
    full[start..end].to_string()
}

fn mode_of(fx: &Fixture) -> Mode {
    if fx.display {
        Mode::Display
    } else {
        Mode::Inline
    }
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected {needle:?} in:\n{haystack}"
    );
}

// ---------------------------------------------------------------------------
// Validity
// ---------------------------------------------------------------------------

/// Elements MathML Core defines, plus `menclose` (decision 3).
const CORE_ELEMENTS: &[&str] = &[
    "math",
    "semantics",
    "annotation",
    "mrow",
    "mi",
    "mn",
    "mo",
    "mtext",
    "mspace",
    "ms",
    "msub",
    "msup",
    "msubsup",
    "munder",
    "mover",
    "munderover",
    "mfrac",
    "msqrt",
    "mroot",
    "mtable",
    "mtr",
    "mtd",
    "mstyle",
    "mpadded",
    "mphantom",
    "merror",
    "menclose",
];

/// Elements whose child count MathML fixes.
fn required_children(name: &str) -> Option<usize> {
    Some(match name {
        "mfrac" | "msub" | "msup" | "munder" | "mover" | "mroot" => 2,
        "msubsup" | "munderover" => 3,
        _ => return None,
    })
}

/// Check one `<math>` document; returns the problems found.
fn validity_problems(xml: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut reader = Reader::from_str(xml);
    // (element name, children so far)
    let mut stack: Vec<(String, usize)> = Vec::new();
    let count_child = |stack: &mut Vec<(String, usize)>| {
        if let Some(top) = stack.last_mut() {
            top.1 += 1;
        }
    };
    loop {
        match reader.read_event() {
            Ok(ev @ (Event::Start(_) | Event::Empty(_))) => {
                let (e, self_closing) = match ev {
                    Event::Start(e) => (e, false),
                    Event::Empty(e) => (e, true),
                    _ => unreachable!(),
                };
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if !CORE_ELEMENTS.contains(&name.as_str()) {
                    problems.push(format!("non-Core element <{name}>"));
                }
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let value = String::from_utf8_lossy(&attr.value).to_string();
                    if key == "mathvariant" && value != "normal" {
                        problems.push(format!("mathvariant={value:?} on <{name}> is not Core"));
                    }
                    if key == "mathvariant" && name != "mi" {
                        problems.push(format!("mathvariant on <{name}> (Core allows it on mi)"));
                    }
                }
                count_child(&mut stack);
                if !self_closing {
                    stack.push((name, 0));
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let (open, children) = stack.pop().expect("balanced");
                assert_eq!(open, name, "mismatched close tag");
                if let Some(n) = required_children(&name)
                    && children != n
                {
                    problems.push(format!("<{name}> has {children} children, needs {n}"));
                }
            }
            Ok(Event::Text(_)) => {}
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                problems.push(format!("not well-formed: {e}"));
                break;
            }
        }
    }
    problems
}

#[test]
fn every_fixture_is_valid_mathml_core() {
    let mut failures = Vec::new();
    let mut elements = BTreeSet::new();
    for fx in load_all() {
        let n = normalize(&fx.text, mode_of(&fx), Spec::builtin());
        let xml = to_mathml(&n);
        let problems = validity_problems(&xml);
        if !problems.is_empty() {
            failures.push(format!(
                "{}:\n  {}\n  {xml}",
                fx.id(),
                problems.join("\n  ")
            ));
        }
        let mut reader = Reader::from_str(&xml);
        while let Ok(ev) = reader.read_event() {
            match ev {
                Event::Start(e) | Event::Empty(e) => {
                    elements.insert(String::from_utf8_lossy(e.name().as_ref()).to_string());
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} fixture(s) produced invalid MathML:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // The corpus exercises the structural elements, not just tokens.
    for must in [
        "mfrac",
        "msqrt",
        "mroot",
        "msubsup",
        "munderover",
        "mtable",
        "mover",
    ] {
        assert!(elements.contains(must), "corpus never emitted <{must}>");
    }
}

#[test]
fn snapshot_every_fixture() {
    for fx in load_all() {
        let n = normalize(&fx.text, mode_of(&fx), Spec::builtin());
        let name = format!("mathml__{}", fx.id().replace('/', "__").replace('.', "_"));
        insta::assert_snapshot!(name, to_mathml(&n));
    }
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

#[test]
fn envelope_carries_display_and_the_source_tex() {
    let inline = mml("x<y");
    assert!(inline.starts_with(r#"<math xmlns="http://www.w3.org/1998/Math/MathML"><semantics>"#));
    assert!(inline.ends_with(
        r#"<annotation encoding="application/x-tex">x&lt;y</annotation></semantics></math>"#
    ));
    let display = mml_display("x");
    assert!(display.starts_with(
        r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><semantics>"#
    ));
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[test]
fn runs_split_into_mi_mn_mo() {
    assert_eq!(body("4ac"), "<mrow><mn>4</mn><mi>a</mi><mi>c</mi></mrow>");
    assert_eq!(body("3.14"), "<mrow><mn>3.14</mn></mrow>");
    assert_eq!(
        body("x>0"),
        "<mrow><mi>x</mi><mo>&gt;</mo><mn>0</mn></mrow>"
    );
    assert_eq!(
        body("a-b"),
        "<mrow><mi>a</mi><mo>\u{2212}</mo><mi>b</mi></mrow>"
    );
    // A prime is a superscript (`f'` ≡ `f^{\prime}`), so it scripts.
    assert_eq!(body("f'"), "<mrow><msup><mi>f</mi><mo>′</mo></msup></mrow>");
    assert_eq!(
        body("x'^2"),
        "<mrow><msup><mi>x</mi><mrow><mo>′</mo><mn>2</mn></mrow></msup></mrow>"
    );
}

#[test]
fn symbols_classify_by_alphabet() {
    assert_eq!(
        body(r"\alpha \to \infty"),
        "<mrow><mi>α</mi><mo>→</mo><mo>∞</mo></mrow>"
    );
    // Uppercase Greek is upright in TeX.
    assert_eq!(
        body(r"\Gamma"),
        r#"<mrow><mi mathvariant="normal">Γ</mi></mrow>"#
    );
    assert_eq!(
        body(r"a \leq b"),
        "<mrow><mi>a</mi><mo>≤</mo><mi>b</mi></mrow>"
    );
}

#[test]
fn bare_delimiters_do_not_stretch_but_left_right_fences_do() {
    assert_eq!(
        body("(x)"),
        r#"<mrow><mo stretchy="false">(</mo><mi>x</mi><mo stretchy="false">)</mo></mrow>"#
    );
    assert_eq!(
        body(r"\left( x \right)"),
        r#"<mrow><mrow><mo stretchy="true">(</mo><mi>x</mi><mo stretchy="true">)</mo></mrow></mrow>"#
    );
    assert_eq!(
        body(r"\left. x \right|"),
        r#"<mrow><mrow><mi>x</mi><mo stretchy="true">|</mo></mrow></mrow>"#
    );
    assert_contains(
        &body(r"\left\{ x \middle| y \right\}"),
        r#"<mo stretchy="true">{</mo><mi>x</mi><mo stretchy="true">|</mo><mi>y</mi><mo stretchy="true">}</mo>"#,
    );
}

#[test]
fn spaces_become_mspace_including_negative() {
    assert_contains(&body(r"a\,b"), r#"<mspace width="0.1667em"></mspace>"#);
    assert_contains(&body(r"a\qquad b"), r#"<mspace width="2em"></mspace>"#);
    assert_contains(&body(r"a\!b"), r#"<mspace width="-0.1667em"></mspace>"#);
}

#[test]
fn text_keeps_edge_spaces_and_escapes() {
    assert_eq!(
        body(r"\text{ if }"),
        "<mrow><mtext>\u{a0}if\u{a0}</mtext></mrow>"
    );
    assert_eq!(body(r"\text{a<b}"), "<mrow><mtext>a&lt;b</mtext></mrow>");
    assert_eq!(body(r"\textbf{ok}"), "<mrow><mtext>𝐨𝐤</mtext></mrow>");
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

#[test]
fn fractions_roots_and_scripts() {
    assert_eq!(
        body(r"\frac{a}{b}"),
        "<mrow><mfrac><mi>a</mi><mi>b</mi></mfrac></mrow>"
    );
    assert_eq!(
        body(r"\frac{a+b}{2}"),
        "<mrow><mfrac><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mn>2</mn></mfrac></mrow>"
    );
    assert_eq!(
        body(r"\dfrac{a}{b}"),
        r#"<mrow><mstyle displaystyle="true"><mfrac><mi>a</mi><mi>b</mi></mfrac></mstyle></mrow>"#
    );
    assert_eq!(
        body(r"\binom{n}{k}"),
        r#"<mrow><mrow><mo stretchy="true">(</mo><mfrac linethickness="0"><mi>n</mi><mi>k</mi></mfrac><mo stretchy="true">)</mo></mrow></mrow>"#
    );
    assert_eq!(body(r"\sqrt{x}"), "<mrow><msqrt><mi>x</mi></msqrt></mrow>");
    assert_eq!(
        body(r"\sqrt[3]{x}"),
        "<mrow><mroot><mi>x</mi><mn>3</mn></mroot></mrow>"
    );
    assert_eq!(
        body("x^2"),
        "<mrow><msup><mi>x</mi><mn>2</mn></msup></mrow>"
    );
    assert_eq!(
        body("x_i^2"),
        "<mrow><msubsup><mi>x</mi><mi>i</mi><mn>2</mn></msubsup></mrow>"
    );
    assert_eq!(
        body("x_{i+1}"),
        "<mrow><msub><mi>x</mi><mrow><mi>i</mi><mo>+</mo><mn>1</mn></mrow></msub></mrow>"
    );
    // A multi-token run as a base is grouped.
    assert_eq!(
        body("{ab}^2"),
        "<mrow><msup><mrow><mi>a</mi><mi>b</mi></mrow><mn>2</mn></msup></mrow>"
    );
}

#[test]
fn big_operators_place_limits_by_mode() {
    assert_eq!(
        body_display(r"\sum_{i=1}^{n} i"),
        "<mrow><munderover><mo>∑</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mi>n</mi></munderover><mi>i</mi></mrow>"
    );
    assert_eq!(
        body(r"\sum_{i=1}^{n} i"),
        "<mrow><msubsup><mo>∑</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mi>n</mi></msubsup><mi>i</mi></mrow>"
    );
    // Integrals keep side limits even in display math.
    assert_eq!(
        body_display(r"\int_0^1 f"),
        "<mrow><msubsup><mo>∫</mo><mn>0</mn><mn>1</mn></msubsup><mi>f</mi></mrow>"
    );
    // `\limits` pins them under/over regardless of mode.
    assert_eq!(
        body(r"\sum\limits_{i} x"),
        r#"<mrow><munder><mo movablelimits="false">∑</mo><mi>i</mi></munder><mi>x</mi></mrow>"#
    );
    assert_eq!(
        body_display(r"\sum\nolimits_{i} x"),
        "<mrow><msub><mo>∑</mo><mi>i</mi></msub><mi>x</mi></mrow>"
    );
}

#[test]
fn functions_get_function_application() {
    assert_eq!(
        body(r"\sin x"),
        "<mrow><mi>sin</mi><mo>\u{2061}</mo><mi>x</mi></mrow>"
    );
    assert_eq!(
        body_display(r"\lim_{x \to 0} f"),
        "<mrow><munder><mi>lim</mi><mrow><mi>x</mi><mo>→</mo><mn>0</mn></mrow></munder><mo>\u{2061}</mo><mi>f</mi></mrow>"
    );
    // The spec pins `\lim` to under/over limits (`limits: und-ovr`), so
    // inline math keeps them under too; `auto` functions follow the mode.
    assert_eq!(
        body(r"\lim_{x \to 0} f"),
        "<mrow><munder><mi>lim</mi><mrow><mi>x</mi><mo>→</mo><mn>0</mn></mrow></munder><mo>\u{2061}</mo><mi>f</mi></mrow>"
    );
    assert_eq!(
        body(r"\operatorname{argmax}_x f"),
        "<mrow><msub><mi>argmax</mi><mi>x</mi></msub><mo>\u{2061}</mo><mi>f</mi></mrow>"
    );
}

#[test]
fn accents_bars_braces_and_stacks() {
    assert_eq!(
        body(r"\hat{x}"),
        r#"<mrow><mover accent="true"><mi>x</mi><mo>ˆ</mo></mover></mrow>"#
    );
    assert_eq!(
        body(r"\vec{v}"),
        r#"<mrow><mover accent="true"><mi>v</mi><mo>→</mo></mover></mrow>"#
    );
    assert_eq!(
        body(r"\overline{x+y}"),
        r#"<mrow><mover accent="true"><mrow><mi>x</mi><mo>+</mo><mi>y</mi></mrow><mo stretchy="true">‾</mo></mover></mrow>"#
    );
    assert_eq!(
        body(r"\underline{x}"),
        r#"<mrow><munder accentunder="true"><mi>x</mi><mo stretchy="true">_</mo></munder></mrow>"#
    );
    assert_eq!(
        body(r"\underbrace{a+b}_{n}"),
        r#"<mrow><munder><munder accentunder="true"><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mo stretchy="true">⏟</mo></munder><mi>n</mi></munder></mrow>"#
    );
    assert_eq!(
        body(r"\overbrace{a+b}"),
        r#"<mrow><mover accent="true"><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mo stretchy="true">⏞</mo></mover></mrow>"#
    );
    assert_eq!(
        body(r"\overset{!}{=}"),
        "<mrow><mover><mo>=</mo><mo>!</mo></mover></mrow>"
    );
    assert_eq!(
        body(r"a \xrightarrow{f} b"),
        r#"<mrow><mi>a</mi><mover><mo stretchy="true">→</mo><mi>f</mi></mover><mi>b</mi></mrow>"#
    );
}

#[test]
fn styles_map_into_the_alphanumeric_block() {
    assert_eq!(body(r"\mathbf{x}"), "<mrow><mi>𝐱</mi></mrow>");
    assert_eq!(
        body(r"\mathbf{ab}"),
        "<mrow><mrow><mi>𝐚</mi><mi>𝐛</mi></mrow></mrow>"
    );
    assert_eq!(body(r"\mathbb{R}"), "<mrow><mi>ℝ</mi></mrow>");
    assert_eq!(body(r"\mathcal{P}"), "<mrow><mi>𝒫</mi></mrow>");
    assert_eq!(body(r"\mathfrak{g}"), "<mrow><mi>𝔤</mi></mrow>");
    assert_eq!(body(r"\boldsymbol{\beta}"), "<mrow><mi>𝜷</mi></mrow>");
    assert_eq!(
        body(r"\mathrm{d}x"),
        r#"<mrow><mi mathvariant="normal">d</mi><mi>x</mi></mrow>"#
    );
    assert_eq!(body(r"\mathbf{2}"), "<mrow><mn>𝟐</mn></mrow>");
}

#[test]
fn phantoms_cancel_and_color() {
    assert_eq!(
        body(r"\phantom{x}"),
        "<mrow><mphantom><mi>x</mi></mphantom></mrow>"
    );
    assert_eq!(
        body(r"\hphantom{x}"),
        r#"<mrow><mpadded height="0" depth="0"><mphantom><mi>x</mi></mphantom></mpadded></mrow>"#
    );
    assert_eq!(
        body(r"\vphantom{x}"),
        r#"<mrow><mpadded width="0"><mphantom><mi>x</mi></mphantom></mpadded></mrow>"#
    );
    assert_eq!(
        body(r"\cancel{x}"),
        r#"<mrow><menclose notation="updiagonalstrike"><mi>x</mi></menclose></mrow>"#
    );
    assert_eq!(
        body(r"\textcolor{red}{x}"),
        r#"<mrow><mstyle mathcolor="red"><mi>x</mi></mstyle></mrow>"#
    );
}

#[test]
fn environments_become_tables() {
    assert_eq!(
        body_display("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}"),
        r#"<mrow><mrow><mo stretchy="true">(</mo><mtable><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr><mtr><mtd><mi>c</mi></mtd><mtd><mi>d</mi></mtd></mtr></mtable><mo stretchy="true">)</mo></mrow></mrow>"#
    );
    assert_eq!(
        body_display("\\begin{matrix} a \\\\ b \\end{matrix}"),
        "<mrow><mtable><mtr><mtd><mi>a</mi></mtd></mtr><mtr><mtd><mi>b</mi></mtd></mtr></mtable></mrow>"
    );
    assert_eq!(
        body_display("\\begin{cases} x & x \\geq 0 \\\\ -x & x < 0 \\end{cases}"),
        r#"<mrow><mrow><mo stretchy="true">{</mo><mtable columnalign="left"><mtr><mtd><mi>x</mi></mtd><mtd><mrow><mi>x</mi><mo>≥</mo><mn>0</mn></mrow></mtd></mtr><mtr><mtd><mrow><mo>−</mo><mi>x</mi></mrow></mtd><mtd><mrow><mi>x</mi><mo>&lt;</mo><mn>0</mn></mrow></mtd></mtr></mtable></mrow></mrow>"#
    );
    assert_eq!(
        body_display("\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}"),
        r#"<mrow><mtable columnalign="right left"><mtr><mtd><mi>a</mi></mtd><mtd><mrow><mo>=</mo><mi>b</mi></mrow></mtd></mtr><mtr><mtd><mi>c</mi></mtd><mtd><mrow><mo>=</mo><mi>d</mi></mrow></mtd></mtr></mtable></mrow>"#
    );
    assert_eq!(
        body_display("\\begin{gathered} a \\\\ b \\end{gathered}"),
        "<mrow><mtable><mtr><mtd><mi>a</mi></mtd></mtr><mtr><mtd><mi>b</mi></mtd></mtr></mtable></mrow>"
    );
    // A top-level `\\` becomes a table of lines.
    assert_eq!(
        body_display(r"a \\ b"),
        "<mtable><mtr><mtd><mrow><mi>a</mi></mrow></mtd></mtr><mtr><mtd><mrow><mi>b</mi></mrow></mtd></mtr></mtable>"
    );
}

#[test]
fn errors_render_as_merror() {
    assert_eq!(
        body(r"x \foo y"),
        r"<mrow><mi>x</mi><merror><mtext>\foo</mtext></merror><mi>y</mi></mrow>"
    );
}
