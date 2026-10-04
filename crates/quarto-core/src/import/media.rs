//! The media plan and the image-link rewrite (I12, I16).
//!
//! Input: the media manifest TS builds from the run's collected files (epic interface 2) and
//! the qmd's project-relative path. Output: where each stored file goes (`media_plan`), the
//! `Image` targets rewritten to point there, and the diagnostics for what was skipped,
//! converted or can't be shown. Nothing here sees image bytes (I9): the manifest carries
//! hashes only.

use std::collections::HashSet;

use pampa::pandoc::Pandoc;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::inline::Inline;
use serde::{Deserialize, Serialize};

use super::report;
use super::request::extract_dir;
use crate::ast_walk::{for_each_inline_list_mut, for_each_meta_inline_list_mut};

/// One entry of `media_manifest_json`, in collected order, then one `skipped` entry per
/// host `collect-limit` warning.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum ManifestEntry {
    Stored {
        pandoc_path: String,
        sha256: String,
        ext: String,
        #[serde(default)]
        converted_from: Option<String>,
        #[serde(default)]
        conversion_failed: bool,
    },
    Skipped {
        pandoc_path: String,
        #[serde(default)]
        reason: String,
        #[serde(default)]
        size: Option<u64>,
    },
}

/// `media_plan[]`: where a stored file goes in the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaPlanEntry {
    pub pandoc_path: String,
    pub project_path: String,
}

/// Browser-displayable image extensions (anything else stored gets Q-24-11).
const DISPLAYABLE: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "bmp", "ico",
];

/// What a link target may not contain raw: space, brackets, parentheses, `#`, `?`, `%`,
/// controls and (always, by `percent-encoding`'s rules) non-ASCII. The qmd reader rejects a
/// space in a link target (Q-2-33) and pampa writes targets verbatim.
const LINK_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'(')
    .add(b')')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b']')
    .add(b'\\')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'|');

fn encode_segment(segment: &str) -> String {
    utf8_percent_encode(segment, LINK_SEGMENT).to_string()
}

/// `(directory, stem)` of a project-relative qmd path: `("a/b", "report 2")` for
/// `a/b/report 2.qmd`; the directory is `""` at the project root.
fn split_target(target_qmd_path: &str) -> (&str, &str) {
    let (dir, file) = match target_qmd_path.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", target_qmd_path),
    };
    let stem = match file.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s,
        _ => file,
    };
    (dir, stem)
}

/// The finished plan.
#[derive(Debug, Default)]
pub struct MediaPlan {
    /// Stored files, in manifest order, one per distinct project path.
    pub entries: Vec<MediaPlanEntry>,
    /// `pandoc_path` → the (percent-encoded) link target relative to the qmd.
    links: Vec<(String, String)>,
    pub diagnostics: Vec<DiagnosticMessage>,
}

impl MediaPlan {
    fn link_for(&self, pandoc_path: &str) -> Option<&str> {
        self.links
            .iter()
            .find(|(p, _)| p == pandoc_path)
            .map(|(_, l)| l.as_str())
    }
}

/// Plan where each manifest entry goes, for a qmd at `target_qmd_path`.
pub fn plan_media(manifest: &[ManifestEntry], target_qmd_path: &str) -> MediaPlan {
    let (dir, stem) = split_target(target_qmd_path);
    let media_dir = format!("{stem}_media");
    let link_dir = encode_segment(&media_dir);
    let project_dir = if dir.is_empty() {
        media_dir
    } else {
        format!("{dir}/{media_dir}")
    };

    let mut plan = MediaPlan::default();
    let mut planned: HashSet<String> = HashSet::new();
    let mut converted = 0usize;

    for entry in manifest {
        match entry {
            ManifestEntry::Stored {
                pandoc_path,
                sha256,
                ext,
                converted_from,
                conversion_failed,
            } => {
                let sha12: String = sha256.chars().take(12).collect();
                let name = if ext.is_empty() {
                    sha12
                } else {
                    format!("{sha12}.{ext}")
                };
                let project_path = format!("{project_dir}/{name}");
                plan.links.push((
                    pandoc_path.clone(),
                    format!("{link_dir}/{}", encode_segment(&name)),
                ));
                if planned.insert(project_path.clone()) {
                    plan.entries.push(MediaPlanEntry {
                        pandoc_path: pandoc_path.clone(),
                        project_path,
                    });
                }
                if converted_from.is_some() {
                    converted += 1;
                }
                if *conversion_failed {
                    plan.diagnostics
                        .push(report::image_conversion_failed(pandoc_path));
                } else if !DISPLAYABLE.contains(&ext.to_ascii_lowercase().as_str()) {
                    plan.diagnostics
                        .push(report::image_format_not_displayable(pandoc_path, ext));
                }
            }
            ManifestEntry::Skipped {
                pandoc_path, size, ..
            } => {
                // Deliberately broken (I16): the link points where the image would have gone.
                let name = report::basename(pandoc_path);
                plan.links.push((
                    pandoc_path.clone(),
                    format!("{link_dir}/{}", encode_segment(name)),
                ));
                plan.diagnostics
                    .push(report::image_skipped(pandoc_path, *size));
            }
        }
    }
    if converted > 0 {
        plan.diagnostics.push(report::images_converted(converted));
    }
    plan
}

/// Rewrite every `Image` whose target is a manifest entry's `pandoc_path`. An image under the
/// extract directory with no entry is Q-24-12 (fatal); targets elsewhere (URLs) are left alone.
pub fn rewrite_image_targets(
    pandoc: &mut Pandoc,
    plan: &MediaPlan,
) -> Result<(), DiagnosticMessage> {
    let extract_prefix = format!("{}/", extract_dir());
    let mut missing: Option<String> = None;
    let mut rewrite = |list: &mut Vec<Inline>| {
        for inline in list.iter_mut() {
            if let Inline::Image(image) = inline {
                let target = image.target.0.as_str();
                if let Some(link) = plan.link_for(target) {
                    image.target.0 = link.to_string();
                } else if target.starts_with(&extract_prefix) && missing.is_none() {
                    missing = Some(target.to_string());
                }
            }
        }
    };
    for_each_inline_list_mut(&mut pandoc.blocks, &mut rewrite);
    for_each_meta_inline_list_mut(&mut pandoc.meta, &mut rewrite);
    match missing {
        Some(path) => Err(report::internal_error(&format!(
            "the image \"{path}\" has no entry in the media manifest"
        ))),
        None => Ok(()),
    }
}

/// Parse `media_manifest_json`. A parse failure is Q-24-12.
pub fn parse_manifest(manifest_json: &str) -> Result<Vec<ManifestEntry>, DiagnosticMessage> {
    serde_json::from_str(manifest_json)
        .map_err(|e| report::internal_error(&format!("the media manifest could not be read: {e}")))
}
