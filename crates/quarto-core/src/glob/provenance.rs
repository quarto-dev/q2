/*
 * glob/provenance.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Which directory is a pattern relative to?
//!
//! The answer is "the directory of the file it was written in", and
//! the file is recovered from the value's provenance:
//! [`SourceInfo::root_file_id`] gives the [`FileId`](quarto_source_map::FileId)
//! the value was parsed from, and the document's [`SourceContext`]
//! maps it back to a path.
//! [`MetadataMergeStage`](crate::stage::stages::MetadataMergeStage)
//! registers the YAML metadata layers there for exactly this lookup:
//!
//! - front matter is a `Substring` into the document → the host
//!   document's directory;
//! - `blog/_metadata.yml` → `blog/`;
//! - `_quarto.yml` → the project root.
//!
//! Values with no recoverable file — runtime `--metadata`,
//! programmatic config, extension metadata — fall back to the
//! caller-supplied directory.

use std::path::Path;

use quarto_source_map::{SourceContext, SourceInfo};

/// Everything the resolver needs to turn provenance into a base
/// directory.
#[derive(Debug, Clone, Copy)]
pub struct BaseDirContext<'a> {
    /// The document's source context, when one is available.
    pub source_context: Option<&'a SourceContext>,
    /// Absolute project root, used to relativize absolute paths
    /// found in the source context.
    pub project_dir: &'a Path,
    /// Base directory for values whose declaring file cannot be
    /// recovered (project-relative, forward slashes, `""` for the
    /// root). Typically the host document's directory.
    pub fallback_dir: &'a str,
}

impl<'a> BaseDirContext<'a> {
    /// Project-relative directory of the file `source` was written
    /// in, or [`Self::fallback_dir`] when it cannot be recovered.
    pub fn base_dir_for(&self, source: &SourceInfo) -> String {
        source
            .root_file_id()
            .and_then(|id| self.source_context?.get_file(id))
            .and_then(|f| project_relative_dir_of(&f.path, self.project_dir))
            .unwrap_or_else(|| self.fallback_dir.to_string())
    }
}

/// Directory of `file_path` as a project-relative forward-slash
/// string.
///
/// Absolute paths are relativized against `project_dir`;
/// already-relative paths are taken as project-relative. Returns
/// `None` for placeholder names (`<unknown>`, empty) and for paths
/// outside the project — in both cases the caller's fallback is a
/// better answer than a wrong directory.
pub fn project_relative_dir_of(file_path: &str, project_dir: &Path) -> Option<String> {
    if file_path.is_empty() || file_path.starts_with('<') {
        return None;
    }
    let path = Path::new(file_path);
    let relative = if quarto_util::is_rooted(path) {
        path.strip_prefix(project_dir).ok()?
    } else {
        path
    };
    let dir = relative.parent().unwrap_or(Path::new(""));
    let mut segments: Vec<&str> = Vec::new();
    for comp in dir.components() {
        match comp {
            std::path::Component::Normal(os) => segments.push(os.to_str()?),
            std::path::Component::CurDir => {}
            // `..` or a root inside a supposedly project-relative
            // path — not resolvable to a project-relative dir.
            _ => return None,
        }
    }
    Some(segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{By, FileId};

    #[test]
    fn dir_of_absolute_path_inside_project() {
        let project = Path::new("/proj");
        assert_eq!(
            project_relative_dir_of("/proj/blog/_metadata.yml", project),
            Some("blog".into())
        );
        assert_eq!(
            project_relative_dir_of("/proj/_quarto.yml", project),
            Some(String::new())
        );
    }

    #[test]
    fn dir_of_relative_path_is_project_relative() {
        let project = Path::new("/proj");
        assert_eq!(
            project_relative_dir_of("sub/index.qmd", project),
            Some("sub".into())
        );
    }

    #[test]
    fn dir_of_placeholder_or_outside_is_none() {
        let project = Path::new("/proj");
        assert_eq!(project_relative_dir_of("<unknown>", project), None);
        assert_eq!(project_relative_dir_of("", project), None);
        assert_eq!(project_relative_dir_of("/elsewhere/doc.qmd", project), None);
    }

    #[test]
    fn unresolvable_provenance_falls_back() {
        let ctx = BaseDirContext {
            source_context: None,
            project_dir: Path::new("/proj"),
            fallback_dir: "sub",
        };
        let generated = SourceInfo::generated(By::programmatic_config());
        assert_eq!(ctx.base_dir_for(&generated), "sub");
    }

    #[test]
    fn frontmatter_resolves_to_the_document_dir() {
        let mut sc = SourceContext::new();
        let doc_id = sc.add_file("/proj/sub/index.qmd".to_string(), None);
        assert_eq!(doc_id, FileId(0));
        let ctx = BaseDirContext {
            source_context: Some(&sc),
            project_dir: Path::new("/proj"),
            fallback_dir: "",
        };
        // Front-matter values are Substrings into FileId(0).
        let source = SourceInfo::substring(SourceInfo::original(FileId(0), 0, 100), 10, 20);
        assert_eq!(ctx.base_dir_for(&source), "sub");
    }

    #[test]
    fn metadata_layer_resolves_to_its_own_dir() {
        let mut sc = SourceContext::new();
        sc.add_file("/proj/blog/deep/index.qmd".to_string(), None);
        let layer_id = quarto_yaml::file_id_for_filename("/proj/blog/_metadata.yml");
        sc.add_file_with_id(layer_id, "/proj/blog/_metadata.yml".to_string(), None);
        let ctx = BaseDirContext {
            source_context: Some(&sc),
            project_dir: Path::new("/proj"),
            fallback_dir: "blog/deep",
        };
        let source = SourceInfo::original(layer_id, 0, 10);
        assert_eq!(ctx.base_dir_for(&source), "blog");
    }
}
