/*
 * engine/content_processors/ipynb.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `ipynb` content processor (Plan 7c).
 */

//! `ipynb` content processor: Jupyter notebook → qmd conversion with
//! per-cell source mapping (Plan 7c).
//!
//! Each cell's **logical** (JSON-unescaped) text becomes its own ephemeral
//! virtual file; the converter assembles the qmd as a
//! `SourceInfo::concat` of `Original` pieces over those files (verbatim
//! cell content) and `Generated` pieces anchored to cells (fences,
//! separators, synthesized front matter). Unescaping happens once, at
//! ingestion, before source tracking begins — no non-affine map is ever
//! represented.
//!
//! Q1-port semantics: `fixupFrontMatter`
//! (`external-sources/quarto-cli/src/core/jupyter/jupyter-fixups.ts`),
//! `mdFromRawCell` (`jupyter.ts`), and `markdownWithExtractedHeading`
//! (`pandoc-partition.ts`). Deliberate Q1 deviation: cell content is
//! emitted as original bytes (contiguous ranges of the cell's logical
//! text), never Q1's `nbLines`-normalized rewrite — rendered output is
//! identical modulo trailing newlines.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use quarto_source_map::{Anchor, AnchorRole, By, FileId, SourceInfo};
use regex::Regex;
use smallvec::smallvec;

use super::{
    ContentProcessor, Converted, ORIGINAL_FILE_ID, ProcessorContext, ProcessorError,
    ProcessorParams,
};

/// `By.kind` for every piece this converter synthesizes (plan decision 5).
const GENERATED_BY_KIND: &str = "ipynb/scaffold";

pub struct Ipynb;

impl ContentProcessor for Ipynb {
    fn sniff(&self, _path: &Path, content: &str, _params: &ProcessorParams) -> bool {
        has_cells_array(content)
    }

    fn convert(
        &self,
        path: &Path,
        content: &str,
        _params: &ProcessorParams,
        _ctx: &ProcessorContext,
    ) -> Result<Converted, ProcessorError> {
        convert_notebook(path, content)
    }
}

/// Direct conversion entry point. Production code reaches this through
/// `super::convert` (registry dispatch on `ProcessorSpec::Ipynb`); the
/// flagship integration harness calls it directly.
pub fn convert_notebook(path: &Path, content: &str) -> Result<Converted, ProcessorError> {
    let name = path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    );

    let nb: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| ProcessorError::Other(format!("{name}: invalid notebook JSON: {e}")))?;
    let cells_json = nb
        .get("cells")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ProcessorError::Other(format!("{name}: missing \"cells\" array")))?;

    let mut cells = Vec::with_capacity(cells_json.len());
    for (i, cell) in cells_json.iter().enumerate() {
        let kind = cell
            .get("cell_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProcessorError::Other(format!("{name}: cell {i} has no \"cell_type\""))
            })?;
        if !matches!(kind, "markdown" | "raw" | "code") {
            return Err(ProcessorError::Other(format!(
                "{name}: cell {i} has unknown cell_type '{kind}'"
            )));
        }
        let source = cell_source(cell).ok_or_else(|| {
            ProcessorError::Other(format!("{name}: cell {i} has no readable \"source\""))
        })?;
        let has_outputs = cell
            .get("outputs")
            .and_then(|v| v.as_array())
            .is_some_and(|o| !o.is_empty());
        let raw_mimetype = cell
            .get("metadata")
            .and_then(|m| m.get("raw_mimetype"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        cells.push(CellData {
            kind: kind.to_string(),
            text: source.text,
            source_is_empty_array: source.is_empty_array,
            has_outputs,
            raw_mimetype,
        });
    }

    // fixupFrontMatter (Q1 jupyter-fixups.ts): find the first raw/markdown
    // cell whose text partitions as YAML front matter. Code cells and
    // non-partitioning raw/markdown cells don't stop that scan (findIndex
    // semantics).
    let front_matter: Option<FrontMatter> = cells.iter().enumerate().find_map(|(i, cell)| {
        if cell.kind != "markdown" && cell.kind != "raw" {
            return None;
        }
        partition_yaml_front_matter(&cell.text).map(|(yaml_inner, remainder_start)| FrontMatter {
            cell_idx: i,
            yaml_inner,
            remainder_start,
        })
    });

    // Q1 parses the partition's yaml via readYamlFromMarkdown, which throws
    // on malformed YAML. 7c deviation (plan § Execution decisions 4): an
    // unparseable or non-mapping front-matter cell is left verbatim and
    // disables the title scan — snipping a heading with no place to inject
    // the title would lose it (Q1's JS mutation silently drops it instead).
    let fm_yaml: Option<serde_yaml::Value> = front_matter
        .as_ref()
        .and_then(|fm| serde_yaml::from_str::<serde_yaml::Value>(&fm.yaml_inner).ok());

    let fm_has_truthy_title = fm_yaml
        .as_ref()
        .and_then(|y| y.as_mapping())
        .and_then(|m| m.get(serde_yaml::Value::from("title")))
        .is_some_and(yaml_truthy);

    // Q1: `if (yaml?.title) return nb` — done when the front matter itself
    // carries a title. Otherwise scan, but only when the front matter is a
    // mapping (or absent).
    let scan_active = match (&front_matter, fm_yaml.as_ref()) {
        (None, _) => true,
        (Some(_), Some(y)) => y.as_mapping().is_some() && !fm_has_truthy_title,
        (Some(_), None) => false,
    };

    // Title scan: markdown cells only (the front-matter cell is now raw,
    // code/raw cells are transparent); the first markdown cell decides —
    // heading-only snips it, anything else stops the scan.
    let mut snip: Option<Snip> = None;
    if scan_active {
        for (i, cell) in cells.iter().enumerate() {
            if cell.kind != "markdown" || front_matter.as_ref().is_some_and(|fm| fm.cell_idx == i) {
                continue;
            }
            if let Some((title, remainder_start)) = extract_heading(&cell.text) {
                snip = Some(Snip {
                    cell_idx: i,
                    remainder_start,
                    title,
                });
                break;
            }
            break;
        }
    }

    // Build the (optional) front-matter action from the snip.
    let (inject, synthesize): (Option<(usize, String)>, Option<(usize, String)>) =
        match (&front_matter, fm_yaml.as_ref(), &snip) {
            (Some(fm), Some(y), Some(s)) if y.as_mapping().is_some() => {
                let mut map = y.as_mapping().expect("checked above").clone();
                map.insert(
                    serde_yaml::Value::from("title"),
                    serde_yaml::Value::from(s.title.clone()),
                );
                let yaml_text =
                    serde_yaml::to_string(&serde_yaml::Value::Mapping(map)).map_err(|e| {
                        ProcessorError::Other(format!("{name}: cannot serialize front matter: {e}"))
                    })?;
                // Q1 rewrites the cell to `---\n${yamlText}---\n\n${remainder}`:
                // the blank line is baked into the injection, the remainder
                // (original bytes after the closer line) follows verbatim.
                let injection = format!("---\n{yaml_text}---\n\n");
                (Some((fm.cell_idx, injection)), None)
            }
            (None, _, Some(s)) => {
                let mut map = serde_yaml::Mapping::new();
                map.insert(
                    serde_yaml::Value::from("title"),
                    serde_yaml::Value::from(s.title.clone()),
                );
                let yaml_text =
                    serde_yaml::to_string(&serde_yaml::Value::Mapping(map)).map_err(|e| {
                        ProcessorError::Other(format!("{name}: cannot serialize title: {e}"))
                    })?;
                // Q1 unshifts a raw cell `---\n${yamlText}---\n` at position
                // 0 — no baked blank line; the usual cell separator follows.
                let injection = format!("---\n{yaml_text}---\n");
                (None, Some((s.cell_idx, injection)))
            }
            _ => (None, None),
        };

    let number_shift = usize::from(synthesize.is_some());
    let cell_file_id = |i: usize| FileId(ORIGINAL_FILE_ID.0 + 1 + i);
    // Q1 conversion path: fence language is the kernelspec language,
    // lowercased; a missing one yields an empty `{}` language.
    let language = nb
        .get("metadata")
        .and_then(|m| m.get("kernelspec"))
        .and_then(|k| k.get("language"))
        .and_then(|v| v.as_str())
        .map_or(String::new(), |l| l.to_lowercase());

    // Emission: optional synthesized front-matter pseudo-cell (numbered 1;
    // anchored to the heading cell — it has no file of its own), then the
    // real cells. Separators are lazy `Generated("\n\n")` anchored to the
    // cell they precede; redacted cells emit nothing (their file entry
    // remains).
    let mut md = Assembler::default();
    let mut emitted = false;

    if let Some((heading_idx, injection)) = &synthesize {
        let anchor = invocation_anchor(cell_file_id(*heading_idx), &cells[*heading_idx].text);
        md.push(generated(anchor), injection);
        emitted = true;
    }

    for (i, cell) in cells.iter().enumerate() {
        // Q1 redaction (mdFromContentCell): an empty-source, no-output code
        // cell contributes nothing.
        if cell.kind == "code" && cell.source_is_empty_array && !cell.has_outputs {
            continue;
        }
        let fid = cell_file_id(i);
        if emitted {
            let anchor = invocation_anchor(fid, &cell.text);
            md.push(generated(anchor), "\n\n");
        }
        if let Some((fm_idx, injection)) = &inject
            && *fm_idx == i
        {
            let anchor = invocation_anchor(fid, &cell.text);
            md.push(generated(anchor), injection);
            let start = front_matter.as_ref().map_or(0, |fm| fm.remainder_start);
            if start < cell.text.len() {
                md.push(
                    SourceInfo::original(fid, start, cell.text.len()),
                    &cell.text[start..],
                );
            }
            emitted = true;
            continue;
        }
        let (before, after) = cell_wrap(cell, &language);
        if let Some(before) = &before {
            let anchor = invocation_anchor(fid, &cell.text);
            md.push(generated(anchor), before);
        }
        let start = if snip.as_ref().is_some_and(|s| s.cell_idx == i) {
            snip.as_ref().map_or(0, |s| s.remainder_start)
        } else {
            0
        };
        if start < cell.text.len() {
            md.push(
                SourceInfo::original(fid, start, cell.text.len()),
                &cell.text[start..],
            );
        }
        if let Some(after) = &after {
            let anchor = invocation_anchor(fid, &cell.text);
            md.push(generated(anchor), after);
        }
        // Fenced cells always emit their fences; plain cells emit content.
        emitted = true;
    }

    let files = cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            (
                format!("{name}[cell {}, {}]", i + 1 + number_shift, cell.kind),
                cell.text.clone(),
            )
        })
        .collect();

    Ok(Converted {
        markdown: md.markdown,
        source_info: SourceInfo::concat(md.pieces),
        files,
    })
}

/// Admission sniff (Plan 7c Phase 2): the content parses as JSON and has a
/// top-level `cells` array. The claim is already extension-scoped, so this
/// is a content sanity check — a non-notebook `.ipynb` fails it and the
/// stage's "Can't determine execution engine" path surfaces the problem,
/// mirroring `non_matching_percent_py_hard_errors`.
fn has_cells_array(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .is_ok_and(|v| v.get("cells").is_some_and(|c| c.is_array()))
}

struct CellData {
    kind: String,
    text: String,
    source_is_empty_array: bool,
    has_outputs: bool,
    raw_mimetype: Option<String>,
}

struct CellSource {
    text: String,
    is_empty_array: bool,
}

/// Q1 `jupyterCellSrcAsStr`: `source` is a single string or an array of
/// lines joined without separators. The empty-array flag survives because
/// Q1's code-cell redaction keys off the *array length*, so a string-form
/// source is never redacted.
fn cell_source(cell: &serde_json::Value) -> Option<CellSource> {
    match cell.get("source") {
        Some(serde_json::Value::Array(items)) => {
            let mut text = String::new();
            for item in items {
                text.push_str(item.as_str()?);
            }
            Some(CellSource {
                text,
                is_empty_array: items.is_empty(),
            })
        }
        Some(serde_json::Value::String(s)) => Some(CellSource {
            text: s.clone(),
            is_empty_array: false,
        }),
        _ => None,
    }
}

struct FrontMatter {
    cell_idx: usize,
    yaml_inner: String,
    remainder_start: usize,
}

struct Snip {
    cell_idx: usize,
    remainder_start: usize,
    title: String,
}

struct Line<'a> {
    /// Line content without its `\r?\n` terminator — for matching only.
    text: &'a str,
    /// Byte offset just past the line's newline (in the original text).
    end: usize,
}

fn lines_with_offsets(text: &str, from: usize) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut offset = from;
    for seg in text[from..].split_inclusive('\n') {
        offset += seg.len();
        let stripped = seg.strip_suffix('\n').unwrap_or(seg);
        let stripped = stripped.strip_suffix('\r').unwrap_or(stripped);
        lines.push(Line {
            text: stripped,
            end: offset,
        });
    }
    lines
}

fn is_yaml_begin(line: &str) -> bool {
    line.strip_prefix("---")
        .is_some_and(|rest| rest.chars().all(|c| c == ' ' || c == '\t'))
}

/// Q1 `kRegExEndYAML`: `/^(?:---|\.\.\.)([ \t]*)$/`.
fn is_yaml_end(line: &str) -> bool {
    line.strip_prefix("---")
        .or_else(|| line.strip_prefix("..."))
        .is_some_and(|rest| rest.chars().all(|c| c == ' ' || c == '\t'))
}

/// Port of Q1 `partitionYamlFrontMatter` (yaml.ts), reduced to what the
/// converter needs: the YAML text between the delimiters and the byte
/// offset where the original text resumes (just past the closer line).
fn partition_yaml_front_matter(text: &str) -> Option<(String, usize)> {
    let lead = text.len() - text.trim_start().len();
    let lines = lines_with_offsets(text, lead);
    if lines.len() < 3 || !is_yaml_begin(lines[0].text) {
        return None;
    }
    if lines[1].text.trim().is_empty() || is_yaml_end(lines[1].text) {
        return None;
    }
    let closer = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, line)| is_yaml_end(line.text))?;
    let yaml_inner = lines[1..closer.0]
        .iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n");
    Some((yaml_inner, lines[closer.0].end))
}

/// JS truthiness for a parsed YAML value (Q1's `yaml?.title`).
fn yaml_truthy(value: &serde_yaml::Value) -> bool {
    match value {
        serde_yaml::Value::Null => false,
        serde_yaml::Value::Bool(b) => *b,
        serde_yaml::Value::Number(n) => n.as_f64().is_none_or(|f| f != 0.0),
        serde_yaml::Value::String(s) => !s.is_empty(),
        serde_yaml::Value::Sequence(_)
        | serde_yaml::Value::Mapping(_)
        | serde_yaml::Value::Tagged(_) => true,
    }
}

/// Port of Q1 `parsePandocTitle` (pandoc-partition.ts). The attr form
/// `# text {attrs}` yields its text — `None` when the attr form carries no
/// text (falsy title). Otherwise the heading marks are stripped;
/// `Some("")` is a falsy title.
fn parse_pandoc_title(line: &str) -> Option<String> {
    static RE_TITLE_ATTR: OnceLock<Regex> = OnceLock::new();
    let re_attr = RE_TITLE_ATTR
        .get_or_init(|| Regex::new(r"^#{1,}\s(?:(.*)\s)?\{(.*)\}$").expect("valid regex"));
    let trimmed = line.trim();
    if let Some(caps) = re_attr.captures(trimmed) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if hashes == 0 {
        return Some(trimmed.to_string());
    }
    Some(trimmed[hashes..].trim_start().trim().to_string())
}

/// Q1 `kRegExATXHeading`: `/^\#{1,}\s/` — no leading-space allowance.
fn is_atx_heading(line: &str) -> bool {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    hashes >= 1 && line[hashes..].starts_with(char::is_whitespace)
}

/// Q1 setext underline: `/^=+\s*$/` or `/^-+\s*$/`.
fn is_setext_underline(line: &str) -> bool {
    let core = line.trim_end_matches(char::is_whitespace);
    let first = match core.chars().next() {
        Some(c) => c,
        None => return false,
    };
    (first == '=' || first == '-') && core.chars().all(|c| c == first)
}

/// Q1 `kFenceOpenRegex`: `/^ {0,3}(`{3,}|~{3,})/` — info string may follow.
fn fence_open(line: &str) -> Option<(char, usize)> {
    let body = line.trim_start_matches(' ');
    if line.len() - body.len() > 3 {
        return None;
    }
    fence_run(body)
}

/// Q1 `kFenceCloseRegex`: the fence line carries nothing but the run.
fn fence_close(line: &str) -> Option<(char, usize)> {
    let body = line.trim_start_matches(' ');
    if line.len() - body.len() > 3 {
        return None;
    }
    let (ch, len) = fence_run(body)?;
    body[len..].trim().is_empty().then_some((ch, len))
}

fn fence_run(body: &str) -> Option<(char, usize)> {
    let first = body.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let run = body.chars().take_while(|&c| c == first).count();
    (run >= 3).then_some((first, run))
}

/// Port of Q1 `markdownWithExtractedHeading`, reduced to the snip
/// decision: `Some((title, remainder_start))` iff the first truthy heading
/// is heading-only (`contentBeforeHeading` false). A falsy title (attr
/// form without text, or empty text) consumes its line and keeps scanning,
/// exactly as Q1's `headingText` staying falsy does.
fn extract_heading(text: &str) -> Option<(String, usize)> {
    let mut md_lines: Vec<&str> = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for line in lines_with_offsets(text, 0) {
        if let Some((open_char, open_len)) = fence {
            md_lines.push(line.text);
            if let Some((close_char, close_len)) = fence_close(line.text)
                && close_char == open_char
                && close_len >= open_len
            {
                fence = None;
            }
            continue;
        }
        if let Some((open_char, open_len)) = fence_open(line.text) {
            fence = Some((open_char, open_len));
            md_lines.push(line.text);
            continue;
        }
        if is_atx_heading(line.text) {
            if let Some(title) = parse_pandoc_title(line.text) {
                let content_before = !md_lines.is_empty();
                if !title.is_empty() && !content_before {
                    return Some((title, line.end));
                }
            }
            continue;
        }
        if is_setext_underline(line.text)
            && let Some(&prev) = md_lines.last()
        {
            md_lines.pop();
            let content_before = !md_lines.is_empty();
            if !prev.is_empty() && !content_before {
                return Some((prev.to_string(), line.end));
            }
            // Q1: a truthy heading with content before it, or a falsy
            // (empty) prev line, ends the snip attempt either way.
            return None;
        }
        md_lines.push(line.text);
    }
    None
}

/// `By::raw("ipynb/scaffold", null)` with the given invocation anchor.
fn generated(anchor: Anchor) -> SourceInfo {
    SourceInfo::generated_with(
        By {
            kind: GENERATED_BY_KIND.to_string(),
            data: serde_json::Value::Null,
        },
        smallvec![anchor],
    )
}

/// (before, after) `Generated` fence texts around a cell's original bytes,
/// anchored to the cell. Port of the Q1 conversion path
/// (`command/convert/jupyter.ts::mdFromCodeCell`) and the Q1 raw-cell
/// wrappers (`core/jupyter/jupyter.ts::mdRawOutput`/`mdFormatOutput`/
/// `mdScriptOutput`). Q1's content rewrites (mdEnsureTrailingNewline
/// padding, `#|` options re-emission) are deliberate deviations — cell
/// content is always emitted as original bytes.
fn cell_wrap(cell: &CellData, language: &str) -> (Option<String>, Option<String>) {
    // nbformat strips the source's trailing newline, so the closing
    // fence must supply the line break itself when it's absent — a
    // glued `code```` run is unparseable markdown and downstream
    // consumers (stored-output replay, editors) cannot match the cell.
    let fence_close = |text: &str, ticks: &str| {
        if text.ends_with('\n') {
            format!("{ticks}\n")
        } else {
            format!("\n{ticks}\n")
        }
    };
    match cell.kind.as_str() {
        "code" => {
            let ticks = "`".repeat(code_fence_ticks(&cell.text));
            (
                Some(format!("{ticks}{{{language}}}\n")),
                Some(fence_close(&cell.text, &ticks)),
            )
        }
        "raw" => match cell.raw_mimetype.as_deref() {
            Some("text/html") => format_output("html", &cell.text),
            Some("text/latex") => format_output("tex", &cell.text),
            Some("text/restructuredtext") => format_output("rst", &cell.text),
            Some("application/rtf") => format_output("rtf", &cell.text),
            // mdScriptOutput: the script tag's prefix/suffix are Generated;
            // the script body stays an Original range of the cell.
            Some("application/javascript") => (
                Some(format!(
                    "```{{=html}}\n<script type=\"{}\">\n",
                    cell.raw_mimetype.as_deref().expect("matched")
                )),
                Some("\n</script>\n```\n".to_string()),
            ),
            _ => (None, None),
        },
        _ => (None, None),
    }
}

/// Q1 `mdFormatOutput`: `ticks{=format}` … `ticks` around the content.
fn format_output(format: &str, text: &str) -> (Option<String>, Option<String>) {
    let ticks = "`".repeat(ticks_for_code(text));
    let close = if text.ends_with('\n') {
        format!("{ticks}\n")
    } else {
        format!("\n{ticks}\n")
    };
    (Some(format!("{ticks}{{={format}}}\n")), Some(close))
}

/// Q1 conversion-path code-fence ticks: the longest `/^`+/` run over the
/// source lines, plus 1, never fewer than 3.
fn code_fence_ticks(text: &str) -> usize {
    let max = text
        .lines()
        .map(|line| line.chars().take_while(|&c| c == '`').count())
        .max()
        .unwrap_or(0);
    (max + 1).max(3)
}

/// Q1 display-path `ticksForCode`: the longest `/^\s*`+/` match over the
/// source lines (whitespace included in the count, faithful to Q1), plus
/// 1, never fewer than 3.
fn ticks_for_code(text: &str) -> usize {
    let max = text.lines().map(leading_ws_ticks).max().unwrap_or(0);
    (max + 1).max(3)
}

fn leading_ws_ticks(line: &str) -> usize {
    let mut count = 0;
    let mut seen_backtick = false;
    for c in line.chars() {
        if c.is_whitespace() && !seen_backtick {
            count += 1;
        } else if c == '`' {
            seen_backtick = true;
            count += 1;
        } else {
            break;
        }
    }
    count
}

/// Every synthesized piece anchors (decision 5) to `Original(fid, 0, len)`
/// of the cell it decorates.
fn invocation_anchor(file_id: FileId, cell_text: &str) -> Anchor {
    Anchor {
        role: AnchorRole::Invocation,
        source_info: Arc::new(SourceInfo::original(file_id, 0, cell_text.len())),
    }
}

#[derive(Default)]
struct Assembler {
    markdown: String,
    pieces: Vec<(SourceInfo, usize)>,
}

impl Assembler {
    fn push(&mut self, source_info: SourceInfo, text: &str) {
        self.pieces.push((source_info, text.len()));
        self.markdown.push_str(text);
    }
}

#[cfg(test)]
mod tests {
    use super::super::ORIGINAL_FILE_ID;
    use super::*;
    use quarto_source_map::{FileId, SourceContext};

    fn notebook_with_cells(cells_json: &str) -> String {
        format!(r#"{{"metadata":{{"kernelspec":{{"language":"python"}}}},"cells":[{cells_json}]}}"#)
    }

    fn convert_cells(cells_json: &str) -> Result<Converted, ProcessorError> {
        convert_notebook(
            Path::new("notebook.ipynb"),
            &notebook_with_cells(cells_json),
        )
    }

    fn convert_ok(cells_json: &str) -> Converted {
        convert_cells(cells_json).expect("conversion must succeed")
    }

    /// Register the original + per-cell virtual files exactly as every
    /// `SourceContext` rebuild site must (decision 6: original at
    /// `ORIGINAL_FILE_ID`, cells contiguous from `FileId(2)`, in order).
    fn registered_context(converted: &Converted, notebook: &str) -> SourceContext {
        let mut ctx = SourceContext::new();
        ctx.add_file_with_id(
            ORIGINAL_FILE_ID,
            "notebook.ipynb".to_string(),
            Some(notebook.to_string()),
        );
        for (i, (label, text)) in converted.files.iter().enumerate() {
            ctx.add_file_with_id(
                FileId(ORIGINAL_FILE_ID.0 + 1 + i),
                label.clone(),
                Some(text.clone()),
            );
        }
        ctx
    }

    // --- T-ipynb: cell-kind conversion (assembled qmd). ---

    // --- T-ipynb: sniff (Plan 7c Phase 2 admission) — content parses as
    // --- JSON with a top-level "cells" array. ---

    #[test]
    fn sniff_accepts_notebook_json() {
        assert!(Ipynb.sniff(
            Path::new("nb.ipynb"),
            &notebook_with_cells(r#"{"cell_type":"markdown","metadata":{},"source":["hi\n"]}"#),
            &ProcessorParams::Ipynb
        ));
    }

    #[test]
    fn sniff_rejects_non_json_content() {
        assert!(!Ipynb.sniff(
            Path::new("nb.ipynb"),
            "plain text, not JSON",
            &ProcessorParams::Ipynb
        ));
    }

    #[test]
    fn sniff_rejects_json_without_cells() {
        assert!(!Ipynb.sniff(
            Path::new("nb.ipynb"),
            r#"{"metadata":{"kernelspec":{"name":"python3"}}}"#,
            &ProcessorParams::Ipynb
        ));
    }

    #[test]
    fn sniff_rejects_cells_that_is_not_an_array() {
        assert!(!Ipynb.sniff(
            Path::new("nb.ipynb"),
            r#"{"cells": {"not": "an array"}}"#,
            &ProcessorParams::Ipynb
        ));
    }

    #[test]
    fn markdown_cells_verbatim_with_separator_between_only() {
        let converted = convert_ok(
            r#"{"cell_type":"markdown","metadata":{},"source":["Hello *world*\n"]},
               {"cell_type":"markdown","metadata":{},"source":["Second paragraph\n"]}"#,
        );
        assert_eq!(converted.markdown, "Hello *world*\n\n\nSecond paragraph\n");
    }

    #[test]
    fn raw_cell_with_html_hint_wraps_in_raw_block() {
        let converted = convert_ok(
            r#"{"cell_type":"raw","metadata":{"raw_mimetype":"text/html"},"source":["<p>hi</p>\n"]}"#,
        );
        assert_eq!(converted.markdown, "```{=html}\n<p>hi</p>\n```\n");
    }

    #[test]
    fn raw_cell_with_latex_hint_uses_tex_format() {
        // Q1 parity: mdLatexOutput → mdFormatOutput("tex", …) — {=tex},
        // NOT {=latex} (jupyter.ts).
        let converted = convert_ok(
            r#"{"cell_type":"raw","metadata":{"raw_mimetype":"text/latex"},"source":["$x$\n"]}"#,
        );
        assert_eq!(converted.markdown, "```{=tex}\n$x$\n```\n");
    }

    #[test]
    fn raw_cell_without_hint_emits_plain_content() {
        let converted =
            convert_ok(r#"{"cell_type":"raw","metadata":{},"source":["plain raw content\n"]}"#);
        assert_eq!(converted.markdown, "plain raw content\n");
    }

    #[test]
    fn raw_cell_with_javascript_hint_wraps_script_tag_in_html_block() {
        let converted = convert_ok(
            r#"{"cell_type":"raw","metadata":{"raw_mimetype":"application/javascript"},"source":["alert(1);\n"]}"#,
        );
        assert_eq!(
            converted.markdown,
            "```{=html}\n<script type=\"application/javascript\">\nalert(1);\n\n</script>\n```\n"
        );
    }

    #[test]
    fn raw_cell_fence_lengthens_over_backtick_runs() {
        // Body contains a ``` run → ticks lengthen to 4 (Q1 ticksForCode:
        // max leading tick run + 1, min 3).
        let converted = convert_ok(
            r#"{"cell_type":"raw","metadata":{"raw_mimetype":"text/html"},"source":["```\ncode\n"]}"#,
        );
        assert_eq!(converted.markdown, "````{=html}\n```\ncode\n````\n");
    }

    #[test]
    fn code_cell_fences_with_lowercased_kernelspec_language() {
        let converted = convert_ok(
            r#"{"cell_type":"code","metadata":{},"outputs":[],"source":["print(1)\nprint(2)\n"]}"#,
        );
        assert_eq!(converted.markdown, "```{python}\nprint(1)\nprint(2)\n```\n");
    }

    /// nbformat convention: the source array's last line carries no
    /// trailing newline (Jupyter strips it). The closing fence must still
    /// land on its own line — a glued `code```` fence is unparseable
    /// markdown and the stored-output replay cannot match the cell.
    /// Found by the Phase 3 end-to-end render of a real notebook.
    #[test]
    fn code_cell_source_without_trailing_newline_keeps_fence_on_own_line() {
        let converted = convert_ok(
            r#"{"cell_type":"code","metadata":{},"outputs":[],"source":["print(1)\nprint(2)"]}"#,
        );
        assert_eq!(converted.markdown, "```{python}\nprint(1)\nprint(2)\n```\n");
    }

    /// Same convention, raw-html wrapped cells: the closing fence must
    /// not glue onto the content's unterminated last line.
    #[test]
    fn raw_html_source_without_trailing_newline_keeps_fence_on_own_line() {
        let converted = convert_ok(
            r#"{"cell_type":"raw","metadata":{"raw_mimetype":"text/html"},"source":["<b>hi</b>"]}"#,
        );
        assert_eq!(converted.markdown, "```{=html}\n<b>hi</b>\n```\n");
    }

    #[test]
    fn code_cell_options_lines_flow_through_verbatim() {
        let converted = convert_ok(
            r##"{"cell_type":"code","metadata":{},"outputs":[],"source":["#| label: fig-1\n","#| echo: false\n","print(1)\n"]}"##,
        );
        assert_eq!(
            converted.markdown,
            "```{python}\n#| label: fig-1\n#| echo: false\nprint(1)\n```\n"
        );
    }

    #[test]
    fn code_cell_without_kernelspec_language_emits_empty_fence_language() {
        let converted = convert_notebook(
            Path::new("notebook.ipynb"),
            r#"{"metadata":{},"cells":[{"cell_type":"code","metadata":{},"outputs":[],"source":["x()\n"]}]}"#,
        )
        .expect("conversion must succeed");
        assert_eq!(converted.markdown, "```{}\nx()\n```\n");
    }

    #[test]
    fn empty_code_cell_is_redacted_but_keeps_its_file_entry() {
        let converted = convert_ok(
            r#"{"cell_type":"code","metadata":{},"outputs":[],"source":[]},
               {"cell_type":"markdown","metadata":{},"source":["text\n"]}"#,
        );
        assert_eq!(converted.markdown, "text\n");
        assert_eq!(
            converted
                .files
                .iter()
                .map(|(l, _)| l.as_str())
                .collect::<Vec<_>>(),
            vec![
                "notebook.ipynb[cell 1, code]",
                "notebook.ipynb[cell 2, markdown]"
            ],
        );
    }

    #[test]
    fn cell_source_as_plain_string_is_accepted() {
        let converted = convert_ok(r#"{"cell_type":"markdown","metadata":{},"source":"hello\n"}"#);
        assert_eq!(converted.markdown, "hello\n");
    }

    #[test]
    fn escaped_text_is_unescaped_into_the_virtual_file() {
        let converted = convert_ok(
            r#"{"cell_type":"markdown","metadata":{},"source":["first\n"]},
               {"cell_type":"markdown","metadata":{},"source":["tab\there\n","new\nline\n","café ✓\n"]}"#,
        );
        assert_eq!(converted.files.len(), 2);
        assert_eq!(
            converted.files[1].1, "tab\there\nnew\nline\ncafé ✓\n",
            "the virtual file must hold the logical (unescaped) text"
        );
        assert!(converted.markdown.contains("tab\there\n"));
    }

    // --- T-ipynb: front matter (fixupFrontMatter port). ---

    #[test]
    fn front_matter_with_title_stays_verbatim() {
        let converted = convert_ok(
            r#"{"cell_type":"markdown","metadata":{},"source":["---\n","title: My Title\n","---\n","\n","Some body text\n"]},
               {"cell_type":"markdown","metadata":{},"source":["more\n"]}"#,
        );
        assert_eq!(
            converted.markdown,
            "---\ntitle: My Title\n---\n\nSome body text\n\n\nmore\n"
        );
    }

    #[test]
    fn front_matter_without_title_snips_heading_from_next_markdown_cell() {
        // FM cell (author, no title) + a later markdown cell that is only a
        // heading → title injected into the FM cell's YAML, heading line
        // removed from the heading cell (original bytes: remainder suffix).
        let converted = convert_ok(
            r##"{"cell_type":"markdown","metadata":{},"source":["---\n","author: Ada\n","---\n","\n","body from fm cell\n"]},
               {"cell_type":"markdown","metadata":{},"source":["# Real Title\n","\n","paragraph\n"]}"##,
        );
        assert_eq!(
            converted.markdown,
            "---\nauthor: Ada\ntitle: Real Title\n---\n\n\nbody from fm cell\n\n\n\nparagraph\n"
        );
    }

    #[test]
    fn title_snipping_without_front_matter_synthesizes_a_front_cell() {
        // Q1 unshifts a raw cell at position 0: it takes cell number 1 and
        // shifts every later cell. Its content is Generated anchored to the
        // heading cell (no file of its own).
        let converted = convert_ok(
            r##"{"cell_type":"markdown","metadata":{},"source":["# Snipped Title\n","\n","intro\n"]},
               {"cell_type":"code","metadata":{},"outputs":[],"source":["print(1)\n"]}"##,
        );
        assert_eq!(
            converted.markdown,
            "---\ntitle: Snipped Title\n---\n\n\n\nintro\n\n\n```{python}\nprint(1)\n```\n"
        );
        assert_eq!(
            converted
                .files
                .iter()
                .map(|(l, _)| l.as_str())
                .collect::<Vec<_>>(),
            vec![
                "notebook.ipynb[cell 2, markdown]",
                "notebook.ipynb[cell 3, code]"
            ],
            "the synthesized cell takes number 1; real cells shift +1"
        );
    }

    #[test]
    fn setext_underline_heading_snips_too() {
        let converted = convert_ok(
            r#"{"cell_type":"markdown","metadata":{},"source":["My Title\n","===\n","\n","intro\n"]}"#,
        );
        assert_eq!(
            converted.markdown,
            "---\ntitle: My Title\n---\n\n\n\nintro\n"
        );
    }

    #[test]
    fn heading_with_content_before_it_is_not_snipped() {
        let converted = convert_ok(
            r##"{"cell_type":"markdown","metadata":{},"source":["lead text\n","# Not Snipped\n","\n","body\n"]}"##,
        );
        // No front matter, but the heading has content before it in its
        // cell — Q1's contentBeforeHeading blocks the snip, the scan stops,
        // and the cell is emitted verbatim.
        assert_eq!(converted.markdown, "lead text\n# Not Snipped\n\nbody\n");
    }

    #[test]
    fn non_mapping_front_matter_is_left_verbatim() {
        let converted = convert_ok(
            r#"{"cell_type":"markdown","metadata":{},"source":["---\n","- a\n","- b\n","---\n","\n","body\n"]}"#,
        );
        assert_eq!(converted.markdown, "---\n- a\n- b\n---\n\nbody\n");
    }

    // --- T-ipynb: cross-cell structure (decision 2). ---

    #[test]
    fn cross_cell_fence_open_and_close_concatenate() {
        let converted = convert_ok(
            r#"{"cell_type":"markdown","metadata":{},"source":["::: {.callout-note}\n","Opening\n"]},
               {"cell_type":"markdown","metadata":{},"source":["Closing\n",":::\n"]}"#,
        );
        assert_eq!(
            converted.markdown,
            "::: {.callout-note}\nOpening\n\n\nClosing\n:::\n"
        );
    }

    // --- T-ipynb: errors. ---

    #[test]
    fn malformed_json_is_a_processor_error() {
        let err = convert_notebook(Path::new("notebook.ipynb"), "{not json")
            .expect_err("malformed JSON must be an error");
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn missing_cells_array_is_a_processor_error() {
        let err = convert_notebook(Path::new("notebook.ipynb"), r#"{"metadata":{}}"#)
            .expect_err("missing cells array must be an error");
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn unknown_cell_type_is_a_processor_error() {
        let err = convert_cells(r#"{"cell_type":"wat","metadata":{},"source":[]}"#)
            .expect_err("unknown cell type must be an error");
        assert!(!err.to_string().is_empty());
    }

    // --- T-ipynb-src: SourceInfo maps output positions into cell files. ---

    #[test]
    fn source_info_maps_markdown_body_into_its_cell_file() {
        let notebook = notebook_with_cells(
            r#"{"cell_type":"markdown","metadata":{},"source":["Hello *world*\n"]},
               {"cell_type":"markdown","metadata":{},"source":["Second paragraph\n"]}"#,
        );
        let converted =
            convert_notebook(Path::new("notebook.ipynb"), &notebook).expect("must succeed");
        let ctx = registered_context(&converted, &notebook);

        // "Second" starts at output byte len("Hello *world*\n") + len("\n\n").
        let offset = "Hello *world*\n\n\n".len();
        let mapped = converted
            .source_info
            .map_offset(offset, &ctx)
            .expect("cell 2's first byte must resolve");
        assert_eq!(mapped.file_id, FileId(3), "must land in cell 2's file");
        assert_eq!(mapped.location.row, 0);
        assert_eq!(mapped.location.column, 0);

        // And cell 1's first byte lands in cell 1's file.
        let mapped = converted
            .source_info
            .map_offset(0, &ctx)
            .expect("cell 1's first byte must resolve");
        assert_eq!(mapped.file_id, FileId(2));
    }

    #[test]
    fn source_info_maps_code_cell_body_into_its_cell_file() {
        let notebook = notebook_with_cells(
            r#"{"cell_type":"code","metadata":{},"outputs":[],"source":["print(1)\nprint(2)\n"]}"#,
        );
        let converted =
            convert_notebook(Path::new("notebook.ipynb"), &notebook).expect("must succeed");
        let ctx = registered_context(&converted, &notebook);

        // "print(1)" starts right after the synthetic "```{python}\n".
        let mapped = converted
            .source_info
            .map_offset("```{python}\n".len(), &ctx)
            .expect("code body must resolve");
        assert_eq!(mapped.file_id, FileId(2));
        assert_eq!(mapped.location.row, 0);
        assert_eq!(mapped.location.column, 0);

        // "print(2)" is the cell file's second line.
        let mapped = converted
            .source_info
            .map_offset("```{python}\nprint(1)\n".len(), &ctx)
            .expect("second code line must resolve");
        assert_eq!(mapped.file_id, FileId(2));
        assert_eq!(mapped.location.row, 1);
    }

    #[test]
    fn source_info_maps_raw_block_body_into_its_cell_file() {
        let notebook = notebook_with_cells(
            r#"{"cell_type":"raw","metadata":{"raw_mimetype":"text/html"},"source":["<p>hi</p>\n"]}"#,
        );
        let converted =
            convert_notebook(Path::new("notebook.ipynb"), &notebook).expect("must succeed");
        let ctx = registered_context(&converted, &notebook);

        let mapped = converted
            .source_info
            .map_offset("```{=html}\n".len(), &ctx)
            .expect("raw body must resolve");
        assert_eq!(mapped.file_id, FileId(2));
        assert_eq!(mapped.location.row, 0);
    }

    #[test]
    fn converted_files_are_contiguous_from_file_id_two_in_order() {
        let converted = convert_ok(
            r#"{"cell_type":"markdown","metadata":{},"source":["a\n"]},
               {"cell_type":"raw","metadata":{},"source":["r\n"]},
               {"cell_type":"code","metadata":{},"outputs":[],"source":["c()\n"]}"#,
        );
        assert_eq!(converted.files.len(), 3);
        assert_eq!(
            converted
                .files
                .iter()
                .map(|(l, _)| l.as_str())
                .collect::<Vec<_>>(),
            vec![
                "notebook.ipynb[cell 1, markdown]",
                "notebook.ipynb[cell 2, raw]",
                "notebook.ipynb[cell 3, code]",
            ],
        );
        // Assembled length must equal the concat's total length.
        let total: usize = converted.source_info.length();
        assert_eq!(total, converted.markdown.len());
    }
}
