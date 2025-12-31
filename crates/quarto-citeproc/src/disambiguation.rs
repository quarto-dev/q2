//! Disambiguation algorithm for CSL citations.
//!
//! This module implements the CSL disambiguation algorithm that resolves
//! ambiguous citations (citations that render identically but refer to
//! different works).
//!
//! The algorithm applies disambiguation methods in order:
//! 1. Add names (expand et-al truncated lists)
//! 2. Add given names (expand initials to full names)
//! 3. Add year suffixes (a, b, c...)
//! 4. Set disambiguate condition (for fallback rendering)

use crate::output::{CitationItemType, Output};
use crate::reference::{Name, NameHint};
use crate::types::Processor;
use quarto_csl::GivenNameDisambiguationRule;
use std::collections::{HashMap, HashSet};

/// Data extracted from a rendered citation for disambiguation analysis.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisambData {
    /// The reference ID this citation refers to.
    pub item_id: String,
    /// Names extracted from the citation (in order).
    pub names: Vec<Name>,
    /// The rendered text of the citation (for grouping ambiguous items).
    pub rendered: String,
}

/// Convert a tagged citation item to DisambData.
///
/// This extracts the item ID, names, and rendered text from a tagged Output.
pub fn to_disamb_data(item_id: String, output: &Output) -> DisambData {
    DisambData {
        item_id,
        names: output.extract_all_names(),
        rendered: output.render(),
    }
}

/// Extract DisambData from all citation items in a list of rendered outputs.
///
/// This mirrors the Haskell `extractTagItems` and `toDisambData` functions.
pub fn extract_disamb_data(outputs: &[Output]) -> Vec<DisambData> {
    let mut result = Vec::new();

    for output in outputs {
        // Extract all tagged items from this output
        for (item_id, item_type, item_output) in output.extract_citation_items() {
            // Only include normal citations (not author-only or suppress-author)
            if item_type == CitationItemType::NormalCite {
                result.push(to_disamb_data(item_id, &item_output));
            }
        }
    }

    result
}

/// Extract DisambData from all citation items, using the processor to get
/// names from references directly (needed when collapsing may hide names).
///
/// This version is more robust for year-suffix disambiguation because
/// it ensures all items have their names even when cite-group-delimiter
/// collapsing suppresses repeated author names.
pub fn extract_disamb_data_with_processor(
    outputs: &[Output],
    processor: &crate::types::Processor,
) -> Vec<DisambData> {
    let mut result = Vec::new();

    for output in outputs {
        // Extract all tagged items from this output
        for (item_id, item_type, item_output) in output.extract_citation_items() {
            // Only include normal citations (not author-only or suppress-author)
            if item_type == CitationItemType::NormalCite {
                // Get names from the reference rather than the output
                // This handles cases where collapsing suppresses author names
                let names = if let Some(reference) = processor.get_reference(&item_id) {
                    reference
                        .author.clone()
                        .unwrap_or_default()
                } else {
                    item_output.extract_all_names()
                };

                result.push(DisambData {
                    item_id,
                    names,
                    rendered: item_output.render(),
                });
            }
        }
    }

    result
}

/// Find groups of ambiguous citations.
///
/// Returns groups of citations that render identically but refer to different works.
/// Each inner Vec contains DisambData for citations that are ambiguous with each other.
///
/// This matches Pandoc's `getAmbiguities` function: simply group by rendered text
/// and return groups with multiple unique item IDs. The previous two-stage approach
/// (grouping by author names first) was incorrect because it prevented detecting
/// ambiguity between items with different authors but identical rendered output
/// (e.g., "Smith et al." from two different author lists).
pub fn find_ambiguities(items: Vec<DisambData>) -> Vec<Vec<DisambData>> {
    // Group by rendered text only - this is what Pandoc does
    // See: external-sources/citeproc/src/Citeproc/Eval.hs getAmbiguities (line 732)
    let mut render_groups: HashMap<String, Vec<DisambData>> = HashMap::new();

    for data in items {
        render_groups
            .entry(data.rendered.clone())
            .or_default()
            .push(data);
    }

    // Return groups with >1 unique item ID
    // (Same rendered text but different reference IDs = ambiguous)
    render_groups
        .into_values()
        .filter(|group| {
            let mut unique_ids: Vec<&str> = group.iter().map(|d| d.item_id.as_str()).collect();
            unique_ids.sort();
            unique_ids.dedup();
            unique_ids.len() > 1
        })
        .collect()
}

/// Find year-suffix ambiguities: items by the same author in the same year.
///
/// This is specifically for year-suffix disambiguation, where we need to
/// identify works by the same author in the same year, even if their
/// rendered output differs due to collapsing.
pub fn find_year_suffix_ambiguities(
    items: Vec<DisambData>,
    processor: &crate::types::Processor,
) -> Vec<Vec<DisambData>> {
    // Group by author family names + year
    let mut groups: HashMap<String, Vec<DisambData>> = HashMap::new();

    for data in items {
        // Get year from reference
        // date_parts is Vec<Vec<i32>> where inner vec is [year, month?, day?]
        let year: Option<i32> = processor
            .get_reference(&data.item_id)
            .and_then(|r| r.issued.as_ref())
            .and_then(|d| d.date_parts.as_ref())
            .and_then(|parts| parts.first())
            .and_then(|part| part.first().copied());

        // Create key from author family names + year
        let family_names: Vec<&str> = data
            .names
            .iter()
            .filter_map(|n| n.family.as_deref())
            .collect();
        let name_part = family_names.join("|");

        let key = match year {
            Some(y) => format!("{}#{}", name_part, y),
            None => format!("{}#NOYEAR", name_part),
        };

        groups.entry(key).or_default().push(data);
    }

    // Filter to groups with >1 unique item
    groups
        .into_values()
        .filter(|group| {
            let mut unique_ids: Vec<&str> = group.iter().map(|d| d.item_id.as_str()).collect();
            unique_ids.sort();
            unique_ids.dedup();
            unique_ids.len() > 1
        })
        .collect()
}

/// Find year-suffix ambiguities using FULL author match (family + given names).
///
/// This is more precise than `find_year_suffix_ambiguities` because it only groups
/// items that have truly identical author lists, not just matching family names.
/// This prevents adding year suffixes when given names differ (e.g., "A. Smith 2001"
/// vs "B. Smith 2001" should be disambiguated by givenname, not year suffix).
///
/// This function is needed for collapsed citations where the rendered text may differ
/// due to author name suppression (e.g., "Brown, 2006; Brown, 2006" becomes
/// "Brown, 2006, 2006" where the second item has no author in rendered text).
pub fn find_year_suffix_with_full_author_match(
    items: Vec<DisambData>,
    processor: &crate::types::Processor,
) -> Vec<Vec<DisambData>> {
    // Group by full author list (family + given) + year
    let mut groups: HashMap<String, Vec<DisambData>> = HashMap::new();

    for data in items {
        // Get year from reference
        let year: Option<i32> = processor
            .get_reference(&data.item_id)
            .and_then(|r| r.issued.as_ref())
            .and_then(|d| d.date_parts.as_ref())
            .and_then(|parts| parts.first())
            .and_then(|part| part.first().copied());

        // Create key from FULL author names (family + given) + year
        // This ensures "A. Smith" and "B. Smith" are in different groups
        let author_key: String = data
            .names
            .iter()
            .map(|n| {
                let family = n.family.as_deref().unwrap_or("");
                let given = n.given.as_deref().unwrap_or("");
                format!("{}|{}", family, given)
            })
            .collect::<Vec<_>>()
            .join("||");

        let key = match year {
            Some(y) => format!("{}#{}", author_key, y),
            None => format!("{}#NOYEAR", author_key),
        };

        groups.entry(key).or_default().push(data);
    }

    // Filter to groups with >1 unique item
    groups
        .into_values()
        .filter(|group| {
            let mut unique_ids: Vec<&str> = group.iter().map(|d| d.item_id.as_str()).collect();
            unique_ids.sort();
            unique_ids.dedup();
            unique_ids.len() > 1
        })
        .collect()
}

/// Merge two sets of ambiguity groups, combining items that appear in either set.
///
/// This is used to combine rendered-text-based ambiguities with author+year-based
/// ambiguities, ensuring year suffixes are added when needed for either case.
///
/// IMPORTANT: This preserves the order of items from groups1 (first priority) and
/// groups2 (second priority) because year suffix assignment depends on citation order.
pub fn merge_ambiguity_groups(
    groups1: &[Vec<DisambData>],
    groups2: &[Vec<DisambData>],
) -> Vec<Vec<DisambData>> {
    // Collect all item IDs in order of first appearance (preserves citation order)
    let mut item_order: Vec<String> = Vec::new();
    let mut all_items: HashMap<String, DisambData> = HashMap::new();

    // Process groups1 first (primary source, preserves citation order)
    for group in groups1.iter() {
        for data in group {
            if !all_items.contains_key(&data.item_id) {
                item_order.push(data.item_id.clone());
            }
            all_items.insert(data.item_id.clone(), data.clone());
        }
    }

    // Process groups2 (secondary source)
    for group in groups2.iter() {
        for data in group {
            if !all_items.contains_key(&data.item_id) {
                item_order.push(data.item_id.clone());
            }
            all_items.insert(data.item_id.clone(), data.clone());
        }
    }

    // Use union-find to merge items that share any group
    let mut parent: HashMap<String, String> = HashMap::new();
    for id in all_items.keys() {
        parent.insert(id.clone(), id.clone());
    }

    fn find(parent: &mut HashMap<String, String>, x: &str) -> String {
        if parent[x] != x {
            let p = find(parent, &parent[x].clone());
            parent.insert(x.to_string(), p.clone());
            p
        } else {
            x.to_string()
        }
    }

    fn union(parent: &mut HashMap<String, String>, x: &str, y: &str) {
        let px = find(parent, x);
        let py = find(parent, y);
        if px != py {
            parent.insert(px, py);
        }
    }

    // Union items that share any group
    for group in groups1.iter().chain(groups2.iter()) {
        if group.len() > 1 {
            let first = &group[0].item_id;
            for item in group.iter().skip(1) {
                union(&mut parent, first, &item.item_id);
            }
        }
    }

    // Collect items by their root, preserving the original item order
    let mut result_groups: HashMap<String, Vec<DisambData>> = HashMap::new();
    for id in &item_order {
        if let Some(data) = all_items.get(id) {
            let root = find(&mut parent, id);
            result_groups.entry(root).or_default().push(data.clone());
        }
    }

    // Collect groups in deterministic order (by first item's position in item_order)
    let mut groups_with_order: Vec<(usize, Vec<DisambData>)> = result_groups
        .into_values()
        .filter(|group| {
            let mut unique_ids: Vec<&str> = group.iter().map(|d| d.item_id.as_str()).collect();
            unique_ids.sort();
            unique_ids.dedup();
            unique_ids.len() > 1
        })
        .map(|group| {
            // Find the earliest position of any item in this group
            let min_pos = group
                .iter()
                .filter_map(|d| item_order.iter().position(|id| id == &d.item_id))
                .min()
                .unwrap_or(usize::MAX);
            (min_pos, group)
        })
        .collect();

    // Sort groups by first appearance order
    groups_with_order.sort_by_key(|(pos, _)| *pos);

    groups_with_order
        .into_iter()
        .map(|(_, group)| group)
        .collect()
}

/// Find groups of ambiguous citations (simple version from strings without names).
pub fn find_ambiguities_simple(items: &[(String, String)]) -> Vec<Vec<DisambData>> {
    let disamb_items: Vec<_> = items
        .iter()
        .map(|(id, rendered)| DisambData {
            item_id: id.clone(),
            names: Vec::new(),
            rendered: rendered.clone(),
        })
        .collect();
    find_ambiguities(disamb_items)
}

/// Try to disambiguate by adding more names from et-al truncated lists.
///
/// This incrementally increases `et_al_use_first` until disambiguation is achieved
/// or maximum names is reached.
pub fn try_add_names(
    processor: &mut Processor,
    ambiguities: &[Vec<DisambData>],
    givenname_rule: Option<GivenNameDisambiguationRule>,
) {
    for group in ambiguities {
        if group.len() < 2 {
            continue;
        }

        // Find the maximum number of names in any reference
        let max_names = group.iter().map(|d| d.names.len()).max().unwrap_or(0);

        // Get unique item IDs in this group
        let mut item_ids: HashSet<&str> = group.iter().map(|d| d.item_id.as_str()).collect();

        // Try increasing et_al_use_first from 1 up to max_names
        for n in 1..=max_names {
            // Check which items (that are still undisambiguated) would be disambiguated at this level
            // Important: only check items still in item_ids, not already-disambiguated items
            let disambiguated: Vec<&str> = group
                .iter()
                .filter(|d| item_ids.contains(d.item_id.as_str())) // Only check remaining items
                .filter(|d| is_disambiguated_at_name_count(d, group, n, givenname_rule))
                .map(|d| d.item_id.as_str())
                .collect();

            if !disambiguated.is_empty() {
                // Set et_al_names for all items in the group (not just disambiguated ones)
                // This ensures consistent rendering
                for id in &item_ids {
                    processor.set_et_al_names(id, n as u32);
                }

                // Remove disambiguated items from consideration
                for id in &disambiguated {
                    item_ids.remove(id);
                }

                if item_ids.is_empty() {
                    break; // All items disambiguated
                }
            }
        }
    }
}

/// Check if a citation would be disambiguated from others by showing n names.
fn is_disambiguated_at_name_count(
    item: &DisambData,
    group: &[DisambData],
    n: usize,
    givenname_rule: Option<GivenNameDisambiguationRule>,
) -> bool {
    let item_name_signature = get_name_signature(&item.names, n, givenname_rule);

    for other in group {
        if other.item_id == item.item_id {
            continue;
        }

        let other_signature = get_name_signature(&other.names, n, givenname_rule);
        if item_name_signature == other_signature {
            return false; // Still ambiguous with at least one other item
        }
    }

    true // Disambiguated from all other items
}

/// Get a signature for names that can be compared for disambiguation.
/// The signature depends on the givenname rule.
fn get_name_signature(
    names: &[Name],
    count: usize,
    givenname_rule: Option<GivenNameDisambiguationRule>,
) -> Vec<(Option<String>, Option<String>)> {
    names
        .iter()
        .take(count)
        .enumerate()
        .map(|(i, name)| {
            let family = name.family.clone();
            let given = match givenname_rule {
                Some(GivenNameDisambiguationRule::AllNames) => name.given.clone(),
                Some(GivenNameDisambiguationRule::AllNamesWithInitials) => {
                    name.given.as_ref().map(|g| get_initials(g))
                }
                Some(GivenNameDisambiguationRule::PrimaryName) => {
                    if i == 0 {
                        name.given.clone()
                    } else {
                        None
                    }
                }
                Some(GivenNameDisambiguationRule::PrimaryNameWithInitials) => {
                    if i == 0 {
                        name.given.as_ref().map(|g| get_initials(g))
                    } else {
                        None
                    }
                }
                Some(GivenNameDisambiguationRule::ByCite) => name.given.clone(),
                None => None, // No given name consideration
            };
            (family, given)
        })
        .collect()
}

/// Get initials from a given name.
/// This normalizes spacing in initials (e.g., "J.J." and "J. J." both become "J. J.")
fn get_initials(given: &str) -> String {
    // First, normalize: split on whitespace OR on periods
    // This handles both "John Paul" and "J.P." and "J. P."
    let parts: Vec<&str> = given
        .split(|c: char| c.is_whitespace() || c == '.')
        .filter(|s| !s.is_empty())
        .collect();

    parts
        .iter()
        .filter_map(|part| part.chars().next())
        .map(|c| format!("{}.", c))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Normalize a given name for comparison in disambiguation.
/// This collapses whitespace and standardizes period placement so that
/// "J. J." and "J.J." are treated as equivalent.
fn normalize_given_name(given: &str) -> String {
    // Split on whitespace and periods, then rejoin with standard formatting
    let parts: Vec<&str> = given
        .split(|c: char| c.is_whitespace() || c == '.')
        .filter(|s| !s.is_empty())
        .collect();

    // Rejoin with consistent formatting: "Word. Word." for initials, "Word Word" for names
    parts
        .iter()
        .map(|part| {
            if part.len() == 1 {
                // Single character = initial
                format!("{}.", part)
            } else {
                (*part).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Try to disambiguate by adding given names based on the disambiguation rule.
///
/// For each name position, check if adding initials or full given names would disambiguate.
pub fn try_add_given_names_with_rule(
    processor: &mut Processor,
    ambiguities: &[Vec<DisambData>],
    rule: GivenNameDisambiguationRule,
) {
    for group in ambiguities {
        if group.len() < 2 {
            continue;
        }

        // Find the maximum number of names in any reference
        let max_names = group.iter().map(|d| d.names.len()).max().unwrap_or(0);

        // For PrimaryName variants, only process the first name position
        let positions_to_check = match rule {
            GivenNameDisambiguationRule::PrimaryName
            | GivenNameDisambiguationRule::PrimaryNameWithInitials => 1,
            _ => max_names,
        };

        // For each name position
        for name_idx in 0..positions_to_check {
            // Collect names at this position with their item IDs
            let names_at_position: Vec<(&str, Option<&Name>)> = group
                .iter()
                .map(|d| (d.item_id.as_str(), d.names.get(name_idx)))
                .collect();

            // For each item, check if it needs disambiguation
            for (item_id, maybe_name) in &names_at_position {
                if let Some(name) = maybe_name {
                    let hint = compute_name_hint_with_rule(name, &names_at_position, rule);
                    if let Some(h) = hint {
                        processor.set_name_hint(item_id, name, h);
                    }
                }
            }
        }
    }
}

/// Compute what hint (if any) is needed for a name to disambiguate it,
/// based on the givenname-disambiguation-rule.
fn compute_name_hint_with_rule(
    name: &Name,
    all_names: &[(&str, Option<&Name>)],
    rule: GivenNameDisambiguationRule,
) -> Option<NameHint> {
    // Find other names with the same family name
    let family_matches: Vec<&Name> = all_names
        .iter()
        .filter_map(|(_, maybe_n)| *maybe_n)
        .filter(|n| *n != name && n.family == name.family)
        .collect();

    if family_matches.is_empty() {
        return None; // No disambiguation needed
    }

    // For "with-initials" variants, always use initials
    let use_initials_only = matches!(
        rule,
        GivenNameDisambiguationRule::AllNamesWithInitials
            | GivenNameDisambiguationRule::PrimaryNameWithInitials
    );

    // Check if initials would disambiguate
    let name_initials = name.given.as_ref().map(|g| get_initials(g));
    let initials_disambiguate = family_matches.iter().all(|other| {
        let other_initials = other.given.as_ref().map(|g| get_initials(g));
        name_initials != other_initials
    });

    if use_initials_only || initials_disambiguate {
        Some(NameHint::AddInitials)
    } else {
        // Need full given name
        Some(NameHint::AddGivenName)
    }
}

/// Assign year suffixes to ambiguous references.
///
/// This assigns suffixes (1=a, 2=b, etc.) based on bibliography sort order.
/// References that render identically get sequential suffixes.
pub fn assign_year_suffixes(
    processor: &Processor,
    ambiguities: &[Vec<DisambData>],
) -> HashMap<String, i32> {
    let mut suffixes: HashMap<String, i32> = HashMap::new();

    for group in ambiguities {
        // Collect unique item IDs in citation order (order they appear in the group)
        // This preserves the citation order, which is the default tiebreaker
        let mut seen = std::collections::HashSet::new();
        let mut item_ids_in_citation_order: Vec<&str> = Vec::new();
        for d in group {
            if seen.insert(d.item_id.as_str()) {
                item_ids_in_citation_order.push(d.item_id.as_str());
            }
        }

        if item_ids_in_citation_order.len() < 2 {
            continue; // Not actually ambiguous
        }

        // Build items with sort keys, preserving citation order index for tiebreaking
        let mut sorted_items: Vec<_> = item_ids_in_citation_order
            .iter()
            .enumerate()
            .filter_map(|(citation_order, id)| {
                processor.get_reference(id).map(|_r| {
                    let sort_key = processor.get_bib_sort_key(id);
                    (id, sort_key, citation_order)
                })
            })
            .collect();

        // Sort by bibliography sort key, then by citation order as tiebreaker
        sorted_items.sort_by(|a, b| {
            let key_cmp = crate::types::compare_sort_keys(&a.1, &b.1);
            if key_cmp == std::cmp::Ordering::Equal {
                a.2.cmp(&b.2) // Use citation order as tiebreaker
            } else {
                key_cmp
            }
        });

        // Assign sequential suffixes
        for (idx, (id, _, _)) in sorted_items.iter().enumerate() {
            suffixes.insert((*id).to_string(), (idx + 1) as i32);
        }
    }

    suffixes
}

/// Set the disambiguate condition for remaining ambiguous items.
pub fn set_disambiguate_condition(processor: &mut Processor, ambiguities: &[Vec<DisambData>]) {
    for group in ambiguities {
        for item in group {
            processor.set_disamb_condition(&item.item_id, true);
        }
    }
}

/// Apply disambiguation to a processor's references (legacy string-based version).
///
/// This is the legacy entry point that works with pre-rendered strings.
/// Prefer `disambiguate_citations_from_outputs` when Output ASTs are available.
#[allow(dead_code)]
pub fn disambiguate_citations(processor: &mut Processor, citation_renderings: &[(String, String)]) {
    let strategy = &processor.style.citation.disambiguation;
    let add_names = strategy.add_names;
    let add_givenname = strategy.add_givenname;
    let add_year_suffix = strategy.add_year_suffix;

    if !add_names && add_givenname.is_none() && !add_year_suffix {
        return; // No disambiguation methods enabled
    }

    // Find ambiguous citations (simple version for now, without names)
    let ambiguities = find_ambiguities_simple(citation_renderings);

    if ambiguities.is_empty() {
        return; // No ambiguities to resolve
    }

    // Legacy version: no global name disambiguation (no names available)
    apply_disambiguation(
        processor,
        ambiguities,
        &[],
        add_names,
        add_givenname,
        add_year_suffix,
    );
}

/// Apply disambiguation to a processor's references using Output ASTs.
///
/// This is the preferred entry point that extracts names from the Output AST
/// for proper name-based disambiguation.
///
/// Note: This function ALWAYS detects ambiguities and sets the `disamb_condition`
/// flag, even if no explicit disambiguation methods are enabled. This is required
/// for the `<if disambiguate="true">` condition to work in CSL styles.
pub fn disambiguate_citations_from_outputs(processor: &mut Processor, outputs: &[Output]) {
    let strategy = &processor.style.citation.disambiguation;
    let add_names = strategy.add_names;
    let add_givenname = strategy.add_givenname;
    let add_year_suffix = strategy.add_year_suffix;

    // Extract DisambData from the Output ASTs (includes names!)
    let disamb_data = extract_disamb_data(outputs);

    // Find ambiguous citations
    let ambiguities = find_ambiguities(disamb_data.clone());

    // Apply disambiguation (global name disambiguation runs even without ambiguities)
    // Note: We always run this, even if no explicit methods are enabled, because
    // the disambiguate condition (`<if disambiguate="true">`) needs to be set.
    apply_disambiguation(
        processor,
        ambiguities,
        &disamb_data,
        add_names,
        add_givenname,
        add_year_suffix,
    );
}

/// Apply disambiguation methods to resolve ambiguities.
fn apply_disambiguation(
    processor: &mut Processor,
    ambiguities: Vec<Vec<DisambData>>,
    all_disamb_data: &[DisambData],
    add_names: bool,
    add_givenname: Option<GivenNameDisambiguationRule>,
    add_year_suffix: bool,
) {
    // Apply disambiguation methods in order

    // 1. For non-ByCite rules, apply global name disambiguation first
    // This adds given names to distinguish people with the same last name
    // across ALL citations, not just ambiguous ones
    if let Some(rule) = add_givenname
        && rule != GivenNameDisambiguationRule::ByCite {
            apply_global_name_disambiguation(processor, all_disamb_data, rule);
        }

    // 2. Add names (expand et-al)
    if add_names {
        try_add_names(processor, &ambiguities, add_givenname);
        // TODO: Re-render and refresh ambiguities
    }

    // 3. Add given names (ByCite rule only - per-ambiguity-group)
    // For other rules, this was already handled globally above
    if let Some(GivenNameDisambiguationRule::ByCite) = add_givenname {
        try_add_given_names_with_rule(processor, &ambiguities, GivenNameDisambiguationRule::ByCite);
        // TODO: Re-render and refresh ambiguities
    }

    // 4. Add year suffixes
    if add_year_suffix {
        let suffixes = assign_year_suffixes(processor, &ambiguities);
        for (item_id, suffix) in suffixes {
            processor.set_year_suffix(&item_id, suffix);
        }
        // TODO: Re-render and refresh ambiguities
    }

    // 5. Set disambiguate condition for any remaining ambiguities
    // (For now, we set it on all ambiguous items since we don't re-render)
    set_disambiguate_condition(processor, &ambiguities);
}

/// Apply global name disambiguation for non-ByCite rules.
///
/// This finds all names that share a family name across ALL citations
/// and sets hints to add given names/initials to distinguish them.
pub fn apply_global_name_disambiguation(
    processor: &mut Processor,
    all_disamb_data: &[DisambData],
    rule: GivenNameDisambiguationRule,
) {
    // Collect all names with their item IDs
    let all_names: Vec<(&str, &Name)> = all_disamb_data
        .iter()
        .flat_map(|d| d.names.iter().map(move |n| (d.item_id.as_str(), n)))
        .collect();

    // For PrimaryName variants, only consider first names
    let relevant_names: Vec<(&str, &Name)> = match rule {
        GivenNameDisambiguationRule::PrimaryName
        | GivenNameDisambiguationRule::PrimaryNameWithInitials => all_disamb_data
            .iter()
            .filter_map(|d| d.names.first().map(|n| (d.item_id.as_str(), n)))
            .collect(),
        _ => all_names,
    };

    // Group by family name (including non-dropping particle)
    let mut family_groups: HashMap<String, Vec<(&str, &Name)>> = HashMap::new();
    for (item_id, name) in relevant_names {
        if let Some(ref family) = name.family {
            // Include non-dropping particle in the key for grouping
            // "dos Santos" and "Santos" should be in different groups
            let key = match &name.non_dropping_particle {
                Some(ndp) => format!("{} {}", ndp, family),
                None => family.clone(),
            };
            family_groups.entry(key).or_default().push((item_id, name));
        }
    }

    // For each family group with >1 unique names, set disambiguation hints
    for (_family, group) in family_groups {
        // Calculate initials for each name in the group
        let initials: Vec<_> = group
            .iter()
            .map(|(_, n)| n.given.as_ref().map(|g| get_initials(g)))
            .collect();
        let unique_initials: HashSet<_> = initials.iter().flatten().collect();

        // Calculate full given names (normalized: collapse whitespace and periods for comparison)
        // This ensures "J. J." and "J.J." are treated as equivalent
        let full_given: Vec<_> = group
            .iter()
            .map(|(_, n)| n.given.as_ref().map(|g| normalize_given_name(g)))
            .collect();
        let unique_full_given: HashSet<_> = full_given.iter().flatten().collect();

        // If all names have the same initials AND the same full given name,
        // there's nothing to disambiguate - skip this group entirely
        if unique_initials.len() <= 1 && unique_full_given.len() <= 1 {
            continue; // All effectively the same person, no disambiguation possible
        }

        // Determine the hint based on the rule
        let use_initials_only = matches!(
            rule,
            GivenNameDisambiguationRule::AllNamesWithInitials
                | GivenNameDisambiguationRule::PrimaryNameWithInitials
        );

        // Check if initials would disambiguate
        // Initials disambiguate if there are as many unique initials as unique full names
        let initials_disambiguate =
            unique_initials.len() >= unique_full_given.len() && unique_initials.len() > 1;

        if use_initials_only {
            // For WithInitials variants, only add initials (don't add full given names)
            // But only if initials actually disambiguate
            if initials_disambiguate {
                for (item_id, name) in &group {
                    processor.set_name_hint(item_id, name, NameHint::AddInitials);
                }
            }
            // If initials don't disambiguate, don't add any hint for WithInitials variants
        } else {
            // For non-WithInitials variants, use initials if they disambiguate, else full given name
            if initials_disambiguate {
                for (item_id, name) in &group {
                    processor.set_name_hint(item_id, name, NameHint::AddInitials);
                }
            } else if unique_full_given.len() > 1 {
                // Full given names can disambiguate
                for (item_id, name) in &group {
                    processor.set_name_hint(item_id, name, NameHint::AddGivenName);
                }
            }
            // If neither initials nor full names disambiguate, don't add any hint
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_ambiguities_basic() {
        let items = vec![
            ("ref1".to_string(), "Smith (2020)".to_string()),
            ("ref2".to_string(), "Smith (2020)".to_string()),
            ("ref3".to_string(), "Jones (2021)".to_string()),
        ];

        let ambiguities = find_ambiguities_simple(&items);
        assert_eq!(ambiguities.len(), 1);
        assert_eq!(ambiguities[0].len(), 2);
    }

    #[test]
    fn test_find_ambiguities_no_ambiguities() {
        let items = vec![
            ("ref1".to_string(), "Smith (2020)".to_string()),
            ("ref2".to_string(), "Jones (2021)".to_string()),
        ];

        let ambiguities = find_ambiguities_simple(&items);
        assert!(ambiguities.is_empty());
    }

    #[test]
    fn test_find_ambiguities_same_ref_not_ambiguous() {
        let items = vec![
            ("ref1".to_string(), "Smith (2020)".to_string()),
            ("ref1".to_string(), "Smith (2020)".to_string()),
        ];

        let ambiguities = find_ambiguities_simple(&items);
        assert!(ambiguities.is_empty());
    }

    #[test]
    fn test_find_ambiguities_multiple_groups() {
        let items = vec![
            ("ref1".to_string(), "Smith (2020)".to_string()),
            ("ref2".to_string(), "Smith (2020)".to_string()),
            ("ref3".to_string(), "Jones (2021)".to_string()),
            ("ref4".to_string(), "Jones (2021)".to_string()),
        ];

        let ambiguities = find_ambiguities_simple(&items);
        assert_eq!(ambiguities.len(), 2);
    }

    #[test]
    fn test_get_initials() {
        assert_eq!(get_initials("John"), "J.");
        assert_eq!(get_initials("John Paul"), "J. P.");
        assert_eq!(get_initials("Mary Jane Watson"), "M. J. W.");
        // Test normalization of different spacing in initials
        assert_eq!(get_initials("J. J."), "J. J.");
        assert_eq!(get_initials("J.J."), "J. J.");
        assert_eq!(get_initials("J.P."), "J. P.");
        assert_eq!(get_initials("J. P."), "J. P.");
    }

    #[test]
    fn test_extract_disamb_data_from_output() {
        use crate::output::{CitationItemType, Output, Tag};
        use crate::reference::Name;

        // Build an output tree like what evaluate_citation_to_output produces
        let name1 = Name {
            family: Some("Malone".to_string()),
            given: Some("Nolan J.".to_string()),
            ..Default::default()
        };
        let name2 = Name {
            family: Some("Malone".to_string()),
            given: Some("Kemp".to_string()),
            ..Default::default()
        };

        // Item 1: Malone (with names tagged)
        let item1_content = Output::tagged(
            Tag::Names {
                variable: "author".to_string(),
                names: vec![name1.clone()],
            },
            Output::literal("Malone"),
        );
        let item1 = Output::tagged(
            Tag::Item {
                item_type: CitationItemType::NormalCite,
                item_id: "ITEM-1".to_string(),
            },
            item1_content,
        );

        // Item 2: Malone (with different name)
        let item2_content = Output::tagged(
            Tag::Names {
                variable: "author".to_string(),
                names: vec![name2.clone()],
            },
            Output::literal("Malone"),
        );
        let item2 = Output::tagged(
            Tag::Item {
                item_type: CitationItemType::NormalCite,
                item_id: "ITEM-2".to_string(),
            },
            item2_content,
        );

        let full_output = Output::sequence(vec![item1, item2]);

        // Extract DisambData
        let disamb_data = extract_disamb_data(&[full_output]);

        assert_eq!(disamb_data.len(), 2, "Should extract 2 items");
        assert_eq!(disamb_data[0].item_id, "ITEM-1");
        assert_eq!(disamb_data[0].names.len(), 1, "Item 1 should have 1 name");
        assert_eq!(disamb_data[0].names[0].given, Some("Nolan J.".to_string()));
        assert_eq!(disamb_data[1].item_id, "ITEM-2");
        assert_eq!(disamb_data[1].names.len(), 1, "Item 2 should have 1 name");
        assert_eq!(disamb_data[1].names[0].given, Some("Kemp".to_string()));

        // Both render the same, so they should be ambiguous
        assert_eq!(disamb_data[0].rendered, "Malone");
        assert_eq!(disamb_data[1].rendered, "Malone");

        let ambiguities = find_ambiguities(disamb_data);
        assert_eq!(ambiguities.len(), 1, "Should find 1 ambiguity group");
        assert_eq!(
            ambiguities[0].len(),
            2,
            "Ambiguity group should have 2 items"
        );
    }
}
