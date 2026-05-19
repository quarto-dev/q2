/*
 * attribution/git_blame.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Native `git blame --porcelain` attribution provider.
//!
//! Shells out to `git` via `RenderContext::binaries.git` (so
//! `QUARTO_GIT` overrides work the same way as `QUARTO_PANDOC` etc.).
//! Pure-Rust port of the TS `attribution-gitblame.ts` adapter from
//! `feat/node-attribution`; multi-byte UTF-8 line lengths are computed
//! via `s.as_bytes().len()` (TextEncoder equivalent).

use std::collections::HashMap;
use std::process::Command;

use quarto_error_reporting::DiagnosticMessage;

use super::builder::AttributionDataBuilder;
use super::palette::{actor_color, fnv1a_hex8};
use super::source::AttributionSourceProvider;
use super::types::{AttributionData, Identity};
use crate::Result;
use crate::error::QuartoError;
use crate::render::RenderContext;

/// One parsed porcelain record per source line.
///
/// `author_mail` has the angle brackets stripped — used as the actor
/// identifier (matching the TS prototype). `committer_time` is the
/// "when did this line land in this branch" signal the rendered
/// viewer surfaces (see [`build_blame_runs`]); author-time is
/// deliberately not parsed because it can be back-dated via
/// `git commit --date=PAST`, rebase, cherry-pick, or amend, and the
/// viewer's freshness reading should not reflect those.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub author: String,
    pub author_mail: String,
    pub committer_time: i64,
}

/// A line-level blame record expanded to a byte range against the
/// source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameRun {
    pub byte_start: usize,
    pub byte_end: usize,
    pub actor: String,
    pub time: i64,
}

#[derive(Debug, Clone)]
struct CachedCommit {
    author: String,
    author_mail: String,
    committer_time: i64,
}

/// Parse `git blame --porcelain` output into one [`BlameLine`] per
/// source line. Commit metadata is emitted only on the first
/// appearance of each commit; the parser caches by commit hash so
/// every line record is fully populated.
pub fn parse_blame_porcelain(output: &str) -> Vec<BlameLine> {
    let mut cache: HashMap<String, CachedCommit> = HashMap::new();
    let mut results: Vec<BlameLine> = Vec::new();

    // Pending state for the commit currently being assembled. Cleared
    // after each content (`\t...`) line is consumed.
    let mut current_hash: Option<String> = None;
    let mut current_author: Option<String> = None;
    let mut current_mail: Option<String> = None;
    let mut current_committer_time: Option<i64> = None;

    for line in output.lines() {
        if let Some(_content) = line.strip_prefix('\t') {
            let Some(hash) = current_hash.take() else {
                // Content line with no header — malformed; skip.
                current_author = None;
                current_mail = None;
                current_committer_time = None;
                continue;
            };
            let record = if let Some(cached) = cache.get(&hash) {
                BlameLine {
                    author: cached.author.clone(),
                    author_mail: cached.author_mail.clone(),
                    committer_time: cached.committer_time,
                }
            } else {
                let author = current_author.take().unwrap_or_default();
                let author_mail = current_mail.take().unwrap_or_default();
                let committer_time = current_committer_time.take().unwrap_or(0);
                let cached = CachedCommit {
                    author: author.clone(),
                    author_mail: author_mail.clone(),
                    committer_time,
                };
                cache.insert(hash, cached);
                BlameLine {
                    author,
                    author_mail,
                    committer_time,
                }
            };
            results.push(record);
            current_author = None;
            current_mail = None;
            current_committer_time = None;
            continue;
        }

        let Some((head, rest)) = line.split_once(' ') else {
            // Single-token lines such as `boundary` carry no value we
            // need; ignore.
            continue;
        };

        if head.len() == 40 && head.chars().all(|c| c.is_ascii_hexdigit()) {
            current_hash = Some(head.to_string());
            continue;
        }

        match head {
            "author" => current_author = Some(rest.to_string()),
            "author-mail" => {
                let trimmed = rest.trim();
                let stripped = trimmed
                    .strip_prefix('<')
                    .and_then(|s| s.strip_suffix('>'))
                    .unwrap_or(trimmed);
                current_mail = Some(stripped.to_string());
            }
            "committer-time" => current_committer_time = rest.trim().parse::<i64>().ok(),
            _ => {}
        }
    }

    results
}

/// Expand line-level blame records into byte-ranged runs using the
/// in-memory source text as the source of truth for per-line byte
/// lengths. UTF-8 is handled via `s.as_bytes().len()` — the
/// porcelain's tab-prefixed content is never trusted for byte
/// arithmetic.
pub fn build_blame_runs(blame: &[BlameLine], text: &str) -> Result<Vec<BlameRun>> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != blame.len() {
        return Err(QuartoError::other(format!(
            "git blame line-count mismatch: porcelain reports {} lines, source has {}",
            blame.len(),
            lines.len()
        )));
    }
    let text_bytes = text.as_bytes();
    let mut out = Vec::with_capacity(lines.len());
    let mut offset = 0usize;
    for (i, line) in lines.iter().enumerate() {
        let line_bytes = line.as_bytes().len();
        let mut byte_end = offset + line_bytes;
        // text.lines() consumes the trailing newline; restore it in
        // the run extent so concatenated runs equal the source bytes.
        if byte_end < text_bytes.len() && text_bytes[byte_end] == b'\n' {
            byte_end += 1;
        }
        out.push(BlameRun {
            byte_start: offset,
            byte_end,
            actor: blame[i].author_mail.clone(),
            time: blame[i].committer_time,
        });
        offset = byte_end;
    }
    Ok(out)
}

/// Shells out to `git blame --porcelain` (via `ctx.binaries.git`)
/// and returns a complete [`AttributionData`] for the document under
/// render.
///
/// Graceful degradation: when git is unavailable (binary not found,
/// document not in a working tree, etc.), emits a diagnostic warning
/// and returns an empty `AttributionData`; the pipeline behaves as
/// if attribution were off.
#[derive(Debug, Clone, Default)]
pub struct GitBlameProvider;

impl GitBlameProvider {
    pub fn new() -> Self {
        Self
    }
}

impl AttributionSourceProvider for GitBlameProvider {
    fn build(&self, ctx: &RenderContext) -> Result<AttributionData> {
        let Some(git_bin) = ctx.binaries.git.as_ref() else {
            // Provider asked for, but no git available. Soft-fail.
            // Diagnostics are written to ctx.diagnostics by the
            // transform layer; here we have only `&RenderContext`.
            // Returning the empty payload preserves the contract.
            return Ok(AttributionData::default());
        };
        let input_path = ctx.document.input.clone();

        let working_dir = input_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let output = match Command::new(git_bin)
            .current_dir(&working_dir)
            .arg("blame")
            .arg("--porcelain")
            .arg("--")
            .arg(&input_path)
            .output()
        {
            Ok(out) => out,
            Err(_) => {
                return Ok(AttributionData::default());
            }
        };

        if !output.status.success() {
            // Not in a git working tree, untracked file, etc. — soft-fail.
            return Ok(AttributionData::default());
        }

        let porcelain = match String::from_utf8(output.stdout) {
            Ok(s) => s,
            Err(e) => {
                return Err(QuartoError::other(format!(
                    "git blame --porcelain emitted non-UTF8 output: {e}"
                )));
            }
        };

        let source = std::fs::read_to_string(&input_path)
            .map_err(|e| QuartoError::other(format!("failed to read {input_path:?}: {e}")))?;

        attribution_from_porcelain(&porcelain, &source)
    }
}

/// Assemble a canonical [`AttributionData`] from raw `git blame
/// --porcelain` output and the source text it was generated from.
///
/// Encapsulates the producer-side half of [`GitBlameProvider::build`]
/// so it can be tested without a [`RenderContext`] (the production
/// `build` only adds I/O: shelling out to git and reading the source
/// file).
///
/// Producer invariant: every actor referenced by the returned
/// `runs` has an entry in `identities` whose `display_name` is the
/// mail-local-part of the email and whose `color` is
/// `actor_color(fnv1a_hex8(email))`.
pub fn attribution_from_porcelain(porcelain: &str, source: &str) -> Result<AttributionData> {
    let blame_lines = parse_blame_porcelain(porcelain);
    if blame_lines.is_empty() {
        return Ok(AttributionData::default());
    }
    let runs = build_blame_runs(&blame_lines, source)?;

    let mut builder = AttributionDataBuilder::new();
    // Single pass: the builder interns each actor on first sight,
    // so `set_identity_if_absent` populates the identity exactly
    // once per distinct email and subsequent `push_run` calls
    // share its `Arc::ptr_eq` key.
    for run in runs {
        let display_name = display_name_from_email(&run.actor);
        let color = actor_color(&fnv1a_hex8(&run.actor));
        builder.set_identity_if_absent(
            &run.actor,
            Identity {
                display_name,
                color,
            },
        );
        builder.push_run(run.byte_start, run.byte_end, &run.actor, run.time);
    }

    Ok(builder.build())
}

/// Mail-local-part if `email` contains an `@`; otherwise the full
/// string. Matches the Phase 3a spec — pathological non-email actors
/// degrade to displaying the raw string rather than empty.
fn display_name_from_email(email: &str) -> String {
    email
        .split_once('@')
        .map(|(local, _)| local.to_string())
        .unwrap_or_else(|| email.to_string())
}

/// Public alias so the transform layer can warn when graceful
/// degradation has fired. Currently unused — provider returns the
/// empty payload silently; the calling transform inspects
/// `attribution_data` and emits the warning if it sees a mode-on /
/// empty-payload mismatch.
#[allow(dead_code)]
fn _diagnostic_marker(_msg: &str) -> DiagnosticMessage {
    DiagnosticMessage::warning(format!("attribution: {_msg}"))
}
