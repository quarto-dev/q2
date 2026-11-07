/*
 * engine/knitr/hash_rule.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! KNR-3: knitr's execution-input hash declaration.
//!
//! One declaration, id `knitr`, with two profiles (KNR-3 decision 1):
//!
//! - the **default** profile has empty exclusion lists, so every chunk option counts. Fallback
//!   units use it, because their finished markdown bakes every option in.
//! - the **`structured`** profile ([`STRUCTURED_PROFILE`]) leaves out a closed list of
//!   presentation-only options ([`STRUCTURED_PRESENTATION_OPTIONS`]), because the presenter reads
//!   them at present time. Structured units name it in `quarto_hash_rule`.
//!
//! Every `execute:` key counts in both (KNR-3 decisions 3 and 9). Names the list does not carry
//! count, which is also how unknown options, `attr.*` and `class.*` extensions are handled.

use crate::record::{HashDefinition, HashProfile};

/// The profile a structured knitr unit names in its `quarto_hash_rule`.
pub const STRUCTURED_PROFILE: &str = "structured";

/// The flat cell option that carries the output format on a structured unit (KNR-3 decision 8);
/// defined in `record::matching`, which the splice reads it through.
pub use crate::record::OUTPUT_FORMAT_OPTION;

/// The declaration version. Bumped from EXR-11's 1 when the `structured` profile was added, so
/// every earlier record reads as stale (KNR-3 decision 1).
pub const KNITR_HASH_VERSION: u32 = 2;

/// Presentation-only options, as KNR-1 S3 closed the list. Each name appears in knitr's dotted
/// spelling where it has one; [`structured_presentation_options`] adds the dashed twin, because the
/// hash sees the author's spelling and q2's R layer accepts either (`normalize_options`).
pub const STRUCTURED_PRESENTATION_OPTIONS: &[&str] = &[
    // knitr's text and code presentation
    "echo",
    "fenced.echo",
    "chunk.echo",
    "results",
    "collapse",
    "prompt",
    "comment",
    "strip.white",
    "highlight",
    "size",
    "background",
    "indent",
    "class.source",
    "class.output",
    "class.message",
    "class.warning",
    "class.error",
    "attr.source",
    "attr.output",
    "attr.message",
    "attr.warning",
    "attr.error",
    // figures: captions, placement, size on the page (the file's pixels are `fig.width` and `dpi`,
    // which count)
    "fig.cap",
    "fig.scap",
    "fig.subcap",
    "fig.alt",
    "fig.align",
    "fig.show",
    "fig.pos",
    "fig.env",
    "fig.lp",
    "fig.link",
    "out.width",
    "out.height",
    "out.extra",
    // Quarto cell options that only affect the page
    "classes",
    "panel",
    "column",
    "fig-column",
    "tbl-column",
    "cap-location",
    "fig-cap-location",
    "tbl-cap-location",
    "code-fold",
    "code-summary",
    "code-overflow",
    "code-line-numbers",
    "lst-cap",
    "lst-label",
    "layout",
    "layout-nrow",
    "layout-ncol",
    "layout-align",
    "layout-valign",
    "keep-hidden",
    "html-table-processing",
];

/// The list the `structured` profile excludes: every name of
/// [`STRUCTURED_PRESENTATION_OPTIONS`] in both spellings, sorted, without duplicates.
pub fn structured_presentation_options() -> Vec<String> {
    let mut names: Vec<String> = STRUCTURED_PRESENTATION_OPTIONS
        .iter()
        .flat_map(|n| [n.to_string(), n.replace('.', "-")])
        .collect();
    names.sort();
    names.dedup();
    names
}

/// knitr's declaration.
pub fn knitr_hash_definition() -> HashDefinition {
    let mut def = HashDefinition::conservative("knitr");
    def.version = KNITR_HASH_VERSION;
    def.document_keys = vec!["knitr".to_string()];
    def.profiles.insert(
        STRUCTURED_PROFILE.to_string(),
        HashProfile {
            presentation_cell_options: structured_presentation_options(),
            presentation_execute_keys: Vec::new(),
        },
    );
    def
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::*;
    use crate::record::{UnitHashInput, UnitHashRef, UnitKind, hash_document, hash_unit};

    fn unit_hash(profile: Option<&str>, options: Value) -> String {
        let options: BTreeMap<String, Value> =
            options.as_object().unwrap().clone().into_iter().collect();
        hash_unit(
            &knitr_hash_definition(),
            &UnitHashInput {
                kind: UnitKind::Cell,
                lang: "r",
                code: "x <- 1",
                options: &options,
                attr_tail: "",
                profile,
            },
        )
        .unwrap()
    }

    fn structured(options: Value) -> String {
        unit_hash(Some(STRUCTURED_PROFILE), options)
    }

    #[test]
    fn the_declaration_keeps_the_id_and_stores_the_structured_profile() {
        let def = knitr_hash_definition();
        assert_eq!(def.id, "knitr");
        assert_eq!(def.version, 2, "bumped from EXR-11's 1");
        assert_eq!(def.document_keys, ["knitr"]);
        assert!(
            def.presentation_cell_options.is_empty(),
            "default profile counts every option"
        );
        assert!(def.presentation_execute_keys.is_empty());
        let profile: &HashProfile = &def.profiles[STRUCTURED_PROFILE];
        assert!(
            profile.presentation_execute_keys.is_empty(),
            "every execute: key counts"
        );
        assert_eq!(
            profile.presentation_cell_options,
            structured_presentation_options()
        );
        assert!(profile.presentation_cell_options.len() > 50);
    }

    /// KNR-3 slice 7, D8: `indent` is inert for the presenter, so it is neither hashed nor
    /// presented. It stays in the presentation list, which keeps every hash and the declaration
    /// version where they were.
    #[test]
    fn an_indent_edit_does_not_move_a_structured_hash_and_the_version_stays_2() {
        assert_eq!(KNITR_HASH_VERSION, 2);
        assert!(STRUCTURED_PRESENTATION_OPTIONS.contains(&"indent"));
        let base = structured(json!({"fig.width": 7}));
        assert_eq!(
            base,
            structured(json!({"fig.width": 7, "indent": "  "})),
            "indent moved the hash"
        );
    }

    #[test]
    fn a_presentation_only_option_does_not_move_a_structured_hash() {
        let base = structured(json!({"fig.width": 7}));
        for (name, a, b) in [
            ("echo", json!(true), json!(false)),
            ("results", json!("markup"), json!("hide")),
            ("fig.cap", json!("a"), json!("b")),
            ("fig-cap", json!("a"), json!("b")),
            ("fig-alt", json!("a"), json!("b")),
            ("fig.show", json!("asis"), json!("hold")),
            ("class.output", json!("a"), json!("b")),
            ("class-output", json!("a"), json!("b")),
            ("code-fold", json!(true), json!(false)),
            ("layout-ncol", json!(2), json!(3)),
            ("out.width", json!("50%"), json!("60%")),
            ("out-width", json!("50%"), json!("60%")),
            ("column", json!("page"), json!("margin")),
        ] {
            let with = |v: &Value| {
                let mut o = json!({"fig.width": 7});
                o[name] = v.clone();
                structured(o)
            };
            assert_eq!(with(&a), with(&b), "{name} is presentation-only");
            assert_eq!(with(&a), base, "{name} present or absent is the same hash");
        }
    }

    #[test]
    fn an_execution_relevant_option_moves_a_structured_hash() {
        let base = structured(json!({}));
        for (name, v) in [
            ("include", json!(false)),
            ("warning", json!(false)),
            ("message", json!(false)),
            ("error", json!(true)),
            ("eval", json!(false)),
            ("fig.keep", json!("none")),
            ("fig.width", json!(5)),
            ("fig-width", json!(5)),
            ("fig.height", json!(5)),
            ("dpi", json!(300)),
            ("dev", json!("svg")),
            ("fig.retina", json!(2)),
            ("cache", json!(true)),
            ("child", json!("a.Rmd")),
            ("engine", json!("bash")),
            ("tidy", json!(true)),
            ("df_print", json!("kable")),
            ("R.options", json!({"digits": 3})),
            ("ref.label", json!("other")),
            ("foo.unknown", json!("1")),
            ("attr.custom", json!("1")),
            (OUTPUT_FORMAT_OPTION, json!("typst")),
        ] {
            let mut o = json!({});
            o[name] = v;
            assert_ne!(structured(o), base, "{name} is execution-relevant");
        }
    }

    #[test]
    fn the_output_format_moves_the_hash_per_format() {
        assert_ne!(
            structured(json!({OUTPUT_FORMAT_OPTION: "html"})),
            structured(json!({OUTPUT_FORMAT_OPTION: "typst"}))
        );
    }

    #[test]
    fn a_fallback_unit_counts_every_option() {
        let base = unit_hash(None, json!({}));
        for name in ["echo", "fig.cap", "class.output", "code-fold"] {
            let mut o = json!({});
            o[name] = json!("x");
            assert_ne!(
                unit_hash(None, o),
                base,
                "{name} counts in the default profile"
            );
        }
    }

    #[test]
    fn the_profiles_differ_exactly_when_a_presentation_option_is_present() {
        // The profile name is not a hash input: only what each profile counts is.
        assert_eq!(unit_hash(None, json!({})), structured(json!({})));
        let with_echo = json!({"echo": false});
        assert_ne!(unit_hash(None, with_echo.clone()), structured(with_echo));
    }

    #[test]
    fn a_unit_naming_a_missing_profile_cannot_be_hashed() {
        let options = BTreeMap::new();
        let err = hash_unit(
            &knitr_hash_definition(),
            &UnitHashInput {
                kind: UnitKind::Cell,
                lang: "r",
                code: "x",
                options: &options,
                attr_tail: "",
                profile: Some("nope"),
            },
        );
        assert!(err.is_err());
    }

    #[test]
    fn every_execute_key_counts_in_any_knitr_document_hash() {
        let def = knitr_hash_definition();
        let md = |echo: bool| -> BTreeMap<String, Value> {
            serde_json::from_value(
                json!({"knitr": {"x": 1}, "execute": {"echo": echo, "fig-width": 5}}),
            )
            .unwrap()
        };
        for refs in [
            vec![UnitHashRef {
                hash: "u",
                profile: None,
            }],
            vec![UnitHashRef {
                hash: "u",
                profile: Some(STRUCTURED_PROFILE),
            }],
            vec![
                UnitHashRef {
                    hash: "u",
                    profile: Some(STRUCTURED_PROFILE),
                },
                UnitHashRef {
                    hash: "v",
                    profile: None,
                },
            ],
        ] {
            assert_ne!(
                hash_document(&def, &md(true), &refs).unwrap(),
                hash_document(&def, &md(false), &refs).unwrap(),
            );
        }
    }

    #[test]
    fn the_list_is_closed_and_well_formed() {
        let list = structured_presentation_options();
        let mut sorted = list.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(list, sorted, "sorted, no duplicates");
        for name in STRUCTURED_PRESENTATION_OPTIONS {
            assert!(list.contains(&name.to_string()));
            if name.contains('.') {
                assert!(
                    list.contains(&name.replace('.', "-")),
                    "{name} lacks its dashed twin"
                );
            }
        }
        // Execution-relevant names are never on it (KNR-1 S3, decision 8).
        for name in [
            "include",
            "warning",
            "message",
            "eval",
            "error",
            "engine",
            "fig.keep",
            "fig.width",
            "fig.height",
            "fig.asp",
            "fig.ext",
            "dev",
            "dev.args",
            "dpi",
            "fig.retina",
            "fig.process",
            "render",
            "df_print",
            "cache",
            "R.options",
            "ref.label",
            "child",
            "file",
            "code",
            "opts.label",
            "tidy",
            "tidy.opts",
            "label",
            "fig.ncol",
            "fig.sep",
            "output",
            OUTPUT_FORMAT_OPTION,
        ] {
            assert!(
                !list.contains(&name.to_string()),
                "{name} must count in the hash"
            );
            assert!(
                !list.contains(&name.replace('.', "-")),
                "{name} (dashed) must count"
            );
        }
    }
}
