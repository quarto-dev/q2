use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;
struct R;
impl MarkedEventReceiver for R {
    fn on_event(&mut self, ev: Event, m: Marker) {
        println!("  {:?} idx={} line={} col={}", ev, m.index(), m.line(), m.col());
    }
}
fn main() {
    let cases = [
        "status: >\n  v2 — x\nbeads: y\n",
        "status: >\n  v2 - x\nbeads: y\n",
        "status: |\n  v2 — x\nbeads: y\n",
        "status: \"v2 — x\"\nbeads: y\n",
        "status: v2 — x\nbeads: y\n",
        "status: >\n  v2 — x\n  more — here\nbeads: y\nnext: z\n",
    ];
    for c in cases {
        println!("=== {:?} (chars={}, bytes={})", c, c.chars().count(), c.len());
        let mut p = Parser::new_from_str(c);
        p.load(&mut R, false).unwrap();
        match quarto_yaml::parse(c) {
            Ok(y) => println!("  quarto-yaml: {:?}", y),
            Err(e) => println!("  err {e:?}"),
        }
    }
}
