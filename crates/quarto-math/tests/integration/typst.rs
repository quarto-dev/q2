//! `MathAst` → plain Typst math (no mitex prelude).
//!
//! Every fixture is snapshotted; when the `typst` binary is on `PATH` the
//! whole corpus is compiled in one document so every emitted expression is
//! known to be valid Typst (skipped, with a message, otherwise); and the
//! syntax choices are asserted directly. The choices were verified against
//! Typst itself before the writer was written (see the plan).

use std::process::Command;

use quarto_math::normalize::{Mode, normalize};
use quarto_math::spec::Spec;
use quarto_math::typst::to_typst;

use crate::fixture_corpus::{Fixture, load_all};

fn typ(text: &str) -> String {
    to_typst(&normalize(text, Mode::Inline, Spec::builtin()))
}

fn typ_display(text: &str) -> String {
    to_typst(&normalize(text, Mode::Display, Spec::builtin()))
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
// Corpus-wide
// ---------------------------------------------------------------------------

#[test]
fn snapshot_every_fixture() {
    for fx in load_all() {
        let n = normalize(&fx.text, mode_of(&fx), Spec::builtin());
        let name = format!("typst__{}", fx.id().replace('/', "__").replace('.', "_"));
        insta::assert_snapshot!(name, to_typst(&n));
    }
}

fn typst_available() -> bool {
    Command::new("typst")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// One document, one display block per fixture, compiled once. A failure
/// names the line, and the line names the fixture.
#[test]
fn whole_corpus_compiles_with_typst() {
    if !typst_available() {
        eprintln!("typst compile check skipped: `typst` is not on PATH");
        return;
    }
    let dir = std::env::temp_dir().join(format!("quarto-math-typst-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut doc = String::from("#set page(width: auto, height: auto)\n");
    let mut lines = Vec::new();
    for fx in load_all() {
        let n = normalize(&fx.text, mode_of(&fx), Spec::builtin());
        let math = to_typst(&n);
        // Comment line, then the math on its own line (line = 2*i + 2..3).
        doc.push_str(&format!("// {}\n$ {} $\n", fx.id(), math));
        lines.push(fx.id());
    }
    let path = dir.join("corpus.typ");
    std::fs::write(&path, &doc).unwrap();
    let out = Command::new("typst")
        .arg("compile")
        .arg(&path)
        .arg(dir.join("corpus.pdf"))
        .output()
        .expect("typst runs");
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        // Map the first reported line back to its fixture.
        let fixture = stderr
            .lines()
            .find_map(|l| l.split("corpus.typ:").nth(1))
            .and_then(|rest| rest.split(':').next())
            .and_then(|n| n.parse::<usize>().ok())
            .map(|line| {
                // Header is line 1; fixture i occupies lines 2i+2 (comment) and 2i+3 (math).
                let i = line.saturating_sub(2) / 2;
                lines.get(i).cloned().unwrap_or_default()
            })
            .unwrap_or_default();
        panic!(
            "typst rejected the corpus (first failure in fixture {fixture:?}):\n{stderr}\n(document at {})",
            path.display()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Constructs
// ---------------------------------------------------------------------------

#[test]
fn runs_split_letters_and_keep_numbers() {
    assert_eq!(typ("4ac"), "4 a c");
    assert_eq!(typ("3.14"), "3.14");
    assert_eq!(typ("x>0"), "x > 0");
    assert_eq!(typ("i=1"), "i = 1");
}

#[test]
fn typst_specials_are_escaped() {
    assert_eq!(typ("a/b"), r"a \/ b");
    assert_contains(&typ(r"\{ x \}"), r"\{ x \}");
    assert_contains(&typ(r"\# \& \$ \%"), r"\# \& \$ %");
    assert_contains(&typ("x#y"), r"x \# y");
}

#[test]
fn symbols_are_unicode_and_spaces_are_named() {
    assert_eq!(typ(r"\alpha \to \infty"), "α → ∞");
    assert_contains(&typ(r"a\,b"), "a thin b");
    assert_contains(&typ(r"a\;b"), "a thick b");
    assert_contains(&typ(r"a \quad b \qquad c"), "a quad b wide c");
    assert_contains(&typ(r"a\!b"), "a #h(-0.1667em) b");
}

#[test]
fn fractions_roots_and_scripts() {
    assert_eq!(typ(r"\frac{a}{b}"), "(a)/(b)");
    assert_eq!(typ(r"\dfrac{a}{b}"), "display((a)/(b))");
    assert_eq!(typ(r"\binom{n}{k}"), "binom(n, k)");
    assert_eq!(typ(r"\sqrt{x}"), "sqrt(x)");
    assert_eq!(typ(r"\sqrt[3]{x}"), "root(3, x)");
    assert_eq!(typ("x^2"), "x^(2)");
    assert_eq!(typ("x_i^2"), "x_(i)^(2)");
    assert_eq!(typ("x_{i+1}"), "x_(i + 1)");
    assert_eq!(typ(r"\frac{a}{b}^2"), "((a)/(b))^(2)");
}

#[test]
fn operators_and_functions() {
    assert_eq!(typ_display(r"\sum_{i=1}^{n} i"), "sum_(i = 1)^(n) i");
    assert_eq!(typ(r"\sum\limits_{i} x_i"), "limits(sum)_(i) x_(i)");
    assert_eq!(typ(r"\sum\nolimits_{i} x_i"), "scripts(sum)_(i) x_(i)");
    assert_eq!(typ(r"\int_0^1 f"), "integral_(0)^(1) f");
    assert_eq!(typ(r"\lim_{x \to 0} f(x)"), "lim_(x → 0) f ( x )");
    assert_eq!(typ(r"\sin x"), "sin x");
    assert_eq!(
        typ(r"\operatorname{argmax}_x f(x)"),
        r#"op("argmax")_(x) f ( x )"#
    );
    assert_eq!(
        typ(r"\argmax_x f(x)"),
        r#"op("argmax", limits: #true)_(x) f ( x )"#
    );
}

#[test]
fn delimiters() {
    assert_eq!(typ(r"\left( x \right)"), r"lr(\( x \))");
    assert_eq!(typ(r"\left. x \right|"), "lr(x |)");
    assert_eq!(typ(r"\left\langle x \right\rangle"), "lr(⟨ x ⟩)");
    assert_eq!(
        typ(r"\left\{ x \middle| y \right\}"),
        r"lr(\{ x mid(|) y \})"
    );
}

#[test]
fn accents_bars_and_stacks() {
    assert_eq!(typ(r"\hat{x}"), "hat(x)");
    assert_eq!(typ(r"\vec{v}"), "arrow(v)");
    assert_eq!(typ(r"\bar{x}"), "macron(x)");
    assert_eq!(typ(r"\ddot{x}"), "dot.double(x)");
    assert_eq!(typ(r"\overline{x+y}"), "overline(x + y)");
    assert_eq!(typ(r"\underbrace{a+b}_{n}"), "underbrace(a + b, n)");
    assert_eq!(typ(r"\overbrace{a+b}"), "overbrace(a + b)");
    assert_eq!(typ(r"\overset{!}{=}"), "limits(=)^(!)");
    assert_eq!(typ(r"a \xrightarrow{f} b"), "a stretch(→)^(f) b");
}

#[test]
fn text_and_styles() {
    assert_eq!(typ(r"\text{ if }"), r#"" if ""#);
    assert_eq!(typ(r#"\text{say "hi"}"#), r#""say \"hi\"""#);
    assert_eq!(typ(r"\textbf{bold}"), r#"bold("bold")"#);
    assert_eq!(typ(r"\mathbf{x}"), "bold(x)");
    assert_eq!(typ(r"\mathrm{d}x"), "upright(d) x");
    assert_eq!(typ(r"\mathbb{R}"), "bb(R)");
    assert_eq!(typ(r"\mathcal{P}"), "cal(P)");
    assert_eq!(typ(r"\boldsymbol{\beta}"), "bold(italic(β))");
    assert_eq!(
        typ(r"\textcolor{red}{x}"),
        r##"text(fill: #rgb("FF0000"), x)"##
    );
    assert_eq!(typ(r"\phantom{x}"), "std.hide(x)");
    assert_eq!(typ(r"\cancel{x}"), "cancel(x)");
}

#[test]
fn environments() {
    assert_eq!(
        typ_display("\\begin{pmatrix}\na & b \\\\\nc & d\n\\end{pmatrix}"),
        r#"mat(delim: "(", a, b; c, d)"#
    );
    assert_eq!(
        typ_display("\\begin{matrix} a & b \\\\ c & d \\end{matrix}"),
        "mat(delim: #none, a, b; c, d)"
    );
    assert_eq!(
        typ_display("\\begin{Vmatrix} a \\\\ b \\end{Vmatrix}"),
        r#"mat(delim: "‖", a; b)"#
    );
    assert_eq!(
        typ_display("\\begin{cases}\nx & x \\geq 0 \\\\\n-x & x < 0\n\\end{cases}"),
        "cases(x & x ≥ 0, - x & x < 0)"
    );
    assert_eq!(
        typ_display("\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}"),
        r"a & = b \ c & = d"
    );
    assert_eq!(
        typ_display("\\begin{gathered}\na = b \\\\\nc = d\n\\end{gathered}"),
        r"a = b \ c = d"
    );
    assert_eq!(typ_display(r"a \\ b"), r"a \ b");
}

#[test]
fn errors_are_literal_strings() {
    assert_eq!(typ(r"x \foo y"), r#"x "\\foo" y"#);
}
