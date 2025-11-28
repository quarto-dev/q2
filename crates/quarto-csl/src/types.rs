//! Core CSL (Citation Style Language) types with source tracking.
//!
//! This module defines the semantic types for CSL 1.0.2, each with associated
//! source location information for error reporting.

use quarto_source_map::SourceInfo;
use std::collections::HashMap;

/// A parsed CSL style with source tracking.
#[derive(Debug, Clone)]
pub struct Style {
    /// CSL version (e.g., "1.0").
    pub version: String,
    /// Version attribute source location.
    pub version_source: SourceInfo,

    /// Style class: "in-text" or "note".
    pub class: StyleClass,

    /// Default locale for the style (e.g., "en-US").
    pub default_locale: Option<String>,

    /// Style options (e.g., demote-non-dropping-particle).
    pub options: StyleOptions,

    /// Style info (title, author, etc.).
    pub info: Option<StyleInfo>,

    /// Locale overrides defined in the style.
    pub locales: Vec<Locale>,

    /// Macro definitions, keyed by name.
    pub macros: HashMap<String, Macro>,

    /// Citation layout.
    pub citation: Layout,

    /// Bibliography layout (optional).
    pub bibliography: Option<Layout>,

    /// Style-level name formatting options.
    pub name_options: InheritableNameOptions,

    /// Source location of the entire style element.
    pub source_info: SourceInfo,
}

/// Style class: determines citation format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleClass {
    /// In-text citations (author-date, numeric).
    InText,
    /// Note-based citations (footnotes, endnotes).
    Note,
}

/// Style-level options.
#[derive(Debug, Clone, Default)]
pub struct StyleOptions {
    /// How to handle non-dropping particles in sorting.
    pub demote_non_dropping_particle: DemoteNonDroppingParticle,
    /// Initialize names with hyphens.
    pub initialize_with_hyphen: bool,
    /// Page range format.
    pub page_range_format: Option<PageRangeFormat>,
    /// Source location.
    pub source_info: Option<SourceInfo>,
}

/// Demote non-dropping particle option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DemoteNonDroppingParticle {
    /// Never demote.
    Never,
    /// Demote for sorting only.
    #[default]
    SortOnly,
    /// Demote for display and sorting.
    DisplayAndSort,
}

/// Page range format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRangeFormat {
    Chicago,
    Expanded,
    Minimal,
    MinimalTwo,
}

/// Style metadata.
#[derive(Debug, Clone)]
pub struct StyleInfo {
    /// Style title.
    pub title: Option<String>,
    /// Short title.
    pub title_short: Option<String>,
    /// Style ID (URI).
    pub id: Option<String>,
    /// Authors.
    pub authors: Vec<Contributor>,
    /// Contributors.
    pub contributors: Vec<Contributor>,
    /// Categories.
    pub categories: Vec<Category>,
    /// Last updated timestamp.
    pub updated: Option<String>,
    /// Source location.
    pub source_info: SourceInfo,
}

/// A contributor (author or contributor).
#[derive(Debug, Clone)]
pub struct Contributor {
    pub name: Option<String>,
    pub email: Option<String>,
    pub uri: Option<String>,
}

/// Style category.
#[derive(Debug, Clone)]
pub struct Category {
    /// Citation format (author-date, numeric, note, etc.).
    pub citation_format: Option<String>,
    /// Field (science, humanities, etc.).
    pub field: Option<String>,
}

/// A locale definition with terms and date formats.
#[derive(Debug, Clone)]
pub struct Locale {
    /// Language code (e.g., "en", "en-US").
    pub lang: Option<String>,
    /// Terms defined in this locale.
    pub terms: Vec<Term>,
    /// Date formats.
    pub date_formats: Vec<DateFormat>,
    /// Source location.
    pub source_info: SourceInfo,
}

/// A term definition.
#[derive(Debug, Clone)]
pub struct Term {
    /// Term name (e.g., "and", "editor").
    pub name: String,
    /// Term form (long, short, verb, verb-short, symbol).
    pub form: TermForm,
    /// Single form of the term.
    pub single: Option<String>,
    /// Plural form of the term.
    pub multiple: Option<String>,
    /// Simple value (when single/multiple not used).
    pub value: Option<String>,
    /// Source location.
    pub source_info: SourceInfo,
}

/// Term form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TermForm {
    #[default]
    Long,
    Short,
    Verb,
    VerbShort,
    Symbol,
}

/// A date format definition.
#[derive(Debug, Clone)]
pub struct DateFormat {
    /// Date form (text, numeric).
    pub form: DateForm,
    /// Date parts.
    pub parts: Vec<DatePart>,
    /// Source location.
    pub source_info: SourceInfo,
}

/// Date form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DateForm {
    #[default]
    Text,
    Numeric,
}

/// A macro definition.
#[derive(Debug, Clone)]
pub struct Macro {
    /// Macro name.
    pub name: String,
    /// Name attribute source location.
    pub name_source: SourceInfo,
    /// Elements in this macro.
    pub elements: Vec<Element>,
    /// Source location of the entire macro element.
    pub source_info: SourceInfo,
}

/// Citation collapse mode.
///
/// Controls how citations are grouped and collapsed within a single citation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collapse {
    /// No collapsing.
    None,
    /// Collapse by citation number into ranges: "[1-3]" instead of "[1, 2, 3]".
    CitationNumber,
    /// Collapse by author, then show years: "(Smith 1900, 2000)" instead of "(Smith 1900, Smith 2000)".
    Year,
    /// Collapse by author and year, show suffixes: "(Smith 2000a, b)".
    YearSuffix,
    /// Like YearSuffix but with ranges: "(Smith 2000a-c)".
    YearSuffixRanged,
}

impl Default for Collapse {
    fn default() -> Self {
        Collapse::None
    }
}

/// A layout (for citation or bibliography).
#[derive(Debug, Clone)]
pub struct Layout {
    /// Formatting for the layout.
    pub formatting: Formatting,
    /// Delimiter between citations/entries.
    pub delimiter: Option<String>,
    /// Sort keys (for bibliography).
    pub sort: Option<Sort>,
    /// Inheritable name options (from citation/bibliography element).
    pub name_options: InheritableNameOptions,
    /// Elements in the layout.
    pub elements: Vec<Element>,
    /// Citation collapse mode (only for citation layouts).
    pub collapse: Collapse,
    /// Delimiter between items in a collapsed group.
    /// Defaults to ", " if not specified.
    pub cite_group_delimiter: Option<String>,
    /// Delimiter after a collapsed group.
    pub after_collapse_delimiter: Option<String>,
    /// Delimiter between year suffixes when collapsing.
    pub year_suffix_delimiter: Option<String>,
    /// Source location.
    pub source_info: SourceInfo,
}

/// Inheritable name formatting options.
///
/// These options can be set on style, citation, bibliography, or names elements
/// and inherit down to name elements. More specific levels override general levels.
#[derive(Debug, Clone, Default)]
pub struct InheritableNameOptions {
    /// And word/symbol.
    pub and: Option<NameAnd>,
    /// Delimiter between names.
    pub delimiter: Option<String>,
    /// Delimiter before last name.
    pub delimiter_precedes_last: Option<DelimiterPrecedesLast>,
    /// Delimiter before et-al.
    pub delimiter_precedes_et_al: Option<DelimiterPrecedesLast>,
    /// Et-al threshold.
    pub et_al_min: Option<u32>,
    /// Et-al use first.
    pub et_al_use_first: Option<u32>,
    /// Et-al use last (show last author after ellipsis).
    pub et_al_use_last: Option<bool>,
    /// Whether to initialize given names. Defaults to true.
    /// When false, given names are not broken into initials even if initialize-with is set.
    pub initialize: Option<bool>,
    /// Initialize with.
    pub initialize_with: Option<String>,
    /// Name form (long, short).
    pub form: Option<NameForm>,
    /// Name-as-sort-order (first, all).
    pub name_as_sort_order: Option<NameAsSortOrder>,
    /// Sort separator.
    pub sort_separator: Option<String>,
}

impl InheritableNameOptions {
    /// Merge two InheritableNameOptions, with `self` taking precedence over `other`.
    ///
    /// This implements the CSL inheritance model where more specific levels
    /// (name → names → layout → style) override general levels.
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            and: self.and.or(other.and),
            delimiter: self.delimiter.clone().or_else(|| other.delimiter.clone()),
            delimiter_precedes_last: self.delimiter_precedes_last.or(other.delimiter_precedes_last),
            delimiter_precedes_et_al: self
                .delimiter_precedes_et_al
                .or(other.delimiter_precedes_et_al),
            et_al_min: self.et_al_min.or(other.et_al_min),
            et_al_use_first: self.et_al_use_first.or(other.et_al_use_first),
            et_al_use_last: self.et_al_use_last.or(other.et_al_use_last),
            initialize: self.initialize.or(other.initialize),
            initialize_with: self
                .initialize_with
                .clone()
                .or_else(|| other.initialize_with.clone()),
            form: self.form.or(other.form),
            name_as_sort_order: self.name_as_sort_order.or(other.name_as_sort_order),
            sort_separator: self
                .sort_separator
                .clone()
                .or_else(|| other.sort_separator.clone()),
        }
    }

    /// Convert a Name element's options to InheritableNameOptions.
    pub fn from_name(name: &Name) -> Self {
        Self {
            and: name.and,
            delimiter: name.delimiter.clone(),
            delimiter_precedes_last: name.delimiter_precedes_last,
            delimiter_precedes_et_al: name.delimiter_precedes_et_al,
            et_al_min: name.et_al_min,
            et_al_use_first: name.et_al_use_first,
            et_al_use_last: name.et_al_use_last,
            initialize: name.initialize,
            initialize_with: name.initialize_with.clone(),
            form: name.form,
            name_as_sort_order: name.name_as_sort_order,
            sort_separator: name.sort_separator.clone(),
        }
    }
}

/// Sort specification.
#[derive(Debug, Clone)]
pub struct Sort {
    /// Sort keys.
    pub keys: Vec<SortKey>,
    /// Source location.
    pub source_info: SourceInfo,
}

/// A sort key.
#[derive(Debug, Clone)]
pub struct SortKey {
    /// Variable or macro to sort by.
    pub key: SortKeyType,
    /// Sort order.
    pub sort_order: SortOrder,
    /// Source location.
    pub source_info: SourceInfo,
}

/// Sort key type.
#[derive(Debug, Clone)]
pub enum SortKeyType {
    Variable(String),
    Macro(String),
}

/// Sort order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Ascending,
    Descending,
}

/// A CSL element (text, names, date, etc.).
#[derive(Debug, Clone)]
pub struct Element {
    /// The element type.
    pub element_type: ElementType,
    /// Formatting attributes.
    pub formatting: Formatting,
    /// Source location.
    pub source_info: SourceInfo,
}

/// Element type variants.
#[derive(Debug, Clone)]
pub enum ElementType {
    /// Text element.
    Text(TextElement),
    /// Number element.
    Number(NumberElement),
    /// Label element.
    Label(LabelElement),
    /// Names element.
    Names(NamesElement),
    /// Date element.
    Date(DateElement),
    /// Group element.
    Group(GroupElement),
    /// Choose element (conditionals).
    Choose(ChooseElement),
}

/// Text element.
#[derive(Debug, Clone)]
pub struct TextElement {
    /// Text source.
    pub source: TextSource,
}

/// Text source variants.
#[derive(Debug, Clone)]
pub enum TextSource {
    /// Variable reference.
    Variable {
        name: String,
        name_source: SourceInfo,
    },
    /// Macro reference.
    Macro {
        name: String,
        name_source: SourceInfo,
    },
    /// Term reference.
    Term {
        name: String,
        form: TermForm,
        plural: bool,
    },
    /// Literal value.
    Value { value: String },
}

/// Number element.
#[derive(Debug, Clone)]
pub struct NumberElement {
    /// Variable name.
    pub variable: String,
    /// Number form.
    pub form: NumberForm,
}

/// Number form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NumberForm {
    #[default]
    Numeric,
    Ordinal,
    LongOrdinal,
    Roman,
}

/// Label element.
#[derive(Debug, Clone)]
pub struct LabelElement {
    /// Variable name.
    pub variable: String,
    /// Label form.
    pub form: TermForm,
    /// Plural handling.
    pub plural: LabelPlural,
}

/// Label plural handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelPlural {
    #[default]
    Contextual,
    Always,
    Never,
}

/// Names element.
#[derive(Debug, Clone)]
pub struct NamesElement {
    /// Variable names (e.g., "author", "editor").
    pub variables: Vec<String>,
    /// Name formatting.
    pub name: Option<Name>,
    /// Et-al formatting.
    pub et_al: Option<EtAl>,
    /// Label for the names.
    pub label: Option<NamesLabel>,
    /// Substitute elements if names are empty.
    pub substitute: Option<Vec<Element>>,
}

/// Name formatting.
#[derive(Debug, Clone, Default)]
pub struct Name {
    /// And word/symbol.
    pub and: Option<NameAnd>,
    /// Delimiter between names.
    pub delimiter: Option<String>,
    /// Delimiter before last name.
    pub delimiter_precedes_last: Option<DelimiterPrecedesLast>,
    /// Delimiter before et-al.
    pub delimiter_precedes_et_al: Option<DelimiterPrecedesLast>,
    /// Et-al threshold.
    pub et_al_min: Option<u32>,
    /// Et-al use first.
    pub et_al_use_first: Option<u32>,
    /// Et-al use last (show last author after ellipsis).
    pub et_al_use_last: Option<bool>,
    /// Whether to initialize given names. Defaults to true.
    /// When false, given names are not broken into initials even if initialize-with is set.
    pub initialize: Option<bool>,
    /// Initialize with.
    pub initialize_with: Option<String>,
    /// Name form (long, short). None means inherit from parent level.
    pub form: Option<NameForm>,
    /// Name-as-sort-order (first, all).
    pub name_as_sort_order: Option<NameAsSortOrder>,
    /// Sort separator.
    pub sort_separator: Option<String>,
    /// Source location.
    pub source_info: Option<SourceInfo>,
}

/// Name-as-sort-order option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameAsSortOrder {
    /// Only the first name is inverted (Family, Given).
    First,
    /// All names are inverted.
    All,
}

/// Name "and" option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameAnd {
    Text,
    Symbol,
}

/// Delimiter precedes last option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DelimiterPrecedesLast {
    #[default]
    Contextual,
    Always,
    Never,
    AfterInvertedName,
}

/// Name form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NameForm {
    #[default]
    Long,
    Short,
    Count,
}

/// Et-al element.
#[derive(Debug, Clone, Default)]
pub struct EtAl {
    /// Term to use (default "et-al").
    pub term: Option<String>,
}

/// Label in names element.
#[derive(Debug, Clone)]
pub struct NamesLabel {
    /// Label form.
    pub form: TermForm,
    /// Plural handling.
    pub plural: LabelPlural,
    /// Formatting.
    pub formatting: Formatting,
    /// Source location.
    pub source_info: SourceInfo,
}

/// Date element.
#[derive(Debug, Clone)]
pub struct DateElement {
    /// Variable name.
    pub variable: String,
    /// Date form (if using localized format).
    pub form: Option<DateForm>,
    /// Which date parts to render (year, year-month, year-month-day).
    pub date_parts: DatePartsFilter,
    /// Date parts (inline definitions, used to override locale formats).
    pub parts: Vec<DatePart>,
    /// Date range delimiter.
    pub range_delimiter: Option<String>,
}

/// Which parts of a date to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DatePartsFilter {
    /// Render only year.
    Year,
    /// Render year and month.
    YearMonth,
    /// Render year, month, and day.
    #[default]
    YearMonthDay,
}

/// Date part.
#[derive(Debug, Clone)]
pub struct DatePart {
    /// Part name (year, month, day).
    pub name: DatePartName,
    /// Part form.
    pub form: Option<DatePartForm>,
    /// Formatting.
    pub formatting: Formatting,
    /// Range delimiter.
    pub range_delimiter: Option<String>,
    /// Strip periods from abbreviated months.
    pub strip_periods: bool,
    /// Source location.
    pub source_info: SourceInfo,
}

/// Date part name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatePartName {
    Year,
    Month,
    Day,
}

/// Date part form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatePartForm {
    Long,
    Short,
    Numeric,
    NumericLeadingZeros,
    Ordinal,
}

/// Group element.
#[derive(Debug, Clone)]
pub struct GroupElement {
    /// Child elements.
    pub elements: Vec<Element>,
    /// Delimiter between elements.
    pub delimiter: Option<String>,
}

/// Choose element (conditionals).
#[derive(Debug, Clone)]
pub struct ChooseElement {
    /// Branches (if, else-if, else).
    pub branches: Vec<ChooseBranch>,
}

/// A branch in a choose element.
#[derive(Debug, Clone)]
pub struct ChooseBranch {
    /// Conditions (empty for else branch).
    pub conditions: Vec<Condition>,
    /// Match type.
    pub match_type: MatchType,
    /// Elements in this branch.
    pub elements: Vec<Element>,
    /// Source location.
    pub source_info: SourceInfo,
}

/// Match type for conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchType {
    #[default]
    All,
    Any,
    None,
}

/// A condition.
#[derive(Debug, Clone)]
pub struct Condition {
    /// Condition type.
    pub condition_type: ConditionType,
    /// Source location.
    pub source_info: SourceInfo,
}

/// Condition type.
#[derive(Debug, Clone)]
pub enum ConditionType {
    /// Type matches (e.g., "book", "article").
    Type(Vec<String>),
    /// Variable exists and is non-empty.
    Variable(Vec<String>),
    /// Variable is numeric.
    IsNumeric(Vec<String>),
    /// Variable is uncertain date.
    IsUncertainDate(Vec<String>),
    /// Locator type matches.
    Locator(Vec<String>),
    /// Position matches.
    Position(Vec<Position>),
    /// Disambiguate flag.
    Disambiguate(bool),
}

/// Citation position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    First,
    Subsequent,
    IbidWithLocator,
    Ibid,
    NearNote,
}

/// Formatting attributes.
#[derive(Debug, Clone, Default)]
pub struct Formatting {
    /// Font style.
    pub font_style: Option<FontStyle>,
    /// Font variant.
    pub font_variant: Option<FontVariant>,
    /// Font weight.
    pub font_weight: Option<FontWeight>,
    /// Text decoration.
    pub text_decoration: Option<TextDecoration>,
    /// Vertical align.
    pub vertical_align: Option<VerticalAlign>,
    /// Text case.
    pub text_case: Option<TextCase>,
    /// Prefix.
    pub prefix: Option<String>,
    /// Suffix.
    pub suffix: Option<String>,
    /// Display mode.
    pub display: Option<Display>,
    /// Quotes.
    pub quotes: bool,
    /// Strip periods.
    pub strip_periods: bool,
}

/// Font style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// Font variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontVariant {
    Normal,
    SmallCaps,
}

/// Font weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontWeight {
    Normal,
    Bold,
    Light,
}

/// Text decoration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDecoration {
    None,
    Underline,
}

/// Vertical alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlign {
    Baseline,
    Sup,
    Sub,
}

/// Text case transformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCase {
    Lowercase,
    Uppercase,
    CapitalizeFirst,
    CapitalizeAll,
    Sentence,
    Title,
}

/// Display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Block,
    LeftMargin,
    RightInline,
    Indent,
}
