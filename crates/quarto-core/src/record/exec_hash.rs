//! The execution-input hashes (EXR-4 Phase 2; epic decisions 34 and 35).
//!
//! Core owns the algorithm and names no engine: each engine declares a [`HashDefinition`] as
//! data. SHA-256 over length-prefixed fields (each field is its UTF-8 bytes preceded by their
//! length as an 8-byte big-endian integer), CRLF normalized to LF, the first two fields being
//! the declaration's `id` and `version` (decimal) so a change never silently matches. The
//! digest is 64 lowercase hex characters. Option values are canonical JSON of the parsed
//! YAML (compact, object keys sorted), so `7`, `7.0` and `"7"` hash differently.
//!
//! This is the *execution-input* hash, used for record-level staleness and as freeze
//! groundwork. It is never used for matching (that is Positron's `quarto_content_hash`).

use std::collections::{BTreeMap, BTreeSet};

use quarto_source_map::{FileId, SourceInfo};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cell_options::partition_cell_options;

use super::hash_definition::{HashDefinition, HashProfile};
use super::types::{RecordCell, UnitKind};

/// Why a hash could not be computed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HashError {
    /// A unit names a profile the stored declaration lacks, so it cannot be recomputed and
    /// reads as stale.
    #[error("hash declaration `{engine}` has no profile `{profile}`")]
    UnknownProfile { engine: String, profile: String },
    /// The cell's option lines are not a YAML mapping.
    #[error("cell options are not a valid YAML mapping: {0}")]
    InvalidOptions(String),
}

/// Canonical JSON: compact, object keys sorted, numbers as `serde_json` prints them (so an
/// integer `7` and a float `7.0` differ).
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            out.push('{');
            for (i, (k, v)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).expect("string serializes"));
                out.push(':');
                write_canonical(v, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(v, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&serde_json::to_string(scalar).expect("scalar serializes")),
    }
}

/// Convert a parsed YAML node to JSON, exactly as q2's YAML parser yields it. A tagged value
/// (`!expr 1 + 1`) keeps its tag, since the tag changes what the value means.
pub fn yaml_to_json(node: &quarto_yaml::YamlWithSourceInfo) -> Value {
    use yaml_rust2::Yaml;
    let inner = if let Some(items) = node.as_array() {
        Value::Array(items.iter().map(yaml_to_json).collect())
    } else if let Some(entries) = node.as_hash() {
        let mut map = serde_json::Map::new();
        for e in entries {
            let key = match yaml_to_json(&e.key) {
                Value::String(s) => s,
                other => canonical_json(&other),
            };
            map.insert(key, yaml_to_json(&e.value));
        }
        Value::Object(map)
    } else {
        match &node.yaml {
            Yaml::Integer(i) => Value::from(*i),
            Yaml::Real(s) => s
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map_or_else(|| Value::String(s.clone()), Value::Number),
            Yaml::String(s) => Value::String(s.clone()),
            Yaml::Boolean(b) => Value::Bool(*b),
            _ => Value::Null,
        }
    };
    match &node.tag {
        Some((tag, _)) => serde_json::json!({ "!tag": tag, "value": inner }),
        None => inner,
    }
}

/// A cell's author text split into its options (as JSON) and its canonical code.
pub fn partition_author_text(
    lang: &str,
    text: &str,
) -> Result<(BTreeMap<String, Value>, String), HashError> {
    let body_source = SourceInfo::original(FileId(0), 0, text.len());
    let partitioned = partition_cell_options(lang, text, body_source)
        .map_err(|e| HashError::InvalidOptions(e.to_string()))?;
    let options = match &partitioned.options {
        None => BTreeMap::new(),
        Some(yaml) => match yaml_to_json(yaml) {
            Value::Object(map) => map.into_iter().collect(),
            Value::Null => BTreeMap::new(),
            other => {
                return Err(HashError::InvalidOptions(format!(
                    "expected a mapping, found {}",
                    canonical_json(&other)
                )));
            }
        },
    };
    Ok((options, partitioned.code))
}

/// The length-prefixed field writer.
struct Fields(Sha256);

impl Fields {
    fn new(def: &HashDefinition, tag: &str) -> Self {
        let mut f = Fields(Sha256::new());
        f.field(&def.id);
        f.field(&def.version.to_string());
        f.field(tag);
        f
    }

    fn field(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n");
        self.0.update((normalized.len() as u64).to_be_bytes());
        self.0.update(normalized.as_bytes());
    }

    fn finish(self) -> String {
        hex::encode(self.0.finalize())
    }
}

fn profile_lists<'a>(
    def: &'a HashDefinition,
    profile: Option<&str>,
) -> Result<(&'a [String], &'a [String]), HashError> {
    match profile {
        None => Ok((
            &def.presentation_cell_options,
            &def.presentation_execute_keys,
        )),
        Some(name) => {
            let p: &HashProfile =
                def.profiles
                    .get(name)
                    .ok_or_else(|| HashError::UnknownProfile {
                        engine: def.id.clone(),
                        profile: name.to_string(),
                    })?;
            Ok((&p.presentation_cell_options, &p.presentation_execute_keys))
        }
    }
}

/// A unit's options minus the presentation options of its profile: the execution-relevant
/// options that the hash covers and that a record stores as flat cell keys, so the two can
/// never disagree.
pub fn execution_options(
    def: &HashDefinition,
    profile: Option<&str>,
    options: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, HashError> {
    let (excluded, _) = profile_lists(def, profile)?;
    Ok(options
        .iter()
        .filter(|(k, _)| !excluded.iter().any(|e| e == *k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect())
}

/// What a unit hash covers.
pub struct UnitHashInput<'a> {
    pub kind: UnitKind,
    /// The language token; lowercased before hashing.
    pub lang: &'a str,
    /// The code after option partition.
    pub code: &'a str,
    /// All of the unit's options (the profile's presentation options are removed here).
    pub options: &'a BTreeMap<String, Value>,
    /// The text after the language token inside the fence braces (`{r echo=FALSE}` gives
    /// `echo=FALSE`), trimmed; empty for an inline span or a bare `{lang}`. It is in no `#|`
    /// option and not in the code, so it is hashed on its own, and every engine over-invalidates
    /// on a change to it (an engine cannot opt out).
    pub attr_tail: &'a str,
    /// The hash profile the unit names (`quarto_hash_rule`); `None` is the default.
    pub profile: Option<&'a str>,
}

/// The unit hash (`quarto_exec_hash`). Fields in order: `id`, `version`, the tag `unit`, kind,
/// language (lowercased), the code, and the execution-relevant options as one canonical JSON
/// object. The kernel is not an input: it is chosen at execution time, which a viewer with no
/// engine loaded cannot reproduce.
pub fn hash_unit(def: &HashDefinition, unit: &UnitHashInput) -> Result<String, HashError> {
    let options = execution_options(def, unit.profile, unit.options)?;
    let mut f = Fields::new(def, "unit");
    f.field(match unit.kind {
        UnitKind::Cell => "cell",
        UnitKind::Inline => "inline",
    });
    f.field(&unit.lang.to_ascii_lowercase());
    f.field(unit.code);
    f.field(&canonical_json(&Value::Object(
        options.into_iter().collect(),
    )));
    // Only when present, so a unit with no tail keeps the hash it had before the tail counted.
    if !unit.attr_tail.is_empty() {
        f.field("attr-tail");
        f.field(unit.attr_tail);
    }
    Ok(f.finish())
}

/// A unit's hash and the profile it was hashed under, as the document hash needs them.
#[derive(Debug, Clone, Copy)]
pub struct UnitHashRef<'a> {
    pub hash: &'a str,
    pub profile: Option<&'a str>,
}

/// The `execute:` keys excluded from the document hash: the presentation keys common to every
/// profile that one of the engine's units uses (the strictest rule), or the default profile's
/// when the record has no units.
fn excluded_execute_keys(
    def: &HashDefinition,
    units: &[UnitHashRef],
) -> Result<BTreeSet<String>, HashError> {
    let used: BTreeSet<Option<&str>> = if units.is_empty() {
        BTreeSet::from([None])
    } else {
        units.iter().map(|u| u.profile).collect()
    };
    let mut common: Option<BTreeSet<String>> = None;
    for profile in used {
        let (_, keys) = profile_lists(def, profile)?;
        let keys: BTreeSet<String> = keys.iter().cloned().collect();
        common = Some(match common {
            None => keys,
            Some(c) => c.intersection(&keys).cloned().collect(),
        });
    }
    Ok(common.unwrap_or_default())
}

fn optional_json(value: Option<&Value>) -> String {
    value.map(canonical_json).unwrap_or_default()
}

/// One engine's document hash, computed with its own declaration over the merged top-level
/// metadata (project and document merged, overlay-free) and the ordered hashes of its units.
///
/// Covers the merged `engine` key, `execute-dir`, `params`, every top-level key in
/// `document_keys`, the `execute:` map minus the excluded presentation keys, and the unit
/// hashes, for every engine whatever its granularity. Unknown keys count: the exclusion lists
/// are the only way a key stops counting. Core always excludes `record` (the opt-in of epic
/// decision 24), so toggling it never changes a hash.
pub fn hash_document(
    def: &HashDefinition,
    metadata: &BTreeMap<String, Value>,
    units: &[UnitHashRef],
) -> Result<String, HashError> {
    let excluded = excluded_execute_keys(def, units)?;
    let mut f = Fields::new(def, "document");
    f.field(&optional_json(metadata.get("engine")));
    f.field(&optional_json(metadata.get("execute-dir")));
    f.field(&optional_json(metadata.get("params")));

    let keys: BTreeSet<&String> = def
        .document_keys
        .iter()
        .filter(|k| *k != "record")
        .collect();
    let extra: serde_json::Map<String, Value> = keys
        .into_iter()
        .filter_map(|k| metadata.get(k).map(|v| (k.clone(), v.clone())))
        .collect();
    f.field(&canonical_json(&Value::Object(extra)));

    let execute: serde_json::Map<String, Value> = match metadata.get("execute") {
        Some(Value::Object(map)) => map
            .iter()
            .filter(|(k, _)| !excluded.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        _ => serde_json::Map::new(),
    };
    f.field(&canonical_json(&Value::Object(execute)));

    f.field(&units.len().to_string());
    for unit in units {
        f.field(unit.hash);
    }
    Ok(f.finish())
}

/// `quarto_execution_hash`: a hash over the ordered per-engine document hashes. It is what
/// record-level staleness compares (EXR-15).
pub fn hash_record(document_hashes: &[&str]) -> String {
    let mut hasher = Sha256::new();
    let mut field = |text: &str| {
        hasher.update((text.len() as u64).to_be_bytes());
        hasher.update(text.as_bytes());
    };
    field("quarto-execution-hash");
    field("1");
    field(&document_hashes.len().to_string());
    for h in document_hashes {
        field(h);
    }
    hex::encode(hasher.finalize())
}

/// Recompute a stored cell's unit hash from the cell alone and its engine's stored
/// declaration, with no engine loaded: the code is the source after the option lines, and the
/// options are the cell's flat keys (already free of the profile's presentation options).
pub fn recompute_cell_hash(def: &HashDefinition, cell: &RecordCell) -> Result<String, HashError> {
    let author_text = cell.source.concat();
    let lang = &cell.metadata.quarto_lang;
    let code = match cell.metadata.quarto_kind {
        // An inline unit's source is the expression; it has no option lines.
        UnitKind::Inline => author_text,
        UnitKind::Cell => {
            let (_, code) = partition_author_text(lang, &author_text)?;
            code
        }
    };
    let options: BTreeMap<String, Value> = cell
        .metadata
        .options()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    hash_unit(
        def,
        &UnitHashInput {
            kind: cell.metadata.quarto_kind,
            lang,
            code: &code,
            options: &options,
            attr_tail: "",
            profile: cell.metadata.quarto_hash_rule.as_deref(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{CellMetadata, CellType, Granularity, content_hash};
    use serde_json::json;

    fn def() -> HashDefinition {
        HashDefinition::conservative("jupyter")
    }

    fn opts(v: Value) -> BTreeMap<String, Value> {
        v.as_object().unwrap().clone().into_iter().collect()
    }

    fn unit<'a>(code: &'a str, options: &'a BTreeMap<String, Value>) -> UnitHashInput<'a> {
        UnitHashInput {
            kind: UnitKind::Cell,
            lang: "python",
            code,
            options,
            attr_tail: "",
            profile: None,
        }
    }

    fn meta(v: Value) -> BTreeMap<String, Value> {
        opts(v)
    }

    #[test]
    fn both_golden_vectors_of_the_algorithm_pass() {
        let o = opts(json!({"fig-width": 7}));
        assert_eq!(
            hash_unit(&def(), &unit("x = 1", &o)).unwrap(),
            "d8cae62dd1cc0cf4301084561ca4bee22fd98c2ab1315ad53ff149043616f07a"
        );
        let none = opts(json!({}));
        assert_eq!(
            hash_unit(
                &def(),
                &UnitHashInput {
                    kind: UnitKind::Inline,
                    ..unit("1 + 1", &none)
                }
            )
            .unwrap(),
            "7c77518b4cf801cf8cf3cfbaf1024e285b8e28366efd0c4043672b11c7080deb"
        );
    }

    #[test]
    fn equal_inputs_hash_equal_and_the_id_and_version_move_it() {
        let o = opts(json!({}));
        let base = hash_unit(&def(), &unit("x", &o)).unwrap();
        assert_eq!(base, hash_unit(&def(), &unit("x", &o)).unwrap());
        assert_eq!(base.len(), 64);
        let mut v2 = def();
        v2.version = 2;
        assert_ne!(base, hash_unit(&v2, &unit("x", &o)).unwrap());
        assert_ne!(
            base,
            hash_unit(&HashDefinition::conservative("knitr"), &unit("x", &o)).unwrap()
        );
    }

    #[test]
    fn each_counted_field_changes_the_unit_hash() {
        let o = opts(json!({"a": 1}));
        let base = hash_unit(&def(), &unit("x", &o)).unwrap();
        let other_code = hash_unit(&def(), &unit("y", &o)).unwrap();
        let other_opts = hash_unit(&def(), &unit("x", &opts(json!({"a": 2})))).unwrap();
        let other_kind = hash_unit(
            &def(),
            &UnitHashInput {
                kind: UnitKind::Inline,
                ..unit("x", &o)
            },
        )
        .unwrap();
        let other_lang = hash_unit(
            &def(),
            &UnitHashInput {
                lang: "r",
                ..unit("x", &o)
            },
        )
        .unwrap();
        for changed in [other_code, other_opts, other_kind, other_lang] {
            assert_ne!(base, changed);
        }
        // The language is lowercased, so its case is not an input.
        let upper = hash_unit(
            &def(),
            &UnitHashInput {
                lang: "PYTHON",
                ..unit("x", &o)
            },
        )
        .unwrap();
        assert_eq!(base, upper);
    }

    #[test]
    fn a_presentation_option_does_not_move_the_hash_and_a_default_declaration_counts_everything() {
        let mut jup = def();
        jup.presentation_cell_options = vec!["echo".into()];
        let with_echo = opts(json!({"fig-width": 7, "echo": false}));
        let without = opts(json!({"fig-width": 7}));
        assert_eq!(
            hash_unit(&jup, &unit("x", &with_echo)).unwrap(),
            hash_unit(&jup, &unit("x", &without)).unwrap()
        );
        // An engine that declares nothing hashes everything.
        assert_ne!(
            hash_unit(&def(), &unit("x", &with_echo)).unwrap(),
            hash_unit(&def(), &unit("x", &without)).unwrap()
        );
        // The recorded flat keys are exactly the options that move the hash.
        let recorded = execution_options(&jup, None, &with_echo).unwrap();
        assert_eq!(recorded, without);
    }

    #[test]
    fn option_encoding_separates_types_and_ignores_key_order() {
        let h = |o: Value| hash_unit(&def(), &unit("x", &opts(o))).unwrap();
        let seven = h(json!({"w": 7}));
        let float = h(json!({"w": 7.0}));
        let string = h(json!({"w": "7"}));
        assert!(seven != float && seven != string && float != string);
        assert_eq!(h(json!({"a": 1, "b": 2})), h(json!({"b": 2, "a": 1})));
        assert_eq!(
            canonical_json(&json!({"b": [1, {"z": 1, "y": 2}], "a": null})),
            r#"{"a":null,"b":[1,{"y":2,"z":1}]}"#
        );
    }

    #[test]
    fn yaml_options_keep_their_type_and_tag() {
        let get = |text: &str| partition_author_text("python", text).unwrap().0;
        assert_eq!(get("#| fig-width: 7\nx"), opts(json!({"fig-width": 7})));
        assert_eq!(get("#| fig-width: 7.0\nx"), opts(json!({"fig-width": 7.0})));
        assert_eq!(
            get("#| fig-width: \"7\"\nx"),
            opts(json!({"fig-width": "7"}))
        );
        assert_ne!(get("#| eval: !expr 1 + 1\nx"), get("#| eval: 1 + 1\nx"));
        assert!(get("x = 1").is_empty());
    }

    #[test]
    fn a_crlf_code_field_hashes_like_its_lf_twin() {
        let o = opts(json!({}));
        assert_eq!(
            hash_unit(&def(), &unit("a\r\nb", &o)).unwrap(),
            hash_unit(&def(), &unit("a\nb", &o)).unwrap()
        );
    }

    /// A moved label line changes the content hash (the author text differs) and not the exec
    /// hash (which is built from canonical code plus the parsed options).
    #[test]
    fn moving_the_label_line_changes_the_content_hash_and_not_the_exec_hash() {
        let a = "#| label: fig-a\n#| fig-width: 7\nx = 1";
        let b = "#| fig-width: 7\n#| label: fig-a\nx = 1";
        assert_ne!(content_hash(a), content_hash(b));
        let exec = |t: &str| {
            let (o, code) = partition_author_text("python", t).unwrap();
            hash_unit(&def(), &unit(&code, &o)).unwrap()
        };
        assert_eq!(exec(a), exec(b));
    }

    fn stored_cell(
        source: &[&str],
        options: Value,
        kernel: &str,
        profile: Option<&str>,
    ) -> RecordCell {
        RecordCell {
            cell_type: CellType::Code,
            execution_count: None,
            id: "x".into(),
            metadata: CellMetadata {
                quarto_cell_id: "0-00000000-unlabeled".into(),
                quarto_content_hash: "0".repeat(16),
                quarto_label: None,
                quarto_kind: UnitKind::Cell,
                quarto_lang: "python".into(),
                quarto_engine: Some("jupyter".into()),
                quarto_kernel: Some(kernel.into()),
                quarto_location: None,
                quarto_exec_hash: String::new(),
                quarto_hash_rule: profile.map(Into::into),
                extra: opts(options),
            },
            outputs: vec![],
            source: source.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn the_recorded_kernel_is_not_a_hash_input() {
        let a = stored_cell(&["x = 1"], json!({}), "python3", None);
        let b = stored_cell(&["x = 1"], json!({}), "python3-other", None);
        assert_eq!(
            recompute_cell_hash(&def(), &a).unwrap(),
            recompute_cell_hash(&def(), &b).unwrap()
        );
    }

    fn profiled() -> HashDefinition {
        let mut d = def();
        d.presentation_cell_options = vec!["echo".into()];
        d.presentation_execute_keys = vec!["echo".into(), "warning".into()];
        d.profiles.insert(
            "all".into(),
            HashProfile {
                presentation_cell_options: vec![],
                presentation_execute_keys: vec!["warning".into()],
            },
        );
        d.profiles.insert(
            "second".into(),
            HashProfile {
                presentation_cell_options: vec!["echo".into(), "fig-cap".into()],
                presentation_execute_keys: vec!["echo".into()],
            },
        );
        d
    }

    #[test]
    fn a_profile_that_counts_every_option_changes_the_hash_on_an_echo_edit() {
        let d = profiled();
        let with = opts(json!({"echo": false}));
        let without = opts(json!({}));
        let tagged = |o: &BTreeMap<String, Value>| {
            hash_unit(
                &d,
                &UnitHashInput {
                    attr_tail: "",
                    profile: Some("all"),
                    ..unit("x", o)
                },
            )
            .unwrap()
        };
        let untagged = |o: &BTreeMap<String, Value>| hash_unit(&d, &unit("x", o)).unwrap();
        assert_ne!(tagged(&with), tagged(&without));
        assert_eq!(untagged(&with), untagged(&without));
    }

    /// The flat keys a cell records are exactly the options that move its hash, under the
    /// profile the unit names: the default-profile unit carries `echo` and `fig-cap`, the
    /// named-profile unit does not.
    #[test]
    fn recorded_option_keys_are_the_options_that_move_each_profiles_hash() {
        let mut d = def();
        d.profiles.insert(
            "second".into(),
            HashProfile {
                presentation_cell_options: vec!["echo".into(), "fig-cap".into()],
                presentation_execute_keys: vec![],
            },
        );
        let all = opts(json!({"echo": false, "fig-cap": "c", "fig-width": 7}));
        for (profile, expected) in [
            (None, vec!["echo", "fig-cap", "fig-width"]),
            (Some("second"), vec!["fig-width"]),
        ] {
            let recorded: Vec<String> = execution_options(&d, profile, &all)
                .unwrap()
                .into_keys()
                .collect();
            assert_eq!(recorded, expected, "profile {profile:?}");
            let hash = |o: &BTreeMap<String, Value>| {
                hash_unit(
                    &d,
                    &UnitHashInput {
                        profile,
                        ..unit("x", o)
                    },
                )
                .unwrap()
            };
            let base = hash(&all);
            for key in all.keys() {
                let mut changed = all.clone();
                changed.insert(key.clone(), json!("changed"));
                assert_eq!(
                    hash(&changed) != base,
                    expected.contains(&key.as_str()),
                    "{key} under {profile:?}"
                );
            }
        }
    }

    #[test]
    fn a_unit_naming_a_missing_profile_cannot_be_recomputed() {
        let o = opts(json!({}));
        let err = hash_unit(
            &def(),
            &UnitHashInput {
                attr_tail: "",
                profile: Some("nope"),
                ..unit("x", &o)
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            HashError::UnknownProfile {
                engine: "jupyter".into(),
                profile: "nope".into()
            }
        );
        let cell = stored_cell(&["x"], json!({}), "k", Some("nope"));
        assert!(recompute_cell_hash(&def(), &cell).is_err());
        assert!(
            hash_document(
                &def(),
                &meta(json!({})),
                &[UnitHashRef {
                    hash: "h",
                    profile: Some("nope")
                }]
            )
            .is_err()
        );
    }

    #[test]
    fn the_document_hash_drops_an_execute_key_only_when_every_used_profile_excludes_it() {
        let d = profiled();
        let m = |warning: bool, echo: bool| {
            meta(json!({"execute": {"warning": warning, "echo": echo}}))
        };
        let doc = |m: &BTreeMap<String, Value>, units: &[UnitHashRef]| {
            hash_document(&d, m, units).unwrap()
        };
        let default = [UnitHashRef {
            hash: "h",
            profile: None,
        }];
        let all = [UnitHashRef {
            hash: "h",
            profile: Some("all"),
        }];
        let both = [
            UnitHashRef {
                hash: "h",
                profile: None,
            },
            UnitHashRef {
                hash: "h",
                profile: Some("all"),
            },
        ];
        // Default profile excludes `warning` and `echo`.
        assert_eq!(
            doc(&m(true, true), &default),
            doc(&m(false, false), &default)
        );
        // `all` excludes only `warning`: `echo` counts again.
        assert_eq!(doc(&m(true, true), &all), doc(&m(false, true), &all));
        assert_ne!(doc(&m(true, true), &all), doc(&m(true, false), &all));
        // Mixed use takes the intersection, which is `warning` alone here.
        assert_eq!(doc(&m(true, true), &both), doc(&m(false, true), &both));
        assert_ne!(doc(&m(true, true), &both), doc(&m(true, false), &both));
        // A record with no units uses the default profile.
        assert_eq!(doc(&m(true, true), &[]), doc(&m(false, false), &[]));
    }

    fn doc_def() -> HashDefinition {
        let mut d = def();
        d.presentation_execute_keys = vec!["echo".into()];
        d.document_keys = vec!["jupyter".into(), "record".into()];
        d
    }

    #[test]
    fn document_hash_inputs_each_move_it_and_excluded_keys_do_not() {
        let d = doc_def();
        let units = [UnitHashRef {
            hash: "u1",
            profile: None,
        }];
        let base_meta = json!({
            "engine": "jupyter", "execute-dir": "file", "params": {"a": 1},
            "jupyter": "python3", "execute": {"eval": true, "echo": true}
        });
        let base = hash_document(&d, &meta(base_meta.clone()), &units).unwrap();
        let changed = |key: &str, new: Value| {
            let mut m = base_meta.clone();
            m[key] = new;
            hash_document(&d, &meta(m), &units).unwrap()
        };
        assert_ne!(base, changed("engine", json!("knitr")));
        assert_ne!(base, changed("execute-dir", json!("project")));
        assert_ne!(base, changed("params", json!({"a": 2})));
        assert_ne!(base, changed("jupyter", json!("python4")));
        assert_ne!(
            base,
            changed("execute", json!({"eval": false, "echo": true}))
        );
        // An unknown `execute:` key counts (only the exclusion list stops a key counting).
        assert_ne!(
            base,
            changed("execute", json!({"eval": true, "echo": true, "novel": 1}))
        );
        // An excluded `execute:` key does not.
        assert_eq!(
            base,
            changed("execute", json!({"eval": true, "echo": false}))
        );
        // A top-level key that is not declared does not exist for the hash.
        assert_eq!(base, changed("title", json!("anything")));
        // The unit hashes are inputs, in order.
        let other = [UnitHashRef {
            hash: "u2",
            profile: None,
        }];
        assert_ne!(base, hash_document(&d, &meta(base_meta), &other).unwrap());
    }

    /// The declaration the marimo engine returns as `recordInfo.hashDefinition` (EXR-9,
    /// Phase 9A). The engine reads `eval`, `external-env` and `pyproject` from top-level
    /// metadata, so they are `document_keys`; the bare-sql flag and the mode need no field
    /// (see the test below).
    fn marimo_def() -> HashDefinition {
        let mut d = HashDefinition::conservative("marimo");
        d.granularity = Granularity::Document;
        d.document_keys = vec!["eval".into(), "external-env".into(), "pyproject".into()];
        d
    }

    #[test]
    fn marimo_declaration_counts_its_top_level_keys_and_every_owned_cell() {
        let d = marimo_def();
        let hash = |m: Value, units: &[&str]| {
            let refs: Vec<UnitHashRef> = units
                .iter()
                .map(|h| UnitHashRef {
                    hash: h,
                    profile: None,
                })
                .collect();
            hash_document(&d, &meta(m), &refs).unwrap()
        };
        let base_meta = json!({"engine": "marimo", "eval": true});
        let base = hash(base_meta.clone(), &["c1", "c2"]);
        // Each top-level key marimo reads moves the document hash (a key absent from the
        // base counts once it is set).
        assert_ne!(
            base,
            hash(json!({"engine": "marimo", "eval": false}), &["c1", "c2"])
        );
        assert_ne!(
            base,
            hash(
                json!({"engine": "marimo", "eval": true, "external-env": true}),
                &["c1", "c2"]
            )
        );
        assert_ne!(
            base,
            hash(
                json!({"engine": "marimo", "eval": true, "pyproject": "p.toml"}),
                &["c1", "c2"]
            )
        );
        // Marimo cells are reactive across the document: editing any one owned cell, or
        // adding or dropping one, moves the single document hash.
        assert_ne!(base, hash(base_meta.clone(), &["c1", "c2-edited"]));
        assert_ne!(base, hash(base_meta.clone(), &["c1"]));
        // A key marimo does not read does not count.
        assert_eq!(
            base,
            hash(
                json!({"engine": "marimo", "eval": true, "title": "t"}),
                &["c1", "c2"]
            )
        );
        // The bare-sql flag has no field of its own: it changes which units the engine owns
        // (an `sql` cell joins or leaves the unit list), and the unit list is hashed.
        assert_ne!(base, hash(base_meta, &["c1", "c2", "sql-cell"]));
    }

    #[test]
    fn toggling_record_changes_no_hash_even_when_an_engine_declares_it() {
        let d = doc_def(); // lists `record` in document_keys: core ignores it
        let units = [UnitHashRef {
            hash: "u1",
            profile: None,
        }];
        let off = hash_document(&d, &meta(json!({"record": false})), &units).unwrap();
        let on = hash_document(&d, &meta(json!({"record": true})), &units).unwrap();
        assert_eq!(off, on);
        assert_eq!(off, hash_document(&d, &meta(json!({})), &units).unwrap());
    }

    #[test]
    fn mixed_engines_use_their_own_declarations_and_the_record_hash_is_ordered() {
        let m = meta(json!({"execute": {"echo": true}, "jupyter": "p", "knitr": "k"}));
        let mut jup = def();
        jup.presentation_execute_keys = vec!["echo".into()];
        jup.document_keys = vec!["jupyter".into()];
        let mut knitr = HashDefinition::conservative("knitr");
        knitr.document_keys = vec!["knitr".into()];
        let units = [UnitHashRef {
            hash: "u",
            profile: None,
        }];
        let hj = hash_document(&jup, &m, &units).unwrap();
        let hk = hash_document(&knitr, &m, &units).unwrap();
        assert_ne!(hj, hk);
        // Toggling `execute: echo` moves knitr (counts everything) and not Jupyter.
        let toggled = meta(json!({"execute": {"echo": false}, "jupyter": "p", "knitr": "k"}));
        assert_eq!(hj, hash_document(&jup, &toggled, &units).unwrap());
        assert_ne!(hk, hash_document(&knitr, &toggled, &units).unwrap());
        // `quarto_execution_hash` is over the ordered per-engine values.
        assert_ne!(hash_record(&[&hj, &hk]), hash_record(&[&hk, &hj]));
        assert_eq!(hash_record(&[&hj, &hk]), hash_record(&[&hj, &hk]));
        assert_ne!(hash_record(&[&hj]), hash_record(&[&hj, &hk]));
    }

    /// A record's stored declarations reproduce its unit hashes and its
    /// `quarto_execution_hash` with no engine loaded.
    #[test]
    fn the_stored_declarations_reproduce_the_hashes_with_no_engine_loaded() {
        let mut d = doc_def();
        d.presentation_cell_options = vec!["echo".into()];
        d.granularity = Granularity::Unit;
        // Build the cell the way a writer would: author text with option lines, flat keys.
        let author = "#| fig-width: 7\n#| echo: false\nx = 1";
        let (all_opts, code) = partition_author_text("python", author).unwrap();
        let recorded = execution_options(&d, None, &all_opts).unwrap();
        let unit_hash = hash_unit(
            &d,
            &UnitHashInput {
                options: &all_opts,
                ..unit(&code, &all_opts)
            },
        )
        .unwrap();
        let mut cell = stored_cell(
            &["#| fig-width: 7\n", "#| echo: false\n", "x = 1"],
            Value::Object(recorded.into_iter().collect()),
            "python3",
            None,
        );
        cell.metadata.quarto_exec_hash = unit_hash.clone();
        assert!(
            cell.metadata.options().all(|(k, _)| k != "echo"),
            "echo is presentation"
        );
        assert_eq!(recompute_cell_hash(&d, &cell).unwrap(), unit_hash);

        let m = meta(json!({"engine": "jupyter", "jupyter": "python3"}));
        let units = [UnitHashRef {
            hash: &unit_hash,
            profile: None,
        }];
        let doc_hash = hash_document(&d, &m, &units).unwrap();
        let again = hash_document(
            &d,
            &m,
            &[UnitHashRef {
                hash: &recompute_cell_hash(&d, &cell).unwrap(),
                profile: None,
            }],
        )
        .unwrap();
        assert_eq!(doc_hash, again);
        assert_eq!(hash_record(&[&doc_hash]), hash_record(&[&again]));
    }

    #[test]
    fn an_inline_unit_recomputes_from_its_expression() {
        let mut cell = stored_cell(&["1 + 1"], json!({}), "k", None);
        cell.metadata.quarto_kind = UnitKind::Inline;
        let expected = hash_unit(
            &def(),
            &UnitHashInput {
                kind: UnitKind::Inline,
                ..unit("1 + 1", &opts(json!({})))
            },
        )
        .unwrap();
        assert_eq!(recompute_cell_hash(&def(), &cell).unwrap(), expected);
    }

    #[test]
    fn attr_tail_changes_the_unit_hash_and_an_empty_tail_changes_nothing() {
        let def = HashDefinition::conservative("knitr");
        let options = BTreeMap::new();
        let hash = |attr_tail: &str| {
            hash_unit(
                &def,
                &UnitHashInput {
                    kind: UnitKind::Cell,
                    lang: "r",
                    code: "1",
                    options: &options,
                    attr_tail,
                    profile: None,
                },
            )
            .unwrap()
        };
        assert_ne!(hash(""), hash("echo=TRUE"));
        assert_ne!(hash("echo=TRUE"), hash("echo=FALSE"));
        assert_eq!(hash("echo=TRUE"), hash("echo=TRUE"));
        // An empty tail adds no field, so every hash recorded before the tail counted still matches.
        let mut f = Fields::new(&def, "unit");
        f.field("cell");
        f.field("r");
        f.field("1");
        f.field(&canonical_json(&Value::Object(Default::default())));
        assert_eq!(hash(""), f.finish());
    }
}
