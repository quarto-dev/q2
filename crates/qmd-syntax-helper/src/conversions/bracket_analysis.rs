// Shared bracket analysis for the `reference-links` and `literal-brackets`
// rules (bd-reference-links-unsupported-ddc4skac).
//
// q2 parses `[...]` as the bracket half of span syntax (`[text]{.class}`).
// Finding no attribute block it emits a bare `<span>` and discards the
// brackets, and link reference definitions are never collected. The same
// happens to `![...]`, which becomes an `<img>` with an empty `src`.
//
// Detection keys off AST *shape* rather than regex, which is what makes it
// safe:
//
//   - "these brackets will be eaten"  <=>  `Span` with an empty `Attr`
//   - "this image will break"         <=>  `Image` with an empty url
//   - `[label][ref]`                  <=>  two of the above whose byte
//                                          ranges touch exactly
//
// A genuine `[x]{.cls}` carries classes, an inline `[a](u)` parses as a
// `Link`, and brackets inside code never produce either node — so all three
// are excluded structurally, with no lookahead for `]{`, `](` or `][`.
//
// The one thing the AST cannot supply is the definition table, because q2
// never collects `[ref]: url` lines. Those are recognized by line shape and
// then cross-checked against the AST, so that a definition-shaped line
// inside a fenced code block is correctly *not* treated as a definition.

use anyhow::Result;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use pampa::filter_context::FilterContext;
use pampa::filters::{Filter, FilterReturn, topdown_traverse};
use pampa::pandoc::attr::is_empty_attr;
use regex::Regex;

/// Whether a bracketed group came from `[...]` or `![...]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    Span,
    Image,
}

/// One bracketed group in the source, e.g. `[label]` or `![alt]`.
#[derive(Debug, Clone)]
struct Part {
    kind: PartKind,
    /// Byte offset of the opening `[` — or of the `!` for an image.
    start: usize,
    /// Byte offset just past the closing `]`.
    end: usize,
    /// Source text between the brackets.
    inner: String,
}

/// A link reference definition line, e.g. `[ref]: https://e.com "Title"`.
#[derive(Debug, Clone)]
pub struct Definition {
    /// Normalized (case-folded, whitespace-collapsed) label.
    pub label: String,
    /// The url, with any `<...>` wrapper stripped.
    pub url: String,
    /// The title, with its quote delimiters stripped.
    pub title: Option<String>,
    /// Byte offset of the start of the definition's line.
    pub line_start: usize,
    /// Byte offset just past the line's trailing newline.
    pub line_end: usize,
}

/// Something the rules can act on.
#[derive(Debug, Clone)]
pub enum Finding {
    /// A reference-style use with a matching definition. Owned by
    /// `reference-links`, which rewrites it to the inline form.
    Reference {
        start: usize,
        end: usize,
        kind: PartKind,
        /// Source text of the label half — `the docs` in `[the docs][gcc]`.
        label: String,
        /// Normalized label of the definition this resolves to.
        definition_label: String,
    },
    /// A bracketed group with no matching definition. Owned by
    /// `literal-brackets`, which escapes it so the brackets survive.
    Literal {
        start: usize,
        end: usize,
        kind: PartKind,
    },
    /// Three or more adjacent bracketed groups, e.g. `[a][b][c]`. Genuinely
    /// ambiguous — `[a][b]` + literal `[c]`, or `[a]` + `[b][c]`? Both rules
    /// decline to touch these and report them for human review.
    Ambiguous {
        start: usize,
        end: usize,
        count: usize,
        /// Normalized labels the run *might* resolve against. A definition
        /// named here is kept, so the human reviewing the run still has it.
        labels: Vec<String>,
    },
}

impl Finding {
    pub fn start(&self) -> usize {
        match self {
            Finding::Reference { start, .. }
            | Finding::Literal { start, .. }
            | Finding::Ambiguous { start, .. } => *start,
        }
    }

    pub fn end(&self) -> usize {
        match self {
            Finding::Reference { end, .. }
            | Finding::Literal { end, .. }
            | Finding::Ambiguous { end, .. } => *end,
        }
    }
}

/// The result of analyzing one file.
#[derive(Debug, Default)]
pub struct Analysis {
    pub definitions: Vec<Definition>,
    /// In ascending source order.
    pub findings: Vec<Finding>,
    used: HashSet<String>,
}

impl Analysis {
    /// Whether any resolvable reference uses this definition.
    pub fn is_used(&self, label: &str) -> bool {
        self.used.contains(&normalize_label(label))
    }

    /// Look up a definition by (un-normalized) label.
    pub fn definition(&self, label: &str) -> Option<&Definition> {
        let key = normalize_label(label);
        self.definitions.iter().find(|d| d.label == key)
    }
}

/// CommonMark reference labels are case-folded and whitespace-collapsed.
pub fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Analyze `source`, returning every definition and finding.
///
/// A file that does not parse yields an empty `Analysis`, so the rules leave
/// it alone rather than editing a document whose structure we cannot trust.
/// Parse errors are the `parse` rule's business, not ours.
pub fn analyze(source: &str, filename: &str) -> Result<Analysis> {
    let mut sink = std::io::sink();
    let parsed =
        pampa::readers::qmd::read(source.as_bytes(), false, filename, &mut sink, true, None);

    let (doc, _ctx, _diags) = match parsed {
        Ok(triple) => triple,
        Err(_) => return Ok(Analysis::default()),
    };

    let parts = collect_parts(doc, source);
    let (definitions, parts) = extract_definitions(source, parts);
    let findings = classify(&parts, &definitions);

    let mut used = HashSet::new();
    for finding in &findings {
        if let Finding::Reference {
            definition_label, ..
        } = finding
        {
            used.insert(definition_label.clone());
        }
    }

    Ok(Analysis {
        definitions,
        findings,
        used,
    })
}

/// Walk the AST collecting every bare span and empty-url image.
fn collect_parts(doc: pampa::pandoc::Pandoc, source: &str) -> Vec<Part> {
    let collected: Rc<RefCell<Vec<Part>>> = Rc::new(RefCell::new(Vec::new()));

    let spans = Rc::clone(&collected);
    let images = Rc::clone(&collected);

    let mut filter = Filter::new()
        .with_span(move |span, _ctx| {
            if is_empty_attr(&span.attr)
                && let Some((start, end)) = byte_range(&span.source_info)
                && let Some(inner) = inner_text(source, start, end, PartKind::Span)
            {
                spans.borrow_mut().push(Part {
                    kind: PartKind::Span,
                    start,
                    end,
                    inner,
                });
            }
            FilterReturn::Unchanged(span)
        })
        .with_image(move |image, _ctx| {
            if image.target.0.is_empty()
                && let Some((start, end)) = byte_range(&image.source_info)
                && let Some(inner) = inner_text(source, start, end, PartKind::Image)
            {
                images.borrow_mut().push(Part {
                    kind: PartKind::Image,
                    start,
                    end,
                    inner,
                });
            }
            FilterReturn::Unchanged(image)
        });

    let mut ctx = FilterContext::new();
    topdown_traverse(doc, &mut filter, &mut ctx);
    // The filter owns the two closures, and they own the other `Rc` handles.
    drop(filter);

    let mut parts = Rc::try_unwrap(collected)
        .expect("the only other handles lived in the filter closures")
        .into_inner();
    parts.sort_by_key(|p| (p.start, std::cmp::Reverse(p.end)));

    // Drop parts nested inside another part — `[a [b] c]` yields both the
    // outer and inner span, and only the outermost is a candidate.
    let mut outermost: Vec<Part> = Vec::with_capacity(parts.len());
    for part in parts {
        if outermost.last().is_some_and(|prev| part.end <= prev.end) {
            continue;
        }
        outermost.push(part);
    }
    outermost
}

/// Resolve a `SourceInfo` to a byte range in the original file.
fn byte_range(info: &quarto_source_map::SourceInfo) -> Option<(usize, usize)> {
    let (_file_id, start, end) = info.resolve_byte_range()?;
    (start < end).then_some((start, end))
}

/// The text between the brackets, given the group's full byte range.
fn inner_text(source: &str, start: usize, end: usize, kind: PartKind) -> Option<String> {
    let open = match kind {
        // `[label]` -> skip `[`
        PartKind::Span => start + 1,
        // `![alt]` -> skip `![`
        PartKind::Image => start + 2,
    };
    let close = end.checked_sub(1)?;
    if open > close {
        return None;
    }
    source.get(open..close).map(str::to_string)
}

/// Recognize link reference definition lines, and remove the parts they
/// consume so those brackets are never reported as literal.
fn extract_definitions(source: &str, parts: Vec<Part>) -> (Vec<Definition>, Vec<Part>) {
    // A definition line: up to three spaces of indent, `[label]:`, a
    // destination, and an optional title. Anything else on the line
    // disqualifies it.
    let re = Regex::new(r"(?m)^(?P<indent>[ ]{0,3})\[(?P<label>[^\]]*)\]:[ \t]*(?P<rest>[^\n]*)$")
        .expect("definition regex is valid");

    let by_start: HashMap<usize, &Part> = parts.iter().map(|p| (p.start, p)).collect();

    let mut definitions = Vec::new();
    let mut consumed: HashSet<usize> = HashSet::new();

    for caps in re.captures_iter(source) {
        let whole = caps.get(0).expect("group 0 always matches");
        let label_start = whole.start() + caps["indent"].len();

        // Cross-check against the AST: without a bare span starting exactly
        // at the `[`, this line is not really a definition — it is inside a
        // code block, or otherwise not parsed as brackets at all.
        match by_start.get(&label_start) {
            Some(part) if part.kind == PartKind::Span => {}
            _ => continue,
        }

        let Some((url, title)) = parse_destination(&caps["rest"]) else {
            continue;
        };

        // Swallow the line's own newline, so deleting the definition does
        // not leave an empty line behind.
        let line_end = if source[whole.end()..].starts_with('\n') {
            whole.end() + 1
        } else {
            whole.end()
        };

        consumed.insert(label_start);
        definitions.push(Definition {
            label: normalize_label(&caps["label"]),
            url,
            title,
            line_start: whole.start(),
            line_end,
        });
    }

    let remaining = parts
        .into_iter()
        .filter(|p| !consumed.contains(&p.start))
        .collect();

    (definitions, remaining)
}

/// Split a definition's tail into `(url, title)`.
///
/// Pandoc peels a trailing quoted title off first and treats *everything*
/// left over as the destination — including spaces, which it percent-encodes
/// on output. So `[r]: https://e.com/a b.png` is a definition whose url is
/// `https://e.com/a b.png`, not a malformed line. Verified against
/// `quarto pandoc` in all four combinations of space-in-url and title; see
/// the plan's investigation notes.
///
/// Returns `None` when the tail has no destination at all, which
/// disqualifies the line from being a definition.
fn parse_destination(rest: &str) -> Option<(String, Option<String>)> {
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }

    // `<...>` is a definition-side wrapper with no inline-form equivalent,
    // so it is stripped here.
    if let Some(inner) = rest.strip_prefix('<') {
        let close = inner.find('>')?;
        let url = inner[..close].to_string();
        if url.is_empty() {
            return None;
        }
        let tail = inner[close + 1..].trim();
        let title = if tail.is_empty() {
            None
        } else {
            Some(unquote_title(tail)?)
        };
        return Some((url, title));
    }

    let (url, title) = match split_trailing_title(rest) {
        Some((dest, title)) => (dest.to_string(), Some(title)),
        None => (rest.to_string(), None),
    };

    if url.is_empty() {
        return None;
    }
    Some((url, title))
}

/// Peel a trailing quoted title off a definition tail.
///
/// The title must be preceded by whitespace, so a url that merely *ends*
/// with a paren — `https://e.com/a(b)` — is not mistaken for one.
fn split_trailing_title(rest: &str) -> Option<(&str, String)> {
    let close = rest.chars().last()?;
    let open = match close {
        '"' => '"',
        '\'' => '\'',
        ')' => '(',
        _ => return None,
    };

    // Search backwards for an opening delimiter that leaves a well-formed
    // title and a non-empty destination behind it.
    let mut candidate = rest.len() - close.len_utf8();
    while let Some(idx) = rest[..candidate].rfind(open) {
        candidate = idx;
        let before = &rest[..idx];
        if before.ends_with(char::is_whitespace)
            && !before.trim().is_empty()
            && let Some(title) = unquote_title(&rest[idx..])
        {
            return Some((before.trim_end(), title));
        }
        if idx == 0 {
            break;
        }
    }
    None
}

/// Strip a title's delimiters, undoing backslash escapes of the delimiter.
fn unquote_title(text: &str) -> Option<String> {
    let (open, close) = match text.chars().next()? {
        '"' => ('"', '"'),
        '\'' => ('\'', '\''),
        '(' => ('(', ')'),
        _ => return None,
    };

    let body = text.strip_prefix(open)?.strip_suffix(close)?;
    // An unescaped delimiter inside the body means we picked the wrong
    // opening delimiter.
    if body.replace(&format!("\\{close}"), "").contains(close) {
        return None;
    }
    Some(body.replace(&format!("\\{close}"), &close.to_string()))
}

/// Group parts into runs of adjacent brackets and classify each run.
fn classify(parts: &[Part], definitions: &[Definition]) -> Vec<Finding> {
    let defined: HashSet<&str> = definitions.iter().map(|d| d.label.as_str()).collect();
    let mut findings = Vec::new();

    for run in runs(parts) {
        match run {
            // `[ref]` / `![ref]` — shortcut reference.
            [only] => {
                let label = normalize_label(&only.inner);
                if defined.contains(label.as_str()) {
                    findings.push(Finding::Reference {
                        start: only.start,
                        end: only.end,
                        kind: only.kind,
                        label: only.inner.clone(),
                        definition_label: label,
                    });
                } else {
                    findings.push(Finding::Literal {
                        start: only.start,
                        end: only.end,
                        kind: only.kind,
                    });
                }
            }
            // `[label][ref]` (full) or `[label][]` (collapsed).
            [first, second] => {
                // An empty second half makes this collapsed, so the label
                // half doubles as the reference label.
                let label = if second.inner.trim().is_empty() {
                    normalize_label(&first.inner)
                } else {
                    normalize_label(&second.inner)
                };

                if defined.contains(label.as_str()) {
                    findings.push(Finding::Reference {
                        start: first.start,
                        end: second.end,
                        kind: first.kind,
                        label: first.inner.clone(),
                        definition_label: label,
                    });
                } else {
                    // Undefined `[a][b]` is literal text in CommonMark, so
                    // both halves need escaping.
                    for part in [first, second] {
                        findings.push(Finding::Literal {
                            start: part.start,
                            end: part.end,
                            kind: part.kind,
                        });
                    }
                }
            }
            // Three or more: decline, and report.
            longer => findings.push(Finding::Ambiguous {
                start: longer[0].start,
                end: longer[longer.len() - 1].end,
                count: longer.len(),
                labels: longer.iter().map(|p| normalize_label(&p.inner)).collect(),
            }),
        }
    }

    findings.sort_by_key(Finding::start);
    findings
}

/// Split parts into maximal runs of touching brackets.
///
/// An image always starts a new run: `![alt]` can only ever be the *label*
/// half of a reference, never the `[ref]` half, so `[a]![b]` is two runs.
fn runs(parts: &[Part]) -> Vec<&[Part]> {
    let mut runs = Vec::new();
    let mut start = 0;

    for i in 1..parts.len() {
        let breaks = parts[i].start != parts[i - 1].end || parts[i].kind == PartKind::Image;
        if breaks {
            runs.push(&parts[start..i]);
            start = i;
        }
    }

    if start < parts.len() {
        runs.push(&parts[start..]);
    }
    runs
}
