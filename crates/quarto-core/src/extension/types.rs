/*
 * extension/types.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Extension data model types.
 */

//! Data types for Quarto extensions.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use quarto_pandoc_types::ConfigValue;

/// Identifies an extension by name and optional organization.
///
/// Examples:
/// - `ExtensionId { name: "lightbox", organization: None }`
/// - `ExtensionId { name: "acm", organization: Some("quarto-journals") }`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExtensionId {
    pub name: String,
    pub organization: Option<String>,
}

impl ExtensionId {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            organization: None,
        }
    }

    pub fn with_organization(name: impl Into<String>, org: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            organization: Some(org.into()),
        }
    }
}

impl fmt::Display for ExtensionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref org) = self.organization {
            write!(f, "{}/{}", org, self.name)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

/// A parsed and resolved Quarto extension.
#[derive(Debug, Clone)]
pub struct Extension {
    pub id: ExtensionId,
    pub title: String,
    pub author: String,
    pub version: Option<String>,
    pub quarto_required: Option<String>,
    /// Absolute path to the extension directory.
    pub path: PathBuf,
    pub contributes: Contributes,
}

/// What an extension contributes.
///
/// All path fields contain absolute paths (resolved during `read_extension`).
/// Format metadata is stored as `ConfigValue` for direct use in the merge pipeline.
#[derive(Debug, Clone, Default)]
pub struct Contributes {
    /// Format-specific metadata, keyed by format name (e.g., "html", "pdf").
    /// The "common" key has already been merged into siblings and removed.
    pub formats: HashMap<String, ConfigValue>,

    /// Top-level filter contributions (absolute paths).
    pub filters: Vec<ExtensionFilter>,

    /// Top-level shortcode contributions (absolute paths).
    pub shortcodes: Vec<PathBuf>,

    /// Raw metadata contribution (merged into project config).
    pub metadata: Option<ConfigValue>,

    /// Raw project contribution.
    pub project: Option<ConfigValue>,
}

/// A filter contributed by an extension.
#[derive(Debug, Clone)]
pub struct ExtensionFilter {
    pub path: PathBuf,
    pub at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_id_display_no_org() {
        let id = ExtensionId::new("lightbox");
        assert_eq!(id.to_string(), "lightbox");
    }

    #[test]
    fn test_extension_id_display_with_org() {
        let id = ExtensionId::with_organization("acm", "quarto-journals");
        assert_eq!(id.to_string(), "quarto-journals/acm");
    }

    #[test]
    fn test_extension_id_equality() {
        let a = ExtensionId::new("lightbox");
        let b = ExtensionId::new("lightbox");
        let c = ExtensionId::new("other");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_extension_id_with_org_equality() {
        let a = ExtensionId::with_organization("acm", "quarto-journals");
        let b = ExtensionId::with_organization("acm", "quarto-journals");
        let c = ExtensionId::with_organization("acm", "other-org");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_extension_id_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ExtensionId::new("a"));
        set.insert(ExtensionId::new("b"));
        set.insert(ExtensionId::new("a")); // duplicate
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_contributes_default() {
        let c = Contributes::default();
        assert!(c.formats.is_empty());
        assert!(c.filters.is_empty());
        assert!(c.shortcodes.is_empty());
        assert!(c.metadata.is_none());
        assert!(c.project.is_none());
    }
}
