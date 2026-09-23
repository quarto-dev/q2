//! End to end through pampa: a `.qmd` is parsed, each `Inline::Math` is
//! handed to `quarto_math::convert` with its `text_source`, and a diagnostic
//! raised inside multi-line, block-quoted display math resolves to the
//! exact bytes of the `.qmd`. This is the seam pampa's docx and Typst
//! writers will use; quarto-math is a dev-dependency here until they do.

use pampa::filter_context::FilterContext;
use pampa::filters::{Filter, FilterReturn, topdown_traverse};
use pampa::pandoc::Math;
use pampa::readers;
use quarto_math::convert::{Target, convert};
use quarto_math::normalize::Mode;
use quarto_math::spec::Spec;
use quarto_pandoc_types::MathType;

fn collect_math(doc: &pampa::pandoc::Pandoc) -> Vec<Math> {
    let mut out = Vec::new();
    {
        let mut filter = Filter::new().with_math(|m, _ctx| {
            out.push(m.clone());
            FilterReturn::Unchanged(m)
        });
        let mut fctx = FilterContext::new();
        topdown_traverse(doc.clone(), &mut filter, &mut fctx);
    }
    out
}

#[test]
fn a_math_error_inside_a_block_quote_points_at_the_qmd_bytes() {
    let source = "> Intro.\n> $$\n> x + \\bogus y\n> $$\n";
    let (doc, ctx, diags) = readers::qmd::read(
        source.as_bytes(),
        false,
        "seam.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("parses");
    assert!(diags.is_empty());
    let file = ctx.current_file_id();
    let maths = collect_math(&doc);
    assert_eq!(maths.len(), 1);
    let m = &maths[0];
    assert_eq!(m.math_type, MathType::DisplayMath);
    let text_source = m
        .text_source
        .clone()
        .unwrap_or_else(|| m.source_info.clone());

    let c = convert(
        &m.text,
        Mode::Display,
        Target::Omml,
        &text_source,
        Spec::builtin(),
    );
    assert!(c.output.is_none(), "the unknown command withholds OMML");
    assert_eq!(c.diagnostics.len(), 1);
    // Inside a block quote the math text maps through a Concat (stripped
    // gutters), and `preimage_in` refuses a substring of a Concat rather
    // than guess; `map_offset` resolves it exactly, which is what the
    // error reporter does.
    let loc = c.diagnostics[0].location.as_ref().expect("located");
    let start = loc
        .map_offset(0, &ctx.source_context)
        .expect("start resolves");
    let end = loc
        .map_offset(loc.length(), &ctx.source_context)
        .expect("end resolves");
    assert_eq!(start.file_id, file);
    assert_eq!(
        &source[start.location.offset..end.location.offset],
        "\\bogus",
        "the diagnostic selects the command in the .qmd"
    );
    // On the third line (0-based row 2), right after `x + `.
    assert_eq!(start.location.row, 2);
    assert_eq!(
        &source[start.location.offset - 2..start.location.offset],
        "+ "
    );
}

#[test]
fn clean_math_converts_for_both_targets() {
    let source = "Inline $E = mc^2$ and\n\n$$\n\\int_0^1 f(x)\\,dx\n$$\n";
    let (doc, _ctx, _) = readers::qmd::read(
        source.as_bytes(),
        false,
        "seam.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("parses");
    for m in collect_math(&doc) {
        let mode = match m.math_type {
            MathType::InlineMath => Mode::Inline,
            MathType::DisplayMath => Mode::Display,
        };
        let ts = m.text_source.clone().unwrap();
        for target in [Target::Omml, Target::Typst] {
            let c = convert(&m.text, mode, target, &ts, Spec::builtin());
            assert!(
                c.succeeded(),
                "{target:?} {:?}: {:?}",
                m.text,
                c.diagnostics
            );
            assert!(c.diagnostics.is_empty());
        }
    }
}
