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
///
/// `Brand` is generic over the color-value type `V`, mirroring Q1's
/// `BrandUnified` / `BrandSingle` split
/// (`external-sources/quarto-cli/src/core/brand/brand.ts`):
///
/// - [`UnifiedBrand`] (= `Brand<BrandColorValue>`) is the **parse
///   form**: every named color slot and typography color accepts
///   either a plain string or a `{light:, dark:}` pair. This is the
///   only form [`Brand::from_yaml_str`](crate::UnifiedBrand) produces.
/// - `Brand` (= `Brand<String>`, the default) is the **single-mode
///   form** every downstream consumer (color resolution, SCSS layer,
///   favicon/navbar) works with. It is obtained via
///   [`UnifiedBrand::split`](crate::SplitBrand), which distributes
///   each pair to its half — so the type system guarantees consumers
///   never see an unsplit light/dark value.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    deserialize = "V: serde::Deserialize<'de>",
    serialize = "V: serde::Serialize"
))]
pub struct Brand<V = String> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<BrandMeta>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<BrandColor<V>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typography: Option<BrandTypography<V>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<BrandLogo>,

    /// Freeform per-consumer defaults: `bootstrap.defaults`,
    /// `quarto.*`, `shiny.*`, etc. We keep this as a typed value rather
    /// than a strict struct because each consumer schema evolves
    /// independently and Q1 itself parses it permissively.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<BrandDefaults>,
}

/// The unified parse form of a brand: color values may be plain
/// strings or `{light:, dark:}` pairs. See [`Brand`] for the
/// unified/single distinction and [`UnifiedBrand::split`] for the way
/// down to the single-mode form.
pub type UnifiedBrand = Brand<BrandColorValue>;

// Manual `Default` (a derive would add a spurious `V: Default` bound;
// every field is an `Option`/container that defaults without one).
impl<V> Default for Brand<V> {
    fn default() -> Self {
        Self {
            meta: None,
            color: None,
            typography: None,
            logo: None,
            defaults: None,
        }
    }
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

/// A color value in the **unified** brand form: either a plain string
/// (applies to both modes) or a `{light:, dark:}` pair.
///
/// Mirrors Q1's `brand-color-light-dark` schema
/// (`external-sources/quarto-cli/src/resources/schema/definitions.yml`).
/// Only *named* color slots and typography colors accept this shape;
/// `palette` entries stay plain strings (a documented limitation
/// shared with Q1).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BrandColorValue {
    /// A plain color string; the split sends it to both halves.
    Single(String),
    /// A per-mode pair; each half of the split receives its side, and
    /// a missing side means the slot is simply absent in that half
    /// (Q1: no fallback to the other side).
    LightDark(BrandColorLightDark),
}

/// The `{light:, dark:}` pair form of [`BrandColorValue`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrandColorLightDark {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
}

impl BrandColorValue {
    /// The value the **light** half of a split receives: the plain
    /// string, or the pair's `light:` side.
    pub fn light(&self) -> Option<&str> {
        match self {
            BrandColorValue::Single(s) => Some(s.as_str()),
            BrandColorValue::LightDark(pair) => pair.light.as_deref(),
        }
    }

    /// The value the **dark** half of a split receives: the plain
    /// string, or the pair's `dark:` side.
    pub fn dark(&self) -> Option<&str> {
        match self {
            BrandColorValue::Single(s) => Some(s.as_str()),
            BrandColorValue::LightDark(pair) => pair.dark.as_deref(),
        }
    }

    /// Whether this value carries an explicit `dark:` side — the
    /// per-value input to Q1's `enablesDarkMode`: only actual `dark:`
    /// keys enable dark mode; plain strings and light-only pairs do
    /// not.
    pub fn has_dark(&self) -> bool {
        matches!(
            self,
            BrandColorValue::LightDark(BrandColorLightDark { dark: Some(_), .. })
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    deserialize = "V: serde::Deserialize<'de>",
    serialize = "V: serde::Serialize"
))]
pub struct BrandColor<V = String> {
    /// Named color aliases.  Iteration order matters — Q1 preserves
    /// source order so that downstream code generates `$brand-foo`
    /// variables in the same order they were authored.
    ///
    /// Deliberately NOT generic over `V`: palette entries stay plain
    /// strings even in the unified form (Q1's `brand-color-unified`
    /// schema keeps `palette` at `brand-color-value`; the docs carry
    /// the matching limitation callout).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<BTreeMap<String, String>>,

    // Bootstrap theme color slots
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tertiary: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub danger: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emphasis: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<V>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<V>,
}

impl<V> Default for BrandColor<V> {
    fn default() -> Self {
        Self {
            palette: None,
            primary: None,
            secondary: None,
            tertiary: None,
            success: None,
            info: None,
            warning: None,
            danger: None,
            light: None,
            dark: None,
            emphasis: None,
            link: None,
            background: None,
            foreground: None,
        }
    }
}

/// The 13 named theme-color slots, as `(name, accessor)` pairs shared
/// by the single-mode accessors, the split, and `has_dark_mode`.
/// Order matches the Bootstrap theme map (and `named_colors`).
macro_rules! for_each_named_color_slot {
    ($macro:ident) => {
        $macro!(
            primary, secondary, tertiary, success, info, warning, danger, light, dark, emphasis,
            link, background, foreground
        )
    };
}
pub(crate) use for_each_named_color_slot;

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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    deserialize = "V: serde::Deserialize<'de>",
    serialize = "V: serde::Serialize"
))]
pub struct BrandTypography<V = String> {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<BrandFont>,

    #[serde(
        default,
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub base: Option<BrandTypographyOptions<V>>,
    #[serde(
        default,
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub headings: Option<BrandTypographyOptions<V>>,
    #[serde(
        default,
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub link: Option<BrandTypographyOptions<V>>,
    #[serde(
        default,
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub monospace: Option<BrandTypographyOptions<V>>,
    #[serde(
        default,
        rename = "monospace-inline",
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub monospace_inline: Option<BrandTypographyOptions<V>>,
    #[serde(
        default,
        rename = "monospace-block",
        deserialize_with = "deserialize_typography_options",
        skip_serializing_if = "Option::is_none"
    )]
    pub monospace_block: Option<BrandTypographyOptions<V>>,
}

impl<V> Default for BrandTypography<V> {
    fn default() -> Self {
        Self {
            fonts: Vec::new(),
            base: None,
            headings: None,
            link: None,
            monospace: None,
            monospace_inline: None,
            monospace_block: None,
        }
    }
}

/// The 6 typography font slots, as a callback-macro over field names
/// (same pattern as [`for_each_named_color_slot`]); used by the split
/// and `has_dark_mode`.
macro_rules! for_each_typography_slot {
    ($macro:ident) => {
        $macro!(
            base,
            headings,
            link,
            monospace,
            monospace_inline,
            monospace_block
        )
    };
}
pub(crate) use for_each_typography_slot;

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
fn deserialize_typography_options<'de, D, V>(
    deserializer: D,
) -> Result<Option<BrandTypographyOptions<V>>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: serde::Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrOptions<V> {
        Family(String),
        Options(BrandTypographyOptions<V>),
    }

    let opt = Option::<StringOrOptions<V>>::deserialize(deserializer)?;
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
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(
    deserialize = "V: serde::Deserialize<'de>",
    serialize = "V: serde::Serialize"
))]
pub struct BrandTypographyOptions<V = String> {
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
    pub color: Option<V>,

    #[serde(
        default,
        rename = "background-color",
        skip_serializing_if = "Option::is_none"
    )]
    pub background_color: Option<V>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoration: Option<String>,
}

impl<V> Default for BrandTypographyOptions<V> {
    fn default() -> Self {
        Self {
            family: None,
            size: None,
            line_height: None,
            weight: None,
            style: None,
            color: None,
            background_color: None,
            decoration: None,
        }
    }
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
    /// If this is a single-mode logo, return the resource itself
    /// (path plus any alt text).
    ///
    /// A light/dark pair has no single resource; picking a side is
    /// deferred to the light/dark work (bd-v5z8w).
    pub fn single(&self) -> Option<&BrandLogoResource> {
        match self {
            LogoEntry::Single(r) => Some(r),
            LogoEntry::LightDark { .. } => None,
        }
    }

    /// If this is a single-mode logo, return its path.
    pub fn single_path(&self) -> Option<&str> {
        self.single().map(|r| r.path())
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
    /// paths and URLs are left unchanged.
    pub fn with_path_relative_to(&self, base: &Path) -> Self {
        let raw = self.path();
        let resolved =
            if quarto_util::is_external_url(raw) || quarto_util::is_rooted(Path::new(raw)) {
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
