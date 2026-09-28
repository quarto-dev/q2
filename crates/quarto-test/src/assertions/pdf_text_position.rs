/*
 * quarto-test/src/assertions/pdf_text_position.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * PDF text position assertions backed by tagged structure and text bounds.
 */

//! `ensurePdfTextPositions` assertion implementation.

use std::collections::{BTreeMap, HashSet};
use std::fs;

use anyhow::{Context, Result, bail};
use pdf_extract::{
    Dictionary, Document, MediaBox, Object, ObjectId, OutputDev, OutputError, Transform,
};
use serde_yaml::Value;

use super::{Assertion, VerifyContext};

const DEFAULT_ALIGNMENT_TOLERANCE: f64 = 2.0;

type PageMcid = (u32, i64);

#[derive(Debug, Clone, Copy, PartialEq)]
struct BBox {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    page: u32,
}

impl BBox {
    fn union(items: impl IntoIterator<Item = BBox>, page: u32) -> Option<Self> {
        let mut boxes = items.into_iter();
        let first = boxes.next()?;
        let (mut left, mut top, mut right, mut bottom) = (
            first.x,
            first.y,
            first.x + first.width,
            first.y + first.height,
        );
        for bbox in boxes {
            left = left.min(bbox.x);
            top = top.min(bbox.y);
            right = right.max(bbox.x + bbox.width);
            bottom = bottom.max(bbox.y + bbox.height);
        }
        Some(Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
            page,
        })
    }

    fn edge(self, edge: Edge) -> f64 {
        match edge {
            Edge::Left => self.x,
            Edge::Right => self.x + self.width,
            Edge::Top => self.y,
            Edge::Bottom => self.y + self.height,
            Edge::CenterX => self.x + self.width / 2.0,
            Edge::CenterY => self.y + self.height / 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Edge {
    Left,
    Right,
    Top,
    Bottom,
    CenterX,
    CenterY,
}

impl Edge {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "top" => Ok(Self::Top),
            "bottom" => Ok(Self::Bottom),
            "centerX" => Ok(Self::CenterX),
            "centerY" => Ok(Self::CenterY),
            _ => bail!("unknown PDF text position edge: {value:?}"),
        }
    }
}

#[derive(Debug, Clone)]
struct Selector {
    text: Option<String>,
    role: Option<String>,
    page: Option<f64>,
    edge: Option<Edge>,
    granularity: Option<String>,
}

impl Selector {
    fn label(&self) -> String {
        if self.role.as_deref() == Some("Page") {
            return format!("Page:{}", self.page.unwrap_or_default());
        }
        let mut label = self.text.clone().unwrap_or_default();
        if let Some(role) = &self.role {
            label.push_str(" [role=");
            label.push_str(role);
            label.push(']');
        }
        if let Some(granularity) = &self.granularity {
            label.push_str(" [granularity=");
            label.push_str(granularity);
            label.push(']');
        }
        label
    }

    fn key(&self) -> SelectorKey {
        if self.role.as_deref() == Some("Page") {
            return SelectorKey::Page(self.page.unwrap_or_default().to_bits());
        }
        SelectorKey::Text {
            text: self.text.clone().unwrap_or_default(),
            role: self.role.clone(),
            granularity: self.granularity.clone(),
            page: self.page.map(f64::to_bits),
            edge: self.edge,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum SelectorKey {
    Page(u64),
    Text {
        text: String,
        role: Option<String>,
        granularity: Option<String>,
        page: Option<u64>,
        edge: Option<Edge>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relation {
    LeftOf,
    RightOf,
    Above,
    Below,
    LeftAligned,
    RightAligned,
    TopAligned,
    BottomAligned,
}

impl Relation {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "leftOf" => Ok(Self::LeftOf),
            "rightOf" => Ok(Self::RightOf),
            "above" => Ok(Self::Above),
            "below" => Ok(Self::Below),
            "leftAligned" => Ok(Self::LeftAligned),
            "rightAligned" => Ok(Self::RightAligned),
            "topAligned" => Ok(Self::TopAligned),
            "bottomAligned" => Ok(Self::BottomAligned),
            _ => bail!("unknown PDF text position relation: {value:?}"),
        }
    }

    fn is_directional(self) -> bool {
        matches!(
            self,
            Self::LeftOf | Self::RightOf | Self::Above | Self::Below
        )
    }

    fn default_edges(self) -> (Edge, Edge) {
        match self {
            Self::LeftOf => (Edge::Right, Edge::Left),
            Self::RightOf => (Edge::Left, Edge::Right),
            Self::Above => (Edge::Bottom, Edge::Top),
            Self::Below => (Edge::Top, Edge::Bottom),
            Self::LeftAligned => (Edge::Left, Edge::Left),
            Self::RightAligned => (Edge::Right, Edge::Right),
            Self::TopAligned => (Edge::Top, Edge::Top),
            Self::BottomAligned => (Edge::Bottom, Edge::Bottom),
        }
    }
}

#[derive(Debug, Clone)]
struct PositionAssertion {
    subject: Selector,
    relation: Option<Relation>,
    object: Option<Selector>,
    by_min: Option<f64>,
    by_max: Option<f64>,
    tolerance: f64,
}

#[derive(Debug)]
pub struct EnsurePdfTextPositions {
    assertions: Vec<PositionAssertion>,
    no_match_assertions: Vec<PositionAssertion>,
}

impl EnsurePdfTextPositions {
    /// Parse the required and forbidden assertion arrays.
    pub fn new(value: &Value) -> Result<Self> {
        let (assertions, no_match_assertions) = parse_assertion_pair(value)?;
        Ok(Self {
            assertions,
            no_match_assertions,
        })
    }
}

impl Assertion for EnsurePdfTextPositions {
    fn name(&self) -> &str {
        "ensurePdfTextPositions"
    }

    fn verify(&self, context: &VerifyContext) -> Result<()> {
        if let Some(err) = &context.render_error {
            bail!("Cannot check PDF text positions: rendering failed with: {err}");
        }

        let bytes = fs::read(&context.output_path).with_context(|| {
            format!(
                "failed to read PDF output file: {}",
                context.output_path.display()
            )
        })?;
        let document = Document::load_mem(&bytes)
            .with_context(|| format!("failed to load PDF: {}", context.output_path.display()))?;
        let extracted = extract_text_items(&document).with_context(|| {
            format!(
                "failed to extract PDF text positions from {}",
                context.output_path.display()
            )
        })?;
        let structure = StructureTree::read(&document)?;

        let mut errors = Vec::new();
        let all_assertions = self
            .assertions
            .iter()
            .chain(self.no_match_assertions.iter());
        let mut searches = BTreeMap::<String, Vec<&Selector>>::new();
        let mut page_selectors = BTreeMap::<SelectorKey, &Selector>::new();
        let mut unique_selectors = BTreeMap::<SelectorKey, &Selector>::new();
        for assertion in all_assertions {
            add_selector(
                &assertion.subject,
                &mut searches,
                &mut page_selectors,
                &mut unique_selectors,
                &mut errors,
            );
            if let Some(object) = &assertion.object {
                add_selector(
                    object,
                    &mut searches,
                    &mut page_selectors,
                    &mut unique_selectors,
                    &mut errors,
                );
            }
        }

        let decoration_texts: HashSet<String> = searches
            .iter()
            .filter(|(_, selectors)| {
                selectors
                    .iter()
                    .any(|selector| selector.role.as_deref() == Some("Decoration"))
            })
            .map(|(text, _)| text.clone())
            .collect();
        let mut found_text = BTreeMap::<String, usize>::new();
        let mut ambiguous = HashSet::<String>::new();
        for search in searches.keys() {
            let matches: Vec<usize> = extracted
                .items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| item.text.contains(search).then_some(index))
                .collect();
            if matches.len() == 1 {
                found_text.insert(search.clone(), matches[0]);
            } else if matches.len() > 1 && !decoration_texts.contains(search) {
                ambiguous.insert(search.clone());
                errors.push(format!(
                    "Text {search:?} is ambiguous - found {} matches. Use a more specific search string.",
                    matches.len()
                ));
            } else if let Some(first) = matches.first() {
                found_text.insert(search.clone(), *first);
            }
        }

        let mut resolved = BTreeMap::<SelectorKey, ResolvedSelector>::new();
        let mut page_dimensions = BTreeMap::<u32, (f64, f64)>::new();
        for (key, selector) in page_selectors {
            let page_num = selector.page.unwrap_or_default() as u32;
            match extracted.page_sizes.get(&page_num) {
                Some((width, height)) => {
                    page_dimensions.insert(page_num, (*width, *height));
                    resolved.insert(
                        key,
                        ResolvedSelector {
                            bbox: BBox {
                                x: 0.0,
                                y: 0.0,
                                width: *width,
                                height: *height,
                                page: page_num,
                            },
                            struct_node: None,
                        },
                    );
                }
                None => errors.push(format!(
                    "Page {page_num} does not exist in PDF (has {} pages)",
                    extracted.page_sizes.len()
                )),
            }
        }

        for (key, selector) in unique_selectors {
            if selector.role.as_deref() == Some("Page") {
                continue;
            }
            let search = selector.text.as_deref().unwrap_or_default();
            let Some(item_index) = found_text.get(search).copied() else {
                if !ambiguous.contains(search) {
                    errors.push(format!("Text not found in PDF: {search:?}"));
                }
                continue;
            };
            let item = &extracted.items[item_index];
            let struct_node = if selector.role.as_deref() == Some("Decoration") {
                None
            } else {
                item.mcid
                    .and_then(|mcid| structure.mcid_nodes.get(&(item.bbox.page, mcid)).copied())
            };

            let bbox = if selector.role.as_deref() == Some("Decoration") {
                item.bbox
            } else if item.mcid.is_none() {
                errors.push(format!(
                    "Text {search:?} has no MCID - PDF may not be tagged. Use role: \"Decoration\" for untagged page elements like headers/footers."
                ));
                continue;
            } else if let Some(granularity) = &selector.granularity {
                if let Some(struct_node) = struct_node {
                    let Some(ancestor) =
                        structure.find_ancestor_with_role(struct_node, granularity)
                    else {
                        errors.push(format!(
                            "No ancestor with role {granularity:?} found for {search:?}"
                        ));
                        continue;
                    };
                    let bbox = granularity_bbox(&structure, &extracted, item.bbox.page, ancestor);
                    let Some(bbox) = bbox else {
                        errors.push(format!(
                            "Could not compute bbox for {search:?} with granularity {granularity:?} - no content items found"
                        ));
                        continue;
                    };
                    bbox
                } else {
                    errors.push(format!("No structure element found for text {search:?}"));
                    continue;
                }
            } else {
                let Some(mcid) = item.mcid else {
                    unreachable!()
                };
                let Some(bbox) = extracted.mcid_boxes.get(&(item.bbox.page, mcid)).copied() else {
                    errors.push(format!(
                        "No text items found for MCID {mcid} containing {search:?}"
                    ));
                    continue;
                };
                bbox
            };

            resolved.insert(key, ResolvedSelector { bbox, struct_node });
        }

        // Q1 validates semantic roles for required assertions only.
        for assertion in &self.assertions {
            validate_role(&assertion.subject, &resolved, &structure, &mut errors);
            if let Some(object) = &assertion.object {
                validate_role(object, &resolved, &structure, &mut errors);
            }
        }

        for assertion in &self.assertions {
            if let (Some(relation), Some(object)) = (assertion.relation, &assertion.object) {
                evaluate_assertion(assertion, relation, object, &resolved, false, &mut errors);
            }
        }
        for assertion in &self.no_match_assertions {
            if let (Some(relation), Some(object)) = (assertion.relation, &assertion.object) {
                evaluate_assertion(assertion, relation, object, &resolved, true, &mut errors);
            }
        }

        if !errors.is_empty() {
            bail!(
                "PDF position assertions failed in {}:\n{}",
                context.output_path.display(),
                errors
                    .iter()
                    .enumerate()
                    .map(|(i, error)| format!("  {}. {error}", i + 1))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ResolvedSelector {
    bbox: BBox,
    struct_node: Option<usize>,
}

fn add_selector<'a>(
    selector: &'a Selector,
    searches: &mut BTreeMap<String, Vec<&'a Selector>>,
    page_selectors: &mut BTreeMap<SelectorKey, &'a Selector>,
    unique_selectors: &mut BTreeMap<SelectorKey, &'a Selector>,
    errors: &mut Vec<String>,
) {
    if selector.role.as_deref() == Some("Page") {
        match selector.page {
            Some(page) if page.fract() == 0.0 && page >= 1.0 && page <= u32::MAX as f64 => {
                page_selectors.insert(selector.key(), selector);
            }
            Some(page) => errors.push(format!(
                "Page {page} does not exist in PDF (page must be a valid 1-based page number)"
            )),

            None => {
                errors.push("Page role requires 'page' field to specify page number".to_string())
            }
        }
        return;
    }
    let Some(text) = selector.text.as_deref().filter(|text| !text.is_empty()) else {
        errors.push("Selector requires 'text' field (unless role is \"Page\")".to_string());
        return;
    };
    searches.entry(text.to_string()).or_default().push(selector);
    unique_selectors.insert(selector.key(), selector);
}

fn validate_role(
    selector: &Selector,
    resolved: &BTreeMap<SelectorKey, ResolvedSelector>,
    structure: &StructureTree,
    errors: &mut Vec<String>,
) {
    if matches!(selector.role.as_deref(), Some("Page" | "Decoration")) {
        return;
    }
    let Some(expected) = selector.role.as_deref() else {
        return;
    };
    let Some(resolved) = resolved.get(&selector.key()) else {
        return;
    };
    let Some(node) = resolved
        .struct_node
        .and_then(|node| structure.nodes.get(node))
    else {
        errors.push(format!(
            "No structure element found for {:?}; cannot validate role {expected}",
            selector.text.as_deref().unwrap_or_default()
        ));
        return;
    };
    if node.role != expected {
        errors.push(format!(
            "Role mismatch for {:?}: expected {expected}, got {}",
            selector.text.as_deref().unwrap_or_default(),
            node.role
        ));
    }
}

fn evaluate_assertion(
    assertion: &PositionAssertion,
    relation: Relation,
    object: &Selector,
    resolved: &BTreeMap<SelectorKey, ResolvedSelector>,
    negative: bool,
    errors: &mut Vec<String>,
) {
    let subject_label = assertion.subject.label();
    let object_label = object.label();
    let subject_key = assertion.subject.key();
    let object_key = object.key();
    let (Some(subject), Some(object_resolved)) =
        (resolved.get(&subject_key), resolved.get(&object_key))
    else {
        // As in Q1, unresolved selectors make negative assertions trivially pass.
        return;
    };
    if subject.bbox.page != object_resolved.bbox.page {
        if negative {
            return;
        }
        errors.push(format!(
            "Cannot compare positions: {subject_label:?} is on page {}, {object_label:?} is on page {}",
            subject.bbox.page, object_resolved.bbox.page
        ));
        return;
    }

    let (default_subject_edge, default_object_edge) = relation.default_edges();
    let subject_edge = assertion.subject.edge.unwrap_or(default_subject_edge);
    let object_edge = object.edge.unwrap_or(default_object_edge);
    let subject_value = subject.bbox.edge(subject_edge);
    let object_value = object_resolved.bbox.edge(object_edge);
    let (passed, distance) = if relation.is_directional() {
        let distance = match relation {
            Relation::LeftOf | Relation::Above => object_value - subject_value,
            _ => subject_value - object_value,
        };
        let direction_passed = if matches!(relation, Relation::LeftOf | Relation::Above) {
            subject_value < object_value
        } else {
            subject_value > object_value
        };
        (
            direction_passed
                && assertion.by_min.is_none_or(|minimum| distance >= minimum)
                && assertion.by_max.is_none_or(|maximum| distance <= maximum),
            distance,
        )
    } else {
        (
            (subject_value - object_value).abs() <= assertion.tolerance,
            (subject_value - object_value).abs(),
        )
    };

    if (negative && passed) || (!negative && !passed) {
        if negative {
            errors.push(format!(
                "Negative assertion failed (page {}): {subject_label:?} IS {relation:?} {object_label:?} (expected NOT to be). Subject.{subject_edge:?}={subject_value:.1}, Object.{object_edge:?}={object_value:.1}, difference={distance:.1}pt",
                subject.bbox.page
            ));
        } else {
            errors.push(format!(
                "Position assertion failed (page {}): {subject_label:?} is NOT {relation:?} {object_label:?}. Subject.{subject_edge:?}={subject_value:.1}, Object.{object_edge:?}={object_value:.1}",
                subject.bbox.page
            ));
        }
    }
}

fn parse_assertion_pair(value: &Value) -> Result<(Vec<PositionAssertion>, Vec<PositionAssertion>)> {
    let array = value
        .as_sequence()
        .context("ensurePdfTextPositions must be an array")?;
    let assertions = match array.first() {
        Some(value) => parse_assertion_array(value).context("invalid required assertions")?,
        None => Vec::new(),
    };
    let no_match_assertions = match array.get(1) {
        Some(value) => parse_assertion_array(value).context("invalid noMatchAssertions")?,
        None => Vec::new(),
    };
    if array.len() > 2 {
        bail!("ensurePdfTextPositions accepts at most two arrays");
    }
    Ok((assertions, no_match_assertions))
}

fn parse_assertion_array(value: &Value) -> Result<Vec<PositionAssertion>> {
    let array = value
        .as_sequence()
        .context("assertion list must be an array")?;
    array
        .iter()
        .enumerate()
        .map(|(index, value)| {
            parse_position_assertion(value).with_context(|| format!("assertion {}", index + 1))
        })
        .collect()
}

fn parse_position_assertion(value: &Value) -> Result<PositionAssertion> {
    let map = value.as_mapping().context("assertion must be a mapping")?;
    let subject = parse_selector(map.get("subject").context("subject is required")?)?;
    let relation = map
        .get("relation")
        .map(|value| Relation::parse(value.as_str().context("relation must be a string")?))
        .transpose()?;
    let object = map.get("object").map(parse_selector).transpose()?;
    if relation.is_some() != object.is_some() {
        bail!("relation and object must either both be present or both be omitted");
    }
    let by_min = parse_optional_number(map.get("byMin"), "byMin")?;
    let by_max = parse_optional_number(map.get("byMax"), "byMax")?;
    let tolerance = parse_optional_number(map.get("tolerance"), "tolerance")?
        .unwrap_or(DEFAULT_ALIGNMENT_TOLERANCE);
    if !relation.is_some_and(Relation::is_directional) && (by_min.is_some() || by_max.is_some()) {
        bail!("byMin and byMax are only valid for directional relations");
    }
    if by_min
        .zip(by_max)
        .is_some_and(|(minimum, maximum)| minimum > maximum)
    {
        bail!("byMin must be <= byMax");
    }
    Ok(PositionAssertion {
        subject,
        relation,
        object,
        by_min,
        by_max,
        tolerance,
    })
}

fn parse_selector(value: &Value) -> Result<Selector> {
    if let Some(text) = value.as_str() {
        return Ok(Selector {
            text: Some(text.to_string()),
            role: None,
            page: None,
            edge: None,
            granularity: None,
        });
    }
    let map = value
        .as_mapping()
        .context("selector must be a string or mapping")?;
    let text = map
        .get("text")
        .map(|value| {
            value
                .as_str()
                .context("selector.text must be a string")
                .map(str::to_string)
        })
        .transpose()?;
    let role = map
        .get("role")
        .map(|value| {
            value
                .as_str()
                .context("selector.role must be a string")
                .map(str::to_string)
        })
        .transpose()?;
    let page = map
        .get("page")
        .map(|value| value.as_f64().context("selector.page must be a number"))
        .transpose()?;
    let edge = map
        .get("edge")
        .map(|value| Edge::parse(value.as_str().context("selector.edge must be a string")?))
        .transpose()?;
    let granularity = map
        .get("granularity")
        .map(|value| {
            value
                .as_str()
                .context("selector.granularity must be a string")
                .map(str::to_string)
        })
        .transpose()?;
    Ok(Selector {
        text,
        role,
        page,
        edge,
        granularity,
    })
}

fn parse_optional_number(value: Option<&Value>, name: &str) -> Result<Option<f64>> {
    value
        .map(|value| {
            value
                .as_f64()
                .with_context(|| format!("{name} must be a number"))
        })
        .transpose()
}

#[derive(Debug)]
struct TextItem {
    text: String,
    bbox: BBox,
    mcid: Option<i64>,
}

#[derive(Debug, Default)]
struct ExtractedText {
    items: Vec<TextItem>,
    mcid_boxes: BTreeMap<PageMcid, BBox>,
    page_sizes: BTreeMap<u32, (f64, f64)>,
}

#[derive(Debug, Clone)]
struct MarkedContent {
    tag: Option<String>,
    mcid: Option<i64>,
}

#[derive(Debug, Clone)]
struct CharacterBox {
    text: String,
    bbox: BBox,
    mcid: Option<i64>,
}

struct TextPositionOutput {
    extracted: ExtractedText,
    page: u32,
    page_left: f64,
    page_top: f64,
    page_height: f64,
    marked_content: Vec<MarkedContent>,
    word: Vec<CharacterBox>,
}

impl TextPositionOutput {
    fn new() -> Self {
        Self {
            extracted: ExtractedText::default(),
            page: 0,
            page_left: 0.0,
            page_top: 0.0,
            page_height: 0.0,
            marked_content: Vec::new(),
            word: Vec::new(),
        }
    }

    fn current_mcid(&self) -> Option<i64> {
        if self
            .marked_content
            .iter()
            .any(|content| content.tag.as_deref() == Some("Artifact"))
        {
            return None;
        }
        self.marked_content
            .iter()
            .rev()
            .find_map(|content| content.mcid)
    }

    fn flush_word(&mut self) {
        if self.word.is_empty() {
            return;
        }
        let page = self.word[0].bbox.page;
        let characters = std::mem::take(&mut self.word);
        let text: String = characters
            .iter()
            .map(|character| character.text.as_str())
            .collect();
        let Some(bbox) = BBox::union(characters.iter().map(|character| character.bbox), page)
        else {
            return;
        };

        let mcids: HashSet<i64> = characters
            .iter()
            .filter(|character| !character.text.trim().is_empty())
            .filter_map(|character| character.mcid)
            .collect();
        for mcid in mcids {
            let key = (page, mcid);
            self.extracted
                .mcid_boxes
                .entry(key)
                .and_modify(|previous| *previous = union_two(*previous, bbox))
                .or_insert(bbox);
        }
        if !text.trim().is_empty() {
            let mcid = characters.iter().rev().find_map(|character| character.mcid);
            self.extracted.items.push(TextItem { text, bbox, mcid });
        }
    }
}

fn union_two(left: BBox, right: BBox) -> BBox {
    BBox::union([left, right], left.page).unwrap_or(left)
}

fn granularity_bbox(
    structure: &StructureTree,
    extracted: &ExtractedText,
    page: u32,
    ancestor: usize,
) -> Option<BBox> {
    BBox::union(
        structure
            .mcid_nodes
            .iter()
            .filter(|((candidate_page, _), node)| {
                *candidate_page == page && structure.is_descendant_or_self(**node, ancestor)
            })
            .filter_map(|((candidate_page, mcid), _)| {
                extracted.mcid_boxes.get(&(*candidate_page, *mcid)).copied()
            }),
        page,
    )
}

impl OutputDev for TextPositionOutput {
    fn begin_page(
        &mut self,
        page_num: u32,
        media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> std::result::Result<(), OutputError> {
        self.page = page_num;
        self.page_left = media_box.llx;
        self.page_top = media_box.ury;
        self.page_height = media_box.ury - media_box.lly;
        self.extracted
            .page_sizes
            .insert(page_num, (media_box.urx - media_box.llx, self.page_height));
        Ok(())
    }

    fn end_page(&mut self) -> std::result::Result<(), OutputError> {
        self.flush_word();
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        spacing: f64,
        font_size: f64,
        text: &str,
    ) -> std::result::Result<(), OutputError> {
        let advance = width * font_size + spacing;
        // Preserve Q1's baseline-as-top convention; these are layout bounds,
        // not glyph-outline ink bounds.
        let baseline = Transform::row_major(1.0, 0.0, 0.0, -1.0, -self.page_left, self.page_top);
        let baseline = trm.post_transform(&baseline);
        let advance_scale = (trm.m11 * trm.m11 + trm.m12 * trm.m12).sqrt();
        let height = (trm.m21 * font_size).hypot(trm.m22 * font_size);
        let bbox = BBox {
            x: baseline.m31,
            y: baseline.m32,
            width: advance * advance_scale,
            height,
            page: self.page,
        };
        self.word.push(CharacterBox {
            text: text.to_string(),
            bbox,
            mcid: self.current_mcid(),
        });
        Ok(())
    }

    fn begin_word(&mut self) -> std::result::Result<(), OutputError> {
        // These callbacks delimit each PDF string operand, not linguistic words;
        // retain adjacent Tj/TJ fragments as one searchable text run.
        Ok(())
    }

    fn end_word(&mut self) -> std::result::Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> std::result::Result<(), OutputError> {
        self.flush_word();
        Ok(())
    }

    fn begin_marked_content(
        &mut self,
        tag: Option<&str>,
        properties: Option<&Dictionary>,
    ) -> std::result::Result<(), OutputError> {
        self.flush_word();
        self.marked_content.push(MarkedContent {
            tag: tag.map(str::to_owned),
            mcid: pdf_extract::mcid(properties),
        });
        Ok(())
    }

    fn end_marked_content(&mut self) -> std::result::Result<(), OutputError> {
        self.flush_word();
        self.marked_content.pop();
        Ok(())
    }
}

fn extract_text_items(document: &Document) -> Result<ExtractedText> {
    let mut output = TextPositionOutput::new();
    pdf_extract::output_doc(document, &mut output).context("pdf-extract failed")?;
    Ok(output.extracted)
}

#[derive(Debug)]
struct StructNode {
    role: String,
    parent: Option<usize>,
}

#[derive(Debug, Default)]
struct StructureTree {
    nodes: Vec<StructNode>,
    mcid_nodes: BTreeMap<PageMcid, usize>,
}

impl StructureTree {
    fn read(document: &Document) -> Result<Self> {
        let mut tree = Self::default();
        let catalog = document.catalog().context("PDF catalog is missing")?;
        let root_object = match catalog.get(b"StructTreeRoot") {
            Ok(root) => root,
            Err(_) => return Ok(tree),
        };
        let (_, root) = document
            .dereference(root_object)
            .context("invalid StructTreeRoot reference")?;
        let root = root
            .as_dict()
            .context("StructTreeRoot must be a dictionary")?;
        let role_map = if let Ok(value) = root.get(b"RoleMap") {
            let (_, role_map) = document
                .dereference(value)
                .context("invalid RoleMap reference")?;
            Some(role_map.as_dict().context("RoleMap must be a dictionary")?)
        } else {
            None
        };
        let pages: BTreeMap<ObjectId, u32> = document
            .get_pages()
            .into_iter()
            .map(|(page, id)| (id, page))
            .collect();
        let inherited_page = if let Ok(value) = root.get(b"Pg") {
            document
                .dereference(value)
                .context("invalid StructTreeRoot /Pg reference")?
                .0
        } else {
            None
        };
        if let Ok(kids) = root.get(b"K") {
            tree.walk_kid(document, kids, None, inherited_page, role_map, &pages)
                .context("walking StructTreeRoot /K")?;
        }
        Ok(tree)
    }

    fn walk_kid(
        &mut self,
        document: &Document,
        kid: &Object,
        parent_node: Option<usize>,
        inherited_page: Option<ObjectId>,
        role_map: Option<&Dictionary>,
        pages: &BTreeMap<ObjectId, u32>,
    ) -> Result<()> {
        match kid {
            Object::Array(kids) => {
                for kid in kids {
                    self.walk_kid(document, kid, parent_node, inherited_page, role_map, pages)?;
                }
            }
            Object::Integer(mcid) => {
                if let Some(page) = inherited_page.and_then(|page| pages.get(&page)).copied()
                    && let Some(node) = parent_node
                {
                    self.mcid_nodes.insert((page, *mcid), node);
                }
            }
            Object::Reference(reference) => {
                let object = document
                    .get_object(*reference)
                    .context("invalid structure-tree reference")?;
                self.walk_kid(
                    document,
                    object,
                    parent_node,
                    inherited_page,
                    role_map,
                    pages,
                )?;
            }
            Object::Dictionary(dict) => {
                if let Some(mcid) = dict.get(b"MCID").ok().and_then(|value| value.as_i64().ok()) {
                    let page_ref = if let Ok(value) = dict.get(b"Pg") {
                        let (id, _) = document
                            .dereference(value)
                            .context("invalid MCR /Pg reference")?;
                        id.or(inherited_page)
                    } else if dict
                        .get(b"Type")
                        .ok()
                        .and_then(|value| value.as_name().ok())
                        == Some(b"MCR")
                    {
                        Some(inherited_page.context(format!(
                            "MCR with MCID {mcid} has no /Pg and no inherited page"
                        ))?)
                    } else {
                        inherited_page
                    };
                    if let Some(page) = page_ref.and_then(|page| pages.get(&page)).copied()
                        && let Some(node) = parent_node
                    {
                        self.mcid_nodes.insert((page, mcid), node);
                    }
                    return Ok(());
                }
                let (Some(role_object), Ok(kids)) = (dict.get(b"S").ok(), dict.get(b"K")) else {
                    return Ok(());
                };
                let (_, role_object) = document
                    .dereference(role_object)
                    .context("invalid StructElem /S reference")?;
                let role_name = role_object
                    .as_name()
                    .context("StructElem /S must be a name")?;
                let mut role = String::from_utf8_lossy(role_name).into_owned();
                if let Some(role_map) = role_map {
                    let mut visited = HashSet::new();
                    while visited.insert(role.clone()) {
                        let mapped = role_map
                            .get(role.as_bytes())
                            .ok()
                            .and_then(|value| value.as_name().ok())
                            .map(String::from_utf8_lossy)
                            .map(|value| value.into_owned());
                        let Some(mapped) = mapped else {
                            break;
                        };
                        role = mapped;
                    }
                }
                let page_ref = if let Ok(value) = dict.get(b"Pg") {
                    let (id, _) = document
                        .dereference(value)
                        .context("invalid StructElem /Pg reference")?;
                    id
                } else {
                    inherited_page
                };
                let node = self.nodes.len();
                self.nodes.push(StructNode {
                    role,
                    parent: parent_node,
                });
                self.walk_kid(document, kids, Some(node), page_ref, role_map, pages)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn find_ancestor_with_role(&self, node: usize, role: &str) -> Option<usize> {
        let mut current = Some(node);
        while let Some(index) = current {
            let current_node = self.nodes.get(index)?;
            if current_node.role == role {
                return Some(index);
            }
            current = current_node.parent;
        }
        None
    }

    fn is_descendant_or_self(&self, mut node: usize, ancestor: usize) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            let Some(parent) = self.nodes.get(node).and_then(|node| node.parent) else {
                return false;
            };
            node = parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(text: &str) -> Selector {
        Selector {
            text: Some(text.into()),
            role: None,
            page: None,
            edge: None,
            granularity: None,
        }
    }

    fn position_assertion(
        relation: Relation,
        subject: Selector,
        object: Selector,
    ) -> PositionAssertion {
        PositionAssertion {
            subject,
            relation: Some(relation),
            object: Some(object),
            by_min: None,
            by_max: None,
            tolerance: DEFAULT_ALIGNMENT_TOLERANCE,
        }
    }

    #[test]
    fn directional_relations_use_expected_edges_and_distance_bounds() {
        let subject = BBox {
            x: 10.0,
            y: 20.0,
            width: 10.0,
            height: 10.0,
            page: 1,
        };
        let object = BBox {
            x: 40.0,
            y: 20.0,
            width: 10.0,
            height: 10.0,
            page: 1,
        };
        let (left_subject, left_object) = Relation::LeftOf.default_edges();
        assert_eq!(subject.edge(left_subject), 20.0);
        assert_eq!(object.edge(left_object), 40.0);
        let assertion = PositionAssertion {
            by_min: Some(19.0),
            by_max: Some(21.0),
            ..position_assertion(Relation::LeftOf, selector("a"), selector("b"))
        };
        assert!(assertion.by_min.unwrap() <= object.edge(left_object) - subject.edge(left_subject));
        assert!(assertion.by_max.unwrap() >= object.edge(left_object) - subject.edge(left_subject));
        for relation in [Relation::RightOf, Relation::Above, Relation::Below] {
            assert!(relation.is_directional());
        }
    }

    #[test]
    fn tree_walk_uses_page_and_mcid_as_a_composite_key_and_scopes_spanning_parent() {
        let mut tree = StructureTree::default();
        tree.nodes.push(StructNode {
            role: "P".into(),
            parent: None,
        });
        tree.nodes.push(StructNode {
            role: "Span".into(),
            parent: Some(0),
        });
        tree.mcid_nodes.insert((1, 0), 0);
        tree.mcid_nodes.insert((2, 0), 1);
        assert_eq!(tree.mcid_nodes.get(&(1, 0)), Some(&0));
        assert_eq!(tree.mcid_nodes.get(&(2, 0)), Some(&1));
        assert_eq!(tree.find_ancestor_with_role(1, "P"), Some(0));
        assert!(tree.is_descendant_or_self(1, 0));
        assert!(!tree.is_descendant_or_self(0, 1));
    }

    #[test]
    fn selector_parser_accepts_edge_granularity_and_extra_assertion_page() {
        let value: Value = serde_yaml::from_str(
            "- { subject: { text: Hello, role: P, edge: centerX, granularity: Div }, relation: leftOf, object: World, page: 2 }",
        ).unwrap();
        let assertions = parse_assertion_array(&value).unwrap();
        assert_eq!(assertions[0].subject.edge, Some(Edge::CenterX));
        assert_eq!(assertions[0].subject.granularity.as_deref(), Some("Div"));
    }

    #[test]
    fn optional_negative_array_and_tag_only_assertion_parse() {
        let value: Value =
            serde_yaml::from_str("- [{ subject: { text: Heading, role: H1 } }]").unwrap();
        let (assertions, no_match) = parse_assertion_pair(&value).unwrap();
        assert_eq!(assertions.len(), 1);
        assert!(assertions[0].relation.is_none());
        assert!(no_match.is_empty());
    }

    #[test]
    fn role_validation_requires_a_node_for_semantic_roles_but_skips_decoration() {
        let semantic_selector = Selector {
            role: Some("P".into()),
            ..selector("text")
        };
        let semantic_resolved = BTreeMap::from([(
            semantic_selector.key(),
            resolved_selector(BBox {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                page: 1,
            }),
        )]);
        let mut errors = Vec::new();
        validate_role(
            &semantic_selector,
            &semantic_resolved,
            &StructureTree::default(),
            &mut errors,
        );
        assert_eq!(errors.len(), 1);

        let decoration_selector = Selector {
            role: Some("Decoration".into()),
            ..selector("decoration")
        };
        let decoration_resolved = BTreeMap::from([(
            decoration_selector.key(),
            resolved_selector(BBox {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                page: 1,
            }),
        )]);
        errors.clear();
        validate_role(
            &decoration_selector,
            &decoration_resolved,
            &StructureTree::default(),
            &mut errors,
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn negative_relation_fails_only_when_the_relation_holds() {
        let subject = ResolvedSelector {
            bbox: BBox {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                page: 1,
            },
            struct_node: None,
        };
        let object = ResolvedSelector {
            bbox: BBox {
                x: 20.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                page: 1,
            },
            struct_node: None,
        };
        let assertion = position_assertion(Relation::LeftOf, selector("a"), selector("b"));
        let subject_selector = assertion.subject.clone();
        let object_selector = assertion.object.clone().unwrap();
        let resolved = BTreeMap::from([
            (subject_selector.key(), subject),
            (object_selector.key(), object),
        ]);
        let mut errors = Vec::new();
        evaluate_assertion(
            &assertion,
            Relation::LeftOf,
            assertion.object.as_ref().unwrap(),
            &resolved,
            true,
            &mut errors,
        );
        assert_eq!(errors.len(), 1);
        let mut no_error = Vec::new();
        evaluate_assertion(
            &assertion,
            Relation::RightOf,
            assertion.object.as_ref().unwrap(),
            &resolved,
            true,
            &mut no_error,
        );
        assert!(no_error.is_empty());
    }

    fn evaluate(
        assertion: &PositionAssertion,
        resolved: &BTreeMap<SelectorKey, ResolvedSelector>,
        negative: bool,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        evaluate_assertion(
            assertion,
            assertion.relation.unwrap(),
            assertion.object.as_ref().unwrap(),
            resolved,
            negative,
            &mut errors,
        );
        errors
    }

    fn resolved_selector(bbox: BBox) -> ResolvedSelector {
        ResolvedSelector {
            bbox,
            struct_node: None,
        }
    }

    #[test]
    fn relation_evaluator_covers_all_relations_and_selector_edges() {
        let a = BBox {
            x: 10.0,
            y: 10.0,
            width: 10.0,
            height: 10.0,
            page: 1,
        };
        let b = BBox {
            x: 40.0,
            y: 40.0,
            width: 10.0,
            height: 10.0,
            page: 1,
        };
        let pairs = [
            (Relation::LeftOf, a, b),
            (Relation::RightOf, b, a),
            (Relation::Above, a, b),
            (Relation::Below, b, a),
        ];
        for (relation, subject_bbox, object_bbox) in pairs {
            let assertion = position_assertion(relation, selector("subject"), selector("object"));
            let resolved = BTreeMap::from([
                (assertion.subject.key(), resolved_selector(subject_bbox)),
                (
                    assertion.object.as_ref().unwrap().key(),
                    resolved_selector(object_bbox),
                ),
            ]);
            assert!(
                evaluate(&assertion, &resolved, false).is_empty(),
                "{relation:?}"
            );
        }

        let aligned = BBox {
            x: 10.0,
            y: 12.0,
            width: 10.0,
            height: 10.0,
            page: 1,
        };
        let within_tolerance = BBox {
            x: 12.0,
            y: 10.0,
            width: 8.0,
            height: 12.0,
            page: 1,
        };
        for relation in [
            Relation::LeftAligned,
            Relation::RightAligned,
            Relation::TopAligned,
            Relation::BottomAligned,
        ] {
            let assertion = position_assertion(relation, selector("subject"), selector("object"));
            let resolved = BTreeMap::from([
                (assertion.subject.key(), resolved_selector(aligned)),
                (
                    assertion.object.as_ref().unwrap().key(),
                    resolved_selector(within_tolerance),
                ),
            ]);
            assert!(
                evaluate(&assertion, &resolved, false).is_empty(),
                "{relation:?}"
            );
        }

        let mut assertion =
            position_assertion(Relation::LeftOf, selector("subject"), selector("object"));
        assertion.subject.edge = Some(Edge::CenterX);
        assertion.object.as_mut().unwrap().edge = Some(Edge::CenterX);
        assertion.by_min = Some(29.0);
        assertion.by_max = Some(31.0);
        let resolved = BTreeMap::from([
            (assertion.subject.key(), resolved_selector(a)),
            (
                assertion.object.as_ref().unwrap().key(),
                resolved_selector(BBox {
                    x: 40.0,
                    y: 10.0,
                    width: 10.0,
                    height: 10.0,
                    page: 1,
                }),
            ),
        ]);
        assert!(evaluate(&assertion, &resolved, false).is_empty());

        assertion.by_max = Some(29.0);
        assert_eq!(evaluate(&assertion, &resolved, false).len(), 1);

        let too_far_aligned = BBox {
            x: 13.0,
            ..within_tolerance
        };
        let assertion = position_assertion(
            Relation::LeftAligned,
            selector("subject"),
            selector("object"),
        );
        let resolved = BTreeMap::from([
            (assertion.subject.key(), resolved_selector(aligned)),
            (
                assertion.object.as_ref().unwrap().key(),
                resolved_selector(too_far_aligned),
            ),
        ]);
        assert_eq!(evaluate(&assertion, &resolved, false).len(), 1);
    }

    fn tagged_structure_document() -> (Document, ObjectId, ObjectId) {
        use pdf_extract::{Object, dictionary};

        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_one_id = document.new_object_id();
        document.objects.insert(
            page_one_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
            }),
        );
        let page_two_id = document.new_object_id();
        document.objects.insert(
            page_two_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
            }),
        );
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_one_id.into(), page_two_id.into()],
                "Count" => 2,
            }),
        );

        let struct_tree_id = document.new_object_id();
        let document_node_id = document.new_object_id();
        let paragraph_node_id = document.new_object_id();
        let span_node_id = document.new_object_id();
        let page_one_mcr_without_page = Object::Dictionary(dictionary! {
            "Type" => "MCR",
            "MCID" => 2,
        });
        let page_two_mcr = Object::Dictionary(dictionary! {
            "Type" => "MCR",
            "Pg" => page_two_id,
            "MCID" => 0,
        });
        document.objects.insert(
            span_node_id,
            Object::Dictionary(dictionary! {
                "Type" => "StructElem",
                "S" => "Span",
                "P" => paragraph_node_id,
                "Pg" => page_two_id,
                "K" => 1,
            }),
        );
        document.objects.insert(
            paragraph_node_id,
            Object::Dictionary(dictionary! {
                "Type" => "StructElem",
                "S" => "BodyParagraph",
                "P" => document_node_id,
                "Pg" => page_one_id,
                "K" => vec![
                    0.into(),
                    page_one_mcr_without_page,
                    page_two_mcr,
                    span_node_id.into(),
                ],
            }),
        );
        document.objects.insert(
            document_node_id,
            Object::Dictionary(dictionary! {
                "Type" => "StructElem",
                "S" => "Document",
                "P" => struct_tree_id,
                "Pg" => page_one_id,
                "K" => paragraph_node_id,
            }),
        );
        document.objects.insert(
            struct_tree_id,
            Object::Dictionary(dictionary! {
                "Type" => "StructTreeRoot",
                "Pg" => page_one_id,
                "K" => document_node_id,
                "RoleMap" => dictionary! { "BodyParagraph" => "P" },
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "StructTreeRoot" => struct_tree_id,
        });
        document.trailer.set("Root", catalog_id);
        (document, page_one_id, page_two_id)
    }

    #[test]
    fn structure_tree_walk_scopes_spanning_nodes_and_granularity_to_page() {
        let (document, page_one_id, page_two_id) = tagged_structure_document();
        let pages: BTreeMap<ObjectId, u32> = document
            .get_pages()
            .into_iter()
            .map(|(page, id)| (id, page))
            .collect();
        let page_one = pages[&page_one_id];
        let page_two = pages[&page_two_id];
        assert_eq!((page_one, page_two), (1, 2));

        let tree = StructureTree::read(&document).unwrap();
        let page_one_node = tree.mcid_nodes[&(page_one, 0)];
        assert_eq!(tree.mcid_nodes.get(&(page_one, 2)), Some(&page_one_node));
        let page_two_paragraph_node = tree.mcid_nodes[&(page_two, 0)];
        let page_two_span_node = tree.mcid_nodes[&(page_two, 1)];
        assert_eq!(tree.nodes[page_one_node].role, "P");
        assert_eq!(tree.nodes[page_two_paragraph_node].role, "P");
        assert_eq!(tree.nodes[page_two_span_node].role, "Span");
        assert_eq!(page_one_node, page_two_paragraph_node);
        assert_eq!(
            tree.find_ancestor_with_role(page_two_span_node, "P"),
            Some(page_two_paragraph_node)
        );

        let extracted = ExtractedText {
            mcid_boxes: BTreeMap::from([
                (
                    (page_one, 0),
                    BBox {
                        x: 150.0,
                        y: 20.0,
                        width: 10.0,
                        height: 10.0,
                        page: page_one,
                    },
                ),
                (
                    (page_two, 0),
                    BBox {
                        x: 20.0,
                        y: 30.0,
                        width: 10.0,
                        height: 10.0,
                        page: page_two,
                    },
                ),
                (
                    (page_two, 1),
                    BBox {
                        x: 40.0,
                        y: 30.0,
                        width: 10.0,
                        height: 10.0,
                        page: page_two,
                    },
                ),
            ]),
            ..ExtractedText::default()
        };
        let bbox = granularity_bbox(&tree, &extracted, page_two, page_two_paragraph_node).unwrap();
        assert_eq!(
            bbox,
            BBox {
                x: 20.0,
                y: 30.0,
                width: 30.0,
                height: 10.0,
                page: page_two
            }
        );
    }

    #[test]
    fn typst_tagged_pdf_resolves_text_roles_and_real_positions() {
        let dir = tempfile::TempDir::new().unwrap();
        let pdf_path = dir.path().join("position-fixture.pdf");
        std::fs::write(
            &pdf_path,
            include_bytes!("../../tests/fixtures/pdf_text_position.pdf"),
        )
        .unwrap();
        let yaml: Value = serde_yaml::from_str(
            r#"
            -
              - subject: { text: LEFT-MARK, role: P, edge: centerX }
                relation: leftOf
                object: { text: RIGHT-MARK, role: P, edge: centerX }
                byMin: 100
                byMax: 160
              - subject: { text: LEFT-MARK, role: P }
                relation: topAligned
                object: { text: RIGHT-MARK, role: P }
              - subject: { text: LEFT-MARK, role: P }
                relation: bottomAligned
                object: { text: RIGHT-MARK, role: P }
              - subject: TOP-MARK
                relation: above
                object: BOTTOM-MARK
              - subject: BOTTOM-MARK
                relation: below
                object: TOP-MARK
              - subject: { text: Position Fixture, role: H1 }
              - subject: { text: LEFT-MARK, role: Decoration }
                relation: leftOf
                object: { text: RIGHT-MARK, role: P }
              - subject: { text: MARK, role: Decoration }
              - subject: { role: Page, page: 1 }
            -
              - subject: TOP-MARK
                relation: leftOf
                object: BOTTOM-MARK
            "#,
        )
        .unwrap();
        let assertion = EnsurePdfTextPositions::new(&yaml).unwrap();
        let context = VerifyContext {
            output_path: pdf_path,
            input_path: dir.path().join("position-fixture.qmd"),
            format: "typst".to_string(),
            render_error: None,
            messages: vec![],
        };
        assertion.verify(&context).unwrap();
    }

    #[test]
    fn position_comparisons_reject_cross_page_matches_but_negative_assertions_pass() {
        let mut subject = selector("subject");
        subject.page = None;
        let assertion = position_assertion(Relation::LeftOf, subject, selector("object"));
        let resolved = BTreeMap::from([
            (
                assertion.subject.key(),
                resolved_selector(BBox {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                    page: 1,
                }),
            ),
            (
                assertion.object.as_ref().unwrap().key(),
                resolved_selector(BBox {
                    x: 20.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                    page: 2,
                }),
            ),
        ]);
        let errors = evaluate(&assertion, &resolved, false);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Cannot compare positions"));
        assert!(evaluate(&assertion, &resolved, true).is_empty());
    }
}
