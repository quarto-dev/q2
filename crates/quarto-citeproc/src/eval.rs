//! Citation evaluation algorithm.
//!
//! This module implements the CSL evaluation algorithm that processes
//! citations and bibliography entries according to a CSL style.
//!
//! The evaluation produces an intermediate `Output` AST that preserves
//! semantic information for post-processing (disambiguation, hyperlinking, etc.),
//! then renders to the final string format.

use crate::Result;
use crate::output::{Output, Tag};
use crate::reference::Reference;
use crate::types::{Citation, CitationItem, Processor};
use quarto_csl::{
    Element, ElementType, Formatting, GroupElement, InheritableNameOptions, Layout, NamesElement,
    TextElement, TextSource,
};
use std::cell::Cell;

/// Tracks variable access for group suppression logic.
/// A group is suppressed if it calls at least one variable but all called variables are empty.
#[derive(Clone, Copy, Default)]
struct VarCount {
    /// Total number of variables called
    called: u32,
    /// Number of variables that were non-empty
    non_empty: u32,
}

/// Pre-computed context for rendering a list of names.
///
/// This struct encapsulates the merged name options and et-al computation,
/// avoiding duplication between `format_names` and `get_names_count`.
struct NameRenderingContext {
    /// Merged effective name options (from element, substitute context, and inherited).
    effective_options: InheritableNameOptions,
    /// Number of names to display (after et-al truncation and disambiguation).
    names_to_show: usize,
    /// Whether to show "et al." after the displayed names.
    show_et_al: bool,
    /// Whether et-al-use-last is enabled (show "... Name" instead of "... et al.").
    et_al_use_last: bool,
}

impl NameRenderingContext {
    /// Create a new NameRenderingContext by computing effective options and et-al truncation.
    fn new(ctx: &EvalContext, names: &[crate::reference::Name], names_el: &NamesElement) -> Self {
        // Merge name element options with inherited options (name takes precedence)
        // When inside a substitute block, also consider the parent names element's options
        let effective_options = if let Some(name) = names_el.name.as_ref() {
            // This names element has its own <name> - use it, but merge with inherited
            InheritableNameOptions::from_name(name).merge(ctx.inherited_name_options)
        } else if ctx.in_substitute {
            // Inside substitute and no <name> on this element - inherit from parent
            if let Some(ref parent_opts) = ctx.substitute_name_options {
                parent_opts.clone()
            } else {
                ctx.inherited_name_options.clone()
            }
        } else {
            ctx.inherited_name_options.clone()
        };

        // et-al handling
        // CSL spec: truncation only happens if BOTH et-al-min AND et-al-use-first are specified
        let et_al_min = effective_options.et_al_min;
        let et_al_use_first = effective_options.et_al_use_first;
        let et_al_use_last = effective_options.et_al_use_last.unwrap_or(false);

        // Check for disambiguation override of et-al-use-first
        let disamb_et_al_names = ctx
            .reference
            .disambiguation
            .as_ref()
            .and_then(|d| d.et_al_names);

        let use_et_al = match (et_al_min, et_al_use_first) {
            (Some(min), Some(_)) => names.len() as u32 >= min,
            _ => false,
        };

        let names_to_show = if let Some(disamb_count) = disamb_et_al_names {
            // Disambiguation override - show this many names (but use et-al if still truncating)
            (disamb_count as usize).min(names.len())
        } else if use_et_al {
            (et_al_use_first.unwrap_or(1) as usize).min(names.len())
        } else {
            names.len()
        };

        // Determine if we still need et-al after disambiguation override
        let show_et_al = if disamb_et_al_names.is_some() {
            // With disambiguation override, still show et-al if we're not showing all names
            names_to_show < names.len()
        } else {
            use_et_al
        };

        Self {
            effective_options,
            names_to_show,
            show_et_al,
            et_al_use_last,
        }
    }

    /// Get the delimiter between names.
    fn delimiter(&self) -> String {
        self.effective_options
            .delimiter
            .clone()
            .unwrap_or_else(|| ", ".to_string())
    }

    /// Get the "and" word (if configured).
    fn and_word(&self, ctx: &EvalContext) -> Option<String> {
        self.effective_options.and.as_ref().map(|a| match a {
            quarto_csl::NameAnd::Text => ctx
                .get_term("and", quarto_csl::TermForm::Long, false)
                .unwrap_or_else(|| "and".to_string()),
            quarto_csl::NameAnd::Symbol => "&".to_string(),
        })
    }

    /// Get delimiter-precedes-last option.
    fn delimiter_precedes_last(&self) -> quarto_csl::DelimiterPrecedesLast {
        self.effective_options
            .delimiter_precedes_last
            .unwrap_or(quarto_csl::DelimiterPrecedesLast::Contextual)
    }

    /// Get delimiter-precedes-et-al option.
    fn delimiter_precedes_et_al(&self) -> quarto_csl::DelimiterPrecedesLast {
        self.effective_options
            .delimiter_precedes_et_al
            .unwrap_or(quarto_csl::DelimiterPrecedesLast::Contextual)
    }

    /// Get name-as-sort-order option.
    fn name_as_sort_order(&self) -> Option<quarto_csl::NameAsSortOrder> {
        self.effective_options.name_as_sort_order
    }
}

/// Evaluation context for processing a single reference.
struct EvalContext<'a> {
    /// The processor (provides style, locales, references).
    processor: &'a mut Processor,
    /// The reference being processed.
    reference: &'a Reference,
    /// Inherited name options from citation/bibliography level.
    inherited_name_options: &'a InheritableNameOptions,
    /// Inherited names-delimiter from citation/bibliography level.
    /// Used as delimiter between name variable groups in <names> elements.
    inherited_names_delimiter: Option<&'a str>,
    /// Name options from parent <names> element for substitute inheritance.
    /// When inside a substitute block, this holds the name options from the
    /// parent <names> element that should be inherited by child <names>.
    substitute_name_options: Option<InheritableNameOptions>,
    /// Label from parent <names> element for substitute inheritance.
    /// When inside a substitute block, child <names> without their own label inherit this.
    substitute_label: Option<quarto_csl::NamesLabel>,
    /// Whether the parent's label appears before the name in substitute context.
    substitute_label_before_name: bool,
    /// Whether we're currently inside a substitute block.
    in_substitute: bool,
    /// Variables that have been rendered via substitute and should be quashed
    /// (not rendered again in the same citation/bibliography entry).
    substituted_variables: std::collections::HashSet<String>,
    /// Whether we're currently evaluating sort keys (not just rendering in sort order).
    /// This affects demote-non-dropping-particle="sort-only" behavior.
    in_sort_key: bool,
    // Citation item context (set when processing a citation, None for bibliography)
    /// Locator value from citation item (e.g., "42-45").
    locator: Option<&'a str>,
    /// Locator label type from citation item (e.g., "page", "chapter").
    /// If not specified, defaults to "page" when locator is present.
    locator_label: Option<&'a str>,
    /// Citation positions (for note-style citations).
    /// Multiple positions can be true simultaneously (e.g., [NearNote, Ibid, Subsequent]).
    positions: Vec<quarto_csl::Position>,
    /// Whether year suffix has been rendered for this reference.
    /// Year suffix should only appear once per reference (CSL 0.8.1 legacy mode).
    year_suffix_rendered: Cell<bool>,
    /// Variable access tracking for group suppression.
    /// A group/macro is suppressed if it calls variables but all are empty.
    var_count: VarCount,
}

impl<'a> EvalContext<'a> {
    /// Create context for bibliography entry (no citation item data).
    fn new(
        processor: &'a mut Processor,
        reference: &'a Reference,
        inherited_name_options: &'a InheritableNameOptions,
        inherited_names_delimiter: Option<&'a str>,
    ) -> Self {
        Self {
            processor,
            reference,
            inherited_name_options,
            inherited_names_delimiter,
            substitute_name_options: None,
            substitute_label: None,
            substitute_label_before_name: false,
            in_substitute: false,
            substituted_variables: std::collections::HashSet::new(),
            in_sort_key: false,
            locator: None,
            locator_label: None,
            positions: Vec::new(), // No positions in bibliography context
            year_suffix_rendered: Cell::new(false),
            var_count: VarCount::default(),
        }
    }

    /// Create context for citation item (includes locator data).
    fn with_citation_item(
        processor: &'a mut Processor,
        reference: &'a Reference,
        inherited_name_options: &'a InheritableNameOptions,
        inherited_names_delimiter: Option<&'a str>,
        citation_item: &'a CitationItem,
    ) -> Self {
        use crate::types::bitmask_to_positions;

        // Convert position from i32 (bitmask or legacy) to Vec<Position>
        let positions = match citation_item.position {
            None => vec![quarto_csl::Position::First],
            Some(v) => bitmask_to_positions(v),
        };

        Self {
            processor,
            reference,
            inherited_name_options,
            inherited_names_delimiter,
            substitute_name_options: None,
            substitute_label: None,
            substitute_label_before_name: false,
            in_substitute: false,
            substituted_variables: std::collections::HashSet::new(),
            in_sort_key: false,
            locator: citation_item.locator.as_deref(),
            locator_label: citation_item.label.as_deref(),
            positions,
            year_suffix_rendered: Cell::new(false),
            var_count: VarCount::default(),
        }
    }

    /// Record that a variable was called.
    fn record_var_call(&mut self, non_empty: bool) {
        self.var_count.called += 1;
        if non_empty {
            self.var_count.non_empty += 1;
        }
    }

    /// Get current variable count (for group suppression).
    fn get_var_count(&self) -> VarCount {
        self.var_count
    }

    /// Restore variable count (for group suppression).
    fn set_var_count(&mut self, count: VarCount) {
        self.var_count = count;
    }

    /// Get the effective formatting (merged from stack).
    /// Also handles language-aware title case: if text_case is TitleCase but
    /// the reference is non-English, title case is skipped (CSL spec 2.4.2).
    fn effective_formatting(&self, element_formatting: &Formatting) -> Formatting {
        let mut formatting = element_formatting.clone();

        // Title case only applies to English references (CSL spec 2.4.2)
        // If text_case is Title and reference is non-English, skip title case
        if formatting.text_case == Some(quarto_csl::TextCase::Title) {
            if !self.is_english_reference() {
                formatting.text_case = None;
            }
        }

        formatting
    }

    /// Check if the current reference is in English.
    /// Returns true if: language field is missing or starts with "en".
    /// Returns false if: language field is present and doesn't start with "en".
    fn is_english_reference(&self) -> bool {
        match &self.reference.language {
            Some(lang) => {
                let lang_lower = lang.to_lowercase();
                lang_lower.starts_with("en")
            }
            // No language field means assume English (default behavior)
            None => true,
        }
    }

    /// Get a term from the locale.
    fn get_term(&self, name: &str, form: quarto_csl::TermForm, plural: bool) -> Option<String> {
        self.processor.get_term(name, form, plural)
    }

    /// Get a variable value, checking citation item context first, then reference.
    ///
    /// Citation item variables (locator, label) take precedence when available.
    /// Variables that were rendered via substitute are quashed (return None).
    fn get_variable(&self, name: &str) -> Option<String> {
        // Check if this variable was rendered via substitute and should be quashed
        if self.substituted_variables.contains(name) {
            return None;
        }

        match name {
            "locator" => self.locator.map(|s| {
                // If the locator contains '&', replace it with the localized "and" symbol term
                // This follows Pandoc citeproc's handleAmpersands behavior
                if s.contains('&') {
                    if let Some(and_symbol) =
                        self.get_term("and", quarto_csl::TermForm::Symbol, false)
                    {
                        s.replace('&', &and_symbol)
                    } else {
                        s.to_string()
                    }
                } else {
                    s.to_string()
                }
            }),
            "label" => self
                .locator_label
                .or_else(|| {
                    // Default to "page" if locator is present but no label specified
                    if self.locator.is_some() {
                        Some("page")
                    } else {
                        None
                    }
                })
                .map(|s| s.to_string()),
            _ => self.reference.get_variable(name),
        }
    }

    /// Get the effective locator label for term lookup.
    ///
    /// Returns the label type (e.g., "page", "chapter") for locator term lookup.
    fn get_locator_label(&self) -> Option<&str> {
        if self.locator.is_some() {
            // Default to "page" if no explicit label
            Some(self.locator_label.unwrap_or("page"))
        } else {
            None
        }
    }

    /// Get the page-range-delimiter from the locale, defaulting to en-dash.
    ///
    /// The page-range-delimiter term specifies the delimiter used between
    /// page numbers in a range (e.g., "1–5"). If not defined in the locale,
    /// defaults to en-dash (–, U+2013).
    fn get_page_range_delimiter(&self) -> String {
        use quarto_csl::TermForm;
        // Use processor.get_term() to check style-level locale overrides first
        self.processor
            .get_term("page-range-delimiter", TermForm::Long, false)
            .unwrap_or_else(|| "\u{2013}".to_string()) // Default to en-dash
    }
}

/// Evaluate a citation and return the Output AST.
pub fn evaluate_citation_to_output(
    processor: &mut Processor,
    citation: &Citation,
) -> Result<Output> {
    evaluate_citation_to_output_impl(processor, citation, true)
}

/// Evaluate a citation WITHOUT applying collapse logic.
///
/// This is used for disambiguation detection, which needs to see each item's
/// full rendered form before any name suppression from collapsing.
/// Pandoc's citeproc runs disambiguation before collapsing, so this matches that behavior.
pub fn evaluate_citation_to_output_no_collapse(
    processor: &mut Processor,
    citation: &Citation,
) -> Result<Output> {
    evaluate_citation_to_output_impl(processor, citation, false)
}

/// Internal implementation that optionally applies collapse logic.
fn evaluate_citation_to_output_impl(
    processor: &mut Processor,
    citation: &Citation,
    apply_collapse: bool,
) -> Result<Output> {
    use quarto_csl::Collapse;

    // Clone layout to avoid borrow conflicts
    let layout = processor.style.citation.clone();
    let style_name_options = processor.style.name_options.clone();
    let delimiter = layout.delimiter.clone().unwrap_or_else(|| "; ".to_string());
    // Merge citation-level options with style-level options (citation takes precedence)
    let name_options = layout.name_options.merge(&style_name_options);

    // Assign initial citation numbers for each item (based on citation order)
    for item in &citation.items {
        processor.get_initial_citation_number(&item.id);
    }

    // Sort citation items if sort keys are defined
    let sorted_items: Vec<_> = if let Some(ref sort) = layout.sort {
        let mut items_with_keys: Vec<_> = citation
            .items
            .iter()
            .map(|item| {
                let keys = processor.compute_sort_keys(&item.id, &sort.keys);
                (item, keys)
            })
            .collect();
        items_with_keys.sort_by(|a, b| crate::types::compare_sort_keys(&a.1, &b.1));
        items_with_keys.into_iter().map(|(item, _)| item).collect()
    } else {
        citation.items.iter().collect()
    };

    let mut item_outputs = Vec::new();

    for item in sorted_items {
        let reference = processor
            .get_reference(&item.id)
            .ok_or_else(|| crate::Error::ReferenceNotFound {
                id: item.id.clone(),
                location: None,
            })?
            .clone();

        // Use with_citation_item to include locator/label from citation item
        // names_delimiter: layout (citation) > style
        let names_delimiter = layout
            .names_delimiter
            .clone()
            .or_else(|| processor.style.names_delimiter.clone());
        let mut ctx = EvalContext::with_citation_item(
            processor,
            &reference,
            &name_options,
            names_delimiter.as_deref(),
            item,
        );
        let output = evaluate_layout(&mut ctx, &layout)?;

        // Apply prefix/suffix from citation item
        // Prefixes/suffixes may contain CSL rich text (quotes, formatting)
        // See: https://github.com/jgm/citeproc (addFormatting uses parseCslJson for affixes)
        // If there's no prefix, capitalize the first letter of the output (sentence-initial capitalization)
        let mut parts = Vec::new();
        if let Some(ref prefix) = item.prefix {
            // Parse prefix for CSL rich text (quotes, HTML markup)
            let parsed_prefix = crate::output::parse_csl_rich_text(prefix);
            // Only add separator space if prefix doesn't already end with whitespace
            let needs_space = !prefix.ends_with(' ') && !prefix.ends_with('\t');
            let prefix_with_sep = if needs_space {
                Output::sequence(vec![parsed_prefix, Output::literal(" ")])
            } else {
                parsed_prefix
            };
            parts.push(Output::tagged(Tag::Prefix, prefix_with_sep));

            // Per Pandoc citeproc: In note styles, if the prefix ends with a sentence-ending
            // pattern and has more than one word (i.e., it's a complete sentence), capitalize
            // the first letter of the citation output. This handles cases like:
            //   prefix: ". He said \"Please work.\" " -> "Ibid." (not "ibid.")
            // Check if prefix ends with sentence-ending punctuation followed by optional
            // quote(s) and trailing space.
            let is_note_style = processor.style.class == quarto_csl::StyleClass::Note;
            let ends_with_sentence = {
                let trimmed = prefix.trim_end();
                // Check last char(s) after trimming trailing whitespace
                trimmed.ends_with('.')
                    || trimmed.ends_with('!')
                    || trimmed.ends_with('?')
                    // Also check for quote after punctuation (e.g., '."' or ".'" or "."")
                    || trimmed.ends_with(".\"")
                    || trimmed.ends_with(".'")
                    || trimmed.ends_with("!\u{201D}") // !"
                    || trimmed.ends_with(".\u{201D}") // ."
                    || trimmed.ends_with("?\u{201D}") // ?"
                    || trimmed.ends_with("!\u{2019}") // !'
                    || trimmed.ends_with(".\u{2019}") // .'
                    || trimmed.ends_with("?\u{2019}") // ?'
            };
            let has_multiple_words = prefix.split_whitespace().count() > 1;

            if is_note_style && ends_with_sentence && has_multiple_words {
                parts.push(output.capitalize_first());
            } else {
                parts.push(output);
            }
        } else {
            // No prefix - do NOT capitalize. Capitalization only happens
            // when prefix ends with sentence-ending punctuation (handled above).
            // Per Pandoc's citeproc: items without prefix are NOT capitalized.
            parts.push(output);
        }
        if let Some(ref suffix) = item.suffix {
            // Parse suffix for CSL rich text (quotes, HTML markup)
            let parsed_suffix = crate::output::parse_csl_rich_text(suffix);
            // Only add separator space if suffix doesn't already start with whitespace or punctuation
            // Punctuation (., ,) should not have a space before it - this is important for
            // punctuation-in-quote processing.
            let first_char = suffix.chars().next();
            let needs_space = !suffix.starts_with(' ')
                && !suffix.starts_with('\t')
                && first_char != Some('.')
                && first_char != Some(',');
            let suffix_with_sep = if needs_space {
                Output::sequence(vec![Output::literal(" "), parsed_suffix])
            } else {
                parsed_suffix
            };
            parts.push(Output::tagged(Tag::Suffix, suffix_with_sep));
        }

        // Determine citation item type based on author_only and suppress_author flags
        let item_type = if item.author_only == Some(true) {
            crate::output::CitationItemType::AuthorOnly
        } else if item.suppress_author == Some(true) {
            crate::output::CitationItemType::SuppressAuthor
        } else {
            crate::output::CitationItemType::NormalCite
        };

        // Wrap with Tag::Item for disambiguation
        let tagged_output = Output::tagged(
            Tag::Item {
                item_type,
                item_id: item.id.clone(),
            },
            Output::sequence(parts),
        );

        item_outputs.push(tagged_output);
    }

    // Apply collapse logic based on collapse mode (unless disabled for disambiguation)
    let combined = if apply_collapse {
        match layout.collapse {
            Collapse::Year | Collapse::YearSuffix | Collapse::YearSuffixRanged => {
                collapse_by_year(item_outputs, &layout, layout.collapse.clone())
            }
            Collapse::CitationNumber => collapse_by_citation_number(item_outputs, &layout),
            Collapse::None => {
                // Even without collapse mode, cite-group-delimiter groups same-author citations
                if layout.cite_group_delimiter.is_some() {
                    group_by_author_with_delimiter(item_outputs, &layout)
                } else {
                    Output::formatted_with_delimiter(Formatting::default(), item_outputs, &delimiter)
                }
            }
        }
    } else {
        // No collapsing - just join items with delimiter (for disambiguation detection)
        Output::formatted_with_delimiter(Formatting::default(), item_outputs, &delimiter)
    };

    // Apply layout-level formatting
    Ok(Output::formatted(layout.formatting.clone(), vec![combined]))
}

/// Collapse citations by author name (year collapse).
///
/// Groups items by author name (including non-adjacent items) and suppresses repeated names.
/// "(Smith 1900, Jones 2000, Smith 2001)" becomes "(Smith 1900, 2001; Jones 2000)"
///
/// This implements Pandoc's groupWith algorithm which:
/// 1. Takes the first item
/// 2. Scans remaining items for matches (same author)
/// 3. Pulls ALL matching items into the current group
/// 4. Recurses on the remaining non-matching items
///
/// Items with suffix tags are not moved (they become single-item groups).
///
/// For YearSuffix and YearSuffixRanged collapse modes, this also collapses
/// same-year items within each author group, showing only the suffix for
/// subsequent items with the same year.
fn collapse_by_year(
    item_outputs: Vec<Output>,
    layout: &quarto_csl::Layout,
    collapse_mode: quarto_csl::Collapse,
) -> Output {
    use quarto_csl::Collapse;

    if item_outputs.is_empty() {
        return Output::Null;
    }

    let delimiter = layout.delimiter.clone().unwrap_or_else(|| "; ".to_string());
    // cite-group-delimiter defaults to ", " if not specified, BUT for year-suffix
    // collapsing logic, we need to know if it was explicitly set
    let cite_group_delimiter = layout
        .cite_group_delimiter
        .clone()
        .unwrap_or_else(|| ", ".to_string());
    // For year groups within an author group, use cite-group-delimiter if set,
    // otherwise fall back to layout delimiter
    let year_group_delimiter = layout
        .cite_group_delimiter
        .clone()
        .unwrap_or_else(|| delimiter.clone());
    // year-suffix-delimiter fallback chain (per Pandoc citeproc):
    // 1. year-suffix-delimiter attribute
    // 2. cite-group-delimiter attribute
    // 3. layout delimiter
    let year_suffix_delimiter = layout
        .year_suffix_delimiter
        .clone()
        .or_else(|| layout.cite_group_delimiter.clone())
        .unwrap_or_else(|| delimiter.clone());

    // Group items by author name using Pandoc's groupWith algorithm
    let groups = group_by_names(item_outputs);

    // For each author group, suppress names and optionally collapse year suffixes
    let group_outputs: Vec<Output> = groups
        .into_iter()
        .map(|group| {
            // First, suppress names on all but the first item
            let items_with_suppressed_names: Vec<Output> = group
                .into_iter()
                .enumerate()
                .map(|(i, output)| {
                    if i == 0 {
                        output
                    } else {
                        output.suppress_names()
                    }
                })
                .collect();

            // For year-suffix modes, also collapse same-year items
            match collapse_mode {
                Collapse::YearSuffix | Collapse::YearSuffixRanged => {
                    let use_ranges = matches!(collapse_mode, Collapse::YearSuffixRanged);
                    collapse_year_suffixes(
                        items_with_suppressed_names,
                        &cite_group_delimiter,
                        &year_group_delimiter,
                        &year_suffix_delimiter,
                        use_ranges,
                    )
                }
                _ => Output::formatted_with_delimiter(
                    Formatting::default(),
                    items_with_suppressed_names,
                    &cite_group_delimiter,
                ),
            }
        })
        .collect();

    Output::formatted_with_delimiter(Formatting::default(), group_outputs, &delimiter)
}

/// Collapse same-year items within an author group to show only year suffixes.
///
/// For items like "2000a, 2000b, 2000c, 2001", this produces "2000a,b,c, 2001".
/// The first item in each year group keeps the full date, subsequent items
/// show only their year suffix.
///
/// `cite_group_delimiter` - delimiter when no year-suffix collapse happens
/// `year_group_delimiter` - delimiter between year groups when collapse happens
/// `year_suffix_delimiter` - delimiter between suffixes within same year (e.g., ",")
fn collapse_year_suffixes(
    items: Vec<Output>,
    cite_group_delimiter: &str,
    year_group_delimiter: &str,
    year_suffix_delimiter: &str,
    use_ranges: bool,
) -> Output {
    if items.is_empty() {
        return Output::Null;
    }

    // Group items by their date text (year), ignoring the year suffix for comparison.
    // This allows collapsing "2000a, 2000b" even when the suffix is embedded in the date.
    let mut year_groups: Vec<Vec<Output>> = Vec::new();
    let mut current_group: Vec<Output> = Vec::new();
    let mut current_date: Option<String> = None;
    let mut had_collapse = false; // Track if any year-suffix collapse happened

    for item in items {
        // Use date without suffix for comparison so "2000a" and "2000b" are considered same year
        let date = item.extract_date_text_without_suffix();

        if current_group.is_empty() {
            current_date = date;
            current_group.push(item);
        } else if date == current_date && date.is_some() {
            // Same year - extract only the year suffix for this item
            current_group.push(item.extract_year_suffix_only());
            had_collapse = true; // Year-suffix collapse happened
        } else {
            // Different year - flush current group and start new one
            year_groups.push(current_group);
            current_date = date;
            current_group = vec![item];
        }
    }

    // Don't forget the last group
    if !current_group.is_empty() {
        year_groups.push(current_group);
    }

    // Apply ranging if requested (for YearSuffixRanged)
    let formatted_groups: Vec<Output> = year_groups
        .into_iter()
        .map(|group| {
            if use_ranges && group.len() >= 3 {
                // Check if we can make a range (consecutive suffixes)
                maybe_range_year_suffixes(group, year_suffix_delimiter)
            } else {
                Output::formatted_with_delimiter(
                    Formatting::default(),
                    group,
                    year_suffix_delimiter,
                )
            }
        })
        .collect();

    // Per Pandoc: when year-suffix collapse happens, use year_group_delimiter (layout delimiter)
    // When no collapse happens (all items have different years), use cite_group_delimiter
    let final_delimiter = if had_collapse {
        year_group_delimiter
    } else {
        cite_group_delimiter
    };
    Output::formatted_with_delimiter(Formatting::default(), formatted_groups, final_delimiter)
}

/// Try to create ranges for consecutive year suffixes.
///
/// Groups consecutive suffixes into ranges where 3+ consecutive become a range:
/// "a,b,c,d,e" becomes "a–e"
/// "a,c,d,e" becomes "a,c–e" (a is isolated, c,d,e is a range)
fn maybe_range_year_suffixes(items: Vec<Output>, delimiter: &str) -> Output {
    if items.is_empty() {
        return Output::Null;
    }

    // Extract year suffix values alongside items
    let items_with_suffixes: Vec<(Output, Option<i32>)> = items
        .into_iter()
        .map(|o| {
            let suffix = o.extract_year_suffix();
            (o, suffix)
        })
        .collect();

    // Group into consecutive runs
    let mut runs: Vec<Vec<Output>> = Vec::new();
    let mut current_run: Vec<Output> = Vec::new();
    let mut prev_suffix: Option<i32> = None;

    for (output, suffix) in items_with_suffixes {
        match (prev_suffix, suffix) {
            (None, s) => {
                // First item
                current_run.push(output);
                prev_suffix = s;
            }
            (Some(prev), Some(curr)) if curr == prev + 1 => {
                // Consecutive - extend current run
                current_run.push(output);
                prev_suffix = Some(curr);
            }
            _ => {
                // Not consecutive - flush current run and start new one
                if !current_run.is_empty() {
                    runs.push(current_run);
                }
                current_run = vec![output];
                prev_suffix = suffix;
            }
        }
    }

    // Don't forget the last run
    if !current_run.is_empty() {
        runs.push(current_run);
    }

    // Convert runs to output: 3+ consecutive become ranges, others stay as-is
    let formatted_runs: Vec<Output> = runs
        .into_iter()
        .map(|run| {
            if run.len() >= 3 {
                // Create a range: first–last
                let mut iter = run.into_iter();
                let first = iter.next().unwrap();
                let last = iter.last().unwrap();
                Output::sequence(vec![first, Output::literal("–"), last])
            } else {
                // Too short for range, join with delimiter
                Output::formatted_with_delimiter(Formatting::default(), run, delimiter)
            }
        })
        .collect();

    Output::formatted_with_delimiter(Formatting::default(), formatted_runs, delimiter)
}

/// Group items by author name, collecting non-adjacent same-author items.
///
/// This implements Pandoc's groupWith algorithm:
/// - Items with suffix tags become single-item groups (not moved)
/// - First item in a group can have a prefix, but subsequent items must not have prefix/suffix
/// - Same-author items are collected into groups even if not originally adjacent
fn group_by_names(items: Vec<Output>) -> Vec<Vec<Output>> {
    fn group_recursive(mut items: Vec<Output>) -> Vec<Vec<Output>> {
        if items.is_empty() {
            return Vec::new();
        }

        let first = items.remove(0);

        // If first item has a suffix, it becomes its own group
        if first.has_suffix_tag() {
            let mut rest = group_recursive(items);
            rest.insert(0, vec![first]);
            return rest;
        }

        // Find items that can be grouped (no prefix/suffix) and match the first item's names
        let first_names = first.extract_names_text();

        // Separate items into:
        // - those that can potentially be grouped (no prefix or suffix)
        // - those that cannot (have prefix or suffix) - these stay in order
        let (groupable, rest): (Vec<_>, Vec<_>) = items
            .into_iter()
            .partition(|item| !item.has_prefix_tag() && !item.has_suffix_tag());

        // From groupable items, find those with matching names
        let (matching, non_matching): (Vec<_>, Vec<_>) = groupable
            .into_iter()
            .partition(|item| item.extract_names_text() == first_names && first_names.is_some());

        // Build the group: first item + all matching items
        let mut group = vec![first];
        group.extend(matching);

        // Recurse on non-matching items followed by items with prefix/suffix
        let remaining: Vec<_> = non_matching.into_iter().chain(rest).collect();
        let mut groups = vec![group];
        groups.extend(group_recursive(remaining));
        groups
    }

    group_recursive(items)
}

/// Group citations by author name using cite-group-delimiter (without collapse).
///
/// Unlike `collapse_by_year`, this does NOT suppress repeated names.
/// It just groups consecutive same-author citations and uses cite-group-delimiter
/// between items in a group, and the main delimiter between groups.
///
/// "Smith 2000; Smith 2001; Jones 2002" becomes "Smith 2000, Smith 2001; Jones 2002"
/// (with cite-group-delimiter=", " and delimiter="; ")
fn group_by_author_with_delimiter(item_outputs: Vec<Output>, layout: &quarto_csl::Layout) -> Output {
    if item_outputs.is_empty() {
        return Output::Null;
    }

    let delimiter = layout.delimiter.clone().unwrap_or_else(|| "; ".to_string());
    let cite_group_delimiter = layout
        .cite_group_delimiter
        .clone()
        .unwrap_or_else(|| ", ".to_string());

    // Group consecutive items by their names text
    let mut groups: Vec<Vec<Output>> = Vec::new();
    let mut current_group: Vec<Output> = Vec::new();
    let mut current_names: Option<String> = None;

    for output in item_outputs {
        let names = output.extract_names_text();

        if current_group.is_empty() {
            // First item in a group
            current_names = names;
            current_group.push(output);
        } else if names == current_names && names.is_some() {
            // Same author, add to current group (but DON'T suppress names)
            current_group.push(output);
        } else {
            // Different author, start a new group
            groups.push(current_group);
            current_names = names;
            current_group = vec![output];
        }
    }

    // Don't forget the last group
    if !current_group.is_empty() {
        groups.push(current_group);
    }

    // Join items within each group with cite-group-delimiter
    // Join groups with the main delimiter
    let group_outputs: Vec<Output> = groups
        .into_iter()
        .map(|group| {
            Output::formatted_with_delimiter(Formatting::default(), group, &cite_group_delimiter)
        })
        .collect();

    Output::formatted_with_delimiter(Formatting::default(), group_outputs, &delimiter)
}

/// Collapse citations by citation number (numeric collapse with ranges).
///
/// "[1, 2, 3, 5, 6, 7]" becomes "[1-3, 5-7]"
fn collapse_by_citation_number(item_outputs: Vec<Output>, layout: &quarto_csl::Layout) -> Output {
    if item_outputs.is_empty() {
        return Output::Null;
    }

    let delimiter = layout.delimiter.clone().unwrap_or_else(|| ", ".to_string());

    // Extract citation numbers and their associated outputs
    let mut numbered_items: Vec<(i32, Output)> = Vec::new();
    for output in item_outputs {
        if let Some(num) = output.extract_citation_number() {
            numbered_items.push((num, output));
        } else {
            // No citation number - can't collapse, just include as-is
            numbered_items.push((-1, output));
        }
    }

    // Find consecutive ranges
    let mut result_outputs: Vec<Output> = Vec::new();
    let mut range_start: Option<(i32, Output)> = None;
    let mut range_end: Option<(i32, Output)> = None;

    for (num, output) in numbered_items {
        if num == -1 {
            // Non-numbered item, flush any range and add this item
            if let Some((start_num, start_output)) = range_start.take() {
                if let Some((end_num, end_output)) = range_end.take() {
                    if end_num - start_num >= 2 {
                        // Create a range output: render start–end
                        result_outputs.push(create_range_output(start_output, end_output));
                    } else {
                        // Too short for range, add separately
                        result_outputs.push(start_output);
                        result_outputs.push(end_output);
                    }
                } else {
                    result_outputs.push(start_output);
                }
            }
            result_outputs.push(output);
            continue;
        }

        match (range_start.as_ref(), range_end.as_ref()) {
            (None, _) => {
                // Start a new potential range
                range_start = Some((num, output));
                range_end = None;
            }
            (Some((start_num, _)), None) => {
                if num == start_num + 1 {
                    // Consecutive, extend range
                    range_end = Some((num, output));
                } else {
                    // Not consecutive, flush start and begin new range
                    let (_, start_output) = range_start.take().unwrap();
                    result_outputs.push(start_output);
                    range_start = Some((num, output));
                }
            }
            (Some(_), Some((end_num, _))) => {
                if num == end_num + 1 {
                    // Extend range
                    range_end = Some((num, output));
                } else {
                    // Not consecutive, flush range and begin new range
                    let (start_num_val, start_output) = range_start.take().unwrap();
                    let (end_num_val, end_output) = range_end.take().unwrap();
                    if end_num_val - start_num_val >= 2 {
                        result_outputs.push(create_range_output(start_output, end_output));
                    } else {
                        result_outputs.push(start_output);
                        result_outputs.push(end_output);
                    }
                    range_start = Some((num, output));
                }
            }
        }
    }

    // Flush any remaining range
    if let Some((start_num, start_output)) = range_start {
        if let Some((end_num, end_output)) = range_end {
            if end_num - start_num >= 2 {
                result_outputs.push(create_range_output(start_output, end_output));
            } else {
                result_outputs.push(start_output);
                result_outputs.push(end_output);
            }
        } else {
            result_outputs.push(start_output);
        }
    }

    Output::formatted_with_delimiter(Formatting::default(), result_outputs, &delimiter)
}

/// Create a range output from start and end outputs.
/// For citation numbers, this renders as "[1]–[7]" from "[1]" and "[7]".
fn create_range_output(start: Output, end: Output) -> Output {
    Output::sequence(vec![start, Output::literal("–"), end])
}

/// Evaluate a bibliography entry and return formatted output as a String.
pub fn evaluate_bibliography_entry(
    processor: &mut Processor,
    reference: &Reference,
) -> Result<String> {
    let output = evaluate_bibliography_entry_to_output(processor, reference)?;
    Ok(output.render())
}

/// Evaluate a bibliography entry and return the Output AST.
pub fn evaluate_bibliography_entry_to_output(
    processor: &mut Processor,
    reference: &Reference,
) -> Result<Output> {
    // Clone layout to avoid borrow conflicts
    let layout = processor
        .style
        .bibliography
        .clone()
        .expect("bibliography layout required");
    let style_name_options = processor.style.name_options.clone();

    // Merge bibliography-level options with style-level options (bibliography takes precedence)
    let name_options = layout.name_options.merge(&style_name_options);
    // names_delimiter: layout (bibliography) > style
    let names_delimiter = layout
        .names_delimiter
        .clone()
        .or_else(|| processor.style.names_delimiter.clone());
    let mut ctx = EvalContext::new(
        processor,
        reference,
        &name_options,
        names_delimiter.as_deref(),
    );
    let output = evaluate_layout(&mut ctx, &layout)?;

    // Apply second-field-align transformation if enabled
    // When second-field-align is used, the layout suffix needs to be moved INSIDE
    // the right-inline div, not applied to the outer wrapper.
    // See: display_SecondFieldAlignMigratePunctuation.txt
    let (output, formatting) = if layout.second_field_align.is_some() {
        let aligned = apply_second_field_align(output, layout.formatting.suffix.as_deref());
        // Remove suffix from formatting since it's now inside right-inline
        let mut formatting = layout.formatting.clone();
        formatting.suffix = None;
        (aligned, formatting)
    } else {
        (output, layout.formatting.clone())
    };

    // Apply layout-level formatting
    Ok(Output::formatted(formatting, vec![output]))
}

/// Apply second-field-align transformation to a bibliography entry output.
///
/// This takes the first element of the entry and wraps it with `Display::LeftMargin`,
/// then wraps all remaining elements with `Display::RightInline`.
///
/// This creates the two-column layout used in styles like IEEE where the citation
/// number is in a left margin column and the rest of the content is inline.
///
/// The `layout_suffix` parameter is the suffix from the layout formatting (e.g., ".")
/// which needs to be moved INSIDE the right-inline div rather than wrapping everything.
/// See: display_SecondFieldAlignMigratePunctuation.txt
fn apply_second_field_align(output: Output, layout_suffix: Option<&str>) -> Output {
    use quarto_csl::Formatting;

    // Extract the children from the output
    let children = match &output {
        Output::Formatted { children, .. } => children.clone(),
        Output::Tagged { child, .. } => {
            // If tagged, look inside
            return Output::Tagged {
                tag: match &output {
                    Output::Tagged { tag, .. } => tag.clone(),
                    _ => unreachable!(),
                },
                child: Box::new(apply_second_field_align(*child.clone(), layout_suffix)),
            };
        }
        _ => return output, // Nothing to split
    };

    if children.is_empty() {
        return output;
    }

    // Find the first non-null child
    let mut first_idx = None;
    for (i, child) in children.iter().enumerate() {
        if !child.is_null() {
            first_idx = Some(i);
            break;
        }
    }

    let Some(first_idx) = first_idx else {
        return output; // All children are null
    };

    // Split into first element and rest
    let first = children[first_idx].clone();
    let rest: Vec<_> = children[first_idx + 1..]
        .iter()
        .filter(|c| !c.is_null())
        .cloned()
        .collect();

    // Wrap first element with Display::LeftMargin
    let left_margin = Output::Formatted {
        formatting: Formatting {
            display: Some(quarto_csl::Display::LeftMargin),
            ..Formatting::default()
        },
        children: vec![first],
    };

    // Wrap rest with Display::RightInline (only if there's content)
    if rest.is_empty() {
        left_margin
    } else {
        // Apply the layout suffix to the right-inline formatting
        let right_inline = Output::Formatted {
            formatting: Formatting {
                display: Some(quarto_csl::Display::RightInline),
                suffix: layout_suffix.map(|s| s.to_string()),
                ..Formatting::default()
            },
            children: rest,
        };

        // Return a sequence containing both
        Output::Formatted {
            formatting: Formatting::default(),
            children: vec![left_margin, right_inline],
        }
    }
}

/// Evaluate a macro for sorting purposes.
///
/// This evaluates the macro's elements and returns the plain text result
/// (stripped of formatting) for use as a sort key.
///
/// The `sort_key` parameter provides name formatting overrides (names-min,
/// names-use-first, names-use-last) that affect all names generated via macros.
pub fn evaluate_macro_for_sort(
    processor: &Processor,
    reference: &Reference,
    elements: &[Element],
    sort_key: &quarto_csl::SortKey,
) -> Result<String> {
    // Create a temporary mutable processor for evaluation
    // This is a bit awkward but necessary due to the EvalContext design
    let mut temp_processor = Processor::new(processor.style.clone());
    temp_processor.add_reference(reference.clone());
    // Copy citation numbers so macros that use citation-number work correctly
    temp_processor.copy_initial_citation_numbers(processor);

    let style_name_options = temp_processor.style.name_options.clone();
    let layout_name_options = temp_processor
        .style
        .bibliography
        .as_ref()
        .map(|b| b.name_options.clone())
        .unwrap_or_default();

    // Build name options from sort key overrides.
    // These map to et-al-min, et-al-use-first, et-al-use-last.
    let sort_key_name_options = quarto_csl::InheritableNameOptions {
        et_al_min: sort_key.names_min,
        et_al_use_first: sort_key.names_use_first,
        et_al_use_last: sort_key.names_use_last,
        ..Default::default()
    };

    // Merge name options: sort_key > layout > style (higher priority first)
    let name_options = sort_key_name_options.merge(&layout_name_options.merge(&style_name_options));

    // Get names_delimiter for sort key evaluation: bibliography > style
    let names_delimiter = temp_processor
        .style
        .bibliography
        .as_ref()
        .and_then(|b| b.names_delimiter.clone())
        .or_else(|| temp_processor.style.names_delimiter.clone());

    let mut ctx = EvalContext::new(
        &mut temp_processor,
        reference,
        &name_options,
        names_delimiter.as_deref(),
    );
    // Mark this as a sort key evaluation so demote-non-dropping-particle works correctly
    ctx.in_sort_key = true;
    let output = evaluate_elements(&mut ctx, elements, "")?;

    Ok(output.render())
}

/// Evaluate a layout (citation or bibliography).
///
/// Note: The layout delimiter is for joining citation items, not elements within
/// a single layout evaluation. Elements within a layout are concatenated without
/// a delimiter. The layout delimiter is applied at a higher level when combining
/// the results of multiple citation items.
fn evaluate_layout(ctx: &mut EvalContext, layout: &Layout) -> Result<Output> {
    evaluate_elements(ctx, &layout.elements, "")
}

/// Evaluate a sequence of elements.
fn evaluate_elements(
    ctx: &mut EvalContext,
    elements: &[Element],
    delimiter: &str,
) -> Result<Output> {
    let mut outputs = Vec::new();

    for element in elements {
        let output = evaluate_element(ctx, element)?;
        if !output.is_null() {
            outputs.push(output);
        }
    }

    // Use formatted_with_delimiter for smart punctuation handling
    Ok(Output::formatted_with_delimiter(
        Formatting::default(),
        outputs,
        delimiter,
    ))
}

/// Evaluate a single element.
fn evaluate_element(ctx: &mut EvalContext, element: &Element) -> Result<Output> {
    let formatting = ctx.effective_formatting(&element.formatting);

    let output = match &element.element_type {
        ElementType::Text(text_el) => evaluate_text(ctx, text_el, &formatting)?,
        ElementType::Names(names_el) => evaluate_names(ctx, names_el, &formatting)?,
        ElementType::Group(group_el) => evaluate_group(ctx, group_el, &formatting)?,
        ElementType::Choose(choose_el) => evaluate_choose(ctx, choose_el)?,
        ElementType::Number(num_el) => evaluate_number(ctx, num_el, &formatting)?,
        ElementType::Label(label_el) => evaluate_label(ctx, label_el, &formatting)?,
        ElementType::Date(date_el) => evaluate_date(ctx, date_el, &formatting)?,
    };

    // Apply formatting (prefix/suffix are part of formatting)
    if output.is_null() {
        Ok(Output::Null)
    } else {
        Ok(Output::formatted(formatting, vec![output]))
    }
}

/// Evaluate a text element.
fn evaluate_text(
    ctx: &mut EvalContext,
    text_el: &TextElement,
    _formatting: &Formatting,
) -> Result<Output> {
    let output = match &text_el.source {
        TextSource::Variable { name, form, .. } => {
            // Special handling for citation-number
            if name == "citation-number" {
                let result = if let Some(num) = ctx.processor.get_citation_number(&ctx.reference.id)
                {
                    // Tag for collapse detection
                    Output::tagged(Tag::CitationNumber(num), Output::literal(num.to_string()))
                } else {
                    Output::Null
                };
                ctx.record_var_call(!result.is_null());
                result
            } else if name == "year-suffix" {
                // Year suffix from disambiguation (1=a, 2=b, etc.)
                // NOTE: year-suffix does NOT count as a variable for group suppression purposes
                // (per Haskell citeproc: "we don't update var count here; this doesn't count as a variable")
                if let Some(suffix) = ctx
                    .reference
                    .disambiguation
                    .as_ref()
                    .and_then(|d| d.year_suffix)
                {
                    let letter = suffix_to_letter(suffix);
                    Output::tagged(Tag::YearSuffix(suffix), Output::literal(letter))
                } else {
                    Output::Null
                }
            } else if name == "citation-label" {
                // Citation label needs year suffix appended (like in Pandoc citeproc)
                // Get the base label (either from data or generated)
                let base_label = ctx.get_variable("citation-label");
                ctx.record_var_call(base_label.is_some());

                if let Some(label) = base_label {
                    // Get year suffix if present
                    let suffix_output = ctx
                        .reference
                        .disambiguation
                        .as_ref()
                        .and_then(|d| d.year_suffix)
                        .map(|suffix| {
                            let letter = suffix_to_letter(suffix);
                            Output::tagged(Tag::YearSuffix(suffix), Output::literal(letter))
                        });

                    let label_output = Output::literal(label);

                    // Combine base label with year suffix
                    if let Some(suffix) = suffix_output {
                        Output::sequence(vec![label_output, suffix])
                    } else {
                        label_output
                    }
                } else {
                    Output::Null
                }
            } else {
                // For short form, try {name}-short first, then fall back to {name}
                // Note: journalAbbreviation is handled as an alias for container-title-short
                // at parse time (see Reference struct), so no special case needed here.
                let value = if *form == quarto_csl::VariableForm::Short {
                    let short_name = format!("{}-short", name);
                    ctx.get_variable(&short_name)
                        .or_else(|| ctx.get_variable(name))
                } else {
                    ctx.get_variable(name)
                };

                // Record variable call for group suppression
                ctx.record_var_call(value.is_some());

                if let Some(value) = value {
                    // Mark variable as substituted if we're in substitute context
                    // This prevents the variable from being rendered again
                    if ctx.in_substitute {
                        ctx.substituted_variables.insert(name.to_string());
                        // Also mark the short form variant if we used it
                        if *form == quarto_csl::VariableForm::Short {
                            ctx.substituted_variables.insert(format!("{}-short", name));
                        }
                    }

                    // Special handling for page-like variables: format ranges with en-dash
                    // and optional abbreviation. This applies to:
                    // - "page" variable: full formatting with page-range-format
                    // - "locator" variable with page label: same as page
                    // - "locator" variable with non-page label: en-dash only, no abbreviation
                    //
                    // Pandoc citeproc always calls formatPageRange for locators, just with
                    // Nothing for page-range-format when the label isn't "page".
                    let is_page_like = name == "page"
                        || (name == "locator"
                            && ctx.get_locator_label().map_or(true, |l| l == "page"));

                    if name == "page" || name == "locator" {
                        let delimiter = ctx.get_page_range_delimiter();
                        // Only apply page-range-format abbreviation for page-like locators
                        let page_range_format = if is_page_like {
                            ctx.processor.style.options.page_range_format
                        } else {
                            None
                        };
                        let formatted = format_page_range(&value, &delimiter, page_range_format);
                        Output::tagged(Tag::Locator, crate::output::parse_csl_rich_text(&formatted))
                    } else {
                        // Parse CSL rich text (HTML-like markup) from the value
                        let parsed = crate::output::parse_csl_rich_text(&value);
                        // Tag title for potential hyperlinking
                        if name == "title" {
                            Output::tagged(Tag::Title, parsed)
                        } else {
                            parsed
                        }
                    }
                } else {
                    Output::Null
                }
            }
        }
        TextSource::Macro { name, .. } => {
            // Look up and evaluate the macro
            // Macros use group suppression: if the macro calls variables but all are empty,
            // the entire macro output is suppressed.
            if let Some(macro_def) = ctx.processor.style.macros.get(name).cloned() {
                let old_var_count = ctx.get_var_count();
                let delimiter = "".to_string();
                let result = evaluate_elements(ctx, &macro_def.elements, &delimiter)?;
                let new_var_count = ctx.get_var_count();

                // Check if macro should be suppressed:
                // - It called at least one variable (new_called > old_called)
                // - But none were non-empty (new_non_empty == old_non_empty)
                let vars_called = new_var_count.called > old_var_count.called;
                let all_empty = new_var_count.non_empty == old_var_count.non_empty;

                if vars_called && all_empty {
                    // Suppress the macro - restore var count and return null
                    ctx.set_var_count(old_var_count);
                    Output::Null
                } else {
                    // Macro is non-empty - treat it as a non-empty variable for parent group
                    if !result.is_null() {
                        ctx.record_var_call(true);
                    }
                    result
                }
            } else {
                Output::Null
            }
        }
        TextSource::Term { name, form, plural } => {
            if let Some(term) = ctx.get_term(name, *form, *plural) {
                Output::tagged(Tag::Term(name.clone()), Output::literal(term))
            } else {
                Output::Null
            }
        }
        TextSource::Value { value } => crate::output::parse_csl_rich_text(value),
    };

    Ok(output)
}

/// Evaluate a names element.
fn evaluate_names(
    ctx: &mut EvalContext,
    names_el: &NamesElement,
    _formatting: &Formatting,
) -> Result<Output> {
    // Check if form="count" is set - consider both explicit and inherited form
    // In substitute context, inherit form from parent names element
    let explicit_form = names_el.name.as_ref().and_then(|n| n.form);
    let inherited_form = ctx.substitute_name_options.as_ref().and_then(|opts| opts.form);

    let is_count_form = if let Some(form) = explicit_form {
        // Explicit form on this element
        form == quarto_csl::NameForm::Count
    } else if ctx.in_substitute {
        // No explicit form - check inherited from substitute context
        inherited_form
            .map(|f| f == quarto_csl::NameForm::Count)
            .unwrap_or(false)
    } else {
        false
    };

    // Step 1: Check if ALL primary variables are empty
    // We need to record var calls for group suppression regardless
    let mut all_empty = true;
    for var in &names_el.variables {
        let names = ctx.reference.get_names(var);
        let has_names = names.as_ref().map_or(false, |n| !n.is_empty());
        ctx.record_var_call(has_names);
        if has_names {
            all_empty = false;
        }
    }

    // Step 2: If all variables are empty, try substitute
    // (The substitute inherits form="count" through substitute_name_options)
    if all_empty {
        return evaluate_names_substitute(ctx, names_el);
    }

    // Step 3: We have names - handle form="count" or format normally
    if is_count_form {
        // For form="count", sum counts across all variables
        let mut total_count: usize = 0;
        for var in &names_el.variables {
            if let Some(names) = ctx.reference.get_names(var) {
                if !names.is_empty() {
                    total_count += get_names_count(ctx, names, names_el);
                }
            }
        }
        // Output the total count (empty string if 0)
        if total_count > 0 {
            return Ok(Output::literal(total_count.to_string()));
        } else {
            return Ok(Output::Null);
        }
    }

    // Step 4: Format names normally (non-count form)
    // Note: var calls were already recorded in step 1
    let mut var_outputs: Vec<Output> = Vec::new();
    for var in &names_el.variables {
        if let Some(names) = ctx.reference.get_names(var) {
            if !names.is_empty() {
                // Format the names - now returns structured Output
                let formatted = format_names(ctx, names, names_el);

                // Apply the <name> element's formatting (prefix, suffix, etc.) if present
                let formatted = if let Some(ref name_el) = names_el.name {
                    if let Some(ref fmt) = name_el.formatting {
                        Output::formatted(fmt.clone(), vec![formatted])
                    } else {
                        formatted
                    }
                } else {
                    formatted
                };

                let names_output = Output::tagged(
                    Tag::Names {
                        variable: var.clone(),
                        names: names.to_vec(),
                    },
                    formatted,
                );

                // Check for label - use the element's own label, or inherit from parent
                // in substitute context if no local label is defined
                let (effective_label, effective_label_before_name) =
                    if let Some(ref label) = names_el.label {
                        (Some(label.clone()), names_el.label_before_name)
                    } else if ctx.in_substitute {
                        // Inherit label from parent <names> in substitute context
                        (
                            ctx.substitute_label.clone(),
                            ctx.substitute_label_before_name,
                        )
                    } else {
                        (None, false)
                    };

                let var_with_label = if let Some(ref label) = effective_label {
                    // Determine plural based on name count
                    let is_plural = match label.plural {
                        quarto_csl::LabelPlural::Always => true,
                        quarto_csl::LabelPlural::Never => false,
                        quarto_csl::LabelPlural::Contextual => names.len() > 1,
                    };

                    // Look up the term for this variable (e.g., "editor" -> "Ed." term)
                    if let Some(term) = ctx.get_term(var, label.form, is_plural) {
                        // Apply formatting to term only (not prefix/suffix)
                        // Prefix/suffix should be rendered outside the formatting
                        let mut term_formatting = label.formatting.clone();
                        let prefix = term_formatting.prefix.take();
                        let suffix = term_formatting.suffix.take();

                        // Build the label output: prefix + formatted_term + suffix
                        let formatted_term = if term_formatting.has_any_formatting() {
                            Output::formatted(term_formatting, vec![Output::literal(term)])
                        } else {
                            Output::literal(term)
                        };

                        let label_output = if prefix.is_some() || suffix.is_some() {
                            let mut parts = Vec::new();
                            if let Some(p) = prefix {
                                parts.push(Output::literal(p));
                            }
                            parts.push(formatted_term);
                            if let Some(s) = suffix {
                                parts.push(Output::literal(s));
                            }
                            Output::sequence(parts)
                        } else {
                            formatted_term
                        };

                        // Combine label + names or names + label based on CSL order
                        if effective_label_before_name {
                            Output::sequence(vec![label_output, names_output])
                        } else {
                            Output::sequence(vec![names_output, label_output])
                        }
                    } else {
                        names_output
                    }
                } else {
                    names_output
                };

                var_outputs.push(var_with_label);
            }
        }
    }

    // We get here only when all_empty=false, so var_outputs should have content
    // Join with the delimiter between name variable groups.
    // Priority: <names delimiter="..."> > inherited names-delimiter > ""
    let delimiter = names_el
        .delimiter
        .as_deref()
        .or(ctx.inherited_names_delimiter)
        .unwrap_or("");
    Ok(crate::output::join_outputs(var_outputs, delimiter))
}

/// Try substitute elements when primary name variables are empty.
/// This is called when all primary variables in a <names> element have no names.
fn evaluate_names_substitute(
    ctx: &mut EvalContext,
    names_el: &NamesElement,
) -> Result<Output> {
    let Some(ref substitute) = names_el.substitute else {
        return Ok(Output::Null);
    };

    // Save the current substitute context
    let prev_substitute_options = ctx.substitute_name_options.clone();
    let prev_substitute_label = ctx.substitute_label.clone();
    let prev_substitute_label_before_name = ctx.substitute_label_before_name;
    let prev_in_substitute = ctx.in_substitute;

    // Set up substitute context with the parent names element's options
    // so that child <names> elements can inherit name formatting (including form="count")
    let parent_options = if let Some(name) = names_el.name.as_ref() {
        InheritableNameOptions::from_name(name).merge(ctx.inherited_name_options)
    } else {
        ctx.inherited_name_options.clone()
    };
    ctx.substitute_name_options = Some(parent_options);
    // Store the parent's label for inheritance by child <names> elements
    ctx.substitute_label = names_el.label.clone();
    ctx.substitute_label_before_name = names_el.label_before_name;
    ctx.in_substitute = true;

    let mut result = Output::Null;
    for element in substitute {
        let sub_output = evaluate_element(ctx, element)?;
        if !sub_output.is_null() {
            result = sub_output;
            break;
        }
    }

    // Restore previous substitute context
    ctx.substitute_name_options = prev_substitute_options;
    ctx.substitute_label = prev_substitute_label;
    ctx.substitute_label_before_name = prev_substitute_label_before_name;
    ctx.in_substitute = prev_in_substitute;

    // Wrap the substitute output in Tag::Names with empty names list.
    // This is needed for subsequent-author-substitute to work: the feature
    // detects Names tags and replaces them with the substitute string.
    // Using the first variable from the names element as the variable name.
    if result.is_null() {
        Ok(result)
    } else {
        let variable = names_el.variables.first().cloned().unwrap_or_default();
        Ok(Output::tagged(
            Tag::Names {
                variable,
                names: vec![],
            },
            result,
        ))
    }
}

/// Get the count of names that would be rendered (after et-al truncation).
/// Used when `form="count"` is specified on the name element.
fn get_names_count(
    ctx: &EvalContext,
    names: &[crate::reference::Name],
    names_el: &NamesElement,
) -> usize {
    let name_ctx = NameRenderingContext::new(ctx, names, names_el);
    name_ctx.names_to_show
}

/// Format a list of names according to CSL rules.
fn format_names(
    ctx: &EvalContext,
    names: &[crate::reference::Name],
    names_el: &NamesElement,
) -> Output {
    use quarto_csl::{DelimiterPrecedesLast, NameAsSortOrder};

    // Use NameRenderingContext for option resolution and et-al computation
    let name_ctx = NameRenderingContext::new(ctx, names, names_el);

    // Extract values we need from the context
    let delimiter = name_ctx.delimiter();
    let and_word = name_ctx.and_word(ctx);
    let delimiter_precedes_last = name_ctx.delimiter_precedes_last();
    let delimiter_precedes_et_al = name_ctx.delimiter_precedes_et_al();
    let name_as_sort_order = name_ctx.name_as_sort_order();
    let names_to_show = name_ctx.names_to_show;
    let show_et_al = name_ctx.show_et_al;
    let et_al_use_last = name_ctx.et_al_use_last;

    // Format individual names, tracking which ones are actually inverted
    // Literal names are never inverted even with name-as-sort-order="all"
    let mut formatted_names: Vec<Output> = Vec::new();
    let mut is_inverted: Vec<bool> = Vec::new();

    // Get disambiguation hints for names
    let disamb = ctx.reference.disambiguation.as_ref();

    // Get name-part formatting from the Name element (if present)
    let family_formatting = names_el
        .name
        .as_ref()
        .and_then(|n| n.family_formatting.as_ref());
    let given_formatting = names_el
        .name
        .as_ref()
        .and_then(|n| n.given_formatting.as_ref());

    // Get demote-non-dropping-particle option from style
    let demote_ndp = ctx.processor.style.options.demote_non_dropping_particle;

    // Get initialize-with-hyphen option from style (defaults to true)
    let init_with_hyphen = ctx.processor.style.options.initialize_with_hyphen;

    for (i, n) in names.iter().take(names_to_show).enumerate() {
        // Literal names are never inverted
        let is_literal = n.literal.is_some();
        // Per Haskell citeproc: when computing sort keys (in_sort_key=true),
        // names are always rendered in sort order (family first), regardless of name-as-sort-order
        let inverted = if is_literal {
            false
        } else if ctx.in_sort_key {
            true
        } else {
            match name_as_sort_order {
                Some(NameAsSortOrder::All) => true,
                Some(NameAsSortOrder::First) => i == 0,
                None => false,
            }
        };
        // Check if there's a disambiguation hint for this name
        let is_primary = i == 0;
        let formatted_name = format_single_name(
            n,
            &name_ctx.effective_options,
            inverted,
            ctx.in_sort_key,
            disamb,
            is_primary,
            family_formatting,
            given_formatting,
            demote_ndp,
            init_with_hyphen,
        );
        // Wrap each name with Tag::Name for subsequent-author-substitute support.
        // This allows the substitution logic to identify and replace individual names.
        formatted_names.push(Output::tagged(Tag::Name(n.clone()), formatted_name));
        is_inverted.push(inverted);
    }

    // Helper to check if we should include delimiter before last/et-al
    // For AfterInvertedName, we need to check if the second-to-last name was actually inverted
    let should_include_delimiter = |rule: DelimiterPrecedesLast, count: usize| -> bool {
        match rule {
            DelimiterPrecedesLast::Always => true,
            DelimiterPrecedesLast::Never => false,
            DelimiterPrecedesLast::Contextual => count >= 3, // Only with 3+ names
            DelimiterPrecedesLast::AfterInvertedName => {
                // Include only if the second-to-last name was actually inverted
                // (not a literal name)
                if count >= 2 && is_inverted.len() >= count - 1 {
                    is_inverted[count - 2]
                } else {
                    false
                }
            }
        }
    };

    // Don't use "and" connector when truncating with et-al
    // The "and" is only for joining the last two names when showing ALL names
    // Exception: et-al-use-last still uses the ellipsis format, not "and"
    let use_and_connector = !show_et_al;

    // Build the output - we construct an Output tree that preserves structure
    // while producing the same rendered result as before
    let mut result_parts: Vec<Output> = Vec::new();

    if formatted_names.is_empty() {
        // No names - return null
        return Output::Null;
    } else if formatted_names.len() == 1 {
        result_parts.push(formatted_names.into_iter().next().unwrap());
    } else if formatted_names.len() == 2 {
        let mut iter = formatted_names.into_iter();
        let first = iter.next().unwrap();
        let second = iter.next().unwrap();

        if use_and_connector {
            if let Some(ref and) = and_word {
                let use_delim = should_include_delimiter(delimiter_precedes_last, 2);
                if use_delim {
                    // "Name1, and Name2"
                    result_parts.push(first);
                    result_parts.push(Output::literal(format!(
                        "{} {} ",
                        delimiter.trim_end(),
                        and
                    )));
                    result_parts.push(second);
                } else {
                    // "Name1 and Name2"
                    result_parts.push(first);
                    result_parts.push(Output::literal(format!(" {} ", and)));
                    result_parts.push(second);
                }
            } else {
                // No "and" - just delimiter: "Name1, Name2"
                result_parts.push(first);
                result_parts.push(Output::literal(delimiter.clone()));
                result_parts.push(second);
            }
        } else {
            // Not using "and" connector (et-al truncation) - just delimiter
            result_parts.push(first);
            result_parts.push(Output::literal(delimiter.clone()));
            result_parts.push(second);
        }
    } else {
        // 3+ names
        let last_idx = formatted_names.len() - 1;
        let mut iter = formatted_names.into_iter().enumerate();

        while let Some((i, name_output)) = iter.next() {
            if i == last_idx {
                // Last name
                if use_and_connector {
                    if let Some(ref and) = and_word {
                        let use_delim =
                            should_include_delimiter(delimiter_precedes_last, last_idx + 1);
                        if use_delim {
                            result_parts.push(Output::literal(format!(
                                "{} {} ",
                                delimiter.trim_end(),
                                and
                            )));
                        } else {
                            result_parts.push(Output::literal(format!(" {} ", and)));
                        }
                    } else {
                        result_parts.push(Output::literal(delimiter.clone()));
                    }
                } else {
                    result_parts.push(Output::literal(delimiter.clone()));
                }
                result_parts.push(name_output);
            } else {
                // Not the last name
                result_parts.push(name_output);
                if i < last_idx - 1 {
                    // Add delimiter between non-last names
                    result_parts.push(Output::literal(delimiter.clone()));
                }
            }
        }
    }

    // Handle et-al
    if show_et_al {
        if et_al_use_last && names.len() > names_to_show {
            // Show ellipsis and last name: "A, B, … Z"
            // The last name is not primary for disambiguation purposes
            let last_name = format_single_name(
                &names[names.len() - 1],
                &name_ctx.effective_options,
                name_as_sort_order == Some(NameAsSortOrder::All),
                ctx.in_sort_key,
                disamb,
                false, // not primary
                family_formatting,
                given_formatting,
                demote_ndp,
                init_with_hyphen,
            );
            let use_delim = should_include_delimiter(delimiter_precedes_et_al, names_to_show + 1);
            if use_delim {
                result_parts.push(Output::literal(format!("{} … ", delimiter.trim_end())));
            } else {
                result_parts.push(Output::literal(" … ".to_string()));
            }
            result_parts.push(last_name);
        } else {
            // Regular et al.
            let et_al_term = ctx
                .get_term("et-al", quarto_csl::TermForm::Long, false)
                .unwrap_or_else(|| "et al.".to_string());

            // Only add et-al output if the term is not empty
            // (CSL allows overriding et-al to empty string to suppress it)
            if !et_al_term.is_empty() {
                // Get formatting from the <et-al> element if present
                let et_al_formatting = names_el
                    .et_al
                    .as_ref()
                    .and_then(|ea| ea.formatting.clone())
                    .unwrap_or_default();

                // Create the formatted et-al output
                let et_al_output =
                    Output::formatted(et_al_formatting, vec![Output::literal(et_al_term)]);

                let use_delim =
                    should_include_delimiter(delimiter_precedes_et_al, names_to_show + 1);
                if use_delim {
                    // Add delimiter and space before et-al
                    result_parts.push(Output::literal(format!("{} ", delimiter.trim_end())));
                    result_parts.push(et_al_output);
                } else {
                    // Just add space before et-al
                    result_parts.push(Output::literal(" ".to_string()));
                    result_parts.push(et_al_output);
                }
            }
        }
    }

    Output::sequence(result_parts)
}

/// Check if a string ends with a character that should NOT be followed by a space.
/// Per Haskell citeproc: apostrophe, curly apostrophe, hyphen, en-dash, or non-breaking space.
/// Used when joining non-dropping particle with family name.
fn ends_with_particle_punct(s: &str) -> bool {
    s.chars().last().map_or(false, |c| {
        c == '\'' || c == '\u{2019}' || c == '-' || c == '\u{2013}' || c == '\u{00a0}'
    })
}

/// Format a single name, returning structured Output.
///
/// If `inverted` is true, format as "Family, Given" (sort order).
/// Otherwise, format as "Given Family" (display order).
///
/// If disambiguation data is provided and contains a hint for this name,
/// the form and initialization may be overridden to show given names.
///
/// This returns an `Output` AST rather than a plain string, enabling
/// per-name-part formatting (e.g., uppercase family names) in future phases.
fn format_single_name(
    name: &crate::reference::Name,
    options: &quarto_csl::InheritableNameOptions,
    inverted: bool,
    in_sort_key: bool,
    disamb: Option<&crate::reference::DisambiguationData>,
    is_primary: bool,
    family_formatting: Option<&Formatting>,
    given_formatting: Option<&Formatting>,
    demote_non_dropping_particle: quarto_csl::DemoteNonDroppingParticle,
    initialize_with_hyphen: bool,
) -> Output {
    use crate::reference::NameHint;
    use quarto_csl::DemoteNonDroppingParticle;

    // Handle literal names
    // For literal names, apply family_formatting (Pandoc treats literal as "family-only")
    if let Some(ref lit) = name.literal {
        let base = Output::literal(lit.clone());
        return if let Some(fmt) = family_formatting {
            Output::formatted(fmt.clone(), vec![base])
        } else {
            base
        };
    }

    // Look up disambiguation hint for this name
    let name_key = name
        .family
        .clone()
        .or_else(|| name.literal.clone())
        .unwrap_or_default();
    let hint = disamb.and_then(|d| d.name_hints.get(&name_key));

    // Determine effective form based on disambiguation hint
    let base_form = options.form.unwrap_or_default();
    let (form, force_no_initialize) = match hint {
        Some(NameHint::AddInitials) => {
            // Switch to long form (shows given name as initials)
            (quarto_csl::NameForm::Long, false)
        }
        Some(NameHint::AddGivenName) => {
            // Switch to long form AND don't initialize (show full given name)
            (quarto_csl::NameForm::Long, true)
        }
        Some(NameHint::AddInitialsIfPrimary) if is_primary => {
            // Only expand if this is the primary (first) name
            (quarto_csl::NameForm::Long, false)
        }
        Some(NameHint::AddGivenNameIfPrimary) if is_primary => {
            // Only expand if this is the primary (first) name
            (quarto_csl::NameForm::Long, true)
        }
        _ => (base_form, false),
    };

    let initialize_with = options.initialize_with.clone();
    let sort_separator = options
        .sort_separator
        .clone()
        .unwrap_or_else(|| ", ".to_string());

    match form {
        quarto_csl::NameForm::Short => {
            // Short form: family name only (non-dropping particle + family)
            let mut parts: Vec<Output> = Vec::new();
            if let Some(ref ndp) = name.non_dropping_particle {
                parts.push(Output::literal(ndp.clone()));
            }
            if let Some(ref family) = name.family {
                parts.push(Output::literal(family.clone()));
            }
            // Join with space delimiter, unless ndp ends with particle punctuation
            // Per Haskell citeproc: no space after apostrophe, hyphen, etc.
            let delimiter = if name
                .non_dropping_particle
                .as_ref()
                .map_or(false, |ndp| ends_with_particle_punct(ndp))
            {
                ""
            } else if name.is_byzantine() {
                " "
            } else {
                ""
            };
            let base = Output::formatted_with_delimiter(Formatting::default(), parts, delimiter);
            // Apply family_formatting if specified (Short form only shows family name)
            if let Some(fmt) = family_formatting {
                Output::formatted(fmt.clone(), vec![base])
            } else {
                base
            }
        }
        quarto_csl::NameForm::Long | quarto_csl::NameForm::Count => {
            // Determine if non-dropping particle should be demoted (moved from family to given)
            // Per Haskell citeproc, demote decision depends on both option and context:
            // - Never: particle stays with family name always
            // - SortOnly: demote ONLY when computing sort keys (in_sort_key=true), not during display
            //   Note: "name-as-sort-order" makes names render inverted, but that's different from
            //   actually computing sort keys. inSortKey is true only during sort key evaluation.
            // - DisplayAndSort: demote in both display and sort contexts (when inverted OR in sort key)
            let demote_particle = match demote_non_dropping_particle {
                DemoteNonDroppingParticle::Never => false,
                DemoteNonDroppingParticle::SortOnly => in_sort_key,
                DemoteNonDroppingParticle::DisplayAndSort => inverted || in_sort_key,
            };

            // Split family_formatting into font styling (for individual elements) and affixes (for wrapper)
            // This matches Haskell citeproc's approach: familyFormatting vs familyAffixes
            let (family_font_styling, family_affixes): (Option<Formatting>, Option<Formatting>) =
                if let Some(fmt) = family_formatting {
                    // Font styling: everything except prefix/suffix
                    let font_styling = Formatting {
                        prefix: None,
                        suffix: None,
                        ..fmt.clone()
                    };
                    // Affixes: only prefix/suffix
                    let affixes = if fmt.prefix.is_some() || fmt.suffix.is_some() {
                        Some(Formatting {
                            prefix: fmt.prefix.clone(),
                            suffix: fmt.suffix.clone(),
                            ..Formatting::default()
                        })
                    } else {
                        None
                    };
                    // Only include font_styling if it has actual styling
                    let has_font_styling = font_styling.font_style.is_some()
                        || font_styling.font_weight.is_some()
                        || font_styling.font_variant.is_some()
                        || font_styling.text_decoration.is_some()
                        || font_styling.vertical_align.is_some()
                        || font_styling.text_case.is_some()
                        || font_styling.display.is_some()
                        || font_styling.quotes
                        || font_styling.strip_periods;
                    (
                        if has_font_styling {
                            Some(font_styling)
                        } else {
                            None
                        },
                        affixes,
                    )
                } else {
                    (None, None)
                };

            // Build family "core" part (non-dropping particle + family name)
            // If demoting, non-dropping particle is NOT included
            // Apply font styling to individual parts, but DON'T apply affixes yet
            // Affixes are applied differently for inverted vs display order
            let family_core: Option<Output> = {
                let mut fp: Vec<Output> = Vec::new();
                if !demote_particle {
                    if let Some(ref ndp) = name.non_dropping_particle {
                        let base = Output::literal(ndp.clone());
                        let formatted = if let Some(ref fmt) = family_font_styling {
                            Output::formatted(fmt.clone(), vec![base])
                        } else {
                            base
                        };
                        fp.push(formatted);
                    }
                }
                if let Some(ref family) = name.family {
                    let base = Output::literal(family.clone());
                    let formatted = if let Some(ref fmt) = family_font_styling {
                        Output::formatted(fmt.clone(), vec![base])
                    } else {
                        base
                    };
                    fp.push(formatted);
                }
                if fp.is_empty() {
                    None
                } else {
                    // Determine delimiter: no space if ndp ends with particle punctuation
                    // Per Haskell citeproc: apostrophe, hyphen, etc. aren't followed by space
                    let delimiter = if !demote_particle
                        && name
                            .non_dropping_particle
                            .as_ref()
                            .map_or(false, |ndp| ends_with_particle_punct(ndp))
                    {
                        ""
                    } else if name.is_byzantine() {
                        " "
                    } else {
                        ""
                    };
                    Some(Output::formatted_with_delimiter(
                        Formatting::default(),
                        fp,
                        delimiter,
                    ))
                }
            };

            // For inverted order, family_part = family_core wrapped with affixes (ndp + family only)
            // For display order, we'll build a larger unit before applying affixes
            let family_part_for_inverted: Option<Output> = family_core.clone().map(|core| {
                if let Some(ref affixes) = family_affixes {
                    Output::formatted(affixes.clone(), vec![core])
                } else {
                    core
                }
            });

            // Split given_formatting into font styling (for individual elements) and affixes (for wrapper)
            // This matches Haskell citeproc's approach: givenFormatting vs givenAffixes
            let (given_font_styling, given_affixes): (Option<Formatting>, Option<Formatting>) =
                if let Some(fmt) = given_formatting {
                    // Font styling: everything except prefix/suffix
                    let font_styling = Formatting {
                        prefix: None,
                        suffix: None,
                        ..fmt.clone()
                    };
                    // Affixes: only prefix/suffix
                    let affixes = if fmt.prefix.is_some() || fmt.suffix.is_some() {
                        Some(Formatting {
                            prefix: fmt.prefix.clone(),
                            suffix: fmt.suffix.clone(),
                            ..Formatting::default()
                        })
                    } else {
                        None
                    };
                    // Only include font_styling if it has actual styling
                    let has_font_styling = font_styling.font_style.is_some()
                        || font_styling.font_weight.is_some()
                        || font_styling.font_variant.is_some()
                        || font_styling.text_decoration.is_some()
                        || font_styling.vertical_align.is_some()
                        || font_styling.text_case.is_some()
                        || font_styling.display.is_some()
                        || font_styling.quotes
                        || font_styling.strip_periods;
                    (
                        if has_font_styling {
                            Some(font_styling)
                        } else {
                            None
                        },
                        affixes,
                    )
                } else {
                    (None, None)
                };

            // Build given part (possibly initialized)
            // CSL rule: if a name has only a given name (no family name), don't initialize it
            // because that given name IS their name (e.g., "Banksy", "Cher")
            // force_no_initialize is set by disambiguation hints (AddGivenName) to show full names
            let should_initialize = options.initialize.unwrap_or(true) && !force_no_initialize;
            let given_part: Option<Output> = name.given.as_ref().map(|given| {
                let given_text = if name.family.is_none() {
                    // No family name - given name is their full name, don't initialize
                    given.clone()
                } else if force_no_initialize {
                    // Disambiguation override: show full given name
                    given.clone()
                } else if let Some(ref init) = initialize_with {
                    if should_initialize {
                        initialize_name(given, init, initialize_with_hyphen)
                    } else {
                        // initialize="false": normalize with initialize-with pattern but don't break into initials
                        normalize_given_name(given, init)
                    }
                } else {
                    given.clone()
                };
                let base = Output::literal(given_text);
                // Apply font styling only (not affixes) - affixes wrap the combined given+particle
                if let Some(ref fmt) = given_font_styling {
                    Output::formatted(fmt.clone(), vec![base])
                } else {
                    base
                }
            });

            // Build suffix part
            let suffix_part: Option<Output> =
                name.suffix.as_ref().map(|s| Output::literal(s.clone()));

            // Build dropping particle part with font styling (not affixes)
            let dropping_particle_part: Option<Output> =
                name.dropping_particle.as_ref().map(|dp| {
                    let base = Output::literal(dp.clone());
                    if let Some(ref fmt) = given_font_styling {
                        Output::formatted(fmt.clone(), vec![base])
                    } else {
                        base
                    }
                });

            // Build demoted non-dropping particle part with FAMILY font styling (when demoted, goes after given)
            // Per Haskell citeproc: non-dropping particle always uses familyFormatting, even when demoted
            let demoted_ndp_part: Option<Output> = if demote_particle {
                name.non_dropping_particle.as_ref().map(|ndp| {
                    let base = Output::literal(ndp.clone());
                    if let Some(ref fmt) = family_font_styling {
                        Output::formatted(fmt.clone(), vec![base])
                    } else {
                        base
                    }
                })
            } else {
                None
            };

            if inverted {
                // Sort order: "Family, Given [particles]" or "Family, Given [particles], Suffix"
                // Following Haskell citeproc pattern:
                // - familyAffixes [ family ] <:> givenAffixes [ given <+> droppingParticle <+> ndp ] <:> suffix
                // Where given, droppingParticle, ndp each have givenFormatting (font styling only)
                // And the combined result is wrapped with givenAffixes (prefix/suffix only)
                let mut parts: Vec<Output> = Vec::new();

                // Non-Byzantine names don't use comma in sort order
                let effective_separator = if name.is_byzantine() {
                    sort_separator.clone()
                } else {
                    " ".to_string()
                };

                if let Some(family) = family_part_for_inverted {
                    parts.push(family);
                }

                // Build the given part with particles, then wrap with affixes
                let mut given_parts: Vec<Output> = Vec::new();
                if let Some(given) = given_part {
                    given_parts.push(given);
                }

                // Dropping particle goes after given (already has font styling applied)
                if let Some(dp) = dropping_particle_part {
                    if !given_parts.is_empty() {
                        given_parts.push(Output::literal(" ".to_string()));
                    }
                    given_parts.push(dp);
                }

                // Demoted non-dropping particle goes after dropping particle (already has font styling applied)
                if let Some(ndp) = demoted_ndp_part.clone() {
                    if !given_parts.is_empty() {
                        given_parts.push(Output::literal(" ".to_string()));
                    }
                    given_parts.push(ndp);
                }

                // Combine given parts and wrap with affixes if present
                if !given_parts.is_empty() {
                    if !parts.is_empty() {
                        parts.push(Output::literal(effective_separator.clone()));
                    }
                    let given_combined = Output::sequence(given_parts);
                    // Wrap with givenAffixes (prefix/suffix only) if present
                    let wrapped = if let Some(ref affixes) = given_affixes {
                        Output::formatted(affixes.clone(), vec![given_combined])
                    } else {
                        given_combined
                    };
                    parts.push(wrapped);
                }

                if let Some(suffix) = suffix_part {
                    if !parts.is_empty() {
                        // In sort order, always use sort_separator before suffix (like Pandoc's <:>)
                        parts.push(Output::literal(effective_separator.clone()));
                    }
                    parts.push(suffix);
                }

                Output::sequence(parts)
            } else {
                // Display order depends on whether name is Byzantine (Western) or not
                // Byzantine: "Given Dropping-particle Family" (with spaces)
                // Non-Byzantine (CJK, etc.): "FamilyGiven" (no spaces, family first)
                let is_byzantine = name.is_byzantine();
                let mut parts: Vec<Output> = Vec::new();

                if is_byzantine {
                    // Western display order per Haskell citeproc:
                    // givenAffixes [ given ] <+>
                    // familyAffixes [ droppingParticle <+> ndp <+> family <+> suffix ]
                    //
                    // Key insight: familyAffixes wraps the ENTIRE family unit including
                    // dropping particle and suffix, not just ndp + family.

                    // Part 1: Given name wrapped with givenAffixes
                    if let Some(given) = given_part {
                        let wrapped = if let Some(ref affixes) = given_affixes {
                            Output::formatted(affixes.clone(), vec![given])
                        } else {
                            given
                        };
                        parts.push(wrapped);
                    }

                    // Part 2: Build family unit (dp + ndp + family + suffix) and wrap with familyAffixes
                    let mut family_unit_parts: Vec<Output> = Vec::new();

                    // Dropping particle (with given font styling, per Haskell)
                    if let Some(dp) = dropping_particle_part {
                        family_unit_parts.push(dp);
                    }

                    // Non-dropping particle + family (from family_core, already has family font styling)
                    if let Some(core) = family_core.clone() {
                        if !family_unit_parts.is_empty() {
                            // Add space before core unless previous ends with no-space char
                            if !crate::output::ends_with_no_space_char(
                                family_unit_parts.last().unwrap(),
                            ) {
                                family_unit_parts.push(Output::literal(" ".to_string()));
                            }
                        }
                        family_unit_parts.push(core);
                    }

                    // Suffix (with appropriate separator)
                    if let Some(suffix) = suffix_part.clone() {
                        if !family_unit_parts.is_empty() {
                            let separator = if name.comma_suffix.unwrap_or(false) {
                                ", "
                            } else {
                                " "
                            };
                            family_unit_parts.push(Output::literal(separator.to_string()));
                        }
                        family_unit_parts.push(suffix);
                    }

                    // Wrap the entire family unit with familyAffixes
                    if !family_unit_parts.is_empty() {
                        if !parts.is_empty() {
                            // Add space between given and family unit
                            if !crate::output::ends_with_no_space_char(parts.last().unwrap()) {
                                parts.push(Output::literal(" ".to_string()));
                            }
                        }
                        let family_unit = Output::sequence(family_unit_parts);
                        let wrapped = if let Some(ref affixes) = family_affixes {
                            Output::formatted(affixes.clone(), vec![family_unit])
                        } else {
                            family_unit
                        };
                        parts.push(wrapped);
                    }

                    Output::sequence(parts)
                } else {
                    // Non-Byzantine display order: Family + Given (no particles typically)
                    // Use family_part_for_inverted which has affixes applied to just ndp + family
                    if let Some(family) = family_part_for_inverted {
                        parts.push(family);
                    }

                    if let Some(given) = given_part {
                        // Apply given_affixes (prefix/suffix from name-part formatting)
                        let wrapped = if let Some(ref affixes) = given_affixes {
                            Output::formatted(affixes.clone(), vec![given])
                        } else {
                            given
                        };
                        parts.push(wrapped);
                    }

                    Output::sequence(parts)
                }
            }
        }
    }
}

/// Format a single name to a string (convenience wrapper for tests and compatibility).
///
/// This is a thin wrapper around the Output-returning version that renders to string.
#[cfg(test)]
fn format_single_name_to_string(
    name: &crate::reference::Name,
    options: &quarto_csl::InheritableNameOptions,
    inverted: bool,
    disamb: Option<&crate::reference::DisambiguationData>,
    is_primary: bool,
) -> String {
    // Use default SortOnly for tests (most common case)
    // Use true for initialize_with_hyphen (the default)
    // Use false for in_sort_key (test helper is for display, not sort key computation)
    format_single_name(
        name,
        options,
        inverted,
        false, // in_sort_key
        disamb,
        is_primary,
        None,
        None,
        quarto_csl::DemoteNonDroppingParticle::SortOnly,
        true, // initialize_with_hyphen default
    )
    .render()
}

/// Normalize a given name without breaking into initials (for initialize="false").
/// This follows Pandoc's citeproc algorithm:
/// - Parse into tokens (single letters at period/space boundaries are "initials")
/// - Initials get initialize-with appended
/// - Multi-letter words get space appended
/// - Consecutive uppercase (like "ME") is preserved unchanged
///
/// Examples with initialize-with=". ":
/// - "M.E" -> "M. E." (both are initials)
/// - "M E" -> "M. E." (both are initials)
/// - "John M.E." -> "John M. E." (John is a word, M and E are initials)
/// - "ME" -> "ME" (consecutive uppercase, unchanged)
fn normalize_given_name(given: &str, initialize_with: &str) -> String {
    // Parse the string into tokens, tracking whether each is an "initial" (Left) or "word" (Right)
    // A token is an "initial" if it's a single letter that ends at a period, space, or end of string
    // A token is a "word" if it's multiple letters

    #[derive(Debug)]
    enum Token {
        Initial(String),   // Single letter at period/space/end boundary
        Word(String),      // Multi-letter sequence
        Unchanged(String), // Consecutive uppercase like "ME" - preserve as-is
    }

    let mut tokens: Vec<Token> = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = given.chars().collect();

    for &c in chars.iter() {
        match c {
            '.' => {
                // Period ends a token
                if !current.is_empty() {
                    // At a period boundary, short tokens (≤2 chars) are initials
                    // Longer tokens starting with uppercase that contain lowercase are words
                    if current.len() <= 2 {
                        // Short tokens at period boundary are initials (e.g., "M", "Ph")
                        tokens.push(Token::Initial(current.clone()));
                    } else if current.chars().next().map_or(false, |c| c.is_uppercase())
                        && current.chars().skip(1).any(|c| c.is_lowercase())
                    {
                        // Mixed case word like "John" - it's a word
                        tokens.push(Token::Word(current.clone()));
                    } else {
                        // Other cases (all uppercase, etc.) - treat as initial
                        tokens.push(Token::Initial(current.clone()));
                    }
                    current.clear();
                }
            }
            ' ' => {
                // Space ends a token
                if !current.is_empty() {
                    if current.len() == 1
                        && current.chars().next().map_or(false, |c| c.is_uppercase())
                    {
                        tokens.push(Token::Initial(current.clone()));
                    } else if current.len() > 1 && current.chars().all(|c| c.is_uppercase()) {
                        // Consecutive uppercase at space boundary - preserve unchanged
                        tokens.push(Token::Unchanged(current.clone()));
                    } else if current.chars().next().map_or(false, |c| c.is_uppercase())
                        && current.chars().skip(1).any(|c| c.is_lowercase())
                    {
                        // Mixed case starting with uppercase (like "John") - it's a word
                        tokens.push(Token::Word(current.clone()));
                    } else {
                        tokens.push(Token::Word(current.clone()));
                    }
                    current.clear();
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    // Handle final token
    if !current.is_empty() {
        if current.len() == 1 && current.chars().next().map_or(false, |c| c.is_uppercase()) {
            tokens.push(Token::Initial(current));
        } else if current.len() > 1 && current.chars().all(|c| c.is_uppercase()) {
            // Consecutive uppercase at end without period - preserve unchanged
            tokens.push(Token::Unchanged(current));
        } else if current.chars().next().map_or(false, |c| c.is_uppercase()) && current.len() <= 2 {
            // Short mixed case like "Me" at end - preserve with trailing period if original had it
            tokens.push(Token::Unchanged(current));
        } else if current.chars().next().map_or(false, |c| c.is_uppercase())
            && current.chars().skip(1).any(|c| c.is_lowercase())
        {
            tokens.push(Token::Word(current));
        } else {
            tokens.push(Token::Unchanged(current));
        }
    }

    // Now build the result
    let mut result = String::new();
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::Initial(s) => {
                result.push_str(s);
                result.push_str(initialize_with);
            }
            Token::Word(s) => {
                result.push_str(s);
                // Add space after words, unless it's the last token
                if i < tokens.len() - 1 {
                    result.push(' ');
                }
            }
            Token::Unchanged(s) => {
                result.push_str(s);
                // For unchanged tokens at the end, check if original had trailing period
                // If so, preserve it
                if i == tokens.len() - 1 && given.ends_with('.') {
                    result.push('.');
                }
            }
        }
    }

    // Trim trailing space
    result.trim_end().to_string()
}

/// Initialize a given name (e.g., "John William" -> "J. W.").
///
/// The `initialize_with_hyphen` parameter controls how hyphenated names are handled:
/// - For "Guo-Ping" (both parts uppercase):
///   - true: "G.-P." (preserves hyphen before second initial)
///   - false: "G.P." (no hyphen)
/// - For "Guo-ping" (second part lowercase): "G." (lowercase parts are skipped)
///
/// Pre-existing initials are recognized by periods in the input:
/// - "J.J." -> "JJ" (with initialize_with="")
/// - "J. J." -> "J. J." (with initialize_with=". ")
fn initialize_name(given: &str, initialize_with: &str, initialize_with_hyphen: bool) -> String {
    // Parse the given name into tokens: either initials (already abbreviated) or words (to be initialized)
    // Following Pandoc's algorithm:
    // - Text before a '.' is an initial
    // - A single character before a space is an initial
    // - Multiple characters before a space is a word
    // - '-' introduces a hyphenated continuation

    #[derive(Debug)]
    enum Token {
        Initial(String), // Already an initial (came before a period or single char before space)
        Word(String),    // A full word to be initialized
        Particle(String), // A lowercase particle (de, von, van) to be preserved as-is
        HyphenPart(String), // Part after a hyphen (needs special handling)
    }

    // Helper to classify a string as Word or Particle
    fn classify_word(s: &str) -> Token {
        // If all lowercase and not single char, it's a particle (de, von, van, etc.)
        if s.len() > 1 && s.chars().all(|c| c.is_lowercase()) {
            Token::Particle(s.to_string())
        } else if s.len() == 1 {
            Token::Initial(s.to_string())
        } else {
            Token::Word(s.to_string())
        }
    }

    let mut tokens: Vec<Token> = Vec::new();
    let mut current = String::new();
    let mut after_hyphen = false;

    for c in given.chars() {
        match c {
            '.' => {
                // Period marks the preceding content as an initial
                if !current.is_empty() {
                    if after_hyphen {
                        tokens.push(Token::HyphenPart(current.clone()));
                        after_hyphen = false;
                    } else {
                        tokens.push(Token::Initial(current.clone()));
                    }
                    current.clear();
                }
            }
            ' ' => {
                // Space ends the current token
                if !current.is_empty() {
                    if after_hyphen {
                        tokens.push(Token::HyphenPart(current.clone()));
                        after_hyphen = false;
                    } else {
                        tokens.push(classify_word(&current));
                    }
                    current.clear();
                }
            }
            '-' => {
                // Hyphen: emit current token and mark that next part is after hyphen
                if !current.is_empty() {
                    if after_hyphen {
                        tokens.push(Token::HyphenPart(current.clone()));
                    } else {
                        tokens.push(classify_word(&current));
                    }
                    current.clear();
                }
                after_hyphen = true;
            }
            _ => {
                current.push(c);
            }
        }
    }

    // Handle final token
    if !current.is_empty() {
        if after_hyphen {
            tokens.push(Token::HyphenPart(current));
        } else {
            tokens.push(classify_word(&current));
        }
    }

    // Now render the tokens with initialize_with suffix
    let trimmed = initialize_with.trim_end();
    let mut result = String::new();
    let mut prev_was_initial_or_word = false;
    let mut prev_was_particle = false;

    for token in &tokens {
        match token {
            Token::Initial(s) => {
                // Add space if after particle or if initialize_with ends with space
                if prev_was_particle || (prev_was_initial_or_word && initialize_with.ends_with(' '))
                {
                    result.push(' ');
                }
                // Initial: output as-is with suffix
                result.push_str(s);
                result.push_str(trimmed);
                prev_was_initial_or_word = true;
                prev_was_particle = false;
            }
            Token::Word(s) => {
                // Add space if after particle or if initialize_with ends with space
                if prev_was_particle || (prev_was_initial_or_word && initialize_with.ends_with(' '))
                {
                    result.push(' ');
                }
                // Word: initialize to first char(s) with suffix
                // Special case: "long abbreviation" pattern like "TSerendorjiin" -> "Ts"
                // If word starts with uppercase, followed by uppercase, followed by all lowercase,
                // output first char uppercase + second char lowercase
                let chars: Vec<char> = s.chars().collect();
                if chars.len() >= 2
                    && chars[0].is_uppercase()
                    && chars[1].is_uppercase()
                    && chars.len() > 2
                    && chars[2..].iter().all(|c| c.is_lowercase())
                {
                    // Long abbreviation: "TSerendorjiin" -> "Ts"
                    result.push_str(&chars[0].to_uppercase().to_string());
                    result.push_str(&chars[1].to_lowercase().to_string());
                    result.push_str(trimmed);
                } else if let Some(first_char) = chars.first() {
                    // Normal case: just first char
                    result.push_str(&first_char.to_uppercase().to_string());
                    result.push_str(trimmed);
                }
                prev_was_initial_or_word = true;
                prev_was_particle = false;
            }
            Token::Particle(s) => {
                // Particle: preserve as-is (de, von, van, etc.)
                // Add space before if needed
                if prev_was_initial_or_word || prev_was_particle {
                    result.push(' ');
                }
                result.push_str(s);
                prev_was_initial_or_word = false;
                prev_was_particle = true;
            }
            Token::HyphenPart(s) => {
                // Hyphenated part: check if first char is uppercase
                if let Some(first_char) = s.chars().next() {
                    if first_char.is_uppercase() {
                        if initialize_with_hyphen {
                            result.push('-');
                        }
                        result.push_str(&first_char.to_uppercase().to_string());
                        result.push_str(trimmed);
                    }
                    // Lowercase parts after hyphen are skipped (e.g., Ji-ping -> J.)
                }
                prev_was_initial_or_word = true;
                prev_was_particle = false;
            }
        }
    }

    result
}

/// Evaluate a group element.
///
/// Groups use suppression logic: if a group calls at least one variable but all
/// called variables are empty, the entire group is suppressed (returns Null).
fn evaluate_group(
    ctx: &mut EvalContext,
    group_el: &GroupElement,
    _formatting: &Formatting,
) -> Result<Output> {
    // Save current variable count before evaluating group
    let old_var_count = ctx.get_var_count();

    let delimiter = group_el.delimiter.clone().unwrap_or_default();
    let output = evaluate_elements(ctx, &group_el.elements, &delimiter)?;

    let new_var_count = ctx.get_var_count();

    // Check if group should be suppressed:
    // - It called at least one variable (new_called > old_called)
    // - But none were non-empty (new_non_empty == old_non_empty)
    let vars_called = new_var_count.called > old_var_count.called;
    let all_empty = new_var_count.non_empty == old_var_count.non_empty;

    if vars_called && all_empty {
        // Suppress the group but DO NOT restore var count.
        // Parent groups need to see that variables were called (directly or indirectly)
        // even if they were all empty, so they can also suppress.
        // This matches Pandoc's eGroup behavior.
        Ok(Output::Null)
    } else {
        Ok(output)
    }
}

/// Evaluate a choose element (conditionals).
fn evaluate_choose(ctx: &mut EvalContext, choose_el: &quarto_csl::ChooseElement) -> Result<Output> {
    for branch in &choose_el.branches {
        // Else branch has no conditions
        if branch.conditions.is_empty() {
            return evaluate_elements(ctx, &branch.elements, "");
        }

        // Evaluate conditions based on match type
        let matches = match branch.match_type {
            quarto_csl::MatchType::All => branch
                .conditions
                .iter()
                .all(|c| evaluate_condition(ctx, c, branch.match_type)),
            quarto_csl::MatchType::Any => branch
                .conditions
                .iter()
                .any(|c| evaluate_condition(ctx, c, branch.match_type)),
            quarto_csl::MatchType::None => branch
                .conditions
                .iter()
                .all(|c| !evaluate_condition(ctx, c, branch.match_type)),
        };

        if matches {
            return evaluate_elements(ctx, &branch.elements, "");
        }
    }

    Ok(Output::Null)
}

/// Evaluate a condition.
///
/// For multi-value conditions (e.g., `variable="title edition"`), the `match_type`
/// determines whether all or any of the values must match:
/// - `All`: all values must satisfy the condition
/// - `Any`: at least one value must satisfy the condition
/// - `None`: interpreted as `Any` for the internal check (negation applied at branch level)
fn evaluate_condition(
    ctx: &EvalContext,
    condition: &quarto_csl::Condition,
    match_type: quarto_csl::MatchType,
) -> bool {
    use quarto_csl::ConditionType;

    // Helper to check if a variable exists (any type: standard, names, date)
    // Uses unified get_variable which checks citation item context first
    let var_exists = |v: &str| {
        ctx.get_variable(v).is_some()
            || ctx.reference.get_names(v).is_some()
            || ctx.reference.get_date(v).is_some()
    };

    // Helper to check if a variable is numeric
    // Uses unified get_variable which checks citation item context first
    // Implements CSL is-numeric semantics: strips leading/trailing letters from each
    // chunk (split by commas, hyphens, ampersands, en-dashes) and checks if cores are all digits.
    // This allows "5th", "3rd", "1-10" to be numeric.
    let is_numeric = |v: &str| {
        ctx.get_variable(v)
            .map(|s| is_numeric_string(&s))
            .unwrap_or(false)
    };

    // Helper to check if a date is uncertain
    let is_uncertain_date = |v: &str| {
        ctx.reference
            .get_date(v)
            .map(|d| d.circa.unwrap_or(false))
            .unwrap_or(false)
    };

    // For match="all", require ALL values in a multi-value condition
    // For match="any" or match="none", require ANY value (none applies negation at branch level)
    let use_all = matches!(match_type, quarto_csl::MatchType::All);

    match &condition.condition_type {
        ConditionType::Type(types) => {
            if use_all {
                types.iter().all(|t| t == &ctx.reference.ref_type)
            } else {
                types.iter().any(|t| t == &ctx.reference.ref_type)
            }
        }
        ConditionType::Variable(vars) => {
            if use_all {
                vars.iter().all(|v| var_exists(v))
            } else {
                vars.iter().any(|v| var_exists(v))
            }
        }
        ConditionType::IsNumeric(vars) => {
            if use_all {
                vars.iter().all(|v| is_numeric(v))
            } else {
                vars.iter().any(|v| is_numeric(v))
            }
        }
        ConditionType::IsUncertainDate(vars) => {
            if use_all {
                vars.iter().all(|v| is_uncertain_date(v))
            } else {
                vars.iter().any(|v| is_uncertain_date(v))
            }
        }
        ConditionType::Locator(locator_types) => {
            // Check if the locator label matches any of the specified types
            if let Some(label) = ctx.get_locator_label() {
                if use_all {
                    locator_types.iter().all(|t| t == label)
                } else {
                    locator_types.iter().any(|t| t == label)
                }
            } else {
                false
            }
        }
        ConditionType::Position(required_positions) => {
            // Check if the citation position matches any of the specified positions.
            // Uses Vec<Position> for position tracking (matching citeproc reference impl).
            // CSL positions have an implicit hierarchy:
            // - "first" matches if First is in positions
            // - "subsequent" matches if Subsequent, Ibid, or IbidWithLocator is in positions
            // - "ibid" matches if Ibid or IbidWithLocator is in positions
            // - "ibid-with-locator" matches if IbidWithLocator is in positions
            // - "near-note" matches if NearNote is in positions
            use quarto_csl::Position;

            let matches_position = |required: &Position| -> bool {
                match required {
                    Position::First => ctx.positions.contains(&Position::First),
                    Position::Subsequent => {
                        // Subsequent is true if any of: Subsequent, Ibid, IbidWithLocator
                        ctx.positions.contains(&Position::Subsequent)
                            || ctx.positions.contains(&Position::Ibid)
                            || ctx.positions.contains(&Position::IbidWithLocator)
                    }
                    Position::Ibid => {
                        // Ibid is true if Ibid or IbidWithLocator
                        ctx.positions.contains(&Position::Ibid)
                            || ctx.positions.contains(&Position::IbidWithLocator)
                    }
                    Position::IbidWithLocator => ctx.positions.contains(&Position::IbidWithLocator),
                    Position::NearNote => ctx.positions.contains(&Position::NearNote),
                }
            };

            if use_all {
                required_positions.iter().all(|p| matches_position(p))
            } else {
                required_positions.iter().any(|p| matches_position(p))
            }
        }
        ConditionType::Disambiguate(expected) => {
            // Check if the reference has been marked for disambiguation
            ctx.reference
                .disambiguation
                .as_ref()
                .map(|d| d.disamb_condition == *expected)
                .unwrap_or(!expected) // If no disambiguation data, condition is false
        }
    }
}

/// Evaluate a number element.
fn evaluate_number(
    ctx: &mut EvalContext,
    num_el: &quarto_csl::NumberElement,
    _formatting: &Formatting,
) -> Result<Output> {
    // Special handling for citation-number
    if num_el.variable == "citation-number" {
        let result = if let Some(num) = ctx.processor.get_citation_number(&ctx.reference.id) {
            // Apply number form (ordinal, roman, etc.)
            // Create closure for term lookup using processor reference
            let processor = &ctx.processor;
            let get_ordinal_term = |name: &str| -> Option<String> {
                processor.get_term(name, quarto_csl::TermForm::Long, false)
            };
            let formatted =
                format_number_with_form(&num.to_string(), num_el.form, get_ordinal_term);
            // Tag for collapse detection
            Output::tagged(Tag::CitationNumber(num), Output::literal(formatted))
        } else {
            Output::Null
        };
        ctx.record_var_call(!result.is_null());
        return Ok(result);
    }

    let value = ctx.get_variable(&num_el.variable);
    ctx.record_var_call(value.is_some());

    if let Some(value) = value {
        // Apply number form (roman, ordinal, etc.)
        // Create closure for term lookup using processor reference
        let processor = &ctx.processor;
        let get_ordinal_term = |name: &str| -> Option<String> {
            processor.get_term(name, quarto_csl::TermForm::Long, false)
        };
        let with_form = format_number_with_form(&value, num_el.form, get_ordinal_term);

        // The <number> element should format all ranges with en-dash,
        // not just page-like variables. This applies to edition, volume, issue, etc.
        // Note: For roman numerals, we've already converted, so range formatting
        // only applies to numeric form.
        // Note: page-range-format abbreviation only applies to "page" variable,
        // so we pass None for other variables to just get delimiter replacement.
        let formatted = if num_el.form == quarto_csl::NumberForm::Numeric {
            let delimiter = ctx.get_page_range_delimiter();
            let page_range_format = if num_el.variable == "page" {
                ctx.processor.style.options.page_range_format
            } else {
                None
            };
            format_page_range(&with_form, &delimiter, page_range_format)
        } else {
            with_form
        };
        Ok(Output::literal(formatted))
    } else {
        Ok(Output::Null)
    }
}

/// Evaluate a label element.
fn evaluate_label(
    ctx: &mut EvalContext,
    label_el: &quarto_csl::LabelElement,
    _formatting: &Formatting,
) -> Result<Output> {
    // For locator variable, we need special handling:
    // - The term to look up is the locator label type (e.g., "page"), not "locator"
    // - Plural is determined by analyzing the locator value
    // - Following Pandoc citeproc: suppress label if locator text already contains
    //   a term abbreviation (e.g., "vol. 1" starts with "vol. ")
    let (term_name, value_for_plural) = if label_el.variable == "locator" {
        let locator_value = ctx.locator.map(|s| s.to_string());

        // Get the explicitly set label (if any)
        let explicit_label = ctx.locator_label;

        match explicit_label {
            Some(lt) => {
                // Explicit label set - use it
                (lt.to_string(), locator_value)
            }
            None => {
                // No explicit label - check if we should suppress the default "page" label
                // Following Pandoc's logic in Citeproc/Eval.hs:
                // 1. If locator begins with space → no label
                // 2. If locator starts with letters followed by ". " → no label (term abbreviation)
                // 3. Otherwise → use "page"
                if let Some(loc) = ctx.locator {
                    if loc.starts_with(|c: char| c.is_whitespace()) {
                        // Begins with space - no label
                        return Ok(Output::Null);
                    }
                    // Check for term abbreviation: letters followed by ". "
                    let after_letters: &str = loc.trim_start_matches(|c: char| c.is_alphabetic());
                    if after_letters.starts_with(". ") {
                        // Starts with term abbreviation like "vol. " - no label
                        return Ok(Output::Null);
                    }
                    // Default to page label
                    ("page".to_string(), locator_value)
                } else {
                    return Ok(Output::Null); // No locator, no label
                }
            }
        }
    } else {
        (
            label_el.variable.clone(),
            ctx.get_variable(&label_el.variable),
        )
    };

    // Determine if plural
    let is_plural = match label_el.plural {
        quarto_csl::LabelPlural::Always => true,
        quarto_csl::LabelPlural::Never => false,
        quarto_csl::LabelPlural::Contextual => {
            // Check if the value indicates plural (ranges, "and", multiple values)
            value_for_plural
                .as_ref()
                .map(|v| is_plural_value(v, &term_name))
                .unwrap_or(false)
        }
    };

    if let Some(term) = ctx.get_term(&term_name, label_el.form, is_plural) {
        Ok(Output::tagged(Tag::Term(term_name), Output::literal(term)))
    } else {
        Ok(Output::Null)
    }
}

/// Check if a value indicates plural (for locator/page labels).
///
/// Plural indicators depend on the variable:
/// - For number-of-volumes: numeric value != 1 and != 0 means plural
/// - For other variables (locators, pages): multiple numbers indicate plural
///   (ranges like "1-5", lists like "1, 5", etc.)
///
/// Following Haskell citeproc's determinePlural logic.
fn is_plural_value(value: &str, variable: &str) -> bool {
    // Special case for number-of-volumes: numeric value > 1 means plural
    // (This is a count, not a page/locator reference)
    if variable == "number-of-volumes" {
        if let Ok(n) = value.trim().parse::<i64>() {
            return n != 1 && n != 0;
        }
        // Non-numeric number-of-volumes is treated as singular
        return false;
    }

    // For locators/pages: check for escaped hyphen (literal hyphen, not range)
    // In CSL, \- means a literal hyphen that doesn't indicate a range
    if value.contains("\\-") {
        return false;
    }

    // Count number sequences in the value
    // Ranges like "1-5" or "1–5" or lists like "1, 5" have multiple numbers
    let num_count = count_number_sequences(value);
    if num_count > 1 {
        return true;
    }

    // Check for "and", "&" indicating multiple values
    if value.contains(" and ") || value.contains(" & ") || value.contains('&') {
        return true;
    }
    // Check for localized "and" words
    // Common translations: et (French/Latin), und (German), y (Spanish), e (Italian/Portuguese)
    if value.contains(" et ")
        || value.contains(" und ")
        || value.contains(" y ")
        || value.contains(" e ")
    {
        return true;
    }

    false
}

/// Count the number of separate number sequences in a string.
/// For example: "101" = 1, "101-105" = 2, "1, 5, 10" = 3
fn count_number_sequences(value: &str) -> usize {
    let mut count = 0;
    let mut in_number = false;

    for c in value.chars() {
        if c.is_ascii_digit() {
            if !in_number {
                count += 1;
                in_number = true;
            }
        } else {
            in_number = false;
        }
    }

    count
}

/// Evaluate a date element.
fn evaluate_date(
    ctx: &mut EvalContext,
    date_el: &quarto_csl::DateElement,
    _formatting: &Formatting,
) -> Result<Output> {
    use crate::reference::DateParts;
    use quarto_csl::{DatePartName, DatePartsFilter};

    let date_var = ctx.reference.get_date(&date_el.variable);

    // Don't record var call yet - we need to check if the date actually produces output
    // For dates, the "non_empty" status depends on whether the requested parts exist,
    // not just whether the date variable exists. A date with only year should not
    // count as non-empty if we're only asking for the month.

    let Some(date_var) = date_var else {
        ctx.record_var_call(false);
        return Ok(Output::Null);
    };

    // Handle literal dates (always takes precedence)
    if let Some(ref literal) = date_var.literal {
        ctx.record_var_call(true);
        let output = Output::literal(literal);
        return Ok(Output::tagged(Tag::Date(date_el.variable.clone()), output));
    }

    // Try to get structured date parts
    let Some(start_parts) = date_var.parts() else {
        // No structured date - fall back to raw (unparsed) date string if available
        if let Some(ref raw) = date_var.raw {
            ctx.record_var_call(true);
            let output = Output::literal(raw);
            return Ok(Output::tagged(Tag::Date(date_el.variable.clone()), output));
        }
        ctx.record_var_call(false);
        return Ok(Output::Null);
    };

    // Determine which date parts to render based on date_parts filter
    let include_year = true; // Always include year
    let include_month = matches!(
        date_el.date_parts,
        DatePartsFilter::YearMonth | DatePartsFilter::YearMonthDay
    );
    let include_day = matches!(date_el.date_parts, DatePartsFilter::YearMonthDay);

    // Get the date format from the locale if form is specified
    let locale_format = date_el
        .form
        .and_then(|form| ctx.processor.get_date_format(form));

    // Build format parts list, merging locale parts with inline overrides.
    // Per CSL spec: when using a localized date format (form="text" or "numeric"),
    // inline date-part elements override attributes of the locale's date parts.
    // The inline part's attributes take precedence, but locale's formatting is merged.
    let format_parts: Vec<quarto_csl::DatePart> = if let Some(locale_fmt) = locale_format {
        // Start with locale's parts, apply overrides from inline parts
        locale_fmt
            .parts
            .iter()
            .map(|locale_part| {
                // Find matching inline override by name
                if let Some(inline_part) = date_el.parts.iter().find(|p| p.name == locale_part.name)
                {
                    // Merge: inline attributes override locale attributes,
                    // but locale formatting is the base, overridden by inline formatting
                    merge_date_parts(locale_part, inline_part)
                } else {
                    locale_part.clone()
                }
            })
            .collect()
    } else if !date_el.parts.is_empty() {
        date_el.parts.clone()
    } else {
        Vec::new()
    };
    let format_parts_refs: Vec<_> = format_parts.iter().collect();

    // Helper to check if a part should be included
    let should_include_part = |name: DatePartName, parts: &DateParts| -> bool {
        match name {
            DatePartName::Year => include_year && parts.year.is_some(),
            DatePartName::Month => include_month && parts.month.is_some(),
            DatePartName::Day => include_day && parts.day.is_some(),
        }
    };

    // Get the delimiter between date parts
    // Prefer style's delimiter, fall back to locale's delimiter
    let date_delimiter = date_el
        .delimiter
        .as_deref()
        .or_else(|| locale_format.and_then(|f| f.delimiter.as_deref()));

    // Build the final date output
    let date_output = if let Some(end_parts) = date_var.end_parts() {
        // Get range delimiter (default "–" en-dash)
        let range_delimiter = date_el.range_delimiter.as_deref().unwrap_or("–");

        // Smart date range collapsing: suppress parts that are the same between start and end
        // For example: "10 August 2003–23 August 2003" becomes "10–23 August 2003"
        render_date_range(
            ctx,
            &start_parts,
            &end_parts,
            &format_parts_refs,
            &should_include_part,
            date_delimiter,
            range_delimiter,
        )
    } else {
        // Single date
        let start_output = render_date_parts(
            ctx,
            &start_parts,
            &format_parts_refs,
            &should_include_part,
            date_delimiter,
        );
        if format_parts_refs.is_empty() {
            // Just render the year if no format parts
            if let Some(year) = start_parts.year {
                Output::literal(year.to_string())
            } else {
                Output::Null
            }
        } else {
            start_output
        }
    };

    if date_output.is_null() {
        // Date variable exists but produces no output (e.g., asking for month when only year exists)
        ctx.record_var_call(false);
        Ok(Output::Null)
    } else {
        // Date produces output - record as non-empty
        ctx.record_var_call(true);

        // Append year suffix for disambiguation (like citeproc does)
        // Only add implicit year suffix if:
        // 1. The style doesn't explicitly use year-suffix variable
        // 2. Year suffix hasn't already been rendered for this reference
        let uses_year_suffix_var = ctx.processor.style.options.uses_year_suffix_variable;
        let already_rendered = ctx.year_suffix_rendered.get();
        let year_suffix_output = if !uses_year_suffix_var && !already_rendered {
            if let Some(suffix) = ctx
                .reference
                .disambiguation
                .as_ref()
                .and_then(|d| d.year_suffix)
            {
                // Mark as rendered so subsequent dates don't get the suffix
                ctx.year_suffix_rendered.set(true);
                let letter = suffix_to_letter(suffix);
                Output::tagged(Tag::YearSuffix(suffix), Output::literal(letter))
            } else {
                Output::Null
            }
        } else {
            Output::Null
        };

        let final_output = if year_suffix_output.is_null() {
            date_output
        } else {
            Output::sequence(vec![date_output, year_suffix_output])
        };

        // Tag the date output for disambiguation
        Ok(Output::tagged(
            Tag::Date(date_el.variable.clone()),
            final_output,
        ))
    }
}

/// Render date parts according to the format specification.
fn render_date_parts<F>(
    ctx: &EvalContext,
    parts: &crate::reference::DateParts,
    format_parts: &[&quarto_csl::DatePart],
    should_include: &F,
    delimiter: Option<&str>,
) -> Output
where
    F: Fn(quarto_csl::DatePartName, &crate::reference::DateParts) -> bool,
{
    use quarto_csl::{DatePartForm, DatePartName};

    let mut outputs = Vec::new();

    for part in format_parts {
        if !should_include(part.name, parts) {
            continue;
        }

        let value = match part.name {
            DatePartName::Year => {
                parts.year.map(|y| {
                    // For negative years, use absolute value and add era suffix
                    // For years 0 < n < 1000, add AD suffix
                    // Note: The default terms in CSL have a leading space for readability.
                    // When there's a delimiter between date parts, we strip the leading space
                    // to avoid awkward output like "100 BC-7-13". Without a delimiter,
                    // we keep the space for proper separation like "499 AD".
                    let (display_year, era_suffix) = if y < 0 {
                        let bc = ctx
                            .get_term("bc", quarto_csl::TermForm::Long, false)
                            .unwrap_or_else(|| "BC".to_string());
                        let bc = if delimiter.is_some() {
                            bc.trim_start().to_string()
                        } else {
                            bc
                        };
                        ((-y).to_string(), bc)
                    } else if y > 0 && y < 1000 {
                        let ad = ctx
                            .get_term("ad", quarto_csl::TermForm::Long, false)
                            .unwrap_or_else(|| "AD".to_string());
                        let ad = if delimiter.is_some() {
                            ad.trim_start().to_string()
                        } else {
                            ad
                        };
                        (y.to_string(), ad)
                    } else {
                        (y.to_string(), String::new())
                    };
                    format!("{}{}", display_year, era_suffix)
                })
            }
            DatePartName::Month => parts.month.and_then(|m| {
                let form = part.form.unwrap_or(DatePartForm::Long);
                format_month_or_season(ctx, m, form)
            }),
            DatePartName::Day => parts.day.map(|d| {
                let form = part.form.unwrap_or(DatePartForm::Numeric);
                let limit_ordinals = ctx.processor.limit_day_ordinals_to_day_1();
                format_day(d, form, limit_ordinals)
            }),
        };

        if let Some(v) = value {
            // Add delimiter before this part (except for the first part)
            if !outputs.is_empty() {
                if let Some(d) = delimiter {
                    outputs.push(Output::literal(d));
                }
            }

            // Apply the value (with any strip-periods handling)
            let final_value = if part.strip_periods {
                v.replace('.', "")
            } else {
                v
            };

            // Apply formatting (including text-case, prefix, suffix) via Output::formatted
            // This ensures text transforms like uppercase are applied
            let formatted_output =
                Output::formatted(part.formatting.clone(), vec![Output::literal(final_value)]);
            outputs.push(formatted_output);
        }
    }

    Output::sequence(outputs)
}

/// Render a date range with smart collapsing of repeated parts.
///
/// For example:
/// - "10 August 2003–23 August 2003" becomes "10–23 August 2003" (same month+year)
/// - "3 August 2003–23 October 2003" becomes "3 August–23 October 2003" (same year)
/// - "10 August 2003–23 October 2004" stays as is (different year)
fn render_date_range<F>(
    ctx: &EvalContext,
    start_parts: &crate::reference::DateParts,
    end_parts: &crate::reference::DateParts,
    format_parts: &[&quarto_csl::DatePart],
    should_include: &F,
    delimiter: Option<&str>,
    default_range_delimiter: &str,
) -> Output
where
    F: Fn(quarto_csl::DatePartName, &crate::reference::DateParts) -> bool,
{
    use quarto_csl::DatePartName;

    // Check for open-ended range (year=0 means open)
    let is_open_range =
        end_parts.year == Some(0) && end_parts.month.is_none() && end_parts.day.is_none();

    if is_open_range {
        // Open range: just render start date with trailing range delimiter
        // For open ranges, use the year's range delimiter (or default)
        let open_range_delim = format_parts
            .iter()
            .find(|p| p.name == DatePartName::Year)
            .and_then(|p| p.range_delimiter.as_deref())
            .unwrap_or(default_range_delimiter);
        let start_output =
            render_date_parts(ctx, start_parts, format_parts, should_include, delimiter);
        return Output::sequence(vec![start_output, Output::literal(open_range_delim)]);
    }

    // Determine which date parts are the same between start and end
    // We compare in hierarchical order: year > month > day
    let year_same = start_parts.year == end_parts.year;
    let month_same = start_parts.month == end_parts.month;
    let day_same = start_parts.day == end_parts.day;

    // A part is considered "same" only if all higher-level parts are also same
    // e.g., month is "same" only if year is also same
    let is_same = |name: DatePartName| -> bool {
        match name {
            DatePartName::Year => year_same,
            DatePartName::Month => year_same && month_same,
            DatePartName::Day => year_same && month_same && day_same,
        }
    };

    // Filter format_parts to only those that should be included
    let active_parts: Vec<_> = format_parts
        .iter()
        .filter(|p| should_include(p.name, start_parts) || should_include(p.name, end_parts))
        .copied()
        .collect();

    // Find the split point: first part that differs (in format order)
    // Parts before this are "leading same", parts after include "differing" and "trailing same"
    let first_diff_idx = active_parts
        .iter()
        .position(|p| !is_same(p.name))
        .unwrap_or(active_parts.len());

    // If all parts are the same, just render the start date (no range needed)
    if first_diff_idx == active_parts.len() {
        return render_date_parts(ctx, start_parts, &active_parts, should_include, delimiter);
    }

    // Get range delimiter from the most significant differing date part.
    // Significance order: Year > Month > Day (like Pandoc's sortOn dpName).
    // This determines which range delimiter to use when multiple parts differ.
    let range_delimiter = active_parts
        .iter()
        .filter(|p| !is_same(p.name))
        .min_by_key(|p| match p.name {
            DatePartName::Year => 0,
            DatePartName::Month => 1,
            DatePartName::Day => 2,
        })
        .and_then(|p| p.range_delimiter.as_deref())
        .unwrap_or(default_range_delimiter);

    // Find where trailing same parts begin (after all differing parts)
    // We scan from first_diff_idx to find where parts become same again
    let trailing_same_idx = active_parts[first_diff_idx..]
        .iter()
        .rposition(|p| !is_same(p.name))
        .map(|i| first_diff_idx + i + 1)
        .unwrap_or(active_parts.len());

    // Split into: leading_same, differing (includes the range), trailing_same
    let leading_same = &active_parts[..first_diff_idx];
    let differing = &active_parts[first_diff_idx..trailing_same_idx];
    let trailing_same = &active_parts[trailing_same_idx..];

    let mut outputs = Vec::new();

    // Render leading same parts (from start date)
    if !leading_same.is_empty() {
        let leading = render_date_parts(ctx, start_parts, leading_same, should_include, delimiter);
        if !leading.is_null() {
            outputs.push(leading);
        }
    }

    // Render differing parts as a range
    if !differing.is_empty() {
        // Render start's differing parts (without trailing suffix on last part)
        let start_diff = render_date_parts_for_range(
            ctx,
            start_parts,
            differing,
            should_include,
            delimiter,
            true,  // strip last suffix
            false, // don't strip first prefix
        );

        // Render end's differing parts (without leading prefix on first part)
        let end_diff = render_date_parts_for_range(
            ctx,
            end_parts,
            differing,
            should_include,
            delimiter,
            false, // don't strip last suffix
            true,  // strip first prefix
        );

        if !start_diff.is_null() || !end_diff.is_null() {
            // Add delimiter before range if we have leading parts
            if !outputs.is_empty() {
                if let Some(d) = delimiter {
                    outputs.push(Output::literal(d));
                }
            }
            outputs.push(start_diff);
            outputs.push(Output::literal(range_delimiter));
            outputs.push(end_diff);
        }
    }

    // Render trailing same parts (from start date, since they're the same)
    if !trailing_same.is_empty() {
        let trailing =
            render_date_parts(ctx, start_parts, trailing_same, should_include, delimiter);
        if !trailing.is_null() {
            // Add delimiter between differing parts and trailing same parts
            if !outputs.is_empty() {
                if let Some(d) = delimiter {
                    outputs.push(Output::literal(d));
                }
            }
            outputs.push(trailing);
        }
    }

    Output::sequence(outputs)
}

/// Render date parts with optional prefix/suffix stripping for range formatting.
fn render_date_parts_for_range<F>(
    ctx: &EvalContext,
    parts: &crate::reference::DateParts,
    format_parts: &[&quarto_csl::DatePart],
    should_include: &F,
    delimiter: Option<&str>,
    strip_last_suffix: bool,
    strip_first_prefix: bool,
) -> Output
where
    F: Fn(quarto_csl::DatePartName, &crate::reference::DateParts) -> bool,
{
    use quarto_csl::{DatePartForm, DatePartName};

    let mut outputs = Vec::new();
    let num_parts = format_parts.len();

    for (idx, part) in format_parts.iter().enumerate() {
        if !should_include(part.name, parts) {
            continue;
        }

        let value = match part.name {
            DatePartName::Year => parts.year.map(|y| {
                let (display_year, era_suffix) = if y < 0 {
                    let bc = ctx
                        .get_term("bc", quarto_csl::TermForm::Long, false)
                        .unwrap_or_else(|| "BC".to_string());
                    let bc = if delimiter.is_some() {
                        bc.trim_start().to_string()
                    } else {
                        bc
                    };
                    ((-y).to_string(), bc)
                } else if y > 0 && y < 1000 {
                    let ad = ctx
                        .get_term("ad", quarto_csl::TermForm::Long, false)
                        .unwrap_or_else(|| "AD".to_string());
                    let ad = if delimiter.is_some() {
                        ad.trim_start().to_string()
                    } else {
                        ad
                    };
                    (y.to_string(), ad)
                } else {
                    (y.to_string(), String::new())
                };
                format!("{}{}", display_year, era_suffix)
            }),
            DatePartName::Month => parts.month.and_then(|m| {
                let form = part.form.unwrap_or(DatePartForm::Long);
                format_month_or_season(ctx, m, form)
            }),
            DatePartName::Day => parts.day.map(|d| {
                let form = part.form.unwrap_or(DatePartForm::Numeric);
                let limit_ordinals = ctx.processor.limit_day_ordinals_to_day_1();
                format_day(d, form, limit_ordinals)
            }),
        };

        if let Some(v) = value {
            let is_first = outputs.is_empty();
            let is_last = idx == num_parts - 1;

            // Add delimiter before this part (except for the first part)
            if !is_first {
                if let Some(d) = delimiter {
                    outputs.push(Output::literal(d));
                }
            }

            // Apply prefix (skip if strip_first_prefix and this is first)
            if !(strip_first_prefix && is_first) {
                if let Some(ref prefix) = part.formatting.prefix {
                    outputs.push(Output::literal(prefix));
                }
            }

            // Apply the value
            let final_value = if part.strip_periods {
                v.replace('.', "")
            } else {
                v
            };
            outputs.push(Output::literal(final_value));

            // Apply suffix (skip if strip_last_suffix and this is last)
            if !(strip_last_suffix && is_last) {
                if let Some(ref suffix) = part.formatting.suffix {
                    outputs.push(Output::literal(suffix));
                }
            }
        }
    }

    Output::sequence(outputs)
}

/// Merge a locale date part with an inline override.
///
/// Per CSL spec, when using a localized date format with inline date-part overrides:
/// - The inline part's non-formatting attributes (form, range_delimiter, strip_periods) override
/// - The formatting is merged: locale's formatting is the base, inline's formatting overrides
///
/// This matches Pandoc citeproc's `addOverride` function behavior where:
/// `x{ dpFormatting = dpFormatting olddp <> dpFormatting x }`
fn merge_date_parts(
    locale_part: &quarto_csl::DatePart,
    inline_part: &quarto_csl::DatePart,
) -> quarto_csl::DatePart {
    quarto_csl::DatePart {
        name: locale_part.name,
        // Use inline's form if specified, otherwise locale's
        form: inline_part.form.or(locale_part.form),
        // Merge formatting: locale is base, inline overrides
        formatting: merge_formatting(&locale_part.formatting, &inline_part.formatting),
        // Use inline's range_delimiter if specified, otherwise locale's
        range_delimiter: inline_part
            .range_delimiter
            .clone()
            .or_else(|| locale_part.range_delimiter.clone()),
        // Use inline's strip_periods if true, otherwise locale's
        strip_periods: inline_part.strip_periods || locale_part.strip_periods,
        source_info: locale_part.source_info.clone(),
    }
}

/// Merge two Formatting structs, where override values take precedence over base values.
fn merge_formatting(
    base: &quarto_csl::Formatting,
    override_fmt: &quarto_csl::Formatting,
) -> quarto_csl::Formatting {
    quarto_csl::Formatting {
        font_style: override_fmt.font_style.or(base.font_style),
        font_variant: override_fmt.font_variant.or(base.font_variant),
        font_weight: override_fmt.font_weight.or(base.font_weight),
        text_decoration: override_fmt.text_decoration.or(base.text_decoration),
        vertical_align: override_fmt.vertical_align.or(base.vertical_align),
        text_case: override_fmt.text_case.or(base.text_case),
        prefix: override_fmt.prefix.clone().or_else(|| base.prefix.clone()),
        suffix: override_fmt.suffix.clone().or_else(|| base.suffix.clone()),
        display: override_fmt.display.or(base.display),
        quotes: override_fmt.quotes || base.quotes,
        strip_periods: override_fmt.strip_periods || base.strip_periods,
        delimiter: override_fmt
            .delimiter
            .clone()
            .or_else(|| base.delimiter.clone()),
        affixes_inside: override_fmt.affixes_inside || base.affixes_inside,
    }
}

/// Format a month or season value according to the specified form.
/// Months 1-12 are regular months, 21-24 are seasons (spring, summer, fall, winter).
fn format_month_or_season(
    ctx: &EvalContext,
    month: i32,
    form: quarto_csl::DatePartForm,
) -> Option<String> {
    use quarto_csl::{DatePartForm, TermForm};

    // Handle seasons (months 21-24 in CSL)
    if (21..=24).contains(&month) {
        let season_num = month - 20; // 21->1, 22->2, 23->3, 24->4
        let term_name = format!("season-{:02}", season_num);
        return ctx.get_term(&term_name, TermForm::Long, false);
    }

    match form {
        DatePartForm::Long => {
            // Get month name from locale (month-01, month-02, etc.)
            let term_name = format!("month-{:02}", month);
            ctx.get_term(&term_name, TermForm::Long, false)
        }
        DatePartForm::Short => {
            // Get abbreviated month name
            let term_name = format!("month-{:02}", month);
            ctx.get_term(&term_name, TermForm::Short, false)
                .or_else(|| ctx.get_term(&term_name, TermForm::Long, false))
        }
        DatePartForm::Numeric => Some(month.to_string()),
        DatePartForm::NumericLeadingZeros => Some(format!("{:02}", month)),
        DatePartForm::Ordinal => {
            // For now, just return numeric
            // TODO: Implement ordinal formatting with locale suffix
            Some(month.to_string())
        }
    }
}

/// Format a day value according to the specified form.
/// If `limit_ordinals_to_day_1` is true, only day 1 gets an ordinal suffix.
fn format_day(day: i32, form: quarto_csl::DatePartForm, limit_ordinals_to_day_1: bool) -> String {
    use quarto_csl::DatePartForm;

    match form {
        DatePartForm::Numeric | DatePartForm::Long | DatePartForm::Short => day.to_string(),
        DatePartForm::NumericLeadingZeros => format!("{:02}", day),
        DatePartForm::Ordinal => {
            // If limit_ordinals_to_day_1 is set, only day 1 gets an ordinal suffix
            if limit_ordinals_to_day_1 && day != 1 {
                return day.to_string();
            }
            // Simple English ordinal suffixes for now
            // TODO: Use locale for ordinal suffixes
            let suffix = match day % 10 {
                1 if day != 11 => "st",
                2 if day != 12 => "nd",
                3 if day != 13 => "rd",
                _ => "th",
            };
            format!("{}{}", day, suffix)
        }
    }
}

/// Convert a year suffix number to a letter (1=a, 2=b, ..., 26=z, 27=aa, 28=ab, ...).
///
/// This follows the CSL spec where suffixes beyond 'z' continue as 'aa', 'ab', etc.
fn suffix_to_letter(suffix: i32) -> String {
    if suffix <= 0 {
        return String::new();
    }

    let suffix = suffix as u32;

    // For suffixes 1-26, return a single letter
    if suffix <= 26 {
        let letter = (b'a' + (suffix - 1) as u8) as char;
        return letter.to_string();
    }

    // For suffixes > 26, use multi-letter suffixes
    // 27 = aa, 28 = ab, ..., 52 = az, 53 = ba, ...
    let mut result = String::new();
    let mut n = suffix - 1; // Convert to 0-indexed

    loop {
        let remainder = n % 26;
        result.insert(0, (b'a' + remainder as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1; // Adjust for 1-indexed letters
    }

    result
}

/// Format a page range value by converting hyphens to the page-range-delimiter.
///
/// This follows Pandoc citeproc's pageRange function:
/// - Splits the input on commas and ampersands (preserving them as literal separators)
/// - For each segment, if it contains a hyphen (but not escaped `\-`), the hyphen
/// Convert an integer to lowercase Roman numerals.
///
/// This follows Pandoc's citeproc implementation. Numbers >= 4000 or <= 0
/// are returned as-is (as decimal strings).
fn to_roman_numeral(mut x: i64) -> String {
    if x <= 0 || x >= 4000 {
        return x.to_string();
    }

    let mut result = String::new();

    // Process each Roman numeral value from largest to smallest
    let numerals = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];

    for (value, numeral) in numerals {
        while x >= value {
            result.push_str(numeral);
            x -= value;
        }
    }

    result
}

/// Format a number value according to its NumberForm.
///
/// Handles:
/// - Numeric: plain number (default)
/// - Roman: lowercase roman numerals (xlii for 42)
/// - Ordinal: number with ordinal suffix (1st, 2nd, 3rd, etc.)
/// - LongOrdinal: spelled-out ordinal for 1-10 (first, second, third, etc.),
///   falls back to ordinal for numbers > 10
///
/// The `get_term` closure is used to look up ordinal terms from the locale.
fn format_number_with_form<F>(value: &str, form: quarto_csl::NumberForm, get_term: F) -> String
where
    F: Fn(&str) -> Option<String>,
{
    match form {
        quarto_csl::NumberForm::Numeric => value.to_string(),
        quarto_csl::NumberForm::Roman => {
            // Try to parse as integer and convert to Roman
            // If it's not a simple integer, return as-is
            if let Ok(n) = value.trim().parse::<i64>() {
                to_roman_numeral(n)
            } else {
                value.to_string()
            }
        }
        quarto_csl::NumberForm::LongOrdinal => {
            // For long-ordinal, numbers 1-10 are spelled out, others fall back to ordinal
            if let Ok(n) = value.trim().parse::<i64>() {
                if n >= 1 && n <= 10 {
                    // Try to look up "long-ordinal-01" through "long-ordinal-10"
                    let term_name = format!("long-ordinal-{:02}", n);
                    if let Some(spelled_out) = get_term(&term_name) {
                        return spelled_out;
                    }
                }
                // Fall back to regular ordinal for n > 10 or if term not found
                format_ordinal(n, &get_term)
            } else {
                value.to_string()
            }
        }
        quarto_csl::NumberForm::Ordinal => {
            if let Ok(n) = value.trim().parse::<i64>() {
                format_ordinal(n, &get_term)
            } else {
                value.to_string()
            }
        }
    }
}

/// Format a number as an ordinal (e.g., 1st, 2nd, 3rd, 42nd).
///
/// Looks up ordinal suffixes from the locale:
/// - "ordinal" is the default suffix (e.g., "th" in English)
/// - "ordinal-NN" overrides for specific numbers (e.g., "ordinal-01" = "st")
/// - For numbers > 10, uses last digit matching (e.g., 42 matches "ordinal-02")
/// - Special cases: 11, 12, 13 use the default suffix in English
fn format_ordinal<F>(n: i64, get_term: &F) -> String
where
    F: Fn(&str) -> Option<String>,
{
    // Try exact match first for small numbers (e.g., "ordinal-11", "ordinal-12", "ordinal-13")
    // These are special cases in English where 11th, 12th, 13th don't follow the pattern
    let two_digit_term = format!("ordinal-{:02}", n.abs() % 100);
    if let Some(suffix) = get_term(&two_digit_term) {
        return format!("{}{}", n, suffix);
    }

    // Try last-digit match (e.g., 42 -> "ordinal-02")
    let last_digit = n.abs() % 10;
    let one_digit_term = format!("ordinal-{:02}", last_digit);
    if let Some(suffix) = get_term(&one_digit_term) {
        return format!("{}{}", n, suffix);
    }

    // Fall back to default ordinal suffix
    if let Some(suffix) = get_term("ordinal") {
        return format!("{}{}", n, suffix);
    }

    // If no ordinal terms found at all, just return the number
    n.to_string()
}

/// Format page ranges according to the style's page-range-format option.
///
/// This function handles:
/// - Multiple ranges separated by comma or ampersand
/// - Page range abbreviation based on format (minimal, chicago, expanded, etc.)
/// - Escaped hyphens `\-` which are converted to literal hyphens
///
/// Following Pandoc citeproc's pageRange function:
/// - Splits on comma and ampersand into separate tokens (including separators)
/// - Strips whitespace from each token
/// - Re-joins tokens with a space delimiter
///
/// The `page_range_delimiter` should typically be the locale's "page-range-delimiter"
/// term, defaulting to en-dash (–) if not specified.
fn format_page_range(
    value: &str,
    page_range_delimiter: &str,
    format: Option<quarto_csl::PageRangeFormat>,
) -> String {
    // Check for escaped hyphen - if present, we need special handling
    let has_escaped = value.contains("\\-");

    // Split into tokens, where commas and ampersands are their own tokens
    // This mirrors Pandoc's T.groupBy approach
    let mut tokens = Vec::new();
    let mut current = String::new();

    for c in value.chars() {
        if c == ',' || c == '&' {
            if !current.is_empty() {
                tokens.push(current);
                current = String::new();
            }
            tokens.push(c.to_string());
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    // Process each token: strip whitespace and format ranges (for non-separator tokens)
    let formatted_tokens: Vec<String> = tokens
        .into_iter()
        .map(|token| {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                String::new()
            } else if trimmed == "," || trimmed == "&" {
                // Separators pass through as-is
                trimmed.to_string()
            } else {
                // Format as page segment
                format_page_segment(trimmed, page_range_delimiter, has_escaped, format)
            }
        })
        .filter(|s| !s.is_empty())
        .collect();

    // Re-join tokens with space delimiter, following Pandoc's addDelimiters logic:
    // - Don't add delimiter before comma, semicolon, or period (these are punctuation)
    // - Add delimiter (space) between all other adjacent tokens
    let mut result = String::new();
    for (i, token) in formatted_tokens.iter().enumerate() {
        if i > 0 {
            // Check if current token starts with punctuation that shouldn't have space before
            let first_char = token.chars().next();
            let is_punct = matches!(first_char, Some(',' | ';' | '.'));
            if !is_punct {
                result.push(' ');
            }
        }
        result.push_str(token);
    }
    result
}

/// Format a single page segment (part between comma/ampersand separators).
/// Following Pandoc citeproc's behavior, strips leading/trailing whitespace from segments.
fn format_page_segment(
    segment: &str,
    delimiter: &str,
    has_escaped: bool,
    format: Option<quarto_csl::PageRangeFormat>,
) -> String {
    // Strip leading and trailing whitespace (matches Pandoc's T.strip behavior)
    let trimmed = segment.trim();

    if trimmed.is_empty() {
        return String::new();
    }

    if has_escaped && trimmed.contains("\\-") {
        // Has escaped hyphen - don't treat as range, just replace \- with -
        return trimmed.replace("\\-", "-");
    }

    // Check for range indicator (hyphen or en-dash)
    let is_dash = |c: char| c == '-' || c == '\u{2013}';

    if let Some(pos) = trimmed.find(is_dash) {
        // Split on the dash
        let (start, rest) = trimmed.split_at(pos);
        let end = rest.trim_start_matches(is_dash);

        // Only treat as range if both parts are non-empty
        let start = start.trim();
        let end = end.trim();
        if !start.is_empty() && !end.is_empty() {
            return format_page_range_pair(start, end, delimiter, format);
        }
    }

    // Not a range, return trimmed value
    trimmed.to_string()
}

/// Format a page range pair (start-end) according to the page-range-format.
///
/// This implements the CSL page range abbreviation algorithms:
/// - `None`: Just join with delimiter, no abbreviation
/// - `Minimal`: Keep only the changed digits (minimum 1)
/// - `MinimalTwo`: Keep at least 2 digits in the second number
/// - `Chicago`: Chicago Manual of Style 15th edition rules
/// - `Expanded`: Expand abbreviated ranges to full form with prefixes
fn format_page_range_pair(
    start: &str,
    end: &str,
    delimiter: &str,
    format: Option<quarto_csl::PageRangeFormat>,
) -> String {
    use quarto_csl::PageRangeFormat;

    // Extract prefix (everything except trailing digits) and trailing digits
    // This matches Pandoc's `dropWhileEnd isDigit` approach.
    // Examples:
    //   "456K200" → prefix = "456K", digits = "200"
    //   "N110"    → prefix = "N", digits = "110"
    //   "99"      → prefix = "", digits = "99"
    //   "xxv"     → prefix = "xxv", digits = "" (non-numeric, like roman numerals)
    let (start_prefix, start_digits) = split_prefix_digits(start);
    let (end_prefix, end_digits) = split_prefix_digits(end);

    // If BOTH parts are purely non-numeric (like roman numerals "xxv-xxviii"),
    // they form a valid range - just join with delimiter.
    if start_digits.is_empty() && end_digits.is_empty() {
        return format!("{}{}{}", start, delimiter, end);
    }

    // If prefixes differ AND at least one part has digits, this is NOT a valid page range.
    // Per CSL spec, preserve the original hyphen character (don't use en-dash).
    // Examples: "N110-P5" (different letter prefixes), "110-N6" (one has prefix, one doesn't)
    if start_prefix != end_prefix {
        return format!("{}-{}", start, end);
    }

    let prefix = start_prefix;

    // If one has digits and one doesn't (but prefixes matched above - unusual case),
    // just join with delimiter.
    if start_digits.is_empty() || end_digits.is_empty() {
        return format!("{}{}{}", start, delimiter, end);
    }

    // Expand the end number if it's shorter than start (abbreviated in input)
    let end_expanded = if end_digits.len() < start_digits.len() {
        let borrow_len = start_digits.len() - end_digits.len();
        format!("{}{}", &start_digits[..borrow_len], end_digits)
    } else {
        end_digits.to_string()
    };

    match format {
        None => {
            // No format specified: just replace delimiter, preserve original abbreviation
            format!("{}{}{}{}", start, delimiter, prefix, end_digits)
        }
        Some(PageRangeFormat::Expanded) => {
            // Expanded: show full numbers with prefixes on both
            format!("{}{}{}{}", start, delimiter, prefix, end_expanded)
        }
        Some(PageRangeFormat::Minimal) => {
            // Minimal: keep only changed digits (minimum 1)
            let abbreviated = minimal_abbreviate(start_digits, &end_expanded, 1);
            format!("{}{}{}", start, delimiter, abbreviated)
        }
        Some(PageRangeFormat::MinimalTwo) => {
            // MinimalTwo: keep at least 2 digits
            let abbreviated = minimal_abbreviate(start_digits, &end_expanded, 2);
            format!("{}{}{}", start, delimiter, abbreviated)
        }
        Some(PageRangeFormat::Chicago15) => {
            // Chicago Manual of Style 15th edition
            let abbreviated = chicago15_abbreviate(start_digits, &end_expanded);
            format!("{}{}{}", start, delimiter, abbreviated)
        }
        Some(PageRangeFormat::Chicago16) => {
            // Chicago Manual of Style 16th edition
            let abbreviated = chicago16_abbreviate(start_digits, &end_expanded);
            format!("{}{}{}", start, delimiter, abbreviated)
        }
    }
}

/// Split a page number into prefix (non-trailing-digits) and trailing digits.
/// For example: "456K200" → ("456K", "200"), "xxv" → ("xxv", "")
fn split_prefix_digits(s: &str) -> (&str, &str) {
    // Find where trailing digits start
    let digit_start = s
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, _)| i)
        .unwrap_or(s.len());

    (&s[..digit_start], &s[digit_start..])
}

/// Minimal abbreviation: keep only the differing suffix, with minimum threshold digits.
fn minimal_abbreviate(start: &str, end: &str, threshold: usize) -> String {
    // If end is longer than start, this isn't an abbreviated range - return as-is
    if end.len() > start.len() {
        return end.to_string();
    }

    // Find common prefix length
    let common_len = start
        .chars()
        .zip(end.chars())
        .take_while(|(a, b)| a == b)
        .count();

    // The rest of the end number after the common prefix
    let rest_len = end.len() - common_len;

    if rest_len < threshold && end.len() >= threshold {
        // If the differing part is shorter than threshold, take last `threshold` chars
        end.chars().skip(end.len() - threshold).collect()
    } else {
        // Otherwise, take the differing part
        end.chars().skip(common_len).collect()
    }
}

/// Chicago Manual of Style 15th edition abbreviation rules.
///
/// Rules:
/// - Less than 3 digits: use all digits
/// - Ends in 00: use all digits
/// - 01-09 range in last two digits: use changed digits only (minimal with threshold 1)
/// - 4 digits with 3+ changed: use all digits (this rule is 15th edition specific)
/// - Otherwise: use at least 2 digits (minimal with threshold 2)
fn chicago15_abbreviate(start: &str, end: &str) -> String {
    // If end is longer than start, this isn't an abbreviated range - return as-is
    if end.len() > start.len() {
        return end.to_string();
    }

    let start_len = start.len();

    // Less than 3 digits: use all
    if start_len < 3 {
        return end.to_string();
    }

    // Ends in 00: use all
    if start.ends_with("00") {
        return end.to_string();
    }

    // Check if both have 0 as the tens digit (01-09 pattern)
    // This means we're in a range like 101-109, 201-208, etc.
    if start_len >= 2 && end.len() >= 2 {
        let start_tens = start.chars().nth(start_len - 2);
        let end_tens = end.chars().nth(end.len() - 2);
        if start_tens == Some('0') && end_tens == Some('0') {
            // Use minimal with threshold 1
            return minimal_abbreviate(start, end, 1);
        }
    }

    // Chicago 15th edition specific: 4-digit numbers with 3+ changed digits use all
    if start_len == 4 {
        let changed = start
            .chars()
            .zip(end.chars())
            .filter(|(a, b)| a != b)
            .count();
        if changed >= 3 {
            return end.to_string();
        }
    }

    // Default: minimal with threshold 2
    minimal_abbreviate(start, end, 2)
}

/// Chicago Manual of Style 16th edition abbreviation rules.
///
/// Rules (simpler than 15th edition):
/// - Less than 3 digits: use all digits
/// - Ends in 00: use all digits
/// - 01-09 range in last two digits: use changed digits only (minimal with threshold 1)
/// - Otherwise: use at least 2 digits (minimal with threshold 2)
fn chicago16_abbreviate(start: &str, end: &str) -> String {
    // If end is longer than start, this isn't an abbreviated range - return as-is
    if end.len() > start.len() {
        return end.to_string();
    }

    let start_len = start.len();

    // Less than 3 digits: use all
    if start_len < 3 {
        return end.to_string();
    }

    // Ends in 00: use all
    if start.ends_with("00") {
        return end.to_string();
    }

    // Check if both have 0 as the tens digit (01-09 pattern)
    // This means we're in a range like 101-109, 201-208, etc.
    if start_len >= 2 && end.len() >= 2 {
        let start_tens = start.chars().nth(start_len - 2);
        let end_tens = end.chars().nth(end.len() - 2);
        if start_tens == Some('0') && end_tens == Some('0') {
            // Use minimal with threshold 1
            return minimal_abbreviate(start, end, 1);
        }
    }

    // Default: minimal with threshold 2
    minimal_abbreviate(start, end, 2)
}

/// Check if a string value represents a numeric value according to CSL.
///
/// This implements the CSL is-numeric test, which matches Pandoc citeproc's behavior:
/// 1. Split the string by `,`, `-`, `&`, and en-dash (`\u{2013}`)
/// 2. For each chunk:
///    - Trim whitespace
///    - Strip leading letters (a-z, A-Z)
///    - Strip trailing letters (a-z, A-Z)
///    - The remaining core must be non-empty and all digits
/// 3. All chunks must pass for the string to be "numeric"
///
/// Examples of numeric strings: "5", "5th", "3rd", "1-10", "5, 10, 15"
/// Examples of non-numeric strings: "Fifth", "5a5", "annotated edition"
pub fn is_numeric_string(s: &str) -> bool {
    // Split by delimiters: comma, hyphen, ampersand, en-dash
    let chunks = s.split(|c| c == ',' || c == '-' || c == '&' || c == '\u{2013}');

    let mut has_chunks = false;
    for chunk in chunks {
        has_chunks = true;
        if !is_numeric_chunk(chunk) {
            return false;
        }
    }

    // Empty string has no chunks and is not numeric
    has_chunks
}

/// Check if a single chunk (after splitting by delimiters) is numeric.
///
/// Algorithm:
/// 1. Trim whitespace
/// 2. Strip leading letters
/// 3. Strip trailing letters
/// 4. The core must be non-empty and consist entirely of digits
fn is_numeric_chunk(chunk: &str) -> bool {
    let trimmed = chunk.trim();

    // Strip leading letters
    let after_leading = trimmed.trim_start_matches(|c: char| c.is_ascii_alphabetic());

    // Strip trailing letters
    let core = after_leading.trim_end_matches(|c: char| c.is_ascii_alphabetic());

    // Core must be non-empty and all digits
    !core.is_empty() && core.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::Reference;
    use quarto_csl::parse_csl;

    fn create_test_processor() -> Processor {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <citation>
    <layout>
      <group delimiter=", ">
        <names variable="author">
          <name form="short"/>
        </names>
        <date variable="issued">
          <date-part name="year"/>
        </date>
      </group>
    </layout>
  </citation>
</style>"#;

        let style = parse_csl(csl).unwrap();
        Processor::new(style)
    }

    #[test]
    fn test_basic_citation() {
        let mut processor = create_test_processor();

        let reference: Reference = serde_json::from_str(
            r#"{
            "id": "smith2020",
            "type": "book",
            "author": [{"family": "Smith", "given": "John"}],
            "issued": {"date-parts": [[2020]]}
        }"#,
        )
        .unwrap();

        processor.add_reference(reference);

        let citation = Citation {
            items: vec![crate::types::CitationItem {
                id: "smith2020".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let result = processor.process_citation(&citation).unwrap();
        assert_eq!(result, "Smith, 2020");
    }

    #[test]
    fn test_initialize_name() {
        // Note: trailing space is trimmed
        // Basic initialization with initialize_with_hyphen=true (default)
        assert_eq!(initialize_name("John", ". ", true), "J.");
        assert_eq!(initialize_name("John William", ". ", true), "J. W.");
        assert_eq!(initialize_name("J.", ". ", true), "J.");

        // Hyphenated names with uppercase second part
        assert_eq!(initialize_name("John-Lee", ". ", true), "J.-L.");
        assert_eq!(initialize_name("John-Lee", ". ", false), "J.L.");

        // Hyphenated names with lowercase second part - skipped entirely
        assert_eq!(initialize_name("Guo-ping", ". ", true), "G.");
        assert_eq!(initialize_name("Guo-ping", ". ", false), "G.");
        assert_eq!(initialize_name("Guo-ping", "", true), "G");
        assert_eq!(initialize_name("Guo-ping", "", false), "G");
    }

    #[test]
    fn test_format_single_name_short() {
        let name = crate::reference::Name {
            family: Some("Smith".to_string()),
            given: Some("John".to_string()),
            ..Default::default()
        };

        let options = quarto_csl::InheritableNameOptions {
            form: Some(quarto_csl::NameForm::Short),
            ..Default::default()
        };

        assert_eq!(
            format_single_name_to_string(&name, &options, false, None, true),
            "Smith"
        );
    }

    #[test]
    fn test_format_single_name_inverted() {
        let name = crate::reference::Name {
            family: Some("Smith".to_string()),
            given: Some("John".to_string()),
            ..Default::default()
        };

        let options = quarto_csl::InheritableNameOptions::default();

        // Normal order: Given Family
        assert_eq!(
            format_single_name_to_string(&name, &options, false, None, true),
            "John Smith"
        );
        // Inverted order: Family, Given
        assert_eq!(
            format_single_name_to_string(&name, &options, true, None, true),
            "Smith, John"
        );
    }

    #[test]
    fn test_suffix_to_letter() {
        // Basic single letters
        assert_eq!(suffix_to_letter(1), "a");
        assert_eq!(suffix_to_letter(2), "b");
        assert_eq!(suffix_to_letter(26), "z");

        // Multi-letter suffixes beyond z
        assert_eq!(suffix_to_letter(27), "aa");
        assert_eq!(suffix_to_letter(28), "ab");
        assert_eq!(suffix_to_letter(52), "az");
        assert_eq!(suffix_to_letter(53), "ba");

        // Edge cases
        assert_eq!(suffix_to_letter(0), "");
        assert_eq!(suffix_to_letter(-1), "");
    }

    #[test]
    fn test_is_numeric_string() {
        // Basic numeric values
        assert!(is_numeric_string("5"));
        assert!(is_numeric_string("123"));

        // Numbers with ordinal suffixes (the key fix)
        assert!(is_numeric_string("5th"));
        assert!(is_numeric_string("3rd"));
        assert!(is_numeric_string("1st"));
        assert!(is_numeric_string("2nd"));

        // Numbers with prefixes
        assert!(is_numeric_string("vol5"));
        assert!(is_numeric_string("ch10"));

        // Numbers with both prefix and suffix
        assert!(is_numeric_string("vol5th")); // vol + 5 + th

        // Ranges with hyphens
        assert!(is_numeric_string("1-10"));
        assert!(is_numeric_string("5th-10th"));
        assert!(is_numeric_string("1-2-3"));

        // Comma-separated values
        assert!(is_numeric_string("5, 10, 15"));
        assert!(is_numeric_string("5th, 10th"));

        // Ampersand-separated values
        assert!(is_numeric_string("5 & 10"));

        // En-dash ranges
        assert!(is_numeric_string("5\u{2013}10"));

        // Non-numeric strings
        assert!(!is_numeric_string("Fifth")); // No digits at all
        assert!(!is_numeric_string("annotated edition")); // Letters with space, no core digits
        assert!(!is_numeric_string("5a5")); // Letter in the middle
        assert!(!is_numeric_string("abc")); // Pure letters
        assert!(!is_numeric_string("")); // Empty string
        assert!(!is_numeric_string("   ")); // Only whitespace
        assert!(!is_numeric_string("Fifth ed.")); // "Fifth " has no digits after stripping

        // Edge case: version strings with complex structure
        assert!(!is_numeric_string(
            "version: 2002, amended effective June 1"
        ));
    }

    #[test]
    fn test_is_numeric_chunk() {
        // Basic digits
        assert!(is_numeric_chunk("5"));
        assert!(is_numeric_chunk("123"));

        // With leading letters
        assert!(is_numeric_chunk("vol5"));

        // With trailing letters
        assert!(is_numeric_chunk("5th"));

        // With both
        assert!(is_numeric_chunk("vol5th"));

        // With whitespace
        assert!(is_numeric_chunk("  5  "));
        assert!(is_numeric_chunk("  5th  "));

        // No digits
        assert!(!is_numeric_chunk("abc"));
        assert!(!is_numeric_chunk(""));
        assert!(!is_numeric_chunk("   "));

        // Letter in the middle
        assert!(!is_numeric_chunk("5a5"));
    }
}
