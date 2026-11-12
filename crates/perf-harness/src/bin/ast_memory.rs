//! Study driver for bd-akgst1fi: how much heap does the Pandoc AST cost
//! relative to the source text it was parsed from, and how much of that is
//! small owned `String`s?
//!
//! It parses every `*.md` / `*.qmd` under a directory (recursively) with
//! pampa's qmd reader, RETAINS every `(Pandoc, ASTContext)`, and reports:
//!
//! - allocator traffic per phase (a counting `GlobalAlloc` wrapper),
//! - a census of every text-carrying field in the retained ASTs,
//! - how many `Str` texts are byte-identical to the source range their
//!   `source_info` points at (i.e. could be a borrowed slice),
//! - what is freed when the contexts, then the ASTs, are dropped.
//!
//! Usage:
//!     ast-memory <dir> [--keep-going]
//!
//! See claude-notes/plans/2026-11-12-ast-string-storage-study.md.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::{Citation, Inline};
use quarto_pandoc_types::table::{Cell, Row, Table};
use quarto_source_map::SourceInfo;

// ---------------------------------------------------------------- allocator

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn bump_live(delta: usize) {
    let now = LIVE.fetch_add(delta, Relaxed) + delta;
    PEAK.fetch_max(now, Relaxed);
}

// SAFETY: defers to `System` for every operation; only adds counters.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        bump_live(l.size());
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        FREES.fetch_add(1, Relaxed);
        LIVE.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        REALLOCS.fetch_add(1, Relaxed);
        if new >= l.size() {
            bump_live(new - l.size());
        } else {
            LIVE.fetch_sub(l.size() - new, Relaxed);
        }
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[derive(Clone, Copy)]
struct Snap {
    allocs: usize,
    frees: usize,
    reallocs: usize,
    live: usize,
}

fn snap() -> Snap {
    Snap {
        allocs: ALLOCS.load(Relaxed),
        frees: FREES.load(Relaxed),
        reallocs: REALLOCS.load(Relaxed),
        live: LIVE.load(Relaxed),
    }
}

fn rss_kb() -> usize {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn mb(b: usize) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

fn phase(name: &str, a: Snap, b: Snap) {
    println!(
        "{name:<34} allocs={:>10} reallocs={:>9} frees={:>10} live_blocks={:>+10} live_bytes={:>+9.1} MiB  rss={:.1} MiB",
        b.allocs - a.allocs,
        b.reallocs - a.reallocs,
        b.frees - a.frees,
        (b.allocs - a.allocs) as isize - (b.frees - a.frees) as isize,
        (b.live as f64 - a.live as f64) / (1024.0 * 1024.0),
        rss_kb() as f64 / 1024.0,
    );
}

// The real heap footprint of a block, including size-class rounding.
#[cfg(target_os = "macos")]
fn heap_size(p: *const u8) -> usize {
    unsafe extern "C" {
        fn malloc_size(p: *const std::ffi::c_void) -> usize;
    }
    // SAFETY: `p` is the start of a live allocation made by the system
    // allocator (callers only pass pointers of non-zero-capacity buffers).
    unsafe { malloc_size(p.cast()) }
}
#[cfg(not(target_os = "macos"))]
fn heap_size(_p: *const u8) -> usize {
    0
}

// ------------------------------------------------------------------ census

#[derive(Default)]
struct Bucket {
    count: usize,
    heap_blocks: usize,
    len: usize,
    cap: usize,
    heap: usize,
}

#[derive(Default)]
struct Census {
    text: BTreeMap<&'static str, Bucket>,
    inline_kinds: BTreeMap<&'static str, usize>,
    block_kinds: BTreeMap<&'static str, usize>,
    si_kinds: BTreeMap<&'static str, usize>,
    inline_vecs: usize,
    inline_vec_len: usize,
    inline_vec_cap: usize,
    block_vecs: usize,
    block_vec_len: usize,
    block_vec_cap: usize,
    str_len_hist: [usize; 6], // <=7, <=15, <=23, <=31, <=63, more
    str_verbatim: usize,
    str_verbatim_bytes: usize,
    str_not_verbatim: usize,
    str_unresolvable: usize,
    nonempty_attrs: usize,
    attr_sites: usize,
    // "Text run" what-if: maximal runs of adjacent Str/Space (and, in the
    // second counter, SoftBreak too) inside one Vec<Inline>.
    runs: usize,
    run_members: usize,
    run_text_bytes: usize,
    runs_sb: usize,
    run_members_sb: usize,
}

impl Census {
    fn s(&mut self, cat: &'static str, s: &str, cap: usize) {
        let b = self.text.entry(cat).or_default();
        b.count += 1;
        b.len += s.len();
        b.cap += cap;
        if cap > 0 {
            b.heap_blocks += 1;
            b.heap += heap_size(s.as_ptr());
        }
    }
    fn string(&mut self, cat: &'static str, s: &String) {
        self.s(cat, s, s.capacity());
    }
    fn si(&mut self, si: &SourceInfo) {
        let k = match si {
            SourceInfo::Original { .. } => "Original",
            SourceInfo::Substring { .. } => "Substring",
            SourceInfo::Concat { .. } => "Concat",
            SourceInfo::Generated(_) => "Generated",
        };
        *self.si_kinds.entry(k).or_default() += 1;
    }
    fn attr(&mut self, a: &Attr) {
        self.attr_sites += 1;
        if !(a.0.is_empty() && a.1.is_empty() && a.2.is_empty()) {
            self.nonempty_attrs += 1;
        }
        self.string("attr.id", &a.0);
        for c in &a.1 {
            self.string("attr.class", c);
        }
        for (k, v) in &a.2 {
            self.string("attr.key", k);
            self.string("attr.value", v);
        }
    }
    fn str_node(&mut self, text: &String, si: &SourceInfo, src: &[u8]) {
        self.string("Str.text", text);
        let i = match text.len() {
            0..=7 => 0,
            8..=15 => 1,
            16..=23 => 2,
            24..=31 => 3,
            32..=63 => 4,
            _ => 5,
        };
        self.str_len_hist[i] += 1;
        match si.resolve_byte_range() {
            Some((_, s, e)) => {
                if src.get(s..e) == Some(text.as_bytes()) {
                    self.str_verbatim += 1;
                    self.str_verbatim_bytes += text.len();
                } else {
                    self.str_not_verbatim += 1;
                }
            }
            None => self.str_unresolvable += 1,
        }
    }

    fn inlines(&mut self, v: &Vec<Inline>, src: &[u8]) {
        self.inline_vecs += 1;
        self.inline_vec_len += v.len();
        self.inline_vec_cap += v.capacity();
        let (mut in_run, mut in_run_sb) = (false, false);
        for i in v {
            let word = match i {
                Inline::Str(s) => Some(s.text.len()),
                Inline::Space(_) => Some(1),
                _ => None,
            };
            let soft = matches!(i, Inline::SoftBreak(_));
            if let Some(n) = word {
                self.runs += usize::from(!in_run);
                self.run_members += 1;
                self.run_text_bytes += n;
            }
            in_run = word.is_some();
            if word.is_some() || soft {
                self.runs_sb += usize::from(!in_run_sb);
                self.run_members_sb += 1;
            }
            in_run_sb = word.is_some() || soft;
            self.inline(i, src);
        }
    }
    fn blocks(&mut self, v: &Vec<Block>, src: &[u8]) {
        self.block_vecs += 1;
        self.block_vec_len += v.len();
        self.block_vec_cap += v.capacity();
        for b in v {
            self.block(b, src);
        }
    }
    fn citation(&mut self, c: &Citation, src: &[u8]) {
        self.string("Citation.id", &c.id);
        self.inlines(&c.prefix, src);
        self.inlines(&c.suffix, src);
    }
    fn custom(&mut self, c: &CustomNode, src: &[u8]) {
        self.string("Custom.type_name", &c.type_name);
        self.attr(&c.attr);
        for (k, slot) in &c.slots {
            self.string("Custom.slot_name", k);
            match slot {
                Slot::Block(b) => self.block(b, src),
                Slot::Inline(i) => self.inline(i, src),
                Slot::Blocks(b) => self.blocks(b, src),
                Slot::Inlines(i) => self.inlines(i, src),
            }
        }
    }

    fn inline(&mut self, i: &Inline, src: &[u8]) {
        self.si(i.source_info());
        let kind: &'static str = match i {
            Inline::Str(s) => {
                self.str_node(&s.text, &s.source_info, src);
                "Str"
            }
            Inline::Emph(x) => {
                self.inlines(&x.content, src);
                "Emph"
            }
            Inline::Underline(x) => {
                self.inlines(&x.content, src);
                "Underline"
            }
            Inline::Strong(x) => {
                self.inlines(&x.content, src);
                "Strong"
            }
            Inline::Strikeout(x) => {
                self.inlines(&x.content, src);
                "Strikeout"
            }
            Inline::Superscript(x) => {
                self.inlines(&x.content, src);
                "Superscript"
            }
            Inline::Subscript(x) => {
                self.inlines(&x.content, src);
                "Subscript"
            }
            Inline::SmallCaps(x) => {
                self.inlines(&x.content, src);
                "SmallCaps"
            }
            Inline::Quoted(x) => {
                self.inlines(&x.content, src);
                "Quoted"
            }
            Inline::Cite(x) => {
                for c in &x.citations {
                    self.citation(c, src);
                }
                self.inlines(&x.content, src);
                "Cite"
            }
            Inline::Code(x) => {
                self.attr(&x.attr);
                self.string("Code.text", &x.text);
                "Code"
            }
            Inline::Space(_) => "Space",
            Inline::SoftBreak(_) => "SoftBreak",
            Inline::LineBreak(_) => "LineBreak",
            Inline::Math(x) => {
                self.string("Math.text", &x.text);
                "Math"
            }
            Inline::RawInline(x) => {
                self.string("Raw.format", &x.format);
                self.string("Raw.text", &x.text);
                "RawInline"
            }
            Inline::Link(x) => {
                self.attr(&x.attr);
                self.inlines(&x.content, src);
                self.string("target.url", &x.target.0);
                self.string("target.title", &x.target.1);
                "Link"
            }
            Inline::Image(x) => {
                self.attr(&x.attr);
                self.inlines(&x.content, src);
                self.string("target.url", &x.target.0);
                self.string("target.title", &x.target.1);
                "Image"
            }
            Inline::Note(x) => {
                self.blocks(&x.content, src);
                "Note"
            }
            Inline::Span(x) => {
                self.attr(&x.attr);
                self.inlines(&x.content, src);
                "Span"
            }
            Inline::Shortcode(x) => {
                self.string("Shortcode.name", &x.name);
                "Shortcode"
            }
            Inline::NoteReference(x) => {
                self.string("Note.id", &x.id);
                "NoteReference"
            }
            Inline::Attr(x) => {
                self.attr(&x.attr);
                "Attr"
            }
            Inline::Insert(x) => {
                self.attr(&x.attr);
                self.inlines(&x.content, src);
                "Insert"
            }
            Inline::Delete(x) => {
                self.attr(&x.attr);
                self.inlines(&x.content, src);
                "Delete"
            }
            Inline::Highlight(x) => {
                self.attr(&x.attr);
                self.inlines(&x.content, src);
                "Highlight"
            }
            Inline::EditComment(x) => {
                self.attr(&x.attr);
                self.inlines(&x.content, src);
                "EditComment"
            }
            Inline::Custom(c) => {
                self.custom(c, src);
                "Custom"
            }
        };
        *self.inline_kinds.entry(kind).or_default() += 1;
    }

    fn rows(&mut self, rows: &[Row], src: &[u8]) {
        for r in rows {
            self.attr(&r.attr);
            for Cell { attr, content, .. } in &r.cells {
                self.attr(attr);
                self.blocks(content, src);
            }
        }
    }
    fn table(&mut self, t: &Table, src: &[u8]) {
        self.attr(&t.attr);
        if let Some(s) = &t.caption.short {
            self.inlines(s, src);
        }
        if let Some(l) = &t.caption.long {
            self.blocks(l, src);
        }
        self.attr(&t.head.attr);
        self.rows(&t.head.rows, src);
        for b in &t.bodies {
            self.attr(&b.attr);
            self.rows(&b.head, src);
            self.rows(&b.body, src);
        }
        self.attr(&t.foot.attr);
        self.rows(&t.foot.rows, src);
    }

    fn block(&mut self, b: &Block, src: &[u8]) {
        self.si(b.source_info());
        let kind: &'static str = match b {
            Block::Plain(x) => {
                self.inlines(&x.content, src);
                "Plain"
            }
            Block::Paragraph(x) => {
                self.inlines(&x.content, src);
                "Paragraph"
            }
            Block::LineBlock(x) => {
                for l in &x.content {
                    self.inlines(l, src);
                }
                "LineBlock"
            }
            Block::CodeBlock(x) => {
                self.attr(&x.attr);
                self.string("CodeBlock.text", &x.text);
                "CodeBlock"
            }
            Block::RawBlock(x) => {
                self.string("Raw.format", &x.format);
                self.string("Raw.text", &x.text);
                "RawBlock"
            }
            Block::BlockQuote(x) => {
                self.blocks(&x.content, src);
                "BlockQuote"
            }
            Block::OrderedList(x) => {
                for i in &x.content {
                    self.blocks(i, src);
                }
                "OrderedList"
            }
            Block::BulletList(x) => {
                for i in &x.content {
                    self.blocks(i, src);
                }
                "BulletList"
            }
            Block::DefinitionList(x) => {
                for (t, ds) in &x.content {
                    self.inlines(t, src);
                    for d in ds {
                        self.blocks(d, src);
                    }
                }
                "DefinitionList"
            }
            Block::Header(x) => {
                self.attr(&x.attr);
                self.inlines(&x.content, src);
                "Header"
            }
            Block::HorizontalRule(_) => "HorizontalRule",
            Block::Table(t) => {
                self.table(t, src);
                "Table"
            }
            Block::Figure(x) => {
                self.attr(&x.attr);
                if let Some(s) = &x.caption.short {
                    self.inlines(s, src);
                }
                if let Some(l) = &x.caption.long {
                    self.blocks(l, src);
                }
                self.blocks(&x.content, src);
                "Figure"
            }
            Block::Div(x) => {
                self.attr(&x.attr);
                self.blocks(&x.content, src);
                "Div"
            }
            Block::BlockMetadata(x) => {
                self.meta(&x.meta, src);
                "BlockMetadata"
            }
            Block::NoteDefinitionPara(x) => {
                self.string("Note.id", &x.id);
                self.inlines(&x.content, src);
                "NoteDefinitionPara"
            }
            Block::NoteDefinitionFencedBlock(x) => {
                self.string("Note.id", &x.id);
                self.blocks(&x.content, src);
                "NoteDefinitionFencedBlock"
            }
            Block::CaptionBlock(x) => {
                self.inlines(&x.content, src);
                "CaptionBlock"
            }
            Block::Custom(c) => {
                self.custom(c, src);
                "Custom"
            }
        };
        *self.block_kinds.entry(kind).or_default() += 1;
    }

    fn meta(&mut self, m: &ConfigValue, src: &[u8]) {
        match &m.value {
            ConfigValueKind::PandocInlines(i) => self.inlines(i, src),
            ConfigValueKind::PandocBlocks(b) => self.blocks(b, src),
            ConfigValueKind::Array(a) => {
                for v in a {
                    self.meta(v, src);
                }
            }
            ConfigValueKind::Map(entries) => {
                for e in entries {
                    self.string("meta.key", &e.key);
                    self.meta(&e.value, src);
                }
            }
            ConfigValueKind::Path(s) | ConfigValueKind::Glob(s) | ConfigValueKind::Expr(s) => {
                self.string("meta.scalar", s)
            }
            ConfigValueKind::Scalar { .. } => {
                *self.block_kinds.entry("(meta scalar)").or_default() += 1;
            }
        }
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "md" || x == "qmd") {
            out.push(p);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args().nth(1).ok_or("usage: ast-memory <dir>")?;

    println!("== type sizes (bytes) ==");
    macro_rules! sz {
        ($($t:ty),* $(,)?) => { $( println!("  {:<44} {}", stringify!($t), std::mem::size_of::<$t>()); )* };
    }
    sz!(
        String,
        SourceInfo,
        quarto_pandoc_types::inline::Str,
        quarto_pandoc_types::inline::Space,
        Inline,
        quarto_pandoc_types::inline::Link,
        quarto_pandoc_types::inline::Code,
        Attr,
        quarto_pandoc_types::attr::AttrSourceInfo,
        CustomNode,
        Block,
        quarto_pandoc_types::block::Paragraph,
        quarto_pandoc_types::block::CodeBlock,
        Table,
        ConfigValue,
    );

    let s0 = snap();
    let mut paths = Vec::new();
    collect(Path::new(&dir), &mut paths);
    paths.sort();
    let sources: Vec<Vec<u8>> = paths.iter().filter_map(|p| std::fs::read(p).ok()).collect();
    let total_src: usize = sources.iter().map(Vec::len).sum();
    let s1 = snap();
    println!("\n== corpus ==");
    println!(
        "  files={} source_bytes={} ({:.2} MiB)",
        sources.len(),
        total_src,
        mb(total_src)
    );

    println!("\n== allocator traffic ==");
    phase("read sources", s0, s1);

    let mut asts = Vec::with_capacity(sources.len());
    let mut ctxs = Vec::with_capacity(sources.len());
    let mut ok_src: Vec<&[u8]> = Vec::with_capacity(sources.len());
    let mut failed = 0usize;
    let mut failed_bytes = 0usize;
    let s2 = snap();
    PEAK.store(LIVE.load(Relaxed), Relaxed);
    for (i, src) in sources.iter().enumerate() {
        let name = format!("/doc-{i}.qmd");
        match pampa::readers::qmd::read(src, false, &name, &mut std::io::sink(), true, None) {
            Ok((pandoc, ctx, _diags)) => {
                asts.push(pandoc);
                ctxs.push(ctx);
                ok_src.push(src);
            }
            Err(_) => {
                failed += 1;
                failed_bytes += src.len();
            }
        }
    }
    let s3 = snap();
    phase("parse all (retain AST+ctx)", s2, s3);
    let parsed_bytes = total_src - failed_bytes;
    println!(
        "  parsed files={} ({:.2} MiB); failed files={failed} ({:.2} MiB)",
        asts.len(),
        mb(parsed_bytes),
        mb(failed_bytes)
    );
    println!(
        "  peak live during parse (above pre-parse) = {:.1} MiB",
        mb(PEAK.load(Relaxed) - s2.live)
    );

    // Census (allocates only its own small maps).
    let mut c = Census::default();
    let mut uniq: HashSet<&str> = HashSet::new();
    for (p, src) in asts.iter().zip(&ok_src) {
        c.meta(&p.meta, src);
        c.blocks(&p.blocks, src);
    }
    fn uniq_walk<'a>(v: &'a [Inline], u: &mut HashSet<&'a str>) {
        for i in v {
            if let Inline::Str(s) = i {
                u.insert(&s.text);
            }
        }
    }
    // Unique Str texts, top-level paragraphs only is not enough; do a cheap
    // full pass via the serialized census instead: count from paragraphs,
    // headers and plain blocks at any depth reachable through block lists.
    fn uniq_blocks<'a>(bs: &'a [Block], u: &mut HashSet<&'a str>) {
        for b in bs {
            match b {
                Block::Plain(x) => uniq_walk(&x.content, u),
                Block::Paragraph(x) => uniq_walk(&x.content, u),
                Block::Header(x) => uniq_walk(&x.content, u),
                Block::BlockQuote(x) => uniq_blocks(&x.content, u),
                Block::Div(x) => uniq_blocks(&x.content, u),
                Block::BulletList(x) => x.content.iter().for_each(|i| uniq_blocks(i, u)),
                Block::OrderedList(x) => x.content.iter().for_each(|i| uniq_blocks(i, u)),
                _ => {}
            }
        }
    }
    let mut uniq_seen = 0usize;
    for p in &asts {
        uniq_blocks(&p.blocks, &mut uniq);
    }
    let uniq_bytes: usize = uniq.iter().map(|s| s.len()).sum();
    fn count_top<'a>(bs: &'a [Block], n: &mut usize) {
        for b in bs {
            let f = |v: &'a [Inline], n: &mut usize| {
                *n += v.iter().filter(|i| matches!(i, Inline::Str(_))).count()
            };
            match b {
                Block::Plain(x) => f(&x.content, n),
                Block::Paragraph(x) => f(&x.content, n),
                Block::Header(x) => f(&x.content, n),
                Block::BlockQuote(x) => count_top(&x.content, n),
                Block::Div(x) => count_top(&x.content, n),
                Block::BulletList(x) => x.content.iter().for_each(|i| count_top(i, n)),
                Block::OrderedList(x) => x.content.iter().for_each(|i| count_top(i, n)),
                _ => {}
            }
        }
    }
    for p in &asts {
        count_top(&p.blocks, &mut uniq_seen);
    }

    println!("\n== text fields (retained ASTs) ==");
    println!(
        "  {:<18} {:>9} {:>11} {:>11} {:>11} {:>11}",
        "field", "count", "heap_blocks", "len_bytes", "cap_bytes", "malloc_bytes"
    );
    let mut tot = Bucket::default();
    for (k, b) in &c.text {
        println!(
            "  {:<18} {:>9} {:>11} {:>11} {:>11} {:>11}",
            k, b.count, b.heap_blocks, b.len, b.cap, b.heap
        );
        tot.count += b.count;
        tot.heap_blocks += b.heap_blocks;
        tot.len += b.len;
        tot.cap += b.cap;
        tot.heap += b.heap;
    }
    println!(
        "  {:<18} {:>9} {:>11} {:>11} {:>11} {:>11}",
        "TOTAL", tot.count, tot.heap_blocks, tot.len, tot.cap, tot.heap
    );

    println!("\n== Str detail ==");
    let labels = ["<=7", "8-15", "16-23", "24-31", "32-63", ">=64"];
    let nstr: usize = c.str_len_hist.iter().sum();
    for (l, n) in labels.iter().zip(c.str_len_hist) {
        println!(
            "  len {:<6} {:>9}  {:>5.1}%",
            l,
            n,
            100.0 * n as f64 / nstr.max(1) as f64
        );
    }
    println!(
        "  verbatim-slice-of-source: {} ({:.1}%), {} bytes; differs: {}; unresolvable source_info: {}",
        c.str_verbatim,
        100.0 * c.str_verbatim as f64 / nstr.max(1) as f64,
        c.str_verbatim_bytes,
        c.str_not_verbatim,
        c.str_unresolvable
    );
    println!(
        "  distinct Str texts (para/header/plain reachable via lists/quotes/divs): {} distinct of {} nodes, {} distinct bytes",
        uniq.len(),
        uniq_seen,
        uniq_bytes
    );

    println!("\n== node census ==");
    let ninl: usize = c.inline_kinds.values().sum();
    let nblk: usize = c.block_kinds.values().sum();
    println!("  inlines={ninl} blocks={nblk}");
    let mut ik: Vec<_> = c.inline_kinds.iter().collect();
    ik.sort_by(|a, b| b.1.cmp(a.1));
    for (k, n) in ik {
        println!("    inline {:<16} {:>9}", k, n);
    }
    let mut bk: Vec<_> = c.block_kinds.iter().collect();
    bk.sort_by(|a, b| b.1.cmp(a.1));
    for (k, n) in bk {
        println!("    block  {:<26} {:>9}", k, n);
    }
    for (k, n) in &c.si_kinds {
        println!("    node source_info {:<10} {:>9}", k, n);
    }
    let isz = std::mem::size_of::<Inline>();
    let bsz = std::mem::size_of::<Block>();
    println!(
        "  Vec<Inline>: {} vecs, len {} cap {} -> {:.1} MiB in use, {:.1} MiB reserved",
        c.inline_vecs,
        c.inline_vec_len,
        c.inline_vec_cap,
        mb(c.inline_vec_len * isz),
        mb(c.inline_vec_cap * isz)
    );
    println!(
        "  Vec<Block>:  {} vecs, len {} cap {} -> {:.1} MiB in use, {:.1} MiB reserved",
        c.block_vecs,
        c.block_vec_len,
        c.block_vec_cap,
        mb(c.block_vec_len * bsz),
        mb(c.block_vec_cap * bsz)
    );
    println!(
        "  text-run what-if: Str/Space runs={} covering {} inlines ({} text bytes) -> {} inlines total; \
         with SoftBreak: runs={} covering {} -> {} inlines total",
        c.runs,
        c.run_members,
        c.run_text_bytes,
        ninl - c.run_members + c.runs,
        c.runs_sb,
        c.run_members_sb,
        ninl - c.run_members_sb + c.runs_sb
    );
    println!(
        "  attr sites: {} of which non-empty: {}",
        c.attr_sites, c.nonempty_attrs
    );

    drop(uniq);
    println!("\n== what the retained data costs ==");
    let s4 = snap();
    drop(ctxs);
    let s5 = snap();
    phase("drop ASTContexts", s4, s5);
    drop(asts);
    let s6 = snap();
    phase("drop Pandoc ASTs", s5, s6);
    let ast_bytes = s5.live - s6.live;
    let ast_blocks = (s6.frees - s5.frees) - (s6.allocs - s5.allocs);
    println!(
        "\n  AST heap = {:.1} MiB in {} blocks = {:.2}x parsed source bytes; {:.1} heap blocks per KiB of source",
        mb(ast_bytes),
        ast_blocks,
        ast_bytes as f64 / parsed_bytes as f64,
        ast_blocks as f64 / (parsed_bytes as f64 / 1024.0)
    );
    println!(
        "  Str.text share: {:.1}% of AST heap blocks, {:.1}% of AST heap bytes (malloc-rounded)",
        100.0 * c.text["Str.text"].heap_blocks as f64 / ast_blocks as f64,
        100.0 * c.text["Str.text"].heap as f64 / ast_bytes as f64
    );
    println!(
        "  all text fields: {:.1}% of AST heap blocks, {:.1}% of AST heap bytes (malloc-rounded)",
        100.0 * tot.heap_blocks as f64 / ast_blocks as f64,
        100.0 * tot.heap as f64 / ast_bytes as f64
    );
    println!(
        "  Vec<Inline> buffers: {:.1}% of AST heap bytes (reserved)",
        100.0 * (c.inline_vec_cap * isz) as f64 / ast_bytes as f64
    );
    Ok(())
}
