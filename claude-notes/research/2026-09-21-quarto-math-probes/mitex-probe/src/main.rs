use rowan::NodeOrToken;
fn leaves(n: &mitex_parser::syntax::SyntaxNode, out: &mut Vec<(String, String)>) {
    for c in n.children_with_tokens() {
        match c {
            NodeOrToken::Node(n) => leaves(&n, out),
            NodeOrToken::Token(t) => out.push((format!("{:?}", t.kind()), t.text().to_string())),
        }
    }
}
fn main() {
    for src in std::env::args().skip(1) {
        println!("### {src}");
        let spec = mitex_spec_gen::DEFAULT_SPEC.clone();
        // pass 1: parse (with macro expansion, as mitex_parser::parse does)
        let tree = mitex_parser::parse(&src, spec.clone());
        let mut lv = vec![]; leaves(&tree, &mut lv);
        // pass 2: independent lexer pass with the same macro engine, recover original offsets
        let base = src.as_ptr() as usize;
        let mut lx = mitex_lexer::Lexer::new_with_bumper(&src, spec.clone(), mitex_lexer::MacroEngine::new(spec.clone()));
        let mut toks = vec![];
        while let Some((k, text)) = lx.eat() {
            let p = text.as_ptr() as usize;
            let span = if p >= base && p + text.len() <= base + src.len() { Some((p - base, p - base + text.len())) } else { None };
            toks.push((format!("{:?}", k), text.to_string(), span));
        }
        println!("rowan leaves = {}, lexer tokens = {}", lv.len(), toks.len());
        for (i, (lk, lt)) in lv.iter().enumerate() {
            let (tk, tt, sp) = toks.get(i).cloned().unwrap_or(("-".into(), "-".into(), None));
            let ok = if lt == &tt { "" } else { "  <-- MISMATCH" };
            let spans = match sp { Some((a, b)) => format!("{a}..{b} {:?}", &src[a..b]), None => "SYNTHETIC".into() };
            println!("  {lk:<18} {lt:<10?} | {tk:<28} {spans}{ok}");
        }
    }
}
