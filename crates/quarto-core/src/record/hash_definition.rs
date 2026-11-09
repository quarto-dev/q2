//! The engine's execution-input hash declaration, as data (epic decision 35).
//!
//! Core owns the algorithm; each engine declares its rule with a [`HashDefinition`]. The
//! same type is stored in a record's `quarto_engines[].hash_definition` and travels on the
//! engine wire (EXR-7), always camelCase, with [`Granularity`] as `unit` or `document`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Whether an engine's units are individually matchable.
///
/// `Document` differs only in that the engine's units are not matched one by one: its cells
/// are judged together, by the engine's document hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Granularity {
    Unit,
    Document,
}

/// A named alternative to the default profile.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HashProfile {
    /// Cell options excluded from the unit exec hash.
    #[serde(default)]
    pub presentation_cell_options: Vec<String>,
    /// `execute:` keys excluded from the document hash.
    #[serde(default)]
    pub presentation_execute_keys: Vec<String>,
}

/// One engine's declaration of what counts toward its execution-input hashes.
///
/// The default profile is the top-level `presentation_*` lists. An engine that declares
/// nothing gets empty lists, so everything counts: it over-invalidates and never shows stale
/// output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HashDefinition {
    /// `"jupyter"`, `"knitr"`, `"julia"`, `"marimo"`, ...
    pub id: String,
    pub version: u32,
    pub granularity: Granularity,
    /// The default profile: cell options excluded from the unit exec hash.
    #[serde(default)]
    pub presentation_cell_options: Vec<String>,
    /// The default profile: `execute:` keys excluded from the document hash.
    #[serde(default)]
    pub presentation_execute_keys: Vec<String>,
    /// Extra top-level metadata keys that count toward the document hash.
    #[serde(default)]
    pub document_keys: Vec<String>,
    /// Optional named alternatives to the default profile; omitted when empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, HashProfile>,
}

impl HashDefinition {
    /// The conservative default: empty exclusion lists, unit granularity, version 1.
    pub fn conservative(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: 1,
            granularity: Granularity::Unit,
            presentation_cell_options: Vec::new(),
            presentation_execute_keys: Vec::new(),
            document_keys: Vec::new(),
            profiles: BTreeMap::new(),
        }
    }

    /// Whether core\'s presenter must enforce `option` (`warning`, `message`) for a unit that
    /// names `profile` (decision D2, OPR-2).
    ///
    /// Exactly when the declaration lists the option as display-only in that profile (the
    /// default lists, or the named profile\'s): an option in the hash was enforced at capture.
    /// A declaration that lists nothing in any profile says nothing, so a hash that is
    /// conservative by omission must not read as "capture enforced everything": the presenter
    /// enforces. A declaration that lists something (knitr\'s structured profile, Jupyter\'s
    /// list) is an answer, and an option it leaves out was applied at capture.
    pub fn presenter_enforces(&self, profile: Option<&str>, option: &str) -> bool {
        let lists =
            |cell: &[String], execute: &[String]| cell.iter().chain(execute).any(|k| k == option);
        let declares_anything = !self.presentation_cell_options.is_empty()
            || !self.presentation_execute_keys.is_empty()
            || self.profiles.values().any(|p| {
                !p.presentation_cell_options.is_empty() || !p.presentation_execute_keys.is_empty()
            });
        if !declares_anything {
            return true;
        }
        match profile.and_then(|name| self.profiles.get(name)) {
            Some(p) => lists(&p.presentation_cell_options, &p.presentation_execute_keys),
            None => lists(
                &self.presentation_cell_options,
                &self.presentation_execute_keys,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_profiles() -> HashDefinition {
        let mut def = HashDefinition::conservative("knitr");
        def.granularity = Granularity::Document;
        def.presentation_cell_options = vec!["echo".into(), "output".into()];
        def.presentation_execute_keys = vec!["echo".into()];
        def.document_keys = vec!["knitr".into()];
        def.profiles.insert(
            "fallback".into(),
            HashProfile {
                presentation_cell_options: vec![],
                presentation_execute_keys: vec!["warning".into()],
            },
        );
        def
    }

    fn declaring(display_only: &[&str]) -> HashDefinition {
        let mut def = HashDefinition::conservative("jupyter");
        def.presentation_cell_options = display_only.iter().map(|s| s.to_string()).collect();
        def
    }

    #[test]
    fn an_option_the_engine_lists_as_display_only_is_enforced_by_the_presenter() {
        let def = declaring(&["echo", "warning"]);
        assert!(def.presenter_enforces(None, "warning"));
        // In the hash: capture enforced it, so the presenter leaves it alone.
        assert!(!def.presenter_enforces(None, "message"));
    }

    #[test]
    fn a_declaration_that_lists_nothing_is_no_declaration_and_the_presenter_enforces() {
        let def = HashDefinition::conservative("some-ts-engine");
        assert!(def.presenter_enforces(None, "warning"));
        assert!(def.presenter_enforces(None, "message"));
    }

    #[test]
    fn a_profile_that_declares_something_makes_an_empty_default_a_real_answer() {
        // knitr: the default profile lists nothing, the `structured` one lists presentation
        // options but never `warning`: R enforced warning at capture, for both profiles.
        let mut def = HashDefinition::conservative("knitr");
        def.profiles.insert(
            "structured".into(),
            HashProfile {
                presentation_cell_options: vec!["echo".into()],
                presentation_execute_keys: vec![],
            },
        );
        assert!(!def.presenter_enforces(None, "warning"));
        assert!(!def.presenter_enforces(Some("structured"), "warning"));
        assert!(!def.presenter_enforces(Some("structured"), "message"));
    }

    #[test]
    fn a_named_profile_is_read_instead_of_the_default() {
        let mut def = declaring(&["echo"]);
        def.profiles.insert(
            "p".into(),
            HashProfile {
                presentation_cell_options: vec!["warning".into()],
                presentation_execute_keys: vec![],
            },
        );
        assert!(!def.presenter_enforces(None, "warning"));
        assert!(def.presenter_enforces(Some("p"), "warning"));
    }

    #[test]
    fn round_trips_with_a_non_empty_profiles_map() {
        let def = with_profiles();
        let json = serde_json::to_string(&def).unwrap();
        assert_eq!(serde_json::from_str::<HashDefinition>(&json).unwrap(), def);
    }

    /// The file and the engine wire share this exact form (EXR-7 mirrors it in TypeScript):
    /// camelCase fields, `Granularity` as the lowercase strings `unit` and `document`.
    #[test]
    fn golden_json_pins_the_field_names() {
        let json = serde_json::to_value(with_profiles()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "knitr",
                "version": 1,
                "granularity": "document",
                "presentationCellOptions": ["echo", "output"],
                "presentationExecuteKeys": ["echo"],
                "documentKeys": ["knitr"],
                "profiles": {
                    "fallback": {
                        "presentationCellOptions": [],
                        "presentationExecuteKeys": ["warning"]
                    }
                }
            })
        );
        let unit = serde_json::to_value(HashDefinition::conservative("jupyter")).unwrap();
        assert_eq!(unit["granularity"], "unit");
        assert!(unit.get("profiles").is_none(), "empty profiles are omitted");
    }

    #[test]
    fn a_minimal_wire_declaration_reads_with_empty_lists() {
        let def: HashDefinition =
            serde_json::from_str(r#"{"id":"julia","version":2,"granularity":"unit"}"#).unwrap();
        assert_eq!(def.id, "julia");
        assert_eq!(def.version, 2);
        assert!(def.presentation_cell_options.is_empty() && def.profiles.is_empty());
    }
}
