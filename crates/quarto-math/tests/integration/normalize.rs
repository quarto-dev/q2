//! CST → `MathAst` normalization.
//!
//! Every fixture is snapshotted (tree plus problems), and the structural
//! rules from the plan's Phase 1 design are asserted directly: attachments
//! fold into one `Scripts`, `\limits` binds to its operator, `\left…\right`
//! resolves, environments split into rows and cells, symbols resolve to
//! Unicode, spans nest, and bad input yields `Error` nodes plus problems,
//! never a panic.

use std::ops::Range;

use quarto_math::ast::{EnvLayout, FracStyle, LimLoc, Node, NodeKind, Pos, Variant};
use quarto_math::normalize::{Mode, Normalized, ProblemKind, normalize};
use quarto_math::spec::Spec;

use crate::fixture_corpus::{Fixture, load_all};

fn norm(text: &str) -> Normalized {
    normalize(text, Mode::Inline, Spec::builtin())
}

fn norm_display(text: &str) -> Normalized {
    normalize(text, Mode::Display, Spec::builtin())
}

fn mode_of(fx: &Fixture) -> Mode {
    if fx.display {
        Mode::Display
    } else {
        Mode::Inline
    }
}

/// The root is a `Row`; return its items.
fn items(n: &Normalized) -> &[Node] {
    match &n.root.kind {
        NodeKind::Row(items) => items,
        other => panic!("root must be a Row, got {other:?}"),
    }
}

/// The single meaningful item of the root row (spaces skipped).
fn only(n: &Normalized) -> &Node {
    let meaningful: Vec<&Node> = items(n)
        .iter()
        .filter(|i| !matches!(i.kind, NodeKind::Space(_)))
        .collect();
    assert_eq!(meaningful.len(), 1, "expected one item, got:\n{}", n.root);
    meaningful[0]
}

fn run(node: &Node) -> &str {
    match &node.kind {
        NodeKind::Run(t) => t,
        NodeKind::Row(items) if items.len() == 1 => run(&items[0]),
        other => panic!("expected a Run, got {other:?}"),
    }
}

fn rendered(n: &Normalized) -> String {
    let mut out = n.root.to_string();
    out.push_str("-- problems --\n");
    for p in &n.problems {
        out.push_str(&format!(
            "{:?} {} {}\n",
            p.kind,
            match &p.span {
                Some(r) => format!("@{}..{}", r.start, r.end),
                None => "@-".to_string(),
            },
            p.message
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// Snapshots and corpus-wide properties
// ---------------------------------------------------------------------------

#[test]
fn snapshot_every_fixture() {
    for fx in load_all() {
        let n = normalize(&fx.text, mode_of(&fx), Spec::builtin());
        let name = format!("ast__{}", fx.id().replace('/', "__").replace('.', "_"));
        insta::assert_snapshot!(name, rendered(&n));
    }
}

fn check_nesting(node: &Node, parent: Option<&Range<usize>>, text: &str, id: &str) {
    if let Some(span) = &node.span {
        assert!(
            span.start <= span.end && span.end <= text.len(),
            "{id}: span {span:?} out of bounds"
        );
        if let Some(p) = parent {
            assert!(
                p.start <= span.start && span.end <= p.end,
                "{id}: child span {span:?} escapes parent {p:?}\n{node}"
            );
        }
        if let NodeKind::Run(t) = &node.kind {
            assert_eq!(
                &text[span.clone()],
                t,
                "{id}: a Run's span selects its own text"
            );
        }
    }
    for child in node.children() {
        check_nesting(child, node.span.as_ref().or(parent), text, id);
    }
}

#[test]
fn spans_nest_and_the_root_covers_the_text() {
    for fx in load_all() {
        let n = normalize(&fx.text, mode_of(&fx), Spec::builtin());
        assert_eq!(
            n.root.span,
            Some(0..fx.text.len()),
            "{}: root span",
            fx.id()
        );
        check_nesting(&n.root, None, &fx.text, &fx.id());
    }
}

#[test]
fn errors_group_reports_a_problem_and_never_panics() {
    let must_report = [
        "unknown-command",
        "unknown-command-noarg",
        "unknown-env",
        "unbalanced-open",
        "unbalanced-close",
        "missing-arg",
        "env-mismatch",
        "unclosed-env",
        "left-without-right",
        "right-without-left",
        "double-superscript",
        "double-subscript",
        "recursive-macro",
        "macro-arity-mismatch",
        "undefined-blackboard-shortcuts",
        "stray-end",
    ];
    for fx in load_all() {
        let n = normalize(&fx.text, mode_of(&fx), Spec::builtin());
        if fx.group == "errors" && must_report.contains(&fx.name.as_str()) {
            assert!(
                !n.problems.is_empty() && n.root.has_errors(),
                "{}: expected an Error node and a problem, got:\n{}",
                fx.id(),
                rendered(&n)
            );
        }
        // Valid TeX the vendored macro engine does not expand yet: `\\def`
        // with parameters and optional macro arguments (bd-f047ynng). They stay in
        // the corpus so the snapshot records the gap.
        let known_engine_gap = fx.group == "macros"
            && matches!(fx.name.as_str(), "def-with-arg" | "newcommand-optional-arg");
        if fx.group != "errors" && !known_engine_gap {
            assert!(
                !n.root.has_errors(),
                "{}: a non-error fixture produced errors:\n{}",
                fx.id(),
                rendered(&n)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Attachments and limits
// ---------------------------------------------------------------------------

#[test]
fn consecutive_attachments_fold_into_one_scripts_node() {
    for input in [r"x_i^2", r"x^2_i"] {
        let n = norm(input);
        let NodeKind::Scripts { base, sub, sup, .. } = &only(&n).kind else {
            panic!("{input}: expected Scripts, got\n{}", n.root);
        };
        assert_eq!(run(base), "x");
        assert_eq!(run(sub.as_ref().unwrap()), "i");
        assert_eq!(run(sup.as_ref().unwrap()), "2");
        assert_eq!(only(&n).span, Some(0..input.len()));
    }
}

#[test]
fn grouped_and_nested_scripts() {
    let n = norm(r"x_{i+1}");
    let NodeKind::Scripts { sub, .. } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert_eq!(run(sub.as_ref().unwrap()), "i+1");

    let n = norm(r"e^{x^2}");
    let NodeKind::Scripts { sup, .. } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    let sup = sup.as_ref().unwrap();
    let inner = match &sup.kind {
        NodeKind::Row(items) if items.len() == 1 => &items[0],
        _ => sup,
    };
    assert!(
        matches!(inner.kind, NodeKind::Scripts { .. }),
        "nested superscript stays a Scripts node:\n{}",
        n.root
    );
}

#[test]
fn a_second_superscript_is_an_error_that_keeps_the_first() {
    let n = norm(r"x^2^3");
    assert!(
        n.problems
            .iter()
            .any(|p| p.kind == ProblemKind::DoubleScript),
        "{}",
        rendered(&n)
    );
    let mut found = false;
    n.root.walk(&mut |node| {
        if let NodeKind::Scripts { sup: Some(sup), .. } = &node.kind
            && run(sup) == "2"
        {
            found = true;
        }
    });
    assert!(found, "the first superscript is kept:\n{}", n.root);
}

#[test]
fn limits_commands_bind_to_the_operator() {
    let n = norm_display(r"\sum\limits_{i} x_i");
    let first = items(&n)
        .iter()
        .find(|i| matches!(i.kind, NodeKind::Scripts { .. }))
        .expect("a Scripts node");
    let NodeKind::Scripts { base, .. } = &first.kind else {
        unreachable!()
    };
    assert!(
        matches!(base.kind, NodeKind::Nary { ref text, limits: LimLoc::UndOvr } if text == "∑"),
        "{}",
        n.root
    );

    let n = norm_display(r"\sum\nolimits_{i} x_i");
    let mut saw = false;
    n.root.walk(&mut |node| {
        if matches!(
            node.kind,
            NodeKind::Nary {
                limits: LimLoc::SubSup,
                ..
            }
        ) {
            saw = true;
        }
    });
    assert!(saw, "\\nolimits sets SubSup:\n{}", n.root);

    let n = norm(r"\sum_{i=1}^{n} i");
    let mut saw_auto = false;
    n.root.walk(&mut |node| {
        if matches!(
            node.kind,
            NodeKind::Nary {
                limits: LimLoc::Auto,
                ..
            }
        ) {
            saw_auto = true;
        }
    });
    assert!(
        saw_auto,
        "no explicit limits command leaves Auto:\n{}",
        n.root
    );
}

// ---------------------------------------------------------------------------
// Delimiters
// ---------------------------------------------------------------------------

#[test]
fn left_right_resolve_to_delimited() {
    let n = norm(r"\left( \frac{a}{b} \right)");
    let NodeKind::Delimited { left, right, parts } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert_eq!((left.as_deref(), right.as_deref()), (Some("("), Some(")")));
    assert_eq!(parts.len(), 1);

    let n = norm(r"\left. \frac{df}{dx} \right|_{x=0}");
    let NodeKind::Scripts { base, .. } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    let NodeKind::Delimited { left, right, .. } = &base.kind else {
        panic!("{}", n.root)
    };
    assert_eq!((left.as_deref(), right.as_deref()), (None, Some("|")));

    let n = norm(r"\left\langle x \right\rangle");
    let NodeKind::Delimited { left, right, .. } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert_eq!((left.as_deref(), right.as_deref()), (Some("⟨"), Some("⟩")));

    let n = norm(r"\left\{ x \middle| x > 0 \right\}");
    let NodeKind::Delimited { parts, .. } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert_eq!(parts.len(), 2, "\\middle splits the body:\n{}", n.root);
}

#[test]
fn unbalanced_left_is_an_error() {
    let n = norm(r"\left( x");
    assert!(
        n.problems
            .iter()
            .any(|p| p.kind == ProblemKind::UnbalancedDelimiter),
        "{}",
        rendered(&n)
    );
}

// ---------------------------------------------------------------------------
// Environments
// ---------------------------------------------------------------------------

fn matrix(n: &Normalized) -> (EnvLayout, Option<(String, String)>, Vec<Vec<Node>>) {
    match &only(n).kind {
        NodeKind::Matrix {
            layout,
            delims,
            rows,
        } => (*layout, delims.clone(), rows.clone()),
        other => panic!("expected Matrix, got {other:?}\n{}", n.root),
    }
}

#[test]
fn matrix_environments_split_rows_and_cells() {
    let n = norm_display("\\begin{pmatrix}\na & b \\\\\nc & d\n\\end{pmatrix}");
    let (layout, delims, rows) = matrix(&n);
    assert_eq!(layout, EnvLayout::Matrix);
    assert_eq!(delims, Some(("(".to_string(), ")".to_string())));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].len(), 2);
    assert_eq!(run(&rows[0][0]), "a");
    assert_eq!(run(&rows[1][1]), "d");
    assert!(n.problems.is_empty(), "{}", rendered(&n));
}

#[test]
fn trailing_row_break_adds_no_row() {
    let n = norm_display("\\begin{pmatrix}\na & b \\\\\nc & d \\\\\n\\end{pmatrix}");
    let (_, _, rows) = matrix(&n);
    assert_eq!(rows.len(), 2, "{}", n.root);
}

#[test]
fn ragged_rows_are_padded_and_reported() {
    let n = norm_display("\\begin{pmatrix}\na & b & c \\\\\nd\n\\end{pmatrix}");
    let (_, _, rows) = matrix(&n);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].len(), 3);
    assert_eq!(rows[1].len(), 3, "short rows are padded:\n{}", n.root);
    assert!(
        n.problems.iter().any(|p| p.kind == ProblemKind::RaggedRows),
        "{}",
        rendered(&n)
    );
}

#[test]
fn cases_and_aligned_layouts() {
    let n = norm_display("\\begin{cases}\nx & x \\geq 0 \\\\\n-x & x < 0\n\\end{cases}");
    let (layout, delims, rows) = matrix(&n);
    assert_eq!(layout, EnvLayout::Cases);
    assert_eq!(delims, None);
    assert_eq!(rows.len(), 2);

    let n = norm_display("\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}");
    let (layout, _, rows) = matrix(&n);
    assert_eq!(layout, EnvLayout::Aligned);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].len(), 2);
}

#[test]
fn array_reads_its_column_spec_and_drops_rules() {
    let n = norm_display("\\begin{array}{c|c}\na & b \\\\\n\\hline\nc & d\n\\end{array}");
    let (layout, _, rows) = matrix(&n);
    assert_eq!(layout, EnvLayout::Matrix);
    assert_eq!(rows.len(), 2, "the \\hline row is not a row:\n{}", n.root);
    assert!(
        rows.iter().all(|r| r.len() == 2),
        "the column spec is not a cell:\n{}",
        n.root
    );
}

#[test]
fn unknown_environment_is_an_error() {
    let n = norm(r"\begin{foo} x \end{foo}");
    assert!(
        n.problems
            .iter()
            .any(|p| p.kind == ProblemKind::UnknownEnvironment),
        "{}",
        rendered(&n)
    );
}

#[test]
fn double_backslash_outside_an_environment_is_a_break() {
    let n = norm_display(r"a \\ b");
    assert!(
        items(&n).iter().any(|i| matches!(i.kind, NodeKind::Break)),
        "{}",
        n.root
    );
}

// ---------------------------------------------------------------------------
// Commands, symbols, text, spacing
// ---------------------------------------------------------------------------

#[test]
fn symbols_resolve_to_unicode_with_the_command_span() {
    let n = norm(r"\alpha");
    let node = only(&n);
    assert_eq!(node.kind, NodeKind::Sym("α".to_string()));
    assert_eq!(node.span, Some(0..6));

    let n = norm("α ≠ ∅");
    assert!(
        items(&n)
            .iter()
            .any(|i| matches!(&i.kind, NodeKind::Run(t) if t.contains('≠'))),
        "Unicode input stays a Run:\n{}",
        n.root
    );
}

#[test]
fn unknown_command_is_an_error_with_its_span() {
    let n = norm(r"x \foo y");
    let p = n
        .problems
        .iter()
        .find(|p| p.kind == ProblemKind::UnknownCommand)
        .unwrap_or_else(|| panic!("{}", rendered(&n)));
    assert_eq!(p.span, Some(2..6), "the problem points at \\foo");
    assert!(
        items(&n)
            .iter()
            .any(|i| matches!(&i.kind, NodeKind::Error { verbatim, .. } if verbatim == r"\foo")),
        "{}",
        n.root
    );
}

#[test]
fn fractions_and_roots() {
    let n = norm(r"\frac{a}{b}");
    let NodeKind::Frac { style, num, den } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert_eq!(*style, FracStyle::Bar);
    assert_eq!(run(num), "a");
    assert_eq!(run(den), "b");

    let n = norm(r"\frac12");
    let NodeKind::Frac { num, den, .. } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert_eq!((run(num), run(den)), ("1", "2"));

    let n = norm(r"{a \over b}");
    let mut saw = false;
    n.root.walk(&mut |node| {
        if let NodeKind::Frac {
            style: FracStyle::Bar,
            num,
            den,
        } = &node.kind
            && run(num) == "a"
            && run(den) == "b"
        {
            saw = true;
        }
    });
    assert!(saw, "{}", n.root);

    let n = norm(r"\sqrt[3]{x}");
    let NodeKind::Sqrt { degree, body } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert_eq!(run(degree.as_ref().unwrap()), "3");
    assert_eq!(run(body), "x");

    let n = norm(r"\sqrt x");
    let NodeKind::Sqrt { degree, body } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert!(degree.is_none());
    assert_eq!(run(body), "x");

    let n = norm(r"\binom{n}{k}");
    assert!(matches!(
        only(&n).kind,
        NodeKind::Frac {
            style: FracStyle::Binom,
            ..
        }
    ));
}

#[test]
fn text_styles_and_functions() {
    let n = norm(r"\text{ if }");
    assert_eq!(
        only(&n).kind,
        NodeKind::Text {
            variant: Variant::Roman,
            text: " if ".to_string()
        }
    );

    let n = norm(r"\mathbf{x}");
    let NodeKind::Style { variant, body } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert_eq!(*variant, Variant::Bold);
    assert_eq!(run(body), "x");

    let n = norm(r"\mathrm r");
    let NodeKind::Style { body, .. } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert_eq!(run(body), "r");

    let n = norm(r"\sin x");
    assert!(
        items(&n)
            .iter()
            .any(|i| matches!(&i.kind, NodeKind::Func { name, .. } if name == "sin")),
        "{}",
        n.root
    );

    let n = norm(r"\operatorname{argmax}_x f(x)");
    let mut saw = false;
    n.root.walk(&mut |node| {
        if matches!(&node.kind, NodeKind::Func { name, limits: LimLoc::Auto } if name == "argmax") {
            saw = true;
        }
    });
    assert!(
        saw,
        "\\operatorname is a Func named by its argument:\n{}",
        n.root
    );
    let n = norm(r"\operatorname*{argmax}_x f(x)");
    let mut saw = false;
    n.root.walk(&mut |node| {
        if matches!(
            &node.kind,
            NodeKind::Func {
                limits: LimLoc::UndOvr,
                ..
            }
        ) {
            saw = true;
        }
    });
    assert!(saw, "{}", n.root);
}

#[test]
fn accents_bars_and_stacks() {
    let n = norm(r"\hat{\beta}_0");
    let NodeKind::Scripts { base, .. } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    let NodeKind::Accent { text, body } = &base.kind else {
        panic!("{}", n.root)
    };
    assert_eq!(text, "\u{302}");
    assert!(
        matches!(body.kind, NodeKind::Sym(ref s) if s == "β")
            || matches!(&body.kind, NodeKind::Row(items) if items.len() == 1 && matches!(items[0].kind, NodeKind::Sym(ref s) if s == "β"))
    );

    let n = norm(r"\overline{x+y}");
    assert!(matches!(only(&n).kind, NodeKind::Bar { pos: Pos::Top, .. }));

    let n = norm(r"\underbrace{a+b}_{n}");
    let NodeKind::Scripts { base, .. } = &only(&n).kind else {
        panic!("{}", n.root)
    };
    assert!(matches!(
        base.kind,
        NodeKind::GroupChr { pos: Pos::Bot, .. }
    ));

    let n = norm(r"\overset{!}{=}");
    let NodeKind::LimPos {
        pos,
        annotation,
        body,
    } = &only(&n).kind
    else {
        panic!("{}", n.root)
    };
    assert_eq!(*pos, Pos::Top);
    assert_eq!(run(annotation), "!");
    assert_eq!(run(body), "=");
}

#[test]
fn spacing_commands_become_space_nodes() {
    let n = norm(r"a\,b");
    assert!(
        items(&n)
            .iter()
            .any(|i| matches!(i.kind, NodeKind::Space(em) if (em - 0.1667).abs() < 1e-4)),
        "{}",
        n.root
    );
    let n = norm(r"a \quad b");
    assert!(
        items(&n)
            .iter()
            .any(|i| matches!(i.kind, NodeKind::Space(em) if (em - 1.0).abs() < 1e-4)),
        "{}",
        n.root
    );
    let n = norm(r"a~b");
    assert!(
        items(&n)
            .iter()
            .any(|i| matches!(&i.kind, NodeKind::Sym(s) if s == "\u{a0}")),
        "a tie is a non-breaking space:\n{}",
        n.root
    );
}

#[test]
fn macros_expand_before_normalization() {
    let n = norm(r"\newcommand{\R}{\mathbb{R}} x \in \R");
    let mut saw = false;
    n.root.walk(&mut |node| {
        if matches!(
            node.kind,
            NodeKind::Style {
                variant: Variant::DoubleStruck,
                ..
            }
        ) {
            saw = true;
        }
    });
    assert!(saw, "{}", n.root);
    assert!(n.problems.is_empty(), "{}", rendered(&n));
}
