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
    /// Different, but equal once the covering allowlist entries are applied.
    Allowlisted { applied: Vec<Applied> },
    /// Different, and not explained by the allowlist: the first differences.
    Blocker { diff: String },
}

/// One allowlist entry that changed a pair, with the structural fire count per side (how many
/// matches or merges it made) and whether the verdict needs it (re-comparing without the entry
/// would be a blocker). An entry no document needs is stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub entry: String,
    pub rule: String,
    pub plain_fires: usize,
    pub recorded_fires: usize,
    pub needed: bool,
}

impl Verdict {
    pub fn class(&self) -> Class {
        match self {
            Verdict::Blocker { .. } => Class::Blocker,
            _ => Class::Identical,
        }
    }
}

/// One allowlist entry. `rule` names a built-in normalization (see [`apply_rule`],
/// [`apply_normalizer`], [`PIN_RULE`]). An entry covers documents by `scope` (a name prefix) or by
/// `document` (an exact name), never both. `pin` (only with `document` and [`PIN_RULE`]) is the
/// path of a file beside the allowlist holding the exact remaining difference; [`Allowlist::load`]
/// reads it into `pin_text`. `status` is `proposed` until Gordon signs it off.
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    pub name: String,
    pub rule: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub document: Option<String>,
    #[serde(default)]
    pub pin: Option<String>,
    /// The pin file's text. Not in the TOML: `load` fills it in, tests set it directly.
    #[serde(skip)]
    pub pin_text: Option<String>,
    pub status: String,
    pub reason: String,
}

impl Entry {
    pub fn covers(&self, document: &str) -> bool {
        match &self.document {
            Some(d) => d == document,
            None => document.starts_with(&self.scope),
        }
    }

    /// What the entry covers, for a report.
    pub fn coverage(&self) -> String {
        match &self.document {
            Some(d) => format!("document `{d}`"),
            None => format!("scope `{}`", self.scope),
        }
    }
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
            let known = RULES.contains(&e.rule.as_str())
                || NORMALIZERS.contains(&e.rule.as_str())
                || e.rule == PIN_RULE;
            if !known {
                return Err(format!(
                    "entry `{}` names an unknown rule `{}`",
                    e.name, e.rule
                ));
            }
            match (&e.document, e.scope.is_empty()) {
                (Some(_), false) => {
                    return Err(format!(
                        "entry `{}` sets both `scope` and `document`; use one",
                        e.name
                    ));
                }
                (None, true) => {
                    return Err(format!(
                        "entry `{}` sets neither `scope` nor `document`",
                        e.name
                    ));
                }
                _ => {}
            }
            let needs_document = NORMALIZERS.contains(&e.rule.as_str()) || e.rule == PIN_RULE;
            if needs_document && e.document.is_none() {
                return Err(format!(
                    "entry `{}`: rule `{}` is scoped by an exact `document`, not a `scope` prefix",
                    e.name, e.rule
                ));
            }
            if (e.rule == PIN_RULE) != e.pin.is_some() {
                return Err(format!(
                    "entry `{}`: `pin` goes with rule `{PIN_RULE}` and with nothing else",
                    e.name
                ));
            }
        }
        Ok(list)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut list = Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        for e in &mut list.entries {
            if let Some(pin) = &e.pin {
                let file = base.join(pin);
                let pin_text = std::fs::read_to_string(&file)
                    .map_err(|err| format!("pin {}: {err}", file.display()))?;
                e.pin_text = Some(pin_text);
            }
        }
        Ok(list)
    }

    /// The entries that cover `document`.
    pub fn covering<'a>(&'a self, document: &str) -> Vec<&'a Entry> {
        self.entries.iter().filter(|e| e.covers(document)).collect()
    }
}

/// The rule names an allowlist entry may use. Each is a normalization applied to BOTH sides, so a
/// rule hides exactly the difference it names and nothing else.
pub const RULES: &[&str] = &[
    "knitr-empty-cell-wrapper",
    "knitr-adjacent-blockquotes",
    "julia-code-leading-newline",
];

/// The HTML-aware normalizers an entry may name, each scoped by an exact `document`. Unlike the
/// regex [`RULES`] they restructure markup, so each reports a structural fire count per side.
pub const NORMALIZERS: &[&str] = &["julia-interleaved-cell"];

/// The rule of an entry that pins the exact remaining difference of one document (`pin` file).
pub const PIN_RULE: &str = "pinned-difference";

/// Apply the named regex normalization to one side; returns how many matches it rewrote. Unknown
/// names are a bug in the allowlist file.
pub fn apply_rule(rule: &str, side: &mut Side) -> Result<usize, String> {
    let sub = |pattern: &str, with: &str, side: &mut Side| -> usize {
        let re = Regex::new(pattern).expect("rule pattern");
        let n = re.find_iter(&side.text).count();
        side.text = re.replace_all(&side.text, with).into_owned();
        n
    };
    let fires = match rule {
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
    };
    Ok(fires)
}

/// Apply a named HTML normalizer to one side's text: the new text and its fire count. An error
/// means the page has a shape the normalizer does not recognize, which is a blocker, never a pass.
pub fn apply_normalizer(name: &str, text: &str) -> Result<(String, usize), String> {
    match name {
        "julia-interleaved-cell" => interleaved_cell(text),
        other => Err(format!("unknown normalizer `{other}`")),
    }
}

const MARK_CELL: &str = "<!--OPR-CELL-->";
const MARK_CELL_END: &str = "<!--OPR-CELL-END-->";
const MARK_BEGIN: &str = "<!--OPR-BEGIN-->";
const MARK_END: &str = "<!--OPR-END-->";
const MARK_FIRST_CODE_END: &str = "<!--OPR-FC-->";

/// The writer's copy scaffold around a code block, as measured on real pages (9a S3). The splice
/// asserts this exact text; a writer change makes the normalizer fail, which is a blocker.
const SCAFFOLD_OPEN: &str = "<div class=\"code-copy-outer-scaffold\">\n";
const SCAFFOLD_TAIL: &str = "\n<button title=\"Copy to Clipboard\" class=\"code-copy-button\" aria-label=\"Copy code\"><i class=\"bi\"></i></button>\n</div>\n";

type EndHandler = Box<
    dyn for<'a, 'b> FnOnce(
        &'a mut lol_html::html_content::EndTag<'b>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>,
>;

fn end_marker(marker: &'static str, before: bool) -> EndHandler {
    use lol_html::html_content::ContentType;
    Box::new(move |end| {
        if before {
            end.before(marker, ContentType::Html);
        } else {
            end.after(marker, ContentType::Html);
        }
        Ok(())
    })
}

/// Pass 1 of `julia-interleaved-cell` (lol_html): mark, with comments, every `div.cell` (start and
/// end), the closing `</code>` of each cell's first code block, and every later code block. Every
/// other byte is untouched; nothing is re-serialized.
fn mark_pass(text: &str) -> Result<String, String> {
    use lol_html::html_content::ContentType;
    use lol_html::{RewriteStrSettings, element, rewrite_str};
    use std::cell::RefCell;
    use std::rc::Rc;

    let count = Rc::new(RefCell::new(0usize));
    let (c1, c2, c3) = (count.clone(), count.clone(), count.clone());
    rewrite_str(
        text,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("div.cell", move |el| {
                    *c1.borrow_mut() = 0;
                    el.before(MARK_CELL, ContentType::Html);
                    el.on_end_tag(end_marker(MARK_CELL_END, false))?;
                    Ok(())
                }),
                // A descendant selector: a `fig-subcap` group cell nests its blocks in `figure > div`.
                element!("div.cell div.cell-code", move |el| {
                    let n = {
                        let mut c = c2.borrow_mut();
                        *c += 1;
                        *c
                    };
                    if n > 1 {
                        el.before(MARK_BEGIN, ContentType::Html);
                        el.on_end_tag(end_marker(MARK_END, false))?;
                    }
                    Ok(())
                }),
                element!("div.cell div.cell-code pre code", move |el| {
                    if *c3.borrow() == 1 {
                        el.on_end_tag(end_marker(MARK_FIRST_CODE_END, true))?;
                    }
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::new()
        },
    )
    .map_err(|e| format!("html rewrite failed: {e}"))
}

/// The inner html of a block's `code` element.
fn code_inner(block: &str) -> Result<&str, String> {
    let start = block
        .find("<code")
        .ok_or("a later block has no `code` element")?;
    let start = start
        + block[start..]
            .find('>')
            .ok_or("a later block's `code` tag never closes")?
        + 1;
    let end = block[start..]
        .find("</code>")
        .ok_or("a later block's `code` element never closes")?
        + start;
    Ok(&block[start..end])
}

/// `julia-interleaved-cell`: the 4d interleaving, as a canonical page. In each `div.cell`, every
/// later code block's `code` children are appended (after a newline) to the first block's `code`,
/// and the later block's copy scaffold is cut out. Outputs are NOT joined. Returns the page and the
/// number of later blocks merged (the fire count). Pass 2 is string splicing, so every byte the
/// normalizer does not touch is kept as written (trap 1).
fn interleaved_cell(text: &str) -> Result<(String, usize), String> {
    let marked = mark_pass(text)?;
    let mut fires = 0;
    let mut out = String::with_capacity(marked.len());
    let mut first_code_end: Option<usize> = None;
    let mut depth = 0usize;
    let mut rest = marked.as_str();
    loop {
        let next = [MARK_CELL, MARK_CELL_END, MARK_FIRST_CODE_END, MARK_BEGIN]
            .iter()
            .filter_map(|m| rest.find(m).map(|i| (i, *m)))
            .min_by_key(|(i, _)| *i);
        let Some((i, marker)) = next else { break };
        out.push_str(&rest[..i]);
        let after = &rest[i + marker.len()..];
        match marker {
            MARK_CELL => {
                depth += 1;
                if depth > 1 {
                    return Err("nested `div.cell` elements are not supported".into());
                }
                first_code_end = None;
                rest = after;
            }
            MARK_CELL_END => {
                depth = depth.saturating_sub(1);
                rest = after;
            }
            MARK_FIRST_CODE_END => {
                first_code_end = Some(out.len());
                rest = after;
            }
            _ => {
                let j = after
                    .find(MARK_END)
                    .ok_or("a later block's end marker is missing")?;
                let inner = code_inner(&after[..j])?;
                if !out.ends_with(SCAFFOLD_OPEN) {
                    return Err("unrecognized copy scaffold before a later block".into());
                }
                out.truncate(out.len() - SCAFFOLD_OPEN.len());
                let tail = after[j + MARK_END.len()..]
                    .strip_prefix(SCAFFOLD_TAIL)
                    .ok_or("unrecognized copy scaffold after a later block")?;
                // A later block with no first block would be the hidden-first-unit gap.
                let at = first_code_end.ok_or("a later code block has no first block")?;
                let inserted = format!("\n{inner}");
                out.insert_str(at, &inserted);
                first_code_end = Some(at + inserted.len());
                fires += 1;
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    Ok((out, fires))
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

/// Split `a`'s lines into those with no partner in `b` (a multiset difference) and the rest.
fn split_lines<'a>(a: &'a str, b: &str) -> (Vec<&'a str>, Vec<&'a str>) {
    let mut remaining: std::collections::HashMap<&str, usize> = Default::default();
    for l in b.lines() {
        *remaining.entry(l).or_default() += 1;
    }
    let (mut only, mut common) = (Vec::new(), Vec::new());
    for l in a.lines() {
        match remaining.get_mut(l) {
            Some(n) if *n > 0 => {
                *n -= 1;
                common.push(l);
            }
            _ => only.push(l),
        }
    }
    (only, common)
}

/// The difference of two sides: the lines (and figure names) on one side only, in each side's own
/// order, then a note when the shared lines are in a different order (trap 5: a multiset diff alone
/// is empty for a swap). `clip` limits a report to 8 lines per side of at most 240 characters; a
/// pin is compared unclipped.
fn difference(plain: &Side, recorded: &Side, clip: bool) -> String {
    let show = |l: &str| -> String {
        let l = l.trim();
        if clip && l.chars().count() > 240 {
            format!("{}...", l.chars().take(240).collect::<String>())
        } else {
            l.to_string()
        }
    };
    let (only_p, common_p) = split_lines(&plain.text, &recorded.text);
    let (only_r, common_r) = split_lines(&recorded.text, &plain.text);
    let mut out = String::new();
    for (mark, only) in [("- plain    ", &only_p), ("+ recorded ", &only_r)] {
        let shown = if clip { only.len().min(8) } else { only.len() };
        for l in &only[..shown] {
            out.push_str(&format!("{mark}{}\n", show(l)));
        }
        if only.len() > shown {
            out.push_str(&format!("{mark}... {} more lines\n", only.len() - shown));
        }
    }
    for f in plain.figures.difference(&recorded.figures) {
        out.push_str(&format!("- plain    figure {f}\n"));
    }
    for f in recorded.figures.difference(&plain.figures) {
        out.push_str(&format!("+ recorded figure {f}\n"));
    }
    if common_p != common_r {
        let at = common_p
            .iter()
            .zip(&common_r)
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        let what = if only_p.is_empty() && only_r.is_empty() {
            "same lines, different order"
        } else {
            "the lines both sides share are in a different order"
        };
        out.push_str(&format!(
            "{what}: first at shared line {}: plain `{}` vs recorded `{}`\n",
            at + 1,
            show(common_p[at]),
            show(common_r[at])
        ));
    }
    out
}

/// The unclipped, ordered difference of two sides: what a pin file holds.
pub fn full_difference(plain: &Side, recorded: &Side) -> String {
    difference(plain, recorded, false)
}

/// A blocker's report: the first differing lines.
fn describe_difference(plain: &Side, recorded: &Side) -> String {
    difference(plain, recorded, true)
}

fn side_pair_equal(p: &Side, r: &Side) -> bool {
    p == r
}

/// Compare a plain side with a recorded side under the entries that cover `document`.
///
/// Order: the regex rules run on the raw text first (trap 6), then the HTML normalizers, then a
/// pinned entry compares what is left against its pin. The returned [`Applied`] entries carry
/// whether the verdict needs each one.
pub fn compare(document: &str, plain: &Side, recorded: &Side, allowlist: &Allowlist) -> Verdict {
    let entries = allowlist.covering(document);
    let verdict = compare_with(plain, recorded, &entries);
    let Verdict::Allowlisted { mut applied } = verdict else {
        return verdict;
    };
    for a in &mut applied {
        let without: Vec<&Entry> = entries
            .iter()
            .copied()
            .filter(|e| e.name != a.entry)
            .collect();
        a.needed = matches!(
            compare_with(plain, recorded, &without),
            Verdict::Blocker { .. }
        );
    }
    Verdict::Allowlisted { applied }
}

fn compare_with(plain: &Side, recorded: &Side, entries: &[&Entry]) -> Verdict {
    if plain == recorded {
        return Verdict::Identical;
    }
    let (mut p, mut r) = (plain.clone(), recorded.clone());
    let mut applied: Vec<Applied> = Vec::new();
    fn note(applied: &mut Vec<Applied>, entry: &Entry, pf: usize, rf: usize) {
        if pf + rf > 0 || entry.rule == PIN_RULE {
            applied.push(Applied {
                entry: entry.name.clone(),
                rule: entry.rule.clone(),
                plain_fires: pf,
                recorded_fires: rf,
                needed: true,
            });
        }
    }
    for entry in entries.iter().filter(|e| RULES.contains(&e.rule.as_str())) {
        let pf = apply_rule(&entry.rule, &mut p).expect("rules are validated at parse");
        let rf = apply_rule(&entry.rule, &mut r).expect("rules are validated at parse");
        note(&mut applied, entry, pf, rf);
    }
    for entry in entries
        .iter()
        .filter(|e| NORMALIZERS.contains(&e.rule.as_str()))
    {
        let (pt, pf) = match apply_normalizer(&entry.rule, &p.text) {
            Ok(x) => x,
            Err(e) => return blocker(format!("{} failed on the plain side: {e}\n", entry.rule)),
        };
        let (rt, rf) = match apply_normalizer(&entry.rule, &r.text) {
            Ok(x) => x,
            Err(e) => {
                return blocker(format!("{} failed on the recorded side: {e}\n", entry.rule));
            }
        };
        if pf > 0 {
            // Trap 4: the plain page has multi-block cells the normalizer was not written for.
            return blocker(format!(
                "{} fired {pf} times on the PLAIN side; it is written for the recorded route only\n",
                entry.rule
            ));
        }
        p.text = pt;
        r.text = rt;
        note(&mut applied, entry, pf, rf);
    }
    for entry in entries.iter().filter(|e| e.rule == PIN_RULE) {
        let actual = full_difference(&p, &r);
        let expected = entry.pin_text.as_deref().unwrap_or("");
        if side_pair_equal(&p, &r) {
            // Nothing left to pin: the entry is stale, which `compare` reports through `needed`.
            note(&mut applied, entry, 0, 0);
            continue;
        }
        if actual.trim_end() == expected.trim_end() {
            p = r.clone();
            note(&mut applied, entry, 0, 0);
        } else {
            return blocker(format!(
                "the pinned difference of `{}` changed.\nexpected (the pin):\n{expected}\nactual:\n{actual}",
                entry.name
            ));
        }
    }
    if side_pair_equal(&p, &r) {
        Verdict::Allowlisted { applied }
    } else {
        Verdict::Blocker {
            diff: describe_difference(&p, &r),
        }
    }
}

fn blocker(diff: String) -> Verdict {
    Verdict::Blocker { diff }
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
    let out_dir = result
        .output_path
        .parent()
        .map_or_else(|| dir.to_path_buf(), Path::to_path_buf);
    side_from_output(&text_path, &[dir, &out_dir])
}

/// Text outputs the harness compares as text.
const TEXT_EXTENSIONS: &[&str] = &["html", "htm", "md", "markdown", "typ", "tex", "txt"];

/// The output extensions the harness can read: text, and the zip formats of
/// [`super::opr_parity_zip`]. Anything else has no extractor.
pub fn has_extractor(extension: &str) -> bool {
    let e = extension.to_ascii_lowercase();
    TEXT_EXTENSIONS.contains(&e.as_str()) || super::opr_parity_zip::zip_kind(&e).is_some()
}

/// Reduce one output file to a [`Side`]. Text is normalized; a zip (docx, pptx, epub) is opened
/// and its text parts canonicalized, never its bytes. An output with no extractor is an error that
/// says so, so it is never mistaken for a render failure.
pub fn side_from_output(output: &Path, dirs: &[&Path]) -> Result<Side, String> {
    let ext = output
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if let Some(kind) = super::opr_parity_zip::zip_kind(&ext) {
        let bytes =
            std::fs::read(output).map_err(|e| format!("reading {}: {e}", output.display()))?;
        return super::opr_parity_zip::side_from_zip(&bytes, kind);
    }
    if !has_extractor(&ext) {
        return Err(format!(
            "no extractor for `.{ext}` output ({}); the harness cannot compare it",
            output.display()
        ));
    }
    let raw = std::fs::read_to_string(output)
        .map_err(|e| format!("reading {}: {e}", output.display()))?;
    let out_dir = output.parent().unwrap_or_else(|| Path::new("."));
    let mut figures = BTreeSet::new();
    collect_figures(out_dir, out_dir, &mut figures);
    let mut all_dirs: Vec<&Path> = dirs.to_vec();
    all_dirs.push(out_dir);
    Ok(Side {
        text: normalize_text(&raw, &all_dirs),
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
                applied: vec![Applied {
                    entry: "julia code block leading newline".to_string(),
                    rule: "julia-code-leading-newline".to_string(),
                    plain_fires: 1,
                    recorded_fires: 0,
                    needed: true,
                }]
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

    // ------------------------------------------------------------------ OPR-9d: normalizer, pin

    const DOC: &str = "julia/doc";
    const BUTTON: &str = "<button title=\"Copy to Clipboard\" class=\"code-copy-button\" aria-label=\"Copy code\"><i class=\"bi\"></i></button>";

    fn code_block(code: &str) -> String {
        format!(
            "<div class=\"code-copy-outer-scaffold\">\n<div class=\"sourceCode cell-code code-with-copy\"><pre class=\"sourceCode julia\"><code class=\"sourceCode julia\">{code}</code></pre></div>\n{BUTTON}\n</div>\n"
        )
    }

    fn stdout(text: &str) -> String {
        format!(
            "<div class=\"cell-output cell-output-stdout\">\n<div class=\"code-copy-outer-scaffold\">\n<pre class=\"code-with-copy\"><code>{text}</code></pre>\n{BUTTON}\n</div>\n</div>\n"
        )
    }

    fn cell(id: usize, inner: &str) -> String {
        format!("<div id=\"cell-{id}\" class=\"cell\">\n{inner}</div>\n")
    }

    fn page(body: &str) -> String {
        format!(
            "<main class=\"content\" id=\"quarto-document-content\" tabindex=\"-1\">\n<p>Prose.</p>\n{body}</main>\n"
        )
    }

    fn entry_toml(name: &str, rule: &str, target: &str, extra: &str) -> String {
        format!(
            "[[entry]]\nname='{name}'\nrule='{rule}'\n{target}\nstatus='proposed'\nreason='r'\n{extra}\n"
        )
    }

    /// The interleaving normalizer for `DOC` plus the signed leading-newline rule, as 9f writes them.
    fn interleave_list() -> Allowlist {
        Allowlist::parse(&format!(
            "{}{}",
            entry_toml(
                "interleaved",
                "julia-interleaved-cell",
                &format!("document='{DOC}'"),
                ""
            ),
            entry_toml(
                "leading newline",
                "julia-code-leading-newline",
                "scope='julia/'",
                ""
            ),
        ))
        .unwrap()
    }

    fn html_side(text: &str) -> Side {
        side(text, &[])
    }

    fn applied_of(v: &Verdict) -> &[Applied] {
        match v {
            Verdict::Allowlisted { applied } => applied,
            other => panic!("expected an allowlisted pair, got {other:?}"),
        }
    }

    fn blocker_diff(v: Verdict) -> String {
        match v {
            Verdict::Blocker { diff } => diff,
            other => panic!("expected a blocker, got {other:?}"),
        }
    }

    fn two_stdout_pages() -> (String, String) {
        let plain = page(&cell(
            1,
            &format!(
                "{}{}{}",
                code_block("println(\"a\")\nprintln(\"b\")"),
                stdout("a"),
                stdout("b")
            ),
        ));
        let recorded = page(&cell(
            1,
            &format!(
                "{}{}{}{}",
                code_block("println(\"a\")"),
                stdout("a"),
                code_block("println(\"b\")"),
                stdout("b")
            ),
        ));
        (plain, recorded)
    }

    #[test]
    fn the_interleaving_normalizer_explains_a_split_cell_and_reports_fires_per_side() {
        let (plain, recorded) = two_stdout_pages();
        let v = compare(
            DOC,
            &html_side(&plain),
            &html_side(&recorded),
            &interleave_list(),
        );
        // One later block was merged into the first: the recorded side fires once, plain never.
        // (The plan's text says 2; the 9a note measured 0/1 for a two-block cell, and the note wins.)
        let applied = applied_of(&v);
        assert_eq!(applied.len(), 1, "{applied:?}");
        assert_eq!(applied[0].rule, "julia-interleaved-cell");
        assert_eq!((applied[0].plain_fires, applied[0].recorded_fires), (0, 1));
        assert!(applied[0].needed);

        // Negative control: the two outputs merged into one block must still be reported.
        let merged = page(&cell(
            1,
            &format!(
                "{}{}{}",
                code_block("println(\"a\")"),
                stdout("a\nb"),
                code_block("println(\"b\")")
            ),
        ));
        blocker_diff(compare(
            DOC,
            &html_side(&plain),
            &html_side(&merged),
            &interleave_list(),
        ));
    }

    #[test]
    fn the_normalizer_is_scoped_by_exact_document_name() {
        let (plain, recorded) = two_stdout_pages();
        for name in ["julia/doc-tables", "julia/do", "other/doc"] {
            blocker_diff(compare(
                name,
                &html_side(&plain),
                &html_side(&recorded),
                &interleave_list(),
            ));
        }
    }

    #[test]
    fn trap_4_a_plain_side_fire_is_a_blocker() {
        let (_, recorded) = two_stdout_pages();
        // The plain page already has a two-block cell, and differs elsewhere.
        let plain = format!("{recorded}<p>extra</p>\n");
        let diff = blocker_diff(compare(
            DOC,
            &html_side(&plain),
            &html_side(&recorded),
            &interleave_list(),
        ));
        assert!(diff.contains("PLAIN"), "{diff}");
    }

    #[test]
    fn trap_5_a_reordering_alone_is_a_blocker_that_says_so() {
        let a = html_side("<p>x</p>\n<p>y</p>\n<p>z</p>");
        let b = html_side("<p>y</p>\n<p>x</p>\n<p>z</p>");
        let diff = blocker_diff(compare("julia/a", &a, &b, &Allowlist::default()));
        assert!(diff.contains("same lines, different order"), "{diff}");
        // The lines both sides share, reordered next to a real difference, are reported too.
        let c = html_side("<p>y</p>\n<p>x</p>\n<p>w</p>");
        let diff = blocker_diff(compare("julia/a", &a, &c, &Allowlist::default()));
        assert!(diff.contains("different order"), "{diff}");
    }

    #[test]
    fn trap_6_regex_rules_run_on_the_raw_text_before_the_normalizers() {
        // The presenter trims a block's leading blank line; the engine route keeps it. Here the
        // blank line is on a block that the normalizer merges: only a regex rule that ran first
        // makes the merged code equal.
        let plain = page(&cell(
            1,
            &format!("{}{}", code_block("l1\nl2"), stdout("o")),
        ));
        let recorded = page(&cell(
            1,
            &format!("{}{}{}", code_block("l1"), stdout("o"), code_block("\nl2")),
        ));
        let v = compare(
            DOC,
            &html_side(&plain),
            &html_side(&recorded),
            &interleave_list(),
        );
        let names: Vec<_> = applied_of(&v).iter().map(|a| a.rule.as_str()).collect();
        assert_eq!(
            names,
            ["julia-code-leading-newline", "julia-interleaved-cell"]
        );
    }

    #[test]
    fn trap_3_an_entry_that_changes_nothing_the_verdict_needs_is_marked_not_needed() {
        // Both sides carry the same leading blank line, so the rule fires but explains nothing.
        let plain = page(&cell(
            1,
            &format!("{}{}", code_block("\nl1\nl2"), stdout("o")),
        ));
        let recorded = page(&cell(
            1,
            &format!("{}{}{}", code_block("\nl1"), stdout("o"), code_block("l2")),
        ));
        let v = compare(
            DOC,
            &html_side(&plain),
            &html_side(&recorded),
            &interleave_list(),
        );
        let applied = applied_of(&v);
        let leading = applied
            .iter()
            .find(|a| a.rule == "julia-code-leading-newline");
        assert!(leading.is_some_and(|a| !a.needed), "{applied:?}");
        let inter = applied.iter().find(|a| a.rule == "julia-interleaved-cell");
        assert!(inter.is_some_and(|a| a.needed), "{applied:?}");
    }

    // ---- the real pages and the negative controls (fixtures: recorded and plain html of 9a's S3)

    fn fixture(name: &str, side: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/opr-parity-normalizer")
            .join(format!("{name}-{side}.html.txt"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    const REAL_PAGES: &[&str] = &["presentation-attrs", "records-plot", "fig-subcap"];

    fn real_list(name: &str) -> Allowlist {
        Allowlist::parse(&format!(
            "{}{}",
            entry_toml(
                "interleaved",
                "julia-interleaved-cell",
                &format!("document='julia/{name}'"),
                ""
            ),
            entry_toml(
                "leading newline",
                "julia-code-leading-newline",
                "scope='julia/'",
                ""
            ),
        ))
        .unwrap()
    }

    #[test]
    fn the_real_pages_are_equal_after_the_normalizer_with_plain_never_firing() {
        for name in REAL_PAGES {
            let v = compare(
                &format!("julia/{name}"),
                &html_side(&fixture(name, "plain")),
                &html_side(&fixture(name, "recorded")),
                &real_list(name),
            );
            let inter = applied_of(&v)
                .iter()
                .find(|a| a.rule == "julia-interleaved-cell")
                .unwrap_or_else(|| panic!("{name}: {v:?}"));
            assert_eq!(inter.plain_fires, 0, "{name}");
            assert!(inter.recorded_fires >= 1, "{name}");
        }
    }

    fn balanced_div_end(s: &str, start: usize) -> usize {
        let mut depth = 0i32;
        let mut i = start;
        loop {
            let open = s[i..].find("<div").map(|k| k + i);
            let close = s[i..].find("</div>").map(|k| k + i).expect("balanced");
            match open {
                Some(o) if o < close => {
                    depth += 1;
                    i = o + 4;
                }
                _ => {
                    depth -= 1;
                    i = close + 6;
                    if depth == 0 {
                        return i;
                    }
                }
            }
        }
    }

    fn output_blocks(s: &str) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut at = 0;
        while let Some(k) = s[at..].find("<div class=\"cell-output") {
            let start = at + k;
            let end = balanced_div_end(s, start);
            out.push((start, end));
            at = end;
        }
        out
    }

    fn drop_one_output(s: &str) -> Option<String> {
        let (a, b) = *output_blocks(s).first()?;
        let b = if s[b..].starts_with('\n') { b + 1 } else { b };
        Some(format!("{}{}", &s[..a], &s[b..]))
    }

    fn swap_two_outputs(s: &str) -> Option<String> {
        let blocks = output_blocks(s);
        let w = blocks
            .windows(2)
            .find(|w| s[w[0].0..w[0].1] != s[w[1].0..w[1].1])?;
        let (a, b) = (w[0], w[1]);
        Some(format!(
            "{}{}{}{}{}",
            &s[..a.0],
            &s[b.0..b.1],
            &s[a.1..b.0],
            &s[a.0..a.1],
            &s[b.1..]
        ))
    }

    fn change_code_in_a_split_off_block(s: &str) -> Option<String> {
        let marked = mark_pass(s).ok()?;
        let begin = marked.find(MARK_BEGIN)?;
        let mut stripped = marked[..begin].to_string();
        for m in [
            MARK_BEGIN,
            MARK_END,
            MARK_CELL,
            MARK_CELL_END,
            MARK_FIRST_CODE_END,
        ] {
            stripped = stripped.replace(m, "");
        }
        let at = stripped.len();
        let code = at
            + s[at..].find("<code class=\"sourceCode julia\">")?
            + "<code class=\"sourceCode julia\">".len();
        let mut in_tag = false;
        for (i, c) in s[code..].char_indices() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                c if !in_tag && c.is_ascii_alphabetic() => {
                    let k = code + i;
                    return Some(format!("{}Z{}", &s[..k], &s[k + 1..]));
                }
                _ => {}
            }
        }
        None
    }

    fn fold_two_cells(s: &str) -> Option<String> {
        let re = Regex::new(r#"</div>\n<div id="cell-\d+" class="cell"[^>]*>\n"#).unwrap();
        let m = re.find(s)?;
        Some(format!("{}{}", &s[..m.start()], &s[m.end()..]))
    }

    fn change_prose_outside_cells(s: &str) -> Option<String> {
        let p = s.find("<p>")?;
        if p > s.find("class=\"cell\"")? {
            return None;
        }
        let k = p + 3;
        Some(format!("{}Z{}", &s[..k], &s[k + 1..]))
    }

    fn swap_two_attributes(s: &str) -> Option<String> {
        let from = "<main class=\"content\" id=\"quarto-document-content\"";
        let to = "<main id=\"quarto-document-content\" class=\"content\"";
        s.contains(from).then(|| s.replace(from, to))
    }

    type Mutation = (&'static str, fn(&str) -> Option<String>);
    const CONTROLS: &[Mutation] = &[
        ("drop one output", drop_one_output),
        ("swap two outputs", swap_two_outputs),
        (
            "change one code line in a split-off block",
            change_code_in_a_split_off_block,
        ),
        ("fold two cells into one", fold_two_cells),
        ("change prose outside any cell", change_prose_outside_cells),
        ("swap two attributes (trap 1)", swap_two_attributes),
    ];

    #[test]
    fn every_negative_control_on_a_real_page_is_reported() {
        let mut applied_somewhere = vec![false; CONTROLS.len()];
        for name in REAL_PAGES {
            let plain = html_side(&fixture(name, "plain"));
            let recorded = fixture(name, "recorded");
            for (i, (label, mutate)) in CONTROLS.iter().enumerate() {
                let Some(mutated) = mutate(&recorded) else {
                    continue;
                };
                assert_ne!(mutated, recorded, "{name}: {label} changed nothing");
                applied_somewhere[i] = true;
                match compare(
                    &format!("julia/{name}"),
                    &plain,
                    &html_side(&mutated),
                    &real_list(name),
                ) {
                    Verdict::Blocker { .. } => {}
                    other => panic!("{name}: control `{label}` was not reported: {other:?}"),
                }
            }
        }
        for (i, (label, _)) in CONTROLS.iter().enumerate() {
            assert!(
                applied_somewhere[i],
                "control `{label}` found no site on any real page"
            );
        }
    }

    #[test]
    fn trap_1_an_attribute_order_swap_is_not_masked_by_the_normalizer() {
        let recorded = fixture("presentation-attrs", "recorded");
        let swapped = swap_two_attributes(&recorded).expect("the page has a main element");
        assert_ne!(swapped, recorded);
        let diff = blocker_diff(compare(
            "julia/presentation-attrs",
            &html_side(&fixture("presentation-attrs", "plain")),
            &html_side(&swapped),
            &real_list("presentation-attrs"),
        ));
        assert!(diff.contains("<main"), "{diff}");
    }

    #[test]
    fn a_page_with_a_shape_the_normalizer_does_not_know_is_a_blocker() {
        // A later block whose scaffold is not the writer's today: the splice must refuse.
        let odd = page(&cell(
            1,
            &format!(
                "{}{}<div class=\"code-copy-outer-scaffold\">\n<div class=\"sourceCode cell-code\"><pre><code class=\"sourceCode julia\">x</code></pre></div>\n</div>\n",
                code_block("a"),
                stdout("o")
            ),
        ));
        let diff = blocker_diff(compare(
            DOC,
            &html_side("<p>other</p>"),
            &html_side(&odd),
            &interleave_list(),
        ));
        assert!(diff.contains("julia-interleaved-cell failed"), "{diff}");
    }

    #[test]
    fn a_hidden_first_unit_with_output_is_the_known_gap_and_is_reported_not_hidden() {
        // Known gap (9d plan): a recorded cell whose first unit hides its code but has output has
        // no first block, so the output precedes the merged code. The pair then differs from the
        // plain page: a false blocker, never a hidden difference. Absent from the corpus.
        let plain = page(&cell(
            1,
            &format!("{}{}{}", code_block("a\nb"), stdout("o1"), stdout("o2")),
        ));
        let recorded = page(&cell(
            1,
            &format!("{}{}{}", stdout("o1"), code_block("b"), stdout("o2")),
        ));
        blocker_diff(compare(
            DOC,
            &html_side(&plain),
            &html_side(&recorded),
            &interleave_list(),
        ));
    }

    // ---- the pinned exact-difference entry (julia/capture-facts)

    const CAPTURE: &str = "julia/capture-facts";

    fn pin_fixture() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/opr-parity-normalizer/capture-facts.pin.txt");
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn pinned_list(pin_text: &str, document: &str) -> Allowlist {
        let toml = format!(
            "{}{}{}",
            entry_toml(
                "interleaved",
                "julia-interleaved-cell",
                &format!("document='{document}'"),
                ""
            ),
            entry_toml(
                "leading newline",
                "julia-code-leading-newline",
                "scope='julia/'",
                ""
            ),
            entry_toml(
                "capture-facts pin",
                PIN_RULE,
                &format!("document='{document}'"),
                "pin='capture-facts.pin.txt'"
            ),
        );
        let mut list = Allowlist::parse(&toml).unwrap();
        for e in &mut list.entries {
            if e.pin.is_some() {
                e.pin_text = Some(pin_text.to_string());
            }
        }
        list
    }

    fn capture_pair() -> (Side, Side) {
        (
            html_side(&fixture("capture-facts", "plain")),
            html_side(&fixture("capture-facts", "recorded")),
        )
    }

    #[test]
    fn the_pin_fixture_has_the_24_lines_s3_measured() {
        let lines = pin_fixture()
            .lines()
            .filter(|l| l.starts_with(['-', '+']))
            .count();
        assert_eq!(lines, 24, "{}", pin_fixture());
    }

    #[test]
    fn a_pinned_entry_with_the_exact_difference_allowlists_the_document() {
        let (plain, recorded) = capture_pair();
        // Without the pin the four kinds of difference are a blocker.
        let no_pin = pinned_list("", CAPTURE);
        let mut no_pin = no_pin;
        no_pin.entries.retain(|e| e.rule != PIN_RULE);
        blocker_diff(compare(CAPTURE, &plain, &recorded, &no_pin));
        let v = compare(
            CAPTURE,
            &plain,
            &recorded,
            &pinned_list(&pin_fixture(), CAPTURE),
        );
        let applied = applied_of(&v);
        assert!(
            applied.iter().any(|a| a.rule == PIN_RULE && a.needed),
            "{applied:?}"
        );
    }

    #[test]
    fn a_pin_with_one_changed_line_is_a_blocker_showing_both_texts() {
        let (plain, recorded) = capture_pair();
        let pin = pin_fixture();
        let first = pin.lines().find(|l| l.starts_with(['-', '+'])).unwrap();
        let changed = pin.replacen(first, &format!("{first} CHANGED"), 1);
        let diff = blocker_diff(compare(
            CAPTURE,
            &plain,
            &recorded,
            &pinned_list(&changed, CAPTURE),
        ));
        assert!(
            diff.contains("expected") && diff.contains("actual"),
            "{diff}"
        );
        assert!(diff.contains("CHANGED"), "{diff}");
        assert!(diff.contains(first), "{diff}");
    }

    #[test]
    fn a_pin_on_a_document_with_an_empty_difference_is_stale() {
        let name = "presentation-attrs";
        let v = compare(
            &format!("julia/{name}"),
            &html_side(&fixture(name, "plain")),
            &html_side(&fixture(name, "recorded")),
            &pinned_list(&pin_fixture(), &format!("julia/{name}")),
        );
        let pin = applied_of(&v).iter().find(|a| a.rule == PIN_RULE).unwrap();
        assert!(!pin.needed, "{pin:?}");
    }

    #[test]
    fn allowlist_parse_validates_document_and_pin() {
        let ok = entry_toml("n", "julia-interleaved-cell", "document='julia/x'", "");
        assert!(Allowlist::parse(&ok).is_ok());
        // `scope` and `document` are exclusive; one is required.
        let both = entry_toml(
            "n",
            "julia-interleaved-cell",
            "document='julia/x'\nscope='julia/'",
            "",
        );
        assert!(Allowlist::parse(&both).is_err());
        let neither = entry_toml("n", "julia-interleaved-cell", "", "");
        assert!(Allowlist::parse(&neither).is_err());
        // A normalizer is scoped by an exact document, never a prefix.
        let prefix = entry_toml("n", "julia-interleaved-cell", "scope='julia/'", "");
        assert!(Allowlist::parse(&prefix).is_err());
        // A pin is valid only with `document`, and only with the pin rule.
        let pin_scope = entry_toml("n", PIN_RULE, "scope='julia/'", "pin='p.txt'");
        assert!(Allowlist::parse(&pin_scope).is_err());
        let pin_ok = entry_toml("n", PIN_RULE, "document='julia/x'", "pin='p.txt'");
        assert!(Allowlist::parse(&pin_ok).is_ok());
        let pin_missing = entry_toml("n", PIN_RULE, "document='julia/x'", "");
        assert!(Allowlist::parse(&pin_missing).is_err());
        let pin_wrong_rule = entry_toml(
            "n",
            "julia-interleaved-cell",
            "document='julia/x'",
            "pin='p.txt'",
        );
        assert!(Allowlist::parse(&pin_wrong_rule).is_err());
    }
}
