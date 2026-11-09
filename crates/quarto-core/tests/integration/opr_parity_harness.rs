//! OPR-0b: the parity harness for the plain-render switch.
//!
//! A document is rendered twice through the real render path (`render_document_to_file`): once
//! plain, once with `execute: {record: true}` (the recorded route, which the presenter owns).
//! Each side is reduced to its output text plus the SET OF FIGURE FILE NAMES it wrote (Julia
//! Plots figures are inline base64, so the HTML alone hides a file-name change), normalized,
//! and compared. A difference is either explained by a named rule of the allowlist
//! (`opr-parity-allowlist.toml`) or it is a blocker. The gate rule itself (zero unallowlisted
//! differences) is applied in OPR-7c; this module only measures.
//!
//! Reuse: the knitr corpus's [`Class`] is the vocabulary (an HTML pair has no weaker level than
//! `Identical`, so only `Identical` and `Blocker` occur), and the corpus manifest, `r_available`
//! and `parallel` come from the knitr support modules. The knitr classifier itself compares
//! markdown at three levels and has nothing to say about two rendered pages, so it is not forked.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::NativeRuntime;
use regex::Regex;
use serde::Deserialize;

use super::knitr_corpus_classify::Class;

/// One rendered side of a pair, already reduced to what the comparison needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    /// The output text (HTML, or the `.typ` file for typst), normalized.
    pub text: String,
    /// Figure file names written next to the output, relative to the output directory,
    /// `/`-separated (so `doc_files/figure-html/cell-1-output-1.png`).
    pub figures: BTreeSet<String>,
}

/// What a pair came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Same normalized text and the same figure file names.
    Identical,
    /// Different, but equal once the named allowlist rules are applied.
    Allowlisted { rules: Vec<String> },
    /// Different, and not explained by the allowlist: the first differences.
    Blocker { diff: String },
}

impl Verdict {
    pub fn class(&self) -> Class {
        match self {
            Verdict::Blocker { .. } => Class::Blocker,
            _ => Class::Identical,
        }
    }
}

/// One allowlist entry. `rule` names a built-in normalization (see [`apply_rule`]); `scope` is a
/// document-name prefix the entry covers. `status` is `proposed` until Gordon signs it off.
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    pub name: String,
    pub rule: String,
    pub scope: String,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Allowlist {
    #[serde(default, rename = "entry")]
    pub entries: Vec<Entry>,
}

impl Allowlist {
    pub fn parse(text: &str) -> Result<Self, String> {
        let list: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        for e in &list.entries {
            if e.reason.trim().is_empty() {
                return Err(format!("entry `{}` has no reason", e.name));
            }
            if !RULES.contains(&e.rule.as_str()) {
                return Err(format!(
                    "entry `{}` names an unknown rule `{}`",
                    e.name, e.rule
                ));
            }
        }
        Ok(list)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The entries whose scope covers `document`.
    pub fn covering<'a>(&'a self, document: &str) -> Vec<&'a Entry> {
        self.entries
            .iter()
            .filter(|e| document.starts_with(&e.scope))
            .collect()
    }
}

/// The rule names an allowlist entry may use. Each is a normalization applied to BOTH sides, so a
/// rule hides exactly the difference it names and nothing else.
pub const RULES: &[&str] = &[
    "knitr-empty-cell-wrapper",
    "knitr-adjacent-blockquotes",
    "julia-code-leading-newline",
];

/// Apply the named normalization to one side. Unknown names are a bug in the allowlist file.
pub fn apply_rule(rule: &str, side: &mut Side) -> Result<(), String> {
    let sub = |pattern: &str, with: &str, side: &mut Side| {
        let re = Regex::new(pattern).expect("rule pattern");
        side.text = re.replace_all(&side.text, with).into_owned();
    };
    match rule {
        // R writes an empty `.cell` div for a chunk that renders nothing; the presenter writes none
        // (knitr epic decision 23, signed off for the knitr baseline presenter).
        "knitr-empty-cell-wrapper" => sub(r#"<div class="cell">\s*</div>\s*"#, "", side),
        // R's markdown puts a blank line inside a quoted chunk, which splits the quote in two; the
        // presenter keeps one blockquote (knitr epic decision 23, the baseline's blocker-class
        // entry for structure/chunk_in_blockquote). Joining adjacent quotes hides exactly that.
        "knitr-adjacent-blockquotes" => sub(r"</blockquote>\s*<blockquote>\s*", "", side),
        // The engine route keeps a code block's leading blank line (a newline right after the
        // opening `code` tag); the presenter trims it. A browser drops it anyway.
        "julia-code-leading-newline" => sub(r#"(<code class="sourceCode julia">)\n"#, "$1", side),
        other => return Err(format!("unknown rule `{other}`")),
    }
    Ok(())
}

/// Reduce raw rendered text to what is compared: image payloads and absolute paths are noise.
pub fn normalize_text(text: &str, dirs: &[&Path]) -> String {
    let payload = Regex::new(r"data:image/([A-Za-z0-9.+-]+);base64,[A-Za-z0-9+/=\s]*").unwrap();
    let mut out = payload
        .replace_all(text, "data:image/$1;base64,<DATA>")
        .into_owned();
    // Generated ids that differ on every render: flextable's `cl-<8 hex>` classes and gt's
    // ten-letter table id (a `div id` followed by the table's inline style, then used in the CSS).
    let flextable = Regex::new(r"cl-[0-9a-f]{8}").unwrap();
    out = flextable.replace_all(&out, "cl-<ID>").into_owned();
    let gt = Regex::new(r#"<div id="([a-z]{10})" style="padding-left:0px"#).unwrap();
    let gt_ids: Vec<String> = gt.captures_iter(&out).map(|c| c[1].to_string()).collect();
    for id in gt_ids {
        out = out.replace(&id, "<GTID>");
    }
    // The same directory has several spellings (`/var` and `/private/var` on macOS).
    let mut spellings: Vec<String> = Vec::new();
    for d in dirs {
        let mut paths = vec![d.to_path_buf()];
        if let Ok(c) = d.canonicalize() {
            paths.push(c);
        }
        for p in paths {
            let s = p.to_string_lossy().replace('\\', "/");
            if let Some(stripped) = s.strip_prefix("/private") {
                spellings.push(stripped.to_string());
            }
            spellings.push(s);
        }
    }
    // Longest first, so `/private/var/x` is not left as `/private<DIR>`.
    spellings.sort_by_key(|s| std::cmp::Reverse(s.len()));
    spellings.dedup();
    for s in spellings {
        if !s.is_empty() {
            out = out.replace(&s, "<DIR>");
        }
    }
    out
}

/// The lines (and figure names) present on one side only, for a blocker's report.
fn describe_difference(plain: &Side, recorded: &Side) -> String {
    fn lines_only_in<'a>(a: &'a str, b: &str) -> Vec<&'a str> {
        let mut remaining: std::collections::HashMap<&str, usize> = Default::default();
        for l in b.lines() {
            *remaining.entry(l).or_default() += 1;
        }
        a.lines()
            .filter(|l| match remaining.get_mut(l) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    false
                }
                _ => true,
            })
            .collect()
    }
    let clip = |l: &str| -> String {
        let l = l.trim();
        if l.chars().count() > 240 {
            format!("{}...", l.chars().take(240).collect::<String>())
        } else {
            l.to_string()
        }
    };
    let mut out = String::new();
    for (mark, only) in [
        ("- plain    ", lines_only_in(&plain.text, &recorded.text)),
        ("+ recorded ", lines_only_in(&recorded.text, &plain.text)),
    ] {
        for l in only.iter().take(8) {
            out.push_str(&format!("{mark}{}\n", clip(l)));
        }
        if only.len() > 8 {
            out.push_str(&format!("{mark}... {} more lines\n", only.len() - 8));
        }
    }
    for f in plain.figures.difference(&recorded.figures) {
        out.push_str(&format!("- plain    figure {f}\n"));
    }
    for f in recorded.figures.difference(&plain.figures) {
        out.push_str(&format!("+ recorded figure {f}\n"));
    }
    out
}

/// Compare a plain side with a recorded side under the rules that cover `document`.
pub fn compare(document: &str, plain: &Side, recorded: &Side, allowlist: &Allowlist) -> Verdict {
    if plain == recorded {
        return Verdict::Identical;
    }
    let (mut p, mut r) = (plain.clone(), recorded.clone());
    let mut used: Vec<String> = Vec::new();
    for entry in allowlist.covering(document) {
        let (mut p2, mut r2) = (p.clone(), r.clone());
        apply_rule(&entry.rule, &mut p2).expect("rules are validated at parse");
        apply_rule(&entry.rule, &mut r2).expect("rules are validated at parse");
        if p2 != p || r2 != r {
            if !used.contains(&entry.rule) {
                used.push(entry.rule.clone());
            }
            p = p2;
            r = r2;
        }
    }
    if p == r {
        Verdict::Allowlisted { rules: used }
    } else {
        Verdict::Blocker {
            diff: describe_difference(&p, &r),
        }
    }
}

static SCRATCH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

/// Write the scratch copy's `_quarto.yml`: the document's own (or a default project that renders
/// exactly `file`), plus `execute: {record: true}` on the recorded side. Project metadata is how
/// the key reaches every document kind (a `.ipynb` has no front matter to edit).
fn write_project_config(
    dir: &Path,
    file: &str,
    format: &str,
    recorded: bool,
) -> Result<(), String> {
    let path = dir.join("_quarto.yml");
    let mut config: serde_yaml::Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_yaml::from_str(&text).map_err(|e| format!("_quarto.yml: {e}"))?,
        Err(_) => serde_yaml::from_str(&format!(
            "project:\n  type: default\n  render: [\"{file}\"]\n"
        ))
        .unwrap(),
    };
    let map = config
        .as_mapping_mut()
        .ok_or("_quarto.yml is not a mapping")?;
    if format == "typst" {
        // Typst compiles to a PDF; keep the `.typ` so there is text to compare.
        let key = serde_yaml::Value::from("format");
        let mut formats = match map.remove(&key) {
            Some(serde_yaml::Value::Mapping(m)) => m,
            _ => serde_yaml::Mapping::new(),
        };
        let tkey = serde_yaml::Value::from("typst");
        let mut typst = match formats.remove(&tkey) {
            Some(serde_yaml::Value::Mapping(m)) => m,
            _ => serde_yaml::Mapping::new(),
        };
        typst.insert("keep-typ".into(), true.into());
        formats.insert(tkey, serde_yaml::Value::Mapping(typst));
        map.insert(key, serde_yaml::Value::Mapping(formats));
    }
    if recorded {
        let key = serde_yaml::Value::from("execute");
        let mut execute = match map.remove(&key) {
            Some(serde_yaml::Value::Mapping(m)) => m,
            _ => serde_yaml::Mapping::new(),
        };
        execute.insert("record".into(), true.into());
        map.insert(key, serde_yaml::Value::Mapping(execute));
    }
    let text = serde_yaml::to_string(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

fn is_figure_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".png", ".svg", ".jpg", ".jpeg", ".gif", ".webp", ".pdf"]
        .iter()
        .any(|e| lower.ends_with(e))
}

fn collect_figures(dir: &Path, base: &Path, out: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            // Engine state and execution records are not output.
            if name != ".quarto" && name != "_freeze" {
                collect_figures(&path, base, out);
            }
        } else if is_figure_file(&name)
            && let Ok(rel) = path.strip_prefix(base)
        {
            out.insert(quarto_util::to_forward_slashes(rel));
        }
    }
}

/// Render `file` (inside `src_dir`) in a scratch copy under `root`, plain or recorded, and
/// reduce the result to a [`Side`].
pub fn render_side(
    src_dir: &Path,
    file: &str,
    format: &str,
    recorded: bool,
    root: &Path,
) -> Result<Side, String> {
    let dir = prepare_dir(src_dir, file, format, recorded, root)?;
    render_prepared(&dir, file, format)
}

/// Copy `src_dir` to a fresh scratch directory under `root` and configure it for one route.
/// Rendering the returned directory more than once keeps its caches, which is how a cache miss
/// and then a cache hit are both measured.
pub fn prepare_dir(
    src_dir: &Path,
    file: &str,
    format: &str,
    recorded: bool,
    root: &Path,
) -> Result<PathBuf, String> {
    let n = SCRATCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = root.join(format!(
        "{}-{n}",
        if recorded { "recorded" } else { "plain" }
    ));
    copy_tree(src_dir, &dir).map_err(|e| format!("copy: {e}"))?;
    write_project_config(&dir, file, format, recorded)?;
    Ok(dir)
}

/// Render `file` in a prepared directory and reduce the result to a [`Side`].
pub fn render_prepared(dir: &Path, file: &str, format: &str) -> Result<Side, String> {
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let result = render_document_to_file(
        &dir.join(file),
        format,
        &options,
        None,
        Arc::new(NativeRuntime::new()),
        None,
        None,
        Some(format),
    )
    .map_err(|e| format!("{e:?}"))?;

    // typst compiles to a PDF; the `.typ` beside it (`keep-typ: true`) is what is compared.
    let mut text_path = result.output_path.clone();
    if text_path.extension().is_some_and(|e| e == "pdf") {
        text_path.set_extension("typ");
    }
    let raw = std::fs::read_to_string(&text_path)
        .map_err(|e| format!("reading {}: {e}", text_path.display()))?;
    let out_dir = result
        .output_path
        .parent()
        .map_or_else(|| dir.to_path_buf(), Path::to_path_buf);
    let mut figures = BTreeSet::new();
    collect_figures(&out_dir, &out_dir, &mut figures);
    Ok(Side {
        text: normalize_text(&raw, &[dir, &out_dir]),
        figures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(text: &str, figures: &[&str]) -> Side {
        Side {
            text: text.to_string(),
            figures: figures.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn allowlist() -> Allowlist {
        Allowlist::parse(
            r#"
[[entry]]
name = "julia code block leading newline"
rule = "julia-code-leading-newline"
scope = "julia/"
status = "proposed"
reason = "the engine route keeps a blank first line the presenter trims"
"#,
        )
        .unwrap()
    }

    #[test]
    fn allowlist_parse_rejects_an_entry_without_a_reason() {
        let bad = "[[entry]]\nname='x'\nrule='julia-code-leading-newline'\nscope='j'\nstatus='proposed'\nreason=''\n";
        assert!(Allowlist::parse(bad).is_err());
    }

    #[test]
    fn allowlist_parse_rejects_an_unknown_rule() {
        let bad =
            "[[entry]]\nname='x'\nrule='no-such-rule'\nscope='j'\nstatus='proposed'\nreason='r'\n";
        assert!(Allowlist::parse(bad).is_err());
    }

    #[test]
    fn normalize_strips_image_payloads_and_directories() {
        let dir = Path::new("/tmp/abc/plain");
        let text = "<img src=\"data:image/png;base64,AAAA\"> in /tmp/abc/plain/doc.qmd";
        let n = normalize_text(text, &[dir]);
        assert!(!n.contains("AAAA"), "{n}");
        assert!(n.contains("data:image/png;base64,"), "{n}");
        assert!(!n.contains("/tmp/abc"), "{n}");
    }

    #[test]
    fn normalize_strips_generated_ids_from_flextable_and_gt() {
        let flex = |id: &str| format!("<style>.cl-{id}{{}}</style><td class=\"cl-{id}\">");
        assert_eq!(
            normalize_text(&flex("2a36ba32"), &[]),
            normalize_text(&flex("1ef6e372"), &[])
        );
        let gt = |id: &str| {
            format!(
                "<div id=\"{id}\" style=\"padding-left:0px;\">\n<style>#{id} table {{\n#{id} .gt_table {{"
            )
        };
        assert_eq!(
            normalize_text(&gt("fgculgxszj"), &[]),
            normalize_text(&gt("vdpiehumfu"), &[])
        );
        // A real difference is kept.
        assert_ne!(
            normalize_text("<td>1</td>", &[]),
            normalize_text("<td>2</td>", &[])
        );
    }

    #[test]
    fn the_empty_cell_wrapper_rule_hides_only_an_empty_wrapper() {
        let list = Allowlist::parse(
            "[[entry]]\nname='c'\nrule='knitr-empty-cell-wrapper'\nscope='knitr'\nstatus='proposed'\nreason='r'\n",
        )
        .unwrap();
        let plain = side("<p>a</p>\n<div class=\"cell\">\n</div>\n<p>b</p>", &[]);
        let rec = side("<p>a</p>\n<p>b</p>", &[]);
        assert!(matches!(
            compare("knitr-corpus/x", &plain, &rec, &list),
            Verdict::Allowlisted { .. }
        ));
        let full = side(
            "<p>a</p>\n<div class=\"cell\">\n<pre>1</pre>\n</div>\n<p>b</p>",
            &[],
        );
        assert!(matches!(
            compare("knitr-corpus/x", &full, &rec, &list),
            Verdict::Blocker { .. }
        ));
    }

    #[test]
    fn the_adjacent_blockquotes_rule_hides_only_a_split_quote() {
        let list = Allowlist::parse(
            "[[entry]]\nname='b'\nrule='knitr-adjacent-blockquotes'\nscope='knitr'\nstatus='proposed'\nreason='r'\n",
        )
        .unwrap();
        let plain = side(
            "<blockquote>\n<p>a</p>\n</blockquote>\n<blockquote>\n<p>b</p>\n</blockquote>",
            &[],
        );
        let rec = side("<blockquote>\n<p>a</p>\n<p>b</p>\n</blockquote>", &[]);
        assert!(matches!(
            compare("knitr-corpus/x", &plain, &rec, &list),
            Verdict::Allowlisted { .. }
        ));
        let other = side("<blockquote>\n<p>a</p>\n<p>c</p>\n</blockquote>", &[]);
        assert!(matches!(
            compare("knitr-corpus/x", &plain, &other, &list),
            Verdict::Blocker { .. }
        ));
    }

    #[test]
    fn the_leading_newline_rule_hides_only_that_newline() {
        let list = Allowlist::parse(
            "[[entry]]\nname='n'\nrule='julia-code-leading-newline'\nscope='julia/'\nstatus='proposed'\nreason='r'\n",
        )
        .unwrap();
        let open = "<code class=\"sourceCode julia\">";
        let plain = side(&format!("{open}\nusing Plots</code>"), &[]);
        let rec = side(&format!("{open}using Plots</code>"), &[]);
        assert!(matches!(
            compare("julia/x", &plain, &rec, &list),
            Verdict::Allowlisted { .. }
        ));
        let other = side(&format!("{open}using Other</code>"), &[]);
        assert!(matches!(
            compare("julia/x", &plain, &other, &list),
            Verdict::Blocker { .. }
        ));
    }

    #[test]
    fn identical_pair_is_identical() {
        let a = side("<p>x</p>", &["d_files/figure-html/f-1.png"]);
        assert_eq!(
            compare("knitr/a", &a, &a.clone(), &allowlist()),
            Verdict::Identical
        );
    }

    const OPEN: &str = "<code class=\"sourceCode julia\">";

    #[test]
    fn one_known_difference_is_a_blocker_without_an_entry() {
        let plain = side(&format!("{OPEN}\nx</code>"), &[]);
        let rec = side(&format!("{OPEN}x</code>"), &[]);
        // `knitr/` is outside the entry's scope.
        match compare("knitr/a", &plain, &rec, &allowlist()) {
            Verdict::Blocker { diff } => assert!(diff.contains("sourceCode"), "{diff}"),
            other => panic!("expected a blocker, got {other:?}"),
        }
    }

    #[test]
    fn one_known_difference_is_allowlisted_inside_the_scope() {
        let plain = side(&format!("{OPEN}\nx</code>"), &[]);
        let rec = side(&format!("{OPEN}x</code>"), &[]);
        assert_eq!(
            compare("julia/a", &plain, &rec, &allowlist()),
            Verdict::Allowlisted {
                rules: vec!["julia-code-leading-newline".to_string()]
            }
        );
    }

    #[test]
    fn a_figure_file_name_difference_alone_is_a_blocker_even_inside_the_scope() {
        // Identical text, different file-name sets: the case an HTML-only compare misses.
        // OPR-7f removed the rule that hid this for Julia, so it is a blocker on both sides.
        let plain = side("<p>x</p>", &["d_files/figure-html/cell-2-output-1.png"]);
        let rec = side("<p>x</p>", &["d_files/figure-html/cell-1-output-1.png"]);
        for name in ["knitr/a", "julia/a"] {
            match compare(name, &plain, &rec, &allowlist()) {
                Verdict::Blocker { diff } => {
                    assert!(diff.contains("cell-2-output-1.png"), "{diff}")
                }
                other => panic!("expected a blocker for {name}, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_wrapper_id_or_output_class_difference_is_a_blocker_inside_the_scope() {
        let plain = side("<div id=\"cell-1\" class=\"cell\">x</div>", &[]);
        let rec = side("<div class=\"cell\">x</div>", &[]);
        assert!(matches!(
            compare("julia/a", &plain, &rec, &allowlist()),
            Verdict::Blocker { .. }
        ));
        let plain = side(
            "<div class=\"cell-output cell-output-display\">x</div>",
            &[],
        );
        let rec = side("<div class=\"cell-output-display\">x</div>", &[]);
        assert!(matches!(
            compare("julia/a", &plain, &rec, &allowlist()),
            Verdict::Blocker { .. }
        ));
    }

    #[test]
    fn an_allowlist_rule_does_not_hide_an_unrelated_difference() {
        let plain = side(&format!("{OPEN}\nx</code>"), &[]);
        let rec = side(&format!("{OPEN}y</code>"), &[]);
        assert!(matches!(
            compare("julia/a", &plain, &rec, &allowlist()),
            Verdict::Blocker { .. }
        ));
    }

    /// The real render path on a document with no engine code: both routes must agree.
    #[test]
    fn a_markdown_only_document_renders_identically_both_ways() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(
            src.path().join("doc.qmd"),
            "---\ntitle: Plain\n---\n\nHello *world*.\n",
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let plain = render_side(src.path(), "doc.qmd", "html", false, root.path()).unwrap();
        let rec = render_side(src.path(), "doc.qmd", "html", true, root.path()).unwrap();
        assert!(plain.text.contains("Hello"), "{}", plain.text);
        assert_eq!(
            compare("md/plain", &plain, &rec, &Allowlist::default()),
            Verdict::Identical
        );
    }

    /// A pair of documents that differ in one paragraph: a real render, a known single difference.
    #[test]
    fn a_rendered_pair_with_one_changed_paragraph_is_a_blocker() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("doc.qmd"), "---\ntitle: T\n---\n\nOne.\n").unwrap();
        std::fs::write(b.path().join("doc.qmd"), "---\ntitle: T\n---\n\nTwo.\n").unwrap();
        let root = tempfile::tempdir().unwrap();
        let left = render_side(a.path(), "doc.qmd", "html", false, root.path()).unwrap();
        let right = render_side(b.path(), "doc.qmd", "html", false, root.path()).unwrap();
        match compare("md/changed", &left, &right, &Allowlist::default()) {
            Verdict::Blocker { diff } => {
                assert!(diff.contains("One") && diff.contains("Two"), "{diff}")
            }
            other => panic!("expected a blocker, got {other:?}"),
        }
    }
}
