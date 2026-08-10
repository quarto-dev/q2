/*
 * environment.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Span-annotated parser for Quarto project environment files
 * (`_environment` and variants) — bd-environment-files-372u9qbs.
 */

//! Parser for project environment files (`_environment`,
//! `_environment.local`, `_environment.required`, and — once profiles
//! exist, bd-ev8mk1rp — `_environment-<profile>`).
//!
//! Quarto 2 **never mutates the process environment**. Where Quarto 1
//! loads these files into the ambient env (`Deno.env.set`), q2 parses
//! them into a plain map that consumers receive as data: the `env`
//! shortcode consults it after the real environment misses, and
//! subprocess spawn sites pass it explicitly. The real process
//! environment always wins over file-defined values.
//!
//! The grammar targets Quarto 1's actual dialect — `@std/dotenv`
//! (JSR; Q1 pins 0.225.x) — rather than any Rust dotenv crate:
//!
//! - optional `export ` prefix; keys must match
//!   `[A-Za-z_][A-Za-z0-9_]*` (invalid keys are skipped with a
//!   warning, matching `@std/dotenv`'s console warning);
//! - single-quoted values are literal (may span multiple lines; no
//!   escapes, no expansion; one leading/trailing newline stripped);
//! - double-quoted values (multiline as well) expand the escapes
//!   `\n` `\r` `\t` `\"` `\'` `\\` and leave other `\x` sequences
//!   untouched; no `$` expansion;
//! - unquoted values are trimmed, a `#` starts a trailing comment,
//!   and `$VAR` / `${VAR}` / `${VAR:-default}` expansion applies —
//!   resolved against the same file's entries first (forward
//!   references included, matching `@std/dotenv`'s post-parse
//!   expansion), then a caller-provided lookup (the real environment
//!   and previously-loaded files), then the `:-` default.
//!
//! Deliberate divergences from `@std/dotenv`, all in the direction of
//! diagnostics instead of silent misbehavior:
//!
//! - a reference to a variable that resolves nowhere expands to the
//!   empty string with a warning (`@std/dotenv` interpolates the
//!   JavaScript string `"undefined"`);
//! - circular references terminate with a warning after a bounded
//!   number of passes (`@std/dotenv` loops forever);
//! - a line that is not blank, not a comment, and not a `KEY=value`
//!   entry gets a warning (`@std/dotenv` skips it silently);
//! - a `${NAME:-default}` default ends at the first `}` (the
//!   `@std/dotenv` regex greedily runs to the last `}` on the line).
//!
//! Every entry carries [`SourceInfo`] spans for its key and value.
//! The [`quarto_source_map::FileId`] is derived with
//! [`quarto_yaml::file_id_for_filename`] — the same scheme the YAML
//! config stack uses — so diagnostics pointing into an environment
//! file bind content through the existing
//! [`crate::config_sources::bind_config_source`] machinery.

use hashlink::LinkedHashMap;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::{FileId, Location, Range, SourceInfo};
use quarto_system_runtime::SystemRuntime;

/// One `KEY=value` entry from an environment file.
#[derive(Debug, Clone)]
pub struct EnvEntry {
    pub key: String,
    /// The value after quote removal, escape processing, and `$`
    /// expansion.
    pub value: String,
    /// Span of the key token.
    pub key_span: SourceInfo,
    /// Span of the raw value token (including quotes, when quoted).
    pub value_span: SourceInfo,
}

/// Result of parsing one environment file.
#[derive(Debug)]
pub struct ParsedEnvFile {
    /// Entries in first-definition order; a key defined twice keeps
    /// its first position and last value (matching `@std/dotenv`).
    pub entries: Vec<EnvEntry>,
    /// Warnings (never errors — a malformed environment file must not
    /// fail a render).
    pub diagnostics: Vec<DiagnosticMessage>,
}

/// Upper bound on `$`-expansion passes over a value. `@std/dotenv`
/// re-scans until no pattern remains, which loops forever on cycles;
/// we stop here and warn instead.
const MAX_EXPANSION_PASSES: usize = 8;

/// Parse an environment file.
///
/// `lookup` resolves `$NAME` references that the file itself does not
/// define — callers pass the real process environment plus any
/// previously-loaded environment files (see the loader in this
/// module). It is only consulted during value expansion; it does not
/// affect which entries are returned.
pub fn parse_env_file(
    content: &str,
    filename: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> ParsedEnvFile {
    let file_id = quarto_yaml::file_id_for_filename(filename);
    let mut parser = Parser::new(content, file_id);
    parser.parse();
    let Parser {
        mut entries,
        mut diagnostics,
        ..
    } = parser;

    // `$` expansion for unquoted values, after the whole file has
    // parsed: references see every entry in the file (forward ones
    // included), with values as of the end of parsing — exactly
    // `@std/dotenv`'s variablesMap semantics.
    let raw_map: LinkedHashMap<String, String> = entries
        .iter()
        .map(|(k, e)| (k.clone(), e.value.clone()))
        .collect();
    for entry in entries.values_mut() {
        if entry.unquoted && entry.value.contains('$') {
            entry.value = expand_value(
                &entry.value,
                &raw_map,
                lookup,
                &entry.value_span,
                &mut diagnostics,
            );
        }
    }

    ParsedEnvFile {
        entries: entries
            .into_iter()
            .map(|(_, e)| EnvEntry {
                key: e.key,
                value: e.value,
                key_span: e.key_span,
                value_span: e.value_span,
            })
            .collect(),
        diagnostics,
    }
}

/// Merge parsed environment layers into one map.
///
/// `layers` come in **priority order, highest first** (`.local`, then
/// profile variants in activation order, then `_environment`); the
/// first definition of a key wins, mirroring Q1's "only set if not
/// already set" reads. `real_env` is the real process environment
/// (injected for testability; production passes `std::env::var`).
///
/// `$` references in a layer resolve against that layer's own entries
/// first (see [`parse_env_file`]), then the real environment, then
/// values from higher-priority layers — the same visibility Q1 gets
/// from having already injected higher-priority files into the env.
///
/// The returned map holds **file-defined values only**. Consumers must
/// keep checking the real environment first; a real variable always
/// wins over these values.
pub fn merge_env_layers(
    layers: &[(String, String)],
    real_env: &dyn Fn(&str) -> Option<String>,
) -> (LinkedHashMap<String, String>, Vec<DiagnosticMessage>) {
    let mut acc: LinkedHashMap<String, String> = LinkedHashMap::new();
    let mut diagnostics = Vec::new();
    for (filename, content) in layers {
        let parsed = {
            let lookup = |name: &str| real_env(name).or_else(|| acc.get(name).cloned());
            parse_env_file(content, filename, &lookup)
        };
        diagnostics.extend(parsed.diagnostics);
        for entry in parsed.entries {
            acc.entry(entry.key).or_insert(entry.value);
        }
    }
    (acc, diagnostics)
}

/// Check an `_environment.required` file against the real environment
/// and the merged project env map, producing one warning per required
/// variable that is defined in neither. The diagnostic points at the
/// requiring line.
pub fn check_required(
    content: &str,
    filename: &str,
    real_env: &dyn Fn(&str) -> Option<String>,
    env_map: &LinkedHashMap<String, String>,
) -> Vec<DiagnosticMessage> {
    // Expansion inside a `.required` file is irrelevant (only the
    // keys matter), so resolve references against the merged map to
    // avoid spurious undefined-variable noise.
    let lookup = |name: &str| real_env(name).or_else(|| env_map.get(name).cloned());
    let parsed = parse_env_file(content, filename, &lookup);
    let mut diagnostics = parsed.diagnostics;
    for entry in parsed.entries {
        if real_env(&entry.key).is_none() && !env_map.contains_key(&entry.key) {
            diagnostics.push(
                DiagnosticMessageBuilder::warning("Required environment variable not defined")
                    .problem(format!(
                        "`{}` is listed in `{}` but is defined neither in the \
                         environment nor in the project's environment files",
                        entry.key, filename
                    ))
                    .add_hint(format!(
                        "Set `{}` in the environment, in `_environment`, or in \
                         `_environment.local`",
                        entry.key
                    ))
                    .with_location(entry.key_span)
                    .build(),
            );
        }
    }
    diagnostics
}

/// Load a project's environment files into a map, Q1-style.
///
/// Files considered, priority highest first: `_environment.local`,
/// `_environment-<profile>` per active profile (activation order —
/// always empty until bd-ev8mk1rp lands render profiles), and
/// `_environment`. Missing files are normal. `_environment.required`
/// contributes validation diagnostics only, never values.
///
/// Project-scoped like `_variables.yml`: single-file renders get an
/// empty map (Q1 parity — env files load during project-context
/// creation there too).
pub fn load_project_environment(
    runtime: &dyn SystemRuntime,
    project: &crate::project::ProjectContext,
    active_profiles: &[String],
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> LinkedHashMap<String, String> {
    if project.is_single_file {
        return LinkedHashMap::new();
    }

    let mut names: Vec<String> = vec!["_environment.local".to_string()];
    names.extend(
        active_profiles
            .iter()
            .map(|p| format!("_environment-{}", p)),
    );
    names.push("_environment".to_string());

    let mut layers: Vec<(String, String)> = Vec::new();
    for name in names {
        let path = project.dir.join(&name);
        match runtime.file_read_string(&path) {
            Ok(content) => layers.push((path.display().to_string(), content)),
            Err(_) => {
                // Distinguish "absent" (normal) from "present but
                // unreadable" (worth a warning).
                if matches!(runtime.path_exists(&path, None), Ok(true)) {
                    diagnostics.push(
                        DiagnosticMessageBuilder::warning("Environment file not loaded")
                            .problem(format!(
                                "`{}` exists but could not be read; its variables are \
                                 unavailable",
                                path.display()
                            ))
                            .build(),
                    );
                }
            }
        }
    }

    let real_env = |name: &str| std::env::var(name).ok();
    let (env_map, mut merge_diags) = merge_env_layers(&layers, &real_env);
    diagnostics.append(&mut merge_diags);

    let required_path = project.dir.join("_environment.required");
    if let Ok(content) = runtime.file_read_string(&required_path) {
        diagnostics.extend(check_required(
            &content,
            &required_path.display().to_string(),
            &real_env,
            &env_map,
        ));
    }

    env_map
}

/// The subset of the project env map to pass to a spawned subprocess:
/// every pair whose key is **not** set in the real process environment.
/// Children inherit the real environment on spawn, so applying these
/// pairs with `Command::env(s)` preserves the precedence rule (real
/// environment beats file values) without ever mutating our own env.
pub fn env_for_subprocess(project_env: &LinkedHashMap<String, String>) -> Vec<(String, String)> {
    project_env
        .iter()
        .filter(|(k, _)| std::env::var_os(k).is_none())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Convenience for spawn sites outside the render pipeline (pre/post
/// render scripts): load the project's environment files and return
/// the subprocess-safe pairs in one step. Loader diagnostics are
/// dropped here — the same problems are already reported through
/// [`crate::stage::StageContext`]'s startup diagnostics on every
/// document render, and duplicating them per script run is noise.
pub fn subprocess_env_for_project(
    runtime: &dyn SystemRuntime,
    project: &crate::project::ProjectContext,
) -> Vec<(String, String)> {
    let mut diagnostics = Vec::new();
    let map = load_project_environment(runtime, project, &[], &mut diagnostics);
    env_for_subprocess(&map)
}

/// Internal per-entry state before expansion.
struct RawEntry {
    key: String,
    value: String,
    key_span: SourceInfo,
    value_span: SourceInfo,
    /// Only unquoted values undergo `$` expansion.
    unquoted: bool,
}

struct Parser<'a> {
    content: &'a str,
    /// Byte offset cursor. Always on a char boundary.
    pos: usize,
    file_id: FileId,
    /// Byte offsets at which each line starts (for row/column).
    line_starts: Vec<usize>,
    entries: LinkedHashMap<String, RawEntry>,
    diagnostics: Vec<DiagnosticMessage>,
}

impl<'a> Parser<'a> {
    fn new(content: &'a str, file_id: FileId) -> Self {
        // Skip a UTF-8 BOM so the first key doesn't start with it.
        let start = if content.starts_with('\u{feff}') {
            3
        } else {
            0
        };
        let mut line_starts = vec![0];
        for (i, b) in content.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Parser {
            content,
            pos: start,
            file_id,
            line_starts,
            entries: LinkedHashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    fn parse(&mut self) {
        while self.pos < self.content.len() {
            self.skip_whitespace_and_newlines();
            if self.pos >= self.content.len() {
                break;
            }
            if self.peek() == Some(b'#') {
                self.skip_to_eol();
                continue;
            }
            self.parse_entry();
        }
    }

    fn parse_entry(&mut self) {
        let entry_start = self.pos;

        let (mut key_start, mut key) = self.scan_key_token();
        self.skip_inline_whitespace();

        if self.peek() != Some(b'=') {
            // `export KEY=value` — `export` is a prefix only when a
            // second token follows and leads to `=`.
            if key == "export" {
                let (ks, k) = self.scan_key_token();
                self.skip_inline_whitespace();
                if self.peek() == Some(b'=') && !k.is_empty() {
                    key_start = ks;
                    key = k;
                } else {
                    self.warn_junk_line(entry_start);
                    return;
                }
            } else {
                self.warn_junk_line(entry_start);
                return;
            }
        }
        if key.is_empty() {
            self.warn_junk_line(entry_start);
            return;
        }
        let key_span = self.span(key_start, key_start + key.len());

        self.pos += 1; // consume '='
        // `@std/dotenv` allows only spaces/tabs between `=` and the
        // value (a newline ends an unquoted value).
        self.skip_inline_whitespace();

        let Some((value, value_span, unquoted)) = self.scan_value(entry_start) else {
            return; // diagnostic already emitted
        };

        if !is_valid_key(&key) {
            self.diagnostics.push(
                DiagnosticMessageBuilder::warning("Invalid environment variable name")
                    .problem(format!(
                        "`{}` is not a valid variable name; the entry is ignored",
                        key
                    ))
                    .add_hint(
                        "Names must start with a letter or `_` and contain only \
                         letters, digits, and `_`",
                    )
                    .with_location(key_span)
                    .build(),
            );
            return;
        }

        // Duplicate keys: last value wins, first position is kept
        // (matching `@std/dotenv`'s object-assignment semantics).
        let entry = RawEntry {
            key: key.clone(),
            value,
            key_span,
            value_span,
            unquoted,
        };
        if let Some(existing) = self.entries.get_mut(&key) {
            *existing = entry;
        } else {
            self.entries.insert(key, entry);
        }
    }

    /// Scan a run of key-token characters (`[^\s=#]`, stopping at
    /// line ends). Returns (start offset, token).
    fn scan_key_token(&mut self) -> (usize, String) {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() || b == b'=' || b == b'#' {
                break;
            }
            self.pos += utf8_len(b);
        }
        (start, self.content[start..self.pos].to_string())
    }

    /// Scan the value that follows `=`. Returns
    /// `(value, value_span, unquoted)`, or `None` after emitting a
    /// diagnostic (unterminated quote).
    fn scan_value(&mut self, entry_start: usize) -> Option<(String, SourceInfo, bool)> {
        match self.peek() {
            Some(b'\'') => {
                let open = self.pos;
                let inner_start = open + 1;
                let Some(rel) = self.content[inner_start..].find('\'') else {
                    self.warn_unterminated_quote(entry_start, '\'');
                    return None;
                };
                let close = inner_start + rel;
                self.pos = close + 1;
                self.skip_to_eol(); // trailing junk/comment discarded, as in @std/dotenv
                let value = strip_quote_newlines(&self.content[inner_start..close]).to_string();
                Some((value, self.span(open, close + 1), false))
            }
            Some(b'"') => {
                let open = self.pos;
                let inner_start = open + 1;
                let mut close = None;
                let mut chars = self.content[inner_start..].char_indices();
                while let Some((i, c)) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next(); // escaped char (may be a newline)
                        }
                        '"' => {
                            close = Some(inner_start + i);
                            break;
                        }
                        _ => {}
                    }
                }
                let Some(close) = close else {
                    self.warn_unterminated_quote(entry_start, '"');
                    return None;
                };
                self.pos = close + 1;
                self.skip_to_eol();
                let raw = strip_quote_newlines(&self.content[inner_start..close]);
                Some((expand_escapes(raw), self.span(open, close + 1), false))
            }
            _ => {
                // Unquoted: runs to end of line or a `#` (no
                // preceding whitespace required, matching
                // `@std/dotenv`'s `[^\r\n#]*`).
                let start = self.pos;
                while let Some(b) = self.peek() {
                    if b == b'\n' || b == b'\r' || b == b'#' {
                        break;
                    }
                    self.pos += utf8_len(b);
                }
                let raw = &self.content[start..self.pos];
                let trimmed = raw.trim();
                let tstart = start + (raw.len() - raw.trim_start().len());
                let tend = tstart + trimmed.len();
                self.skip_to_eol();
                Some((trimmed.to_string(), self.span(tstart, tend), true))
            }
        }
    }

    fn warn_junk_line(&mut self, entry_start: usize) {
        self.skip_to_eol_from(entry_start);
        let line = self.content[entry_start..self.pos]
            .trim_end_matches(['\n', '\r'])
            .to_string();
        let span = self.span(entry_start, entry_start + line.len());
        self.diagnostics.push(
            DiagnosticMessageBuilder::warning("Malformed environment file line")
                .problem(format!(
                    "`{}` is not a `KEY=value` entry; it is ignored",
                    line
                ))
                .add_hint("Use `NAME=value`, `# comment`, or a blank line")
                .with_location(span)
                .build(),
        );
    }

    fn warn_unterminated_quote(&mut self, entry_start: usize, quote: char) {
        self.skip_to_eol_from(entry_start);
        let span = self.span(entry_start, self.pos);
        self.diagnostics.push(
            DiagnosticMessageBuilder::warning("Unterminated quote in environment file")
                .problem(format!(
                    "value opened with `{}` is never closed; the entry is ignored",
                    quote
                ))
                .with_location(span)
                .build(),
        );
    }

    fn peek(&self) -> Option<u8> {
        self.content.as_bytes().get(self.pos).copied()
    }

    fn skip_inline_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    fn skip_whitespace_and_newlines(&mut self) {
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Advance past the end of the current line (consuming the
    /// newline).
    fn skip_to_eol(&mut self) {
        while let Some(b) = self.peek() {
            self.pos += utf8_len(b);
            if b == b'\n' {
                break;
            }
        }
    }

    /// Like [`skip_to_eol`], but starting the scan at `from` (used
    /// for error recovery on the line an entry started on).
    fn skip_to_eol_from(&mut self, from: usize) {
        self.pos = self.pos.max(from);
        self.skip_to_eol();
    }

    fn location(&self, offset: usize) -> Location {
        let row = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts[row];
        let column = self.content[line_start..offset].chars().count();
        Location {
            offset,
            row,
            column,
        }
    }

    fn span(&self, start: usize, end: usize) -> SourceInfo {
        SourceInfo::from_range(
            self.file_id,
            Range {
                start: self.location(start),
                end: self.location(end),
            },
        )
    }
}

fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn utf8_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

/// Strip one leading and one trailing newline (`\r\n`, `\n`, or
/// `\r`) inside a quoted value, matching `@std/dotenv`'s
/// `'\r?\n?…\r?\n?'` quote groups.
fn strip_quote_newlines(s: &str) -> &str {
    let s = s
        .strip_prefix("\r\n")
        .or_else(|| s.strip_prefix('\n'))
        .or_else(|| s.strip_prefix('\r'))
        .unwrap_or(s);
    s.strip_suffix("\r\n")
        .or_else(|| s.strip_suffix('\n'))
        .or_else(|| s.strip_suffix('\r'))
        .unwrap_or(s)
}

/// Expand the escape sequences `@std/dotenv` supports in
/// double-quoted values; any other `\x` stays as-is (backslash
/// included).
fn expand_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Run bounded `$`-expansion passes over an unquoted value.
fn expand_value(
    value: &str,
    raw_map: &LinkedHashMap<String, String>,
    lookup: &dyn Fn(&str) -> Option<String>,
    value_span: &SourceInfo,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> String {
    let mut current = value.to_string();
    for _ in 0..MAX_EXPANSION_PASSES {
        let (next, any_pattern) = expand_once(&current, raw_map, lookup, value_span, diagnostics);
        if !any_pattern || next == current {
            return next;
        }
        current = next;
        if !current.contains('$') {
            return current;
        }
    }
    diagnostics.push(
        DiagnosticMessageBuilder::warning("Environment variable expansion did not converge")
            .problem(format!(
                "expansion of `{}` still contains `$` references after {} passes \
                 (possible circular reference)",
                value, MAX_EXPANSION_PASSES
            ))
            .with_location(value_span.clone())
            .build(),
    );
    current
}

/// One expansion pass. Returns the rewritten string and whether any
/// expandable `$` pattern was found.
fn expand_once(
    input: &str,
    raw_map: &LinkedHashMap<String, String>,
    lookup: &dyn Fn(&str) -> Option<String>,
    value_span: &SourceInfo,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> (String, bool) {
    let mut out = String::with_capacity(input.len());
    let mut any_pattern = false;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '$' {
            out.push(c);
            i += 1;
            continue;
        }
        // `${NAME}` / `${NAME:-default}` — no escape for this form in
        // `@std/dotenv` (its lookbehind guards only the bare form).
        if chars.get(i + 1) == Some(&'{') {
            let mut j = i + 2;
            while j < chars.len() && chars[j] != '}' {
                j += 1;
            }
            if j >= chars.len() {
                // Unterminated `${` — literal, keep scanning after it.
                out.push('$');
                out.push('{');
                i += 2;
                continue;
            }
            let inner: String = chars[i + 2..j].iter().collect();
            let (name, default) = match inner.split_once(":-") {
                Some((n, d)) => (n.to_string(), Some(d.to_string())),
                None => (inner, None),
            };
            if name.is_empty() {
                // `${}` (or `${:-…}`) has no name to expand; keep it
                // verbatim, as @std/dotenv's `.+?` group would.
                out.extend(&chars[i..=j]);
                i = j + 1;
                continue;
            }
            any_pattern = true;
            out.push_str(&resolve_name(
                &name,
                default.as_deref(),
                raw_map,
                lookup,
                value_span,
                diagnostics,
            ));
            i = j + 1;
            continue;
        }
        // Bare `$NAME` — suppressed when preceded by `\` (the
        // backslash itself stays in the output, as in `@std/dotenv`).
        let escaped = i > 0 && chars[i - 1] == '\\';
        let mut j = i + 1;
        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
            j += 1;
        }
        if j == i + 1 || escaped {
            out.push('$');
            i += 1;
            continue;
        }
        let name: String = chars[i + 1..j].iter().collect();
        // Optional `:-default` after the bare form runs to the end of
        // the value (matching the `@std/dotenv` regex's greedy `.+`).
        let default: Option<String> =
            if chars.get(j) == Some(&':') && chars.get(j + 1) == Some(&'-') {
                let d: String = chars[j + 2..].iter().collect();
                j = chars.len();
                Some(d)
            } else {
                None
            };
        any_pattern = true;
        out.push_str(&resolve_name(
            &name,
            default.as_deref(),
            raw_map,
            lookup,
            value_span,
            diagnostics,
        ));
        i = j;
    }
    (out, any_pattern)
}

fn resolve_name(
    name: &str,
    default: Option<&str>,
    raw_map: &LinkedHashMap<String, String>,
    lookup: &dyn Fn(&str) -> Option<String>,
    value_span: &SourceInfo,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> String {
    if let Some(v) = raw_map.get(name) {
        return v.clone();
    }
    if let Some(v) = lookup(name) {
        return v;
    }
    if let Some(d) = default {
        return d.to_string();
    }
    diagnostics.push(
        DiagnosticMessageBuilder::warning("Undefined variable in environment file")
            .problem(format!(
                "`${}` is not defined in this file, the environment, or a \
                 previously-loaded environment file; it expands to the empty string",
                name
            ))
            .add_hint(format!(
                "Define `{}`, or provide a default with `${{{}:-default}}`",
                name, name
            ))
            .with_location(value_span.clone())
            .build(),
    );
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_lookup(_: &str) -> Option<String> {
        None
    }

    fn parse(content: &str) -> ParsedEnvFile {
        parse_env_file(content, "_environment", &no_lookup)
    }

    fn values(parsed: &ParsedEnvFile) -> Vec<(&str, &str)> {
        parsed
            .entries
            .iter()
            .map(|e| (e.key.as_str(), e.value.as_str()))
            .collect()
    }

    #[test]
    fn plain_entries() {
        let p = parse("A=hello\nB=world\n");
        assert_eq!(values(&p), vec![("A", "hello"), ("B", "world")]);
        assert!(p.diagnostics.is_empty());
    }

    #[test]
    fn export_prefix() {
        let p = parse("export A=1\n");
        assert_eq!(values(&p), vec![("A", "1")]);
        assert!(p.diagnostics.is_empty());
    }

    #[test]
    fn export_as_key() {
        // `export=x` and `export = x` define a variable literally
        // named `export` (matches @std/dotenv's regex backtracking).
        let p = parse("export=x\n");
        assert_eq!(values(&p), vec![("export", "x")]);
    }

    #[test]
    fn comments_and_blank_lines() {
        let p = parse("\n# a comment\n\nA=1\n   # indented comment\n");
        assert_eq!(values(&p), vec![("A", "1")]);
        assert!(p.diagnostics.is_empty());
    }

    #[test]
    fn unquoted_trailing_comment() {
        let p = parse("A=hello # comment\nB=hello#no-space\n");
        assert_eq!(values(&p), vec![("A", "hello"), ("B", "hello")]);
    }

    #[test]
    fn unquoted_trimmed() {
        let p = parse("A=   spaced out   \n");
        assert_eq!(values(&p), vec![("A", "spaced out")]);
    }

    #[test]
    fn unquoted_value_may_contain_equals() {
        let p = parse("A=b=c\n");
        assert_eq!(values(&p), vec![("A", "b=c")]);
    }

    #[test]
    fn spaces_around_equals() {
        let p = parse("A = 1\n");
        assert_eq!(values(&p), vec![("A", "1")]);
    }

    #[test]
    fn empty_value() {
        let p = parse("A=\nB=1\n");
        assert_eq!(values(&p), vec![("A", ""), ("B", "1")]);
    }

    #[test]
    fn single_quoted_literal() {
        let p = parse("A='  keeps  spaces  '\n");
        assert_eq!(values(&p), vec![("A", "  keeps  spaces  ")]);
    }

    #[test]
    fn single_quoted_no_expansion_no_escapes() {
        let p = parse("B=x\nA='$B and \\n stay literal'\n");
        assert_eq!(
            values(&p),
            vec![("B", "x"), ("A", "$B and \\n stay literal")]
        );
        assert!(p.diagnostics.is_empty());
    }

    #[test]
    fn single_quoted_multiline() {
        let p = parse("A='\nline1\nline2\n'\nB=after\n");
        assert_eq!(values(&p), vec![("A", "line1\nline2"), ("B", "after")]);
    }

    #[test]
    fn double_quoted_escapes() {
        let p = parse(r#"A="a\nb\tc\"d\\e\'f""#);
        assert_eq!(values(&p), vec![("A", "a\nb\tc\"d\\e'f")]);
    }

    #[test]
    fn double_quoted_unknown_escape_kept() {
        let p = parse(r#"A="a\xb""#);
        assert_eq!(values(&p), vec![("A", "a\\xb")]);
    }

    #[test]
    fn double_quoted_multiline() {
        let p = parse("A=\"\nline1\nline2\n\"\nB=after\n");
        assert_eq!(values(&p), vec![("A", "line1\nline2"), ("B", "after")]);
    }

    #[test]
    fn double_quoted_no_dollar_expansion() {
        let p = parse("B=x\nA=\"$B\"\n");
        assert_eq!(values(&p), vec![("B", "x"), ("A", "$B")]);
        assert!(p.diagnostics.is_empty());
    }

    #[test]
    fn expansion_same_file() {
        let p = parse("B=x\nA=$B\n");
        assert_eq!(values(&p), vec![("B", "x"), ("A", "x")]);
    }

    #[test]
    fn expansion_forward_reference() {
        // @std/dotenv expands after parsing the whole file.
        let p = parse("A=$B\nB=x\n");
        assert_eq!(values(&p), vec![("A", "x"), ("B", "x")]);
    }

    #[test]
    fn expansion_braced() {
        let p = parse("B=x\nA=pre${B}post\n");
        assert_eq!(values(&p), vec![("B", "x"), ("A", "prexpost")]);
    }

    #[test]
    fn expansion_braced_default() {
        let p = parse("A=${MISSING:-fallback}\n");
        assert_eq!(values(&p), vec![("A", "fallback")]);
        assert!(p.diagnostics.is_empty());
    }

    #[test]
    fn expansion_bare_default_runs_to_end() {
        let p = parse("A=$MISSING:-a b c\n");
        assert_eq!(values(&p), vec![("A", "a b c")]);
    }

    #[test]
    fn expansion_uses_lookup() {
        let lookup = |name: &str| (name == "EXT").then(|| "ext-value".to_string());
        let p = parse_env_file("A=$EXT\n", "_environment", &lookup);
        assert_eq!(values(&p), vec![("A", "ext-value")]);
    }

    #[test]
    fn expansion_same_file_beats_lookup() {
        let lookup = |name: &str| (name == "B").then(|| "from-lookup".to_string());
        let p = parse_env_file("B=in-file\nA=$B\n", "_environment", &lookup);
        assert_eq!(values(&p), vec![("B", "in-file"), ("A", "in-file")]);
    }

    #[test]
    fn expansion_undefined_is_empty_with_diagnostic() {
        let p = parse("A=x${NOPE}y\n");
        assert_eq!(values(&p), vec![("A", "xy")]);
        assert_eq!(p.diagnostics.len(), 1);
        let text = format!("{:?}", p.diagnostics[0]);
        assert!(
            text.contains("NOPE"),
            "diagnostic names the variable: {text}"
        );
    }

    #[test]
    fn expansion_escaped_bare_dollar() {
        // `\$B` is not expanded; the backslash stays (as in
        // @std/dotenv).
        let p = parse("B=x\nA=\\$B\n");
        assert_eq!(values(&p), vec![("B", "x"), ("A", "\\$B")]);
    }

    #[test]
    fn expansion_transitive() {
        let p = parse("C=z\nB=$C\nA=$B\n");
        assert_eq!(values(&p), vec![("C", "z"), ("B", "z"), ("A", "z")]);
    }

    #[test]
    fn expansion_cycle_terminates_with_diagnostic() {
        let p = parse("A=$B\nB=$A\n");
        assert!(
            p.diagnostics
                .iter()
                .any(|d| format!("{:?}", d).contains("did not converge")
                    || format!("{:?}", d).contains("converge")),
            "expected non-convergence diagnostic, got: {:?}",
            p.diagnostics
        );
    }

    #[test]
    fn dollar_before_non_name_is_literal() {
        let p = parse("A=costs $$ and $ signs\n");
        assert_eq!(values(&p), vec![("A", "costs $$ and $ signs")]);
        assert!(p.diagnostics.is_empty());
    }

    #[test]
    fn invalid_key_skipped_with_diagnostic() {
        let p = parse("1AB=x\nGOOD=1\n");
        assert_eq!(values(&p), vec![("GOOD", "1")]);
        assert_eq!(p.diagnostics.len(), 1);
    }

    #[test]
    fn junk_line_warns() {
        let p = parse("not a kv line\nA=1\n");
        assert_eq!(values(&p), vec![("A", "1")]);
        assert_eq!(p.diagnostics.len(), 1);
        let text = format!("{:?}", p.diagnostics[0]);
        assert!(text.contains("not a kv line"), "{text}");
    }

    #[test]
    fn duplicate_key_last_wins_first_position() {
        let p = parse("A=1\nB=2\nA=3\n");
        assert_eq!(values(&p), vec![("A", "3"), ("B", "2")]);
    }

    #[test]
    fn crlf_line_endings() {
        let p = parse("A=1\r\nB=2\r\n");
        assert_eq!(values(&p), vec![("A", "1"), ("B", "2")]);
    }

    #[test]
    fn unterminated_quote_warns_and_skips() {
        let p = parse("A='never closed\nB=1\n");
        // The single-quote scan runs to EOF without a close; the
        // entry is dropped with a diagnostic. B is consumed by the
        // failed scan? No: the scan looks for a closing quote in the
        // whole rest of the file — absent, so only A's line is
        // skipped.
        assert_eq!(values(&p), vec![("B", "1")]);
        assert_eq!(p.diagnostics.len(), 1);
    }

    #[test]
    fn spans_resolve_to_correct_bytes() {
        let content = "FOO=bar\nBAZ='qux'\n";
        let p = parse(content);
        let fid = quarto_yaml::file_id_for_filename("_environment").0;

        let (f0, s0, e0) = p.entries[0].key_span.resolve_byte_range().unwrap();
        assert_eq!((f0, &content[s0..e0]), (fid, "FOO"));
        let (_, s1, e1) = p.entries[0].value_span.resolve_byte_range().unwrap();
        assert_eq!(&content[s1..e1], "bar");

        let (_, s2, e2) = p.entries[1].key_span.resolve_byte_range().unwrap();
        assert_eq!(&content[s2..e2], "BAZ");
        // Quoted value spans include the quotes.
        let (_, s3, e3) = p.entries[1].value_span.resolve_byte_range().unwrap();
        assert_eq!(&content[s3..e3], "'qux'");
    }

    #[test]
    fn span_of_trimmed_unquoted_value() {
        let content = "A=   padded   \n";
        let p = parse(content);
        let (_, s, e) = p.entries[0].value_span.resolve_byte_range().unwrap();
        assert_eq!(&content[s..e], "padded");
    }

    #[test]
    fn bom_is_skipped() {
        let p = parse("\u{feff}A=1\n");
        assert_eq!(values(&p), vec![("A", "1")]);
        assert!(p.diagnostics.is_empty());
    }

    #[test]
    fn utf8_values() {
        let p = parse("GREETING=olá mundo\n");
        assert_eq!(values(&p), vec![("GREETING", "olá mundo")]);
    }

    // === merge_env_layers ===

    fn layers(specs: &[(&str, &str)]) -> Vec<(String, String)> {
        specs
            .iter()
            .map(|(f, c)| (f.to_string(), c.to_string()))
            .collect()
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn merge_local_beats_base() {
        let (map, diags) = merge_env_layers(
            &layers(&[
                ("_environment.local", "A=local\n"),
                ("_environment", "A=base\nB=only-base\n"),
            ]),
            &no_env,
        );
        assert_eq!(map.get("A").map(String::as_str), Some("local"));
        assert_eq!(map.get("B").map(String::as_str), Some("only-base"));
        assert!(diags.is_empty());
    }

    #[test]
    fn merge_expansion_sees_higher_priority_layers() {
        // A reference in `_environment` resolves to the value the
        // higher-priority `.local` layer defined — the visibility Q1
        // gets from injecting `.local` into the env first.
        let (map, _) = merge_env_layers(
            &layers(&[
                ("_environment.local", "NAME=from-local\n"),
                ("_environment", "GREETING=hi $NAME\n"),
            ]),
            &no_env,
        );
        assert_eq!(
            map.get("GREETING").map(String::as_str),
            Some("hi from-local")
        );
    }

    #[test]
    fn merge_expansion_real_env_wins_over_layers() {
        let real = |name: &str| (name == "NAME").then(|| "from-real-env".to_string());
        let (map, _) = merge_env_layers(
            &layers(&[
                ("_environment.local", "NAME=from-local\n"),
                ("_environment", "GREETING=hi $NAME\n"),
            ]),
            &real,
        );
        assert_eq!(
            map.get("GREETING").map(String::as_str),
            Some("hi from-real-env")
        );
        // The map still carries the file value; consumers checking the
        // real environment first is what makes the real value win.
        assert_eq!(map.get("NAME").map(String::as_str), Some("from-local"));
    }

    #[test]
    fn merge_same_file_beats_real_env_in_expansion() {
        // @std/dotenv resolves same-file references before the
        // environment.
        let real = |name: &str| (name == "B").then(|| "real".to_string());
        let (map, _) = merge_env_layers(&layers(&[("_environment", "B=file\nA=$B\n")]), &real);
        assert_eq!(map.get("A").map(String::as_str), Some("file"));
    }

    #[test]
    fn merge_collects_layer_diagnostics() {
        let (map, diags) =
            merge_env_layers(&layers(&[("_environment", "junk line\nA=1\n")]), &no_env);
        assert_eq!(map.get("A").map(String::as_str), Some("1"));
        assert_eq!(diags.len(), 1);
    }

    // === env_for_subprocess ===

    #[test]
    fn subprocess_env_filters_real_env_keys() {
        // SAFETY: test-local variable name; nextest runs each test in
        // its own process, so no cross-test env races.
        unsafe { std::env::set_var("Q2_TEST_SUBPROC_SHADOWED", "real") };
        let mut map = LinkedHashMap::new();
        map.insert("Q2_TEST_SUBPROC_SHADOWED".to_string(), "file".to_string());
        map.insert("Q2_TEST_SUBPROC_FRESH".to_string(), "file".to_string());
        assert_eq!(
            env_for_subprocess(&map),
            vec![("Q2_TEST_SUBPROC_FRESH".to_string(), "file".to_string())]
        );
    }

    // === check_required ===

    #[test]
    fn required_missing_var_warns_with_span() {
        let content = "PRESENT=\nMISSING=example value\n";
        let mut map = LinkedHashMap::new();
        map.insert("PRESENT".to_string(), "x".to_string());
        let diags = check_required(content, "_environment.required", &no_env, &map);
        assert_eq!(diags.len(), 1);
        let text = format!("{:?}", diags[0]);
        assert!(text.contains("MISSING"), "{text}");
        // The span points at the requiring key.
        let loc = diags[0].location.as_ref().expect("diagnostic has a span");
        let (fid, s, e) = loc.resolve_byte_range().unwrap();
        assert_eq!(
            fid,
            quarto_yaml::file_id_for_filename("_environment.required").0
        );
        assert_eq!(&content[s..e], "MISSING");
    }

    #[test]
    fn required_satisfied_by_real_env() {
        let real = |name: &str| (name == "FROM_ENV").then(|| "1".to_string());
        let diags = check_required(
            "FROM_ENV=\n",
            "_environment.required",
            &real,
            &LinkedHashMap::new(),
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn required_all_satisfied_is_quiet() {
        let mut map = LinkedHashMap::new();
        map.insert("A".to_string(), "1".to_string());
        let diags = check_required("A=\n", "_environment.required", &no_env, &map);
        assert!(diags.is_empty(), "{diags:?}");
    }
}
