//! `MathAst` → OMML (Office Math Markup Language, ECMA-376 Part 1 §22.1).
//!
//! Every fixture is snapshotted and validated against the vendored
//! `shared-math.xsd` (through `xmllint`, skipped where it is absent), and
//! the element choices from the plan's writer conventions are asserted
//! directly: child order by construction, `m:oMathPara` for display,
//! `m:nary` with hidden empty limits and an empty body, `m:func` with the
//! name upright, `m:d` fences, `m:m` with column properties, `m:eqArr` for
//! alignments and cases, `m:nor` text, escaping.

use quarto_math::normalize::{Mode, normalize};
use quarto_math::omml::{to_omml, to_omml_document};
use quarto_math::spec::Spec;

use crate::fixture_corpus::{Fixture, load_all};
use crate::omml_schema::{Validation, assert_valid_omml, validate_omml};

fn omml(text: &str) -> String {
    to_omml(&normalize(text, Mode::Inline, Spec::builtin()))
}

fn omml_display(text: &str) -> String {
    to_omml(&normalize(text, Mode::Display, Spec::builtin()))
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

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

// ---------------------------------------------------------------------------
// Corpus-wide
// ---------------------------------------------------------------------------

#[test]
fn snapshot_every_fixture() {
    for fx in load_all() {
        let n = normalize(&fx.text, mode_of(&fx), Spec::builtin());
        let name = format!("omml__{}", fx.id().replace('/', "__").replace('.', "_"));
        insta::assert_snapshot!(name, to_omml(&n));
    }
}

#[test]
fn every_fixture_validates_against_the_omml_schema() {
    let mut checked = 0;
    let mut skipped = false;
    for fx in load_all() {
        let n = normalize(&fx.text, mode_of(&fx), Spec::builtin());
        let xml = to_omml_document(&n);
        match validate_omml(&xml) {
            Validation::Valid => checked += 1,
            Validation::Invalid(err) => panic!(
                "{}: OMML failed schema validation:\n{err}\n--- xml ---\n{xml}",
                fx.id()
            ),
            Validation::Skipped(_) => skipped = true,
        }
    }
    if !skipped {
        assert!(checked > 250, "only {checked} fixtures validated");
    }
}

#[test]
fn output_is_well_formed_even_without_xmllint() {
    // A cheap structural check that runs everywhere: every `<m:x>` has a
    // matching `</m:x>` and the root is the expected element.
    for fx in load_all() {
        let n = normalize(&fx.text, mode_of(&fx), Spec::builtin());
        let xml = to_omml(&n);
        let root = if fx.display { "m:oMathPara" } else { "m:oMath" };
        assert!(xml.starts_with(&format!("<{root}")), "{}: {xml}", fx.id());
        assert!(xml.ends_with(&format!("</{root}>")), "{}: {xml}", fx.id());
        for tag in [
            "m:r", "m:e", "m:f", "m:d", "m:nary", "m:m", "m:mr", "m:eqArr", "m:sSub", "m:sSup",
        ] {
            // `<tag>` and `<tag …>` open; `<tag/>` neither opens nor closes.
            let open = count(&xml, &format!("<{tag}>")) + count(&xml, &format!("<{tag} "));
            let close = count(&xml, &format!("</{tag}>"));
            assert_eq!(open, close, "{}: unbalanced <{tag}> in {xml}", fx.id());
        }
    }
}

// ---------------------------------------------------------------------------
// Wrappers, runs, escaping
// ---------------------------------------------------------------------------

#[test]
fn inline_is_omath_and_display_is_omathpara() {
    let inline = omml("x");
    assert!(
        inline.starts_with("<m:oMath>") && inline.ends_with("</m:oMath>"),
        "{inline}"
    );
    let display = omml_display("x");
    assert!(
        display.starts_with("<m:oMathPara><m:oMath>")
            && display.ends_with("</m:oMath></m:oMathPara>"),
        "{display}"
    );
    // The document form declares the namespaces; the embedded form does not.
    let doc = to_omml_document(&normalize("x", Mode::Inline, Spec::builtin()));
    assert_contains(
        &doc,
        r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math""#,
    );
    assert!(!inline.contains("xmlns"), "{inline}");
}

#[test]
fn runs_escape_markup_and_preserve_edge_spaces() {
    // `a<b` is one lexical word; the `<` is escaped inside it. A stray `&`
    // is an error run, escaped too.
    let xml = omml(r"a<b & c");
    assert_contains(&xml, "<m:t>a&lt;b</m:t>");
    assert_contains(&xml, "<m:t>&amp;</m:t>");
    let xml = omml(r"x = 1 \text{ if } y > 0");
    assert_contains(
        &xml,
        r#"<m:r><m:rPr><m:nor/></m:rPr><m:t xml:space="preserve"> if </m:t></m:r>"#,
    );
    assert_contains(&xml, "<m:t>&gt;</m:t>");
}

#[test]
fn symbols_and_spaces_are_text_runs() {
    let xml = omml(r"\alpha \to \infty");
    assert_contains(&xml, "<m:t>α</m:t>");
    assert_contains(&xml, "<m:t>→</m:t>");
    assert_contains(&xml, "<m:t>∞</m:t>");
    let xml = omml(r"a\,b \quad c");
    assert_contains(&xml, "<m:t>\u{2009}</m:t>");
    assert_contains(&xml, "<m:t>\u{2003}</m:t>");
    let xml = omml(r"a\!b");
    assert!(
        !xml.contains("\u{2009}"),
        "negative space is dropped: {xml}"
    );
}

// ---------------------------------------------------------------------------
// Constructs
// ---------------------------------------------------------------------------

#[test]
fn fractions() {
    let xml = omml(r"\frac{a}{b}");
    assert_contains(
        &xml,
        "<m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>",
    );
    let xml = omml(r"\binom{n}{k}");
    assert_contains(
        &xml,
        r#"<m:d><m:dPr><m:begChr m:val="("/><m:endChr m:val=")"/></m:dPr><m:e><m:f><m:fPr><m:type m:val="noBar"/></m:fPr>"#,
    );
}

#[test]
fn radicals_hide_or_show_the_degree() {
    let xml = omml(r"\sqrt{x}");
    assert_contains(
        &xml,
        r#"<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad>"#,
    );
    let xml = omml(r"\sqrt[3]{x}");
    assert_contains(
        &xml,
        "<m:rad><m:deg><m:r><m:t>3</m:t></m:r></m:deg><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad>",
    );
}

#[test]
fn scripts_pick_the_element_by_which_scripts_exist() {
    assert_contains(
        &omml("x^2"),
        "<m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>",
    );
    assert_contains(
        &omml("x_i"),
        "<m:sSub><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub></m:sSub>",
    );
    assert_contains(
        &omml("x_i^2"),
        "<m:sSubSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSubSup>",
    );
}

#[test]
fn big_operators_are_nary_with_limits_and_an_empty_body() {
    // Display: sums put limits under and over; nothing after the operator
    // is pulled into its body.
    let xml = omml_display(r"\sum_{i=1}^{n} i");
    assert_contains(
        &xml,
        r#"<m:nary><m:naryPr><m:chr m:val="∑"/><m:limLoc m:val="undOvr"/></m:naryPr><m:sub><m:r><m:t>i=1</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup><m:e/></m:nary><m:r><m:t>i</m:t></m:r>"#,
    );
    // Inline: beside the operator.
    let xml = omml(r"\sum_{i=1}^{n} i");
    assert_contains(&xml, r#"<m:limLoc m:val="subSup"/>"#);
    // Integrals keep side limits even in display, as TeX does.
    let xml = omml_display(r"\int_0^1 f");
    assert_contains(&xml, r#"<m:chr m:val="∫"/><m:limLoc m:val="subSup"/>"#);
    // Explicit \limits wins.
    let xml = omml(r"\int\limits_0^1 f");
    assert_contains(&xml, r#"<m:limLoc m:val="undOvr"/>"#);
    // Missing limits are hidden, not omitted.
    let xml = omml(r"\sum x");
    assert_contains(
        &xml,
        r#"<m:subHide m:val="1"/><m:supHide m:val="1"/></m:naryPr><m:sub/><m:sup/><m:e/></m:nary>"#,
    );
    let xml = omml(r"\sum_{k} a_k");
    assert_contains(&xml, r#"<m:supHide m:val="1"/>"#);
    assert!(!xml.contains("subHide"), "{xml}");
}

#[test]
fn functions_are_upright_names_with_an_empty_body() {
    let xml = omml(r"\sin x");
    assert_contains(
        &xml,
        r#"<m:func><m:fName><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>sin</m:t></m:r></m:fName><m:e/></m:func><m:r><m:t>x</m:t></m:r>"#,
    );
    // \lim in display: limits go below the name.
    let xml = omml_display(r"\lim_{x \to 0} f(x)");
    assert_contains(
        &xml,
        r#"<m:fName><m:limLow><m:e><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>lim</m:t></m:r></m:e><m:lim>"#,
    );
    // \operatorname is a function named by its argument; inline, its
    // limits sit beside the name.
    let xml = omml(r"\operatorname{argmax}_x f(x)");
    assert_contains(
        &xml,
        r#"<m:func><m:fName><m:sSub><m:e><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>argmax</m:t></m:r></m:e><m:sub><m:r><m:t>x</m:t></m:r></m:sub></m:sSub></m:fName><m:e/></m:func>"#,
    );
}

#[test]
fn delimiters() {
    let xml = omml(r"\left( \frac{a}{b} \right)");
    assert_contains(
        &xml,
        r#"<m:d><m:dPr><m:begChr m:val="("/><m:endChr m:val=")"/></m:dPr><m:e><m:f>"#,
    );
    let xml = omml(r"\left. \frac{df}{dx} \right|");
    assert_contains(&xml, r#"<m:begChr m:val=""/><m:endChr m:val="|"/>"#);
    let xml = omml(r"\left\langle x \right\rangle");
    assert_contains(&xml, r#"<m:begChr m:val="⟨"/><m:endChr m:val="⟩"/>"#);
    let xml = omml(r"\left\{ x \middle| y \right\}");
    assert_contains(
        &xml,
        r#"<m:begChr m:val="{"/><m:sepChr m:val="|"/><m:endChr m:val="}"/></m:dPr><m:e>"#,
    );
    assert_eq!(count(&xml, "<m:e>"), 2, "one m:e per part: {xml}");
}

#[test]
fn accents_bars_braces_and_stacks() {
    assert_contains(
        &omml(r"\hat{x}"),
        "<m:acc><m:accPr><m:chr m:val=\"\u{302}\"/></m:accPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:acc>",
    );
    assert_contains(
        &omml(r"\overline{x}"),
        r#"<m:bar><m:barPr><m:pos m:val="top"/></m:barPr><m:e>"#,
    );
    assert_contains(&omml(r"\underline{x}"), r#"<m:pos m:val="bot"/>"#);
    let xml = omml(r"\underbrace{a+b}_{n}");
    assert_contains(
        &xml,
        r#"<m:limLow><m:e><m:groupChr><m:groupChrPr><m:chr m:val="⏟"/><m:pos m:val="bot"/></m:groupChrPr><m:e>"#,
    );
    assert_contains(&xml, "<m:lim><m:r><m:t>n</m:t></m:r></m:lim></m:limLow>");
    let xml = omml(r"\overbrace{a+b}^{n}");
    assert_contains(
        &xml,
        r#"<m:limUpp><m:e><m:groupChr><m:groupChrPr><m:chr m:val="⏞"/><m:pos m:val="top"/><m:vertJc m:val="bot"/></m:groupChrPr>"#,
    );
    let xml = omml(r"\overset{!}{=}");
    assert_contains(
        &xml,
        "<m:limUpp><m:e><m:r><m:t>=</m:t></m:r></m:e><m:lim><m:r><m:t>!</m:t></m:r></m:lim></m:limUpp>",
    );
    let xml = omml(r"a \xrightarrow{f} b");
    assert_contains(
        &xml,
        "<m:limUpp><m:e><m:r><m:t>→</m:t></m:r></m:e><m:lim><m:r><m:t>f</m:t></m:r></m:lim></m:limUpp>",
    );
}

#[test]
fn styles_become_run_properties() {
    assert_contains(
        &omml(r"\mathbf{x}"),
        r#"<m:r><m:rPr><m:sty m:val="b"/></m:rPr><m:t>x</m:t></m:r>"#,
    );
    assert_contains(
        &omml(r"\mathbb{R}"),
        r#"<m:rPr><m:scr m:val="double-struck"/><m:sty m:val="p"/></m:rPr><m:t>R</m:t>"#,
    );
    assert_contains(&omml(r"\mathcal{P}"), r#"<m:scr m:val="script"/>"#);
    assert_contains(
        &omml(r"\mathrm{d}x"),
        r#"<m:rPr><m:sty m:val="p"/></m:rPr><m:t>d</m:t></m:r><m:r><m:t>x</m:t></m:r>"#,
    );
    assert_contains(
        &omml(r"\boldsymbol{\beta}"),
        r#"<m:sty m:val="bi"/></m:rPr><m:t>β</m:t>"#,
    );
    // Text-mode runs cannot carry m:sty (the schema orders scr/sty before
    // nor and Word treats them as alternatives); bold/italic text uses
    // Word's own run properties.
    assert_contains(
        &omml(r"\textbf{bold}"),
        r#"<m:r><m:rPr><m:nor/></m:rPr><w:rPr><w:b/></w:rPr><m:t>bold</m:t></m:r>"#,
    );
    assert_contains(
        &omml(r"\textit{it}"),
        r#"<m:rPr><m:nor/></m:rPr><w:rPr><w:i/></w:rPr><m:t>it</m:t>"#,
    );
    assert_contains(
        &omml(r"\mathbf{a + b}"),
        r#"<m:sty m:val="b"/></m:rPr><m:t>+</m:t>"#,
    );
}

#[test]
fn matrices_cases_and_alignments() {
    let xml = omml_display("\\begin{pmatrix}\na & b \\\\\nc & d\n\\end{pmatrix}");
    assert_contains(
        &xml,
        r#"<m:d><m:dPr><m:begChr m:val="("/><m:endChr m:val=")"/></m:dPr><m:e><m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val="2"/><m:mcJc m:val="center"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:mr><m:mr>"#,
    );
    assert_eq!(count(&xml, "<m:mr>"), 2);

    let xml = omml_display("\\begin{matrix} a & b \\\\ c & d \\end{matrix}");
    assert!(
        xml.starts_with("<m:oMathPara><m:oMath><m:m>"),
        "no fence: {xml}"
    );

    let xml = omml_display("\\begin{cases}\nx & x \\geq 0 \\\\\n-x & x < 0\n\\end{cases}");
    assert_contains(
        &xml,
        r#"<m:d><m:dPr><m:begChr m:val="{"/><m:endChr m:val=""/></m:dPr><m:e><m:eqArr>"#,
    );
    assert_eq!(
        count(&xml, "<m:t>&amp;</m:t>"),
        2,
        "one alignment mark per row: {xml}"
    );

    let xml = omml_display("\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}");
    assert!(
        xml.starts_with("<m:oMathPara><m:oMath><m:eqArr><m:e>"),
        "{xml}"
    );
    assert_eq!(count(&xml, "</m:e></m:eqArr>"), 1);
    assert_eq!(count(&xml, "<m:t>&amp;</m:t>"), 2);

    let xml = omml_display(r"a \\ b");
    assert_contains(
        &xml,
        "<m:eqArr><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:eqArr>",
    );
}

#[test]
fn phantom_cancel_and_color() {
    assert_contains(
        &omml(r"\phantom{x}"),
        r#"<m:phant><m:phantPr><m:show m:val="0"/></m:phantPr><m:e>"#,
    );
    assert_contains(
        &omml(r"\hphantom{x}"),
        r#"<m:zeroAsc m:val="1"/><m:zeroDesc m:val="1"/>"#,
    );
    assert_contains(
        &omml(r"\cancel{x}"),
        r#"<m:borderBox><m:borderBoxPr><m:hideTop m:val="1"/><m:hideBot m:val="1"/><m:hideLeft m:val="1"/><m:hideRight m:val="1"/><m:strikeBLTR m:val="1"/></m:borderBoxPr><m:e>"#,
    );
    let xml = omml(r"\textcolor{red}{x}");
    assert_contains(
        &xml,
        r#"<m:r><w:rPr><w:color w:val="FF0000"/></w:rPr><m:t>x</m:t></m:r>"#,
    );
}

#[test]
fn errors_are_emitted_as_literal_text_runs() {
    let xml = omml(r"x \foo y");
    assert_contains(
        &xml,
        r#"<m:r><m:rPr><m:lit m:val="1"/><m:nor/></m:rPr><m:t>\foo</m:t></m:r>"#,
    );
    // and the fixture still validates
    assert_valid_omml(&to_omml_document(&normalize(
        r"x \foo y",
        Mode::Inline,
        Spec::builtin(),
    )));
}
