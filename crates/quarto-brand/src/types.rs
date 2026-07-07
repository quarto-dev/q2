//! Typed model of `_brand.yml`.
//!
//! Mirrors the Q1 schema at
//! `external-sources/quarto-cli/src/resources/schema/definitions.yml`
//! (the `brand-*` ids) — but expressed as serde-derived Rust types so
//! that `serde(deny_unknown_fields)` catches typos without needing the
//! full YAML-validation framework. When the comprehensive YAML
//! validator lands (see Phase 8 follow-ups), these types stay as the
//! canonical shape and the validator wraps them.
//!
//! Most fields are optional and `kebab-case`-renamed because that is
//! the surface YAML form. Maps preserve insertion order via
//! `serde_yaml::Mapping` only when we need to keep YAML round-trip
//! fidelity; otherwise we use `BTreeMap`/`IndexMap` per slot's needs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A reference to a brand configuration in document or project metadata.
///
/// `BrandRef` is the pre-resolution form: either a path to a
/// `_brand.yml` file or an inline YAML block. The render pipeline
/// produces this from `ConfigValue` (no I/O), then resolves it via
/// `quarto_brand::resolve_brand_ref` into a `Brand`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrandRef {
    /// A path to a `_brand.yml` file (resolved relative to the project /
    /// document directory by the caller).
    Path(PathBuf),
    /// An inline brand block, e.g. document frontmatter `brand: { color: ... }`.
    Inline(Box<serde_yaml::Value>),
}

/// A parsed `_brand.yml`.
///
/// Every section is optional — Q1 accepts a brand with just colors, or
/// just typography, etc.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Brand {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<BrandMeta>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<BrandColor>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typography: Option<BrandTypography>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<BrandLogo>,

    /// Freeform per-consumer defaults: `bootstrap.defaults`,
    /// `quarto.*`, `shiny.*`, etc. We keep this as a typed value rather
    /// than a strict struct because each consumer schema evolves
    /// independently and Q1 itself parses it permissively.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<BrandDefaults>,
}

// ── meta ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<BrandMetaName>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<BrandMetaLink>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BrandMetaName {
    Short(String),
    Detailed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        full: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        short: Option<String>,
    },
}

impl BrandMetaName {
    /// Best-effort "what is the brand called?" — returns the full name
    /// if known, otherwise the short name.
    pub fn full_name(&self) -> Option<&str> {
        match self {
            BrandMetaName::Short(s) => Some(s.as_str()),
            BrandMetaName::Detailed { full, short } => full.as_deref().or(short.as_deref()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BrandMetaLink {
    /// A single homepage URL.
    Home(String),
    /// A typed link object (home, mastodon, bluesky, github, ...).
    Detailed(BrandMetaLinks),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandMetaLinks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mastodon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bluesky: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linkedin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twitter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facebook: Option<String>,
}

// ── color ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandColor {
    /// Named color aliases.  Iteration order matters — Q1 preserves
    /// source order so that downstream code generates `$brand-foo`
    /// variables in the same order they were authored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<BTreeMap<String, String>>,

    // Bootstrap theme color slots
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tertiary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub danger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emphasis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
}

impl BrandColor {
    /// Lookup a named theme color (not the palette).
    pub fn named(&self, name: &str) -> Option<&str> {
        match name {
            "primary" => self.primary.as_deref(),
            "secondary" => self.secondary.as_deref(),
            "tertiary" => self.tertiary.as_deref(),
            "success" => self.success.as_deref(),
            "info" => self.info.as_deref(),
            "warning" => self.warning.as_deref(),
            "danger" => self.danger.as_deref(),
            "light" => self.light.as_deref(),
            "dark" => self.dark.as_deref(),
            "emphasis" => self.emphasis.as_deref(),
            "link" => self.link.as_deref(),
            "background" => self.background.as_deref(),
            "foreground" => self.foreground.as_deref(),
            _ => None,
        }
    }

    /// Set of names that count as "named theme colors" (vs palette
    /// entries or raw color values). Used by color resolution.
    pub fn is_named_theme_color(name: &str) -> bool {
        matches!(
            name,
            "primary"
                | "secondary"
                | "tertiary"
                | "success"
                | "info"
                | "warning"
                | "danger"
                | "light"
                | "dark"
                | "emphasis"
                | "link"
                | "background"
                | "foreground"
        )
    }

    /// Iterate over the named theme colors that are set, in a stable
    /// order (the order they appear in the Bootstrap theme map). Used
    /// by SCSS layer generation.
    pub fn named_colors(&self) -> impl Iterator<Item = (&'static str, &str)> {
        [
            ("primary", self.primary.as_deref()),
            ("secondary", self.secondary.as_deref()),
            ("tertiary", self.tertiary.as_deref()),
            ("success", self.success.as_deref()),
            ("info", self.info.as_deref()),
            ("warning", self.warning.as_deref()),
            ("danger", self.danger.as_deref()),
            ("light", self.light.as_deref()),
            ("dark", self.dark.as_deref()),
            ("emphasis", self.emphasis.as_deref()),
            ("link", self.link.as_deref()),
            ("background", self.background.as_deref()),
            ("foreground", self.foreground.as_deref()),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k, v)))
    }
}

// ── typography ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandTypography {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<BrandFont>,

    #[serde(
        default,
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub base: Option<BrandTypographyOptions>,
    #[serde(
        default,
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub headings: Option<BrandTypographyOptions>,
    #[serde(
        default,
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub link: Option<BrandTypographyOptions>,
    #[serde(
        default,
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub monospace: Option<BrandTypographyOptions>,
    #[serde(
        default,
        rename = "monospace-inline",
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub monospace_inline: Option<BrandTypographyOptions>,
    #[serde(
        default,
        rename = "monospace-block",
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub monospace_block: Option<BrandTypographyOptions>,
}

/// Accept either a bare string (treated as `{ family: <string> }`) or
/// a full options map. Matches Q1's `_brand.yml` shorthand:
///
/// ```yaml
/// typography:
///   base: Open Sans            # ← shorthand
///   headings:                  # ← explicit
///     family: Rubik
///     weight: 400
/// ```
fn deserialize_typography_options<'de, D>(
    deserializer: D,
) -> Result<Option<BrandTypographyOptions>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrOptions {
        Family(String),
        Options(BrandTypographyOptions),
    }

    let opt = Option::<StringOrOptions>::deserialize(deserializer)?;
    Ok(opt.map(|v| match v {
        StringOrOptions::Family(s) => BrandTypographyOptions {
            family: Some(s),
            ..Default::default()
        },
        StringOrOptions::Options(o) => o,
    }))
}

/// One font-slot's options (base / headings / link / monospace / ...).
///
/// Q1 accepts either a string (treated as `{ family: <string> }`) or a
/// map. We always normalize the string form at parse time via the
/// `untagged` enum below.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandTypographyOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,

    #[serde(
        default,
        rename = "line-height",
        skip_serializing_if = "Option::is_none"
    )]
    pub line_height: Option<serde_yaml::Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<BrandFontWeight>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<BrandFontStyle>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    #[serde(
        default,
        rename = "background-color",
        skip_serializing_if = "Option::is_none"
    )]
    pub background_color: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoration: Option<String>,
}

// The YAML can use either a bare string or a map for typography
// options; we deserialize via this helper enum then normalize. This is
// applied at the typography-slot level above by post-processing — for
// now serde sees the explicit struct form. (Q1's monospace-only test
// cases use the map form, so the bare-string convenience can land
// later if a fixture requires it.)

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum BrandFont {
    /// A Google Fonts entry.
    Google(BrandFontGoogle),
    /// A Bunny Fonts entry.
    Bunny(BrandFontGoogle), // same shape as Google
    /// A self-hosted font referenced by file path(s).
    File(BrandFontFile),
    /// A system-installed font (no import needed).
    System(BrandFontSystem),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandFontGoogle {
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<BrandFontWeight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<BrandFontStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandFontFile {
    pub family: String,
    #[serde(default)]
    pub files: Vec<BrandFontFileEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BrandFontFileEntry {
    /// A bare path string.
    Path(String),
    /// A typed file descriptor with optional weight/style.
    Explicit {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<BrandFontWeight>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<BrandFontStyle>,
    },
}

impl BrandFontFileEntry {
    pub fn path(&self) -> &str {
        match self {
            BrandFontFileEntry::Path(p) => p,
            BrandFontFileEntry::Explicit { path, .. } => path,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandFontSystem {
    pub family: String,
}

/// Font weight: either a number (100-900), a named keyword (e.g. "bold"),
/// or an array of either.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BrandFontWeight {
    Number(u32),
    Name(String),
    List(Vec<BrandFontWeightAtom>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BrandFontWeightAtom {
    Number(u32),
    Name(String),
}

/// Font style: a single value or an array.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BrandFontStyle {
    One(String),
    List(Vec<String>),
}

// ── logo ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandLogo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub small: Option<LogoEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medium: Option<LogoEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub large: Option<LogoEntry>,
    /// Named extra logos keyed by user-defined identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<BTreeMap<String, BrandLogoResource>>,
}

/// A logo entry on `small` / `medium` / `large` — either a plain
/// resource or a light/dark pair.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LogoEntry {
    /// Single-mode logo: string path or explicit { path, alt? }.
    Single(BrandLogoResource),
    /// Light/dark pair where each side is itself a resource.
    LightDark {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        light: Option<BrandLogoResource>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dark: Option<BrandLogoResource>,
    },
}

impl LogoEntry {
    /// If this is a single-mode logo, return its path.
    pub fn single_path(&self) -> Option<&str> {
        match self {
            LogoEntry::Single(r) => Some(r.path()),
            LogoEntry::LightDark { .. } => None,
        }
    }

    /// If this is a light/dark pair, return (light_path, dark_path).
    pub fn light_dark_paths(&self) -> Option<(Option<&str>, Option<&str>)> {
        match self {
            LogoEntry::LightDark { light, dark } => Some((
                light.as_ref().map(|r| r.path()),
                dark.as_ref().map(|r| r.path()),
            )),
            LogoEntry::Single(_) => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BrandLogoResource {
    /// Just a path string.
    Path(String),
    /// Typed resource with optional alt text and additional fields.
    Explicit(BrandLogoExplicit),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandLogoExplicit {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

impl BrandLogoResource {
    pub fn path(&self) -> &str {
        match self {
            BrandLogoResource::Path(p) => p,
            BrandLogoResource::Explicit(e) => &e.path,
        }
    }

    pub fn alt(&self) -> Option<&str> {
        match self {
            BrandLogoResource::Path(_) => None,
            BrandLogoResource::Explicit(e) => e.alt.as_deref(),
        }
    }

    /// Return a copy with `path` resolved relative to `base`. Absolute
    /// paths and `http(s)://` URLs are left unchanged.
    pub fn with_path_relative_to(&self, base: &Path) -> Self {
        let raw = self.path();
        let resolved = if is_external_url(raw) || quarto_util::is_rooted(Path::new(raw)) {
            raw.to_string()
        } else {
            quarto_util::to_forward_slashes(&base.join(raw))
        };
        match self {
            BrandLogoResource::Path(_) => BrandLogoResource::Path(resolved),
            BrandLogoResource::Explicit(e) => BrandLogoResource::Explicit(BrandLogoExplicit {
                path: resolved,
                alt: e.alt.clone(),
            }),
        }
    }
}

fn is_external_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("//")
}

// ── defaults ────────────────────────────────────────────────────────

/// Freeform per-consumer defaults section.
///
/// Each top-level key (`bootstrap`, `quarto`, `shiny`, …) targets a
/// specific consumer; the schema beneath each key is owned by that
/// consumer. We keep it as raw YAML for now; the SCSS layer in
/// `quarto-sass` reaches into `defaults.bootstrap.*` with its own
/// typed lens.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct BrandDefaults(pub BTreeMap<String, serde_yaml::Value>);

impl BrandDefaults {
    pub fn bootstrap(&self) -> Option<&serde_yaml::Value> {
        self.0.get("bootstrap")
    }
}
