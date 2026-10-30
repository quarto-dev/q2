// Issue #965 probe. To re-run: copy to crates/pampa/tests/integration/issue_965_probe.rs,
// add `pub mod issue_965_probe;` to main.rs, then
//   cargo nextest run -p pampa --test integration issue_965_probe --no-capture
use pampa::pandoc::{Block, Inline, Pandoc};
use quarto_ast_reconcile::compute_reconciliation;

fn parse(input: &str) -> Pandoc {
    pampa::readers::qmd::read(input.as_bytes(), false, "test.qmd", &mut std::io::sink(), true, None)
        .expect("parse")
        .0
}

fn inlines_mut(block: &mut Block) -> &mut Vec<Inline> {
    match block {
        Block::Paragraph(p) => &mut p.content,
        Block::Plain(p) => &mut p.content,
        Block::Header(h) => &mut h.content,
        Block::BlockQuote(q) => inlines_mut(&mut q.content[0]),
        Block::BulletList(l) => inlines_mut(&mut l.content[0][0]),
        other => panic!("unhandled block {other:?}"),
    }
}

fn first_str_mut(inlines: &mut Vec<Inline>) -> &mut String {
    match &mut inlines[0] {
        Inline::Str(s) => &mut s.text,
        Inline::Emph(e) => first_str_mut(&mut e.content),
        other => panic!("unhandled inline {other:?}"),
    }
}

fn probe(name: &str, src: &str) {
    let original = parse(src);
    let mut edited = original.clone();
    first_str_mut(inlines_mut(&mut edited.blocks[0])).push('Z');
    let plan = compute_reconciliation(&original, &edited);
    let out = pampa::writers::incremental::incremental_write(src, &original, &edited, &plan).unwrap();
    let reparsed = parse(&out);
    let shape = |d: &Pandoc| {
        let mut d = d.clone();
        let v = inlines_mut(&mut d.blocks[0]);
        v.iter().map(|i| match i {
            Inline::Str(s) => s.text.clone(),
            Inline::Space(_) => "_".into(),
            Inline::SoftBreak(_) => "SB".into(),
            Inline::LineBreak(_) => "LB".into(),
            Inline::Emph(_) => "Emph".into(),
            _ => "?".into(),
        }).collect::<Vec<_>>().join(" ")
    };
    println!("[{name}]\n  src    {src:?}\n  out    {out:?}\n  edited {}\n  reread {}\n  same-shape {}",
        shape(&edited), shape(&reparsed), shape(&edited) == shape(&reparsed));
}

#[test]
fn probe_all() {
    probe("issue", "Last line\\\nnext line.\n");
    probe("two-space", "Last line  \nnext line.\n");
    probe("soft-control", "Last line\nnext line.\n");
    probe("blockquote", "> Last line\\\n> next line.\n");
    probe("list", "- Last line\\\n  next line.\n");
    probe("emph", "*Last line\\\nnext line.*\n");
    probe("two-breaks", "Last\\\nmid\\\nend.\n");
    probe("break-before-emph", "Last\\\n*next* line.\n");
}
