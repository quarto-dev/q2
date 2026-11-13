/*
 * presenter/engine_attrs.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * OPR-7f: the channel by which an engine tells the presenter which id, classes and
 * attributes to put on a cell wrapper or an output div.
 */

//! Engine-supplied attributes (`quarto_attrs`).
//!
//! An engine may store `{"id": "...", "classes": [...], "attributes": {...}}` under the
//! `quarto_attrs` key of a record cell\'s `metadata.extra` (the `.cell` wrapper) or of a
//! display_data / execute_result output\'s `metadata` (that output\'s div). The presenter
//! merges it and does not interpret it. All three keys are optional.
//!
//! The merge follows the qmd attribute grammar, which is ordered: id, then classes, then
//! key/value pairs. An engine id fills the id slot only when the presenter wrote none; engine
//! classes go after the presenter\'s own on a cell wrapper and **before** them on an output div
//! (the engine route writes `.cell-output .cell-output-display`, the presenter only
//! `.cell-output-display`); engine attributes go after the presenter\'s pairs, and one whose key
//! the presenter already writes is ignored.
//!
//! Every entry is validated against the grammar, not HTML (a class may not contain a dot: the
//! grammar allows one only in the first class, and an engine class can land anywhere). A malformed entry is skipped (with
//! a reason, which the caller reports as a note) and the rest is kept. Attributes are
//! display-only: nothing here reaches a record hash. Where an output has no div (a bare fence
//! under `collapse` or `results: asis`, a bare linked image) the attributes are dropped, by
//! design.

use serde_json::Value;

/// The key under which an engine stores its attributes.
pub const QUARTO_ATTRS_KEY: &str = "quarto_attrs";

/// Validated engine attributes: every entry already satisfies the qmd attribute grammar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineAttrs {
    id: Option<String>,
    classes: Vec<String>,
    /// `(key, value)` with the value unescaped; [`build_head`] quotes it.
    attributes: Vec<(String, String)>,
}

impl EngineAttrs {
    /// The same attributes without the engine\'s `id` (OPR-9b): Quarto 1 writes the auto cell id
    /// on the wrapper only for html-family output, so other targets keep the classes and
    /// attributes but not the id.
    pub fn without_id(mut self) -> Self {
        self.id = None;
        self
    }
}

/// Where engine classes go relative to the presenter\'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassPlacement {
    /// After the presenter\'s classes (the `.cell` wrapper).
    After,
    /// Before them (an output div).
    Before,
}

fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn valid_class(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn valid_key(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-'))
        && s != "id"
        && s != "class"
}

/// Why `value` cannot be written as a single-quoted attribute value, or `None` when it can.
fn value_problem(value: &str) -> Option<&'static str> {
    if value.ends_with('\\') {
        Some("ends in a backslash")
    } else if value.contains('\n') || value.contains('\r') {
        Some("contains a newline")
    } else if value.contains("{{<") {
        Some("contains a shortcode opener")
    } else if value.starts_with(char::is_whitespace) {
        Some("starts with whitespace")
    } else {
        None
    }
}

impl EngineAttrs {
    /// Read `raw` (the value stored under `quarto_attrs`). Returns the valid part and one
    /// reason per skipped entry. Anything that is not an object is skipped whole.
    pub fn parse(raw: &Value) -> (Self, Vec<String>) {
        let mut out = Self::default();
        let mut rejected = Vec::new();
        let Some(obj) = raw.as_object() else {
            rejected.push("`quarto_attrs` is not an object".to_string());
            return (out, rejected);
        };
        match obj.get("id") {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if valid_id(s) => out.id = Some(s.clone()),
            Some(other) => rejected.push(format!("id {other} is not a valid identifier")),
        }
        match obj.get("classes") {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => {
                for item in items {
                    match item {
                        Value::String(s) if valid_class(s) => {
                            if !out.classes.contains(s) {
                                out.classes.push(s.clone());
                            }
                        }
                        other => rejected.push(format!("class {other} is not a valid class")),
                    }
                }
            }
            Some(_) => rejected.push("`classes` is not an array".to_string()),
        }
        match obj.get("attributes") {
            None | Some(Value::Null) => {}
            Some(Value::Object(map)) => {
                for (key, value) in map {
                    if !valid_key(key) {
                        rejected.push(format!("attribute key `{key}` is not valid"));
                        continue;
                    }
                    let Some(text) = value.as_str() else {
                        rejected.push(format!("attribute `{key}` is not a string"));
                        continue;
                    };
                    if let Some(why) = value_problem(text) {
                        rejected.push(format!("attribute `{key}` {why}"));
                        continue;
                    }
                    out.attributes.push((key.clone(), text.to_string()));
                }
            }
            Some(_) => rejected.push("`attributes` is not an object".to_string()),
        }
        (out, rejected)
    }

    /// The attributes stored in `holder[quarto_attrs]`, if any (with the skipped reasons).
    pub fn from_holder(holder: &Value) -> Option<(Self, Vec<String>)> {
        holder.get(QUARTO_ATTRS_KEY).map(Self::parse)
    }

    /// Whether nothing is left to write.
    pub fn is_empty(&self) -> bool {
        self.id.is_none() && self.classes.is_empty() && self.attributes.is_empty()
    }
}

/// The cell\'s own `quarto_attrs` (from `cell.metadata.extra`), or none.
pub fn cell_engine_attrs(cell: &crate::record::RecordCell) -> Option<EngineAttrs> {
    cell.metadata
        .extra
        .get(QUARTO_ATTRS_KEY)
        .map(|raw| EngineAttrs::parse(raw).0)
}

/// One reason per entry of `quarto_attrs` the grammar refuses, on the cell and on every
/// display_data / execute_result output (the only outputs that keep metadata), in that order.
pub fn rejection_reasons(cell: &crate::record::RecordCell) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(raw) = cell.metadata.extra.get(QUARTO_ATTRS_KEY) {
        reasons.extend(EngineAttrs::parse(raw).1);
    }
    for output in &cell.outputs {
        let keeps_metadata = matches!(
            output.get("output_type").and_then(Value::as_str),
            Some("display_data" | "execute_result")
        );
        if !keeps_metadata {
            continue;
        }
        if let Some((_, rejected)) = output.get("metadata").and_then(EngineAttrs::from_holder) {
            reasons.extend(rejected);
        }
    }
    reasons
}

/// The text between the braces of an attribute head: id, classes, then pairs, joined by
/// spaces. `classes` are written without the dot, `pairs` as their final text (`key='v'`).
/// Engine entries merge as the module documentation says.
pub fn build_head(
    id: Option<&str>,
    classes: &[String],
    pairs: &[String],
    engine: Option<&EngineAttrs>,
    placement: ClassPlacement,
) -> String {
    let empty = EngineAttrs::default();
    let engine = engine.unwrap_or(&empty);
    let mut parts: Vec<String> = Vec::new();
    match (id, &engine.id) {
        (Some(id), _) => parts.push(format!("#{id}")),
        (None, Some(id)) => parts.push(format!("#{id}")),
        (None, None) => {}
    }
    let extra: Vec<&String> = engine
        .classes
        .iter()
        .filter(|c| !classes.contains(c))
        .collect();
    let class_part = |c: &String| format!(".{c}");
    match placement {
        ClassPlacement::Before => {
            parts.extend(extra.iter().map(|c| class_part(c)));
            parts.extend(classes.iter().map(class_part));
        }
        ClassPlacement::After => {
            parts.extend(classes.iter().map(class_part));
            parts.extend(extra.iter().map(|c| class_part(c)));
        }
    }
    parts.extend(pairs.iter().cloned());
    let written = |key: &str| {
        pairs
            .iter()
            .any(|p| p.split_once('=').is_some_and(|(k, _)| k == key))
    };
    for (key, value) in &engine.attributes {
        if !written(key) {
            parts.push(format!("{key}='{}'", value.replace('\'', "\\'")));
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn attrs(v: Value) -> EngineAttrs {
        let (a, rejected) = EngineAttrs::parse(&v);
        assert!(rejected.is_empty(), "unexpected rejections: {rejected:?}");
        a
    }

    /// Parse `head` as the attributes of a fenced div through the real qmd reader, and
    /// return `(id, classes, key/value pairs)`. Panics when the document does not parse
    /// cleanly or the div is missing: a head that only looks right as a string is not enough.
    fn parse_div(head: &str) -> (String, Vec<String>, Vec<(String, String)>) {
        let doc = format!("::: {{{head}}}\n\ncontent\n\n:::\n");
        let mut sink = Vec::<u8>::new();
        let (ast, _ctx, diags) =
            pampa::readers::qmd::read(doc.as_bytes(), false, "t.qmd", &mut sink, true, None)
                .unwrap_or_else(|e| panic!("`{head}` does not parse: {e:?}"));
        assert!(
            diags.is_empty(),
            "`{head}` parses with diagnostics: {diags:?}"
        );
        let quarto_pandoc_types::Block::Div(div) = &ast.blocks[0] else {
            panic!("`{head}` is not a div: {:?}", ast.blocks[0]);
        };
        let (id, classes, kvs) = &*div.attr;
        (
            id.clone(),
            classes.clone(),
            kvs.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        )
    }

    #[test]
    fn absent_engine_attrs_leave_the_head_byte_identical() {
        let head = build_head(
            Some("tbl-a"),
            &s(&["cell", "x"]),
            &s(&["layout=\"[1]\"", "keep-hidden='true'"]),
            None,
            ClassPlacement::After,
        );
        assert_eq!(head, "#tbl-a .cell .x layout=\"[1]\" keep-hidden='true'");
        let empty = EngineAttrs::default();
        assert_eq!(
            build_head(
                None,
                &s(&["cell"]),
                &[],
                Some(&empty),
                ClassPlacement::After
            ),
            ".cell"
        );
    }

    #[test]
    fn cell_head_is_id_then_classes_then_presenter_pairs_then_engine_pairs() {
        let a = attrs(json!({
            "id": "cell-0",
            "classes": ["extra"],
            "attributes": {"execution_count": "1"},
        }));
        let head = build_head(
            None,
            &s(&["cell", "mine"]),
            &s(&["layout=\"[1]\""]),
            Some(&a),
            ClassPlacement::After,
        );
        assert_eq!(
            head,
            "#cell-0 .cell .mine .extra layout=\"[1]\" execution_count='1'"
        );
        let (id, classes, kvs) = parse_div(&head);
        assert_eq!(id, "cell-0");
        assert_eq!(classes, ["cell", "mine", "extra"]);
        assert_eq!(
            kvs,
            [
                ("layout".to_string(), "[1]".to_string()),
                ("execution_count".to_string(), "1".to_string())
            ]
        );
    }

    #[test]
    fn output_div_engine_classes_come_first() {
        let a = attrs(json!({"classes": ["cell-output"]}));
        let head = build_head(
            None,
            &s(&["cell-output-display"]),
            &[],
            Some(&a),
            ClassPlacement::Before,
        );
        assert_eq!(head, ".cell-output .cell-output-display");
        assert_eq!(parse_div(&head).1, ["cell-output", "cell-output-display"]);
    }

    #[test]
    fn a_presenter_id_wins_over_an_engine_id() {
        let a = attrs(json!({"id": "cell-3"}));
        let head = build_head(
            Some("tbl-a"),
            &s(&["cell"]),
            &[],
            Some(&a),
            ClassPlacement::After,
        );
        assert_eq!(head, "#tbl-a .cell");
    }

    #[test]
    fn an_engine_class_never_replaces_or_duplicates_a_presenter_class() {
        let a = attrs(json!({"classes": ["cell", "new"]}));
        let head = build_head(None, &s(&["cell"]), &[], Some(&a), ClassPlacement::After);
        assert_eq!(head, ".cell .new");
    }

    #[test]
    fn an_engine_attribute_colliding_with_a_presenter_key_is_ignored() {
        let a = attrs(json!({"attributes": {"layout": "x", "keep-hidden": "y", "other": "z"}}));
        let head = build_head(
            None,
            &s(&["cell"]),
            &s(&["layout=\"[1]\"", "keep-hidden='true'"]),
            Some(&a),
            ClassPlacement::After,
        );
        assert_eq!(head, ".cell layout=\"[1]\" keep-hidden='true' other='z'");
    }

    #[test]
    fn a_quote_in_a_value_is_escaped_and_parses_back() {
        let a = attrs(json!({"attributes": {"title": "it's"}}));
        let head = build_head(None, &s(&["cell"]), &[], Some(&a), ClassPlacement::After);
        assert_eq!(head, ".cell title='it\\'s'");
        let (_, _, kvs) = parse_div(&head);
        assert_eq!(kvs, [("title".to_string(), "it's".to_string())]);
    }

    /// Each malformed form is skipped with a reason, the rest survives, and what is emitted
    /// parses.
    #[test]
    fn malformed_entries_are_skipped_and_the_rest_renders() {
        let raw = json!({
            "id": "caf\u{e9}",
            "classes": ["1bad", "ok", "has space", "-lead"],
            "attributes": {
                "good": "v",
                "id": "x",
                "class": "y",
                "9key": "v",
                "trail": "back\\",
                "nl": "a\nb",
                "cr": "a\rb",
                "sc": "{{< meta x >}}",
                "lead": " v",
                "num": 3
            },
        });
        let (a, rejected) = EngineAttrs::parse(&raw);
        // id, three classes, `id`, `class`, `9key`, trail, nl, cr, sc, lead, num.
        assert_eq!(rejected.len(), 1 + 3 + 3 + 4 + 1 + 1, "{rejected:?}");
        let head = build_head(None, &s(&["cell"]), &[], Some(&a), ClassPlacement::After);
        assert_eq!(head, ".cell .ok good='v'");
        let (id, classes, kvs) = parse_div(&head);
        assert_eq!(id, "");
        assert_eq!(classes, ["cell", "ok"]);
        assert_eq!(kvs, [("good".to_string(), "v".to_string())]);
    }

    #[test]
    fn a_non_object_is_skipped_whole() {
        let (a, rejected) = EngineAttrs::parse(&json!("cell-0"));
        assert!(a.is_empty());
        assert_eq!(rejected.len(), 1);
    }

    #[test]
    fn an_id_equal_to_another_cells_id_is_not_deduplicated() {
        // The engine route does not deduplicate either.
        let a = attrs(json!({"id": "cell-0"}));
        for _ in 0..2 {
            let head = build_head(None, &s(&["cell"]), &[], Some(&a), ClassPlacement::After);
            assert_eq!(parse_div(&head).0, "cell-0");
        }
    }

    #[test]
    fn dotted_ids_and_dashed_classes_parse_and_a_dotted_class_is_refused() {
        let (a, rejected) =
            EngineAttrs::parse(&json!({"id": "a.b_c-1", "classes": ["x.y", "z_1-2"]}));
        assert_eq!(rejected.len(), 1, "{rejected:?}");
        let head = build_head(None, &s(&["cell"]), &[], Some(&a), ClassPlacement::After);
        let (id, classes, _) = parse_div(&head);
        assert_eq!(id, "a.b_c-1");
        assert_eq!(classes, ["cell", "z_1-2"]);
    }
}
