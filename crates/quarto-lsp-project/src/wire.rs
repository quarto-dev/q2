//! The wire schema of the `quarto/*` requests (epic bd-3f6ijnw9, C4).
//!
//! `DocumentProfile`'s own serde shape is an internal cache format: it
//! changes with every `DOCUMENT_PROFILE_VERSION`, carries internal source
//! ids, and holds listings-only surfaces. Clients get [`WireProfile`]
//! instead, versioned on its own by [`WIRE_SCHEMA_VERSION`], which changes
//! only when a client would notice. Every `DocumentProfile` field is either
//! mapped here or listed in [`OMITTED_PROFILE_FIELDS`]; a test fails when a
//! new profile field is neither (plan: "Wire rules for the profile").
//!
//! Spans become [`WireLocation`]s — `{ uri, range }`, the LSP `Location`
//! shape — resolved through the analysis run's `SourceContext`, so a comment
//! written in an included file points at that file.

use std::path::Path;

use quarto_core::document_profile::DocumentProfile;
use quarto_pandoc_types::Inlines;
use quarto_source_map::{SourceContext, SourceInfo};
use serde::Serialize;

/// Bumped when a client would read a wire profile differently.
pub const WIRE_SCHEMA_VERSION: u32 = 1;

/// `DocumentProfile` fields deliberately not on the wire, with why.
pub const OMITTED_PROFILE_FIELDS: &[(&str, &str)] = &[
    (
        "profile_version",
        "internal cache format version; see WIRE_SCHEMA_VERSION",
    ),
    ("flattened_prose", "a listings diagnostic input (Q-12-26)"),
    ("alias_sources", "folded into aliases[].location"),
    (
        "resource_glob_sources",
        "resolved globs are a post-render collector input",
    ),
    (
        "resource_globs",
        "resolved globs are a post-render collector input",
    ),
    ("rejected_resources", "diagnostic internals"),
    ("categories_raw", "listings-only surface (profile contract)"),
    ("listing_item", "listings-only surface (profile contract)"),
    (
        "derived_listing",
        "listings-only surface (profile contract)",
    ),
    (
        "listing_content_globs",
        "listings-only surface (profile contract)",
    ),
];

/// `DocumentProfile` fields that [`WireProfile`] carries (under a
/// camel-cased name, sometimes reshaped).
pub const MAPPED_PROFILE_FIELDS: &[&str] = &[
    "source_path",
    "output_href",
    "format_id",
    "title",
    "subtitle",
    "description",
    "r#abstract",
    "authors",
    "authors_structured",
    "date",
    "categories",
    "keywords",
    "image",
    "draft",
    "draft_mode",
    "aliases",
    "order",
    "outline",
    "includes",
    "nav_dependencies",
    "always_render",
    "body_link_targets",
    "resources",
    "engine_resolution",
    "comments",
];

/// A zero-based position, as LSP counts it (same convention as the
/// server's diagnostics and symbols).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WirePosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WireRange {
    pub start: WirePosition,
    pub end: WirePosition,
}

/// The LSP `Location` shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WireLocation {
    pub uri: String,
    pub range: WireRange,
}

/// Prose: its plain text and its markdown (the qmd writer, as
/// `q2 get-config` prints prose values).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WireText {
    pub text: String,
    pub markdown: String,
}

impl WireText {
    fn from_inlines(inlines: &Inlines) -> Self {
        Self {
            text: quarto_pandoc_types::inlines_to_plain_text(inlines),
            markdown: pampa::config_json::inlines_to_markdown(inlines),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireOutlineEntry {
    pub title: WireText,
    pub level: i32,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<WireLocation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<WireOutlineEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireComment {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<WireLocation>,
}

/// A raw string from front matter plus where it was written.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireSourced {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<WireLocation>,
}

/// A project file, by project-relative path and URI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

/// The client-facing projection of a `DocumentProfile`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireProfile {
    /// Project-relative, forward slashes.
    pub source_path: String,
    pub output_href: String,
    pub format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<WireText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<WireText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<WireText>,
    #[serde(rename = "abstract", skip_serializing_if = "Option::is_none")]
    pub r#abstract: Option<WireText>,
    pub authors: Vec<String>,
    /// The structured author model, as the profile serializes it.
    pub authors_structured: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub draft: bool,
    pub draft_mode: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    pub aliases: Vec<WireSourced>,
    pub outline: Vec<WireOutlineEntry>,
    pub includes: Vec<WireFile>,
    pub nav_dependencies: Vec<WireFile>,
    pub always_render: bool,
    pub body_link_targets: Vec<WireFile>,
    pub resources: Vec<WireSourced>,
    pub engine_resolution: serde_json::Value,
    pub comments: Vec<WireComment>,
}

/// Maps an absolute path to the URI clients know it by.
pub(crate) type UriFor<'a> = &'a dyn Fn(&Path) -> Option<String>;

/// Resolve a span to a location, with the same end-position fallback as
/// the server's diagnostics.
pub(crate) fn location(
    info: &SourceInfo,
    ctx: &SourceContext,
    uri_for: UriFor<'_>,
) -> Option<WireLocation> {
    let start = info.map_offset(0, ctx)?;
    let end = info
        .map_offset(info.length(), ctx)
        .or_else(|| info.map_offset(info.length().saturating_sub(1), ctx))
        .unwrap_or_else(|| start.clone());
    let file = ctx.get_file(start.file_id)?;
    let uri = uri_for(Path::new(&file.path))?;
    let pos = |m: &quarto_source_map::MappedLocation| WirePosition {
        line: m.location.row as u32,
        character: m.location.column as u32,
    };
    Some(WireLocation {
        uri,
        range: WireRange {
            start: pos(&start),
            end: pos(&end),
        },
    })
}

/// Project the profile. `root` is the project directory (for
/// project-relative paths); `uri_for` maps absolute paths to URIs.
pub(crate) fn wire_profile(
    profile: &DocumentProfile,
    ctx: &SourceContext,
    root: &Path,
    uri_for: UriFor<'_>,
) -> WireProfile {
    let rel_string = |p: &Path| -> String {
        let rel = p.strip_prefix(root).unwrap_or(p);
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    };
    let file = |p: &Path| {
        let absolute = if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        };
        WireFile {
            path: rel_string(&absolute),
            uri: uri_for(&absolute),
        }
    };
    fn outline(
        entries: &[pampa::toc::TocEntry],
        ctx: &SourceContext,
        uri_for: UriFor<'_>,
    ) -> Vec<WireOutlineEntry> {
        entries
            .iter()
            .map(|e| WireOutlineEntry {
                title: WireText::from_inlines(&e.title),
                level: e.level,
                id: e.id.clone(),
                location: e
                    .source_info
                    .as_ref()
                    .and_then(|s| location(s, ctx, uri_for)),
                children: outline(&e.children, ctx, uri_for),
            })
            .collect()
    }

    WireProfile {
        source_path: rel_string(&profile.source_path),
        output_href: profile.output_href.clone(),
        format: profile.format_id.clone(),
        title: profile.title.as_ref().map(WireText::from_inlines),
        subtitle: profile.subtitle.as_ref().map(WireText::from_inlines),
        description: profile.description.as_ref().map(WireText::from_inlines),
        r#abstract: profile.r#abstract.as_ref().map(WireText::from_inlines),
        authors: profile.authors.clone(),
        authors_structured: serde_json::to_value(&profile.authors_structured).unwrap_or_default(),
        date: profile.date.clone(),
        categories: profile.categories.clone(),
        keywords: profile.keywords.clone(),
        image: profile.image.clone(),
        draft: profile.draft,
        draft_mode: serde_json::to_value(profile.draft_mode).unwrap_or_default(),
        order: profile.order,
        aliases: profile
            .aliases
            .iter()
            .enumerate()
            .map(|(i, a)| WireSourced {
                value: a.clone(),
                location: profile
                    .alias_sources
                    .get(i)
                    .and_then(|s| location(s, ctx, uri_for)),
            })
            .collect(),
        outline: outline(&profile.outline, ctx, uri_for),
        includes: profile.includes.iter().map(|i| file(&i.path)).collect(),
        nav_dependencies: profile.nav_dependencies.iter().map(|p| file(p)).collect(),
        always_render: profile.always_render,
        body_link_targets: profile.body_link_targets.iter().map(|p| file(p)).collect(),
        resources: profile
            .resources
            .iter()
            .map(|r| WireSourced {
                value: r.pattern.clone(),
                location: location(&r.source_info, ctx, uri_for),
            })
            .collect(),
        engine_resolution: serde_json::to_value(&profile.engine_resolution).unwrap_or_default(),
        comments: profile
            .comments
            .iter()
            .map(|c| WireComment {
                text: c.text.clone(),
                author: c.author.clone(),
                date: c.date.clone(),
                attributes: c.attributes.clone(),
                location: location(&c.source, ctx, uri_for),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Every `DocumentProfile` field is either mapped or deliberately
    /// omitted. A new profile field fails this test until someone decides
    /// whether clients get it (and bumps WIRE_SCHEMA_VERSION if so).
    #[test]
    fn every_profile_field_is_mapped_or_omitted() {
        let src = include_str!("../../quarto-core/src/document_profile.rs");
        let start = src
            .find("pub struct DocumentProfile {")
            .expect("struct DocumentProfile");
        let body = &src[start..];
        let end = body.find("\n}\n").expect("end of struct");
        let fields: BTreeSet<String> = body[..end]
            .lines()
            .filter_map(|l| l.strip_prefix("    pub "))
            .filter_map(|l| l.split(':').next())
            .map(str::to_string)
            .collect();
        assert!(fields.len() > 30, "parsed {} fields", fields.len());

        let mapped: BTreeSet<String> = MAPPED_PROFILE_FIELDS
            .iter()
            .map(|s| s.to_string())
            .collect();
        let omitted: BTreeSet<String> = OMITTED_PROFILE_FIELDS
            .iter()
            .map(|(f, _)| f.to_string())
            .collect();
        assert!(mapped.is_disjoint(&omitted), "both mapped and omitted");
        let known: BTreeSet<String> = mapped.union(&omitted).cloned().collect();
        let undecided: Vec<_> = fields.difference(&known).collect();
        assert!(
            undecided.is_empty(),
            "DocumentProfile fields neither mapped nor omitted on the wire: {undecided:?}"
        );
        let stale: Vec<_> = known.difference(&fields).collect();
        assert!(
            stale.is_empty(),
            "listed but no longer profile fields: {stale:?}"
        );
    }
}
