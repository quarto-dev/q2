// Repro recipe for issue #964 (bd-87t8pnx6). Not compiled in-tree: drop it into
// crates/pampa/tests/integration/ and register it in main.rs to re-run.
use pampa::pandoc::{Block, Inline, Pandoc};
use pampa::writers;
use quarto_ast_reconcile::compute_reconciliation;
use quarto_source_map::SourceInfo;

fn parse_qmd(input: &str) -> Pandoc {
    pampa::readers::qmd::read(input.as_bytes(), false, "test.qmd", &mut std::io::sink(), true, None)
        .expect("parse")
        .0
}

fn edit_first_str(original_qmd: &str) -> (String, Vec<String>) {
    let orig = parse_qmd(original_qmd);
    let mut new = orig.clone();
    if let Some(Block::Paragraph(p)) = new.blocks.get_mut(0) {
        if let Some(Inline::Str(s)) = p.content.get_mut(0) {
            s.text = format!("{}Z", s.text);
            s.source_info = SourceInfo::for_test();
        }
    }
    let plan = compute_reconciliation(&orig, &new);
    let out = writers::incremental::incremental_write(original_qmd, &orig, &new, &plan).expect("iw");
    // Also dump the source spans of the paragraph's inlines in the ORIGINAL ast
    let mut spans = vec![];
    if let Some(Block::Paragraph(p)) = orig.blocks.get(0) {
        for inl in &p.content {
            let si = inl.source_info();
            let (s, e) = (si.start_offset(), si.end_offset());
            spans.push(format!("{:?} {}..{} {:?}", std::mem::discriminant(inl), s, e, &original_qmd[s..e]));
        }
    }
    (out, spans)
}

#[test]
fn issue_964_repro() {
    let cases = [
        "Last [++ added]{author=\"Bob\"} text.\n",
        "Last [++ added]{author=\"Bob\" date=\"2026-01-01\"} text.\n",
        "Last [-- gone]{author=\"Bob\"} text.\n",
        "Last [!! hot]{author=\"Bob\"} text.\n",
        "Last [>> note]{author=\"Bob\"} text.\n",
        "Last [++ added]{.cls} text.\n",
        "Last [++ added]{#id} text.\n",
        // Control: no attr
        "Last [++ added] text.\n",
        // Control: span with attr (same shape, different node type)
        "Last [added]{author=\"Bob\"} text.\n",
        // Control: emphasis — no attr possible, but check span
        "Last *added* text.\n",
    ];
    let mut report = String::new();
    for c in cases {
        let (out, spans) = edit_first_str(c);
        report.push_str(&format!("INPUT : {:?}\nOUTPUT: {:?}\nSPANS :\n", c, out));
        for s in spans { report.push_str(&format!("  {}\n", s)); }
        let findings = writers::incremental::audit_source_range_tiling(&parse_qmd(c), c);
        report.push_str(&format!("TILING: {} finding(s)\n", findings.len()));
        for f in &findings { report.push_str(&format!("  {:?}\n", f)); }
        report.push('\n');
    }
    println!("{report}");
    std::fs::write("/tmp/repro-964.txt", &report).unwrap();
}

#[test]
fn issue_964_roundtrip_fixture_audit() {
    let p = "tests/roundtrip_tests/qmd-json-qmd/editorial_marks_author_date.qmd";
    let src = std::fs::read_to_string(p).unwrap();
    let ast = parse_qmd(&src);
    let findings = writers::incremental::audit_source_range_tiling(&ast, &src);
    let mut report = format!("FIXTURE {p}: {} finding(s)\n", findings.len());
    for f in &findings { report.push_str(&format!("  {:?}\n", f)); }
    // And the incremental-writer drop on the same fixture
    let (out, _) = edit_first_str(&src);
    report.push_str(&format!("INCREMENTAL OUTPUT: {out:?}\n"));
    println!("{report}");
    std::fs::write("/tmp/repro-964-fixture.txt", &report).unwrap();
}
