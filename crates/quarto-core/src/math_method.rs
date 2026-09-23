/*
 * math_method.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The `html-math-method` document option, parsed once for every consumer.
//!
//! Two stages read the option and must agree on what they read:
//! [`crate::stage::stages::MathJsStage`] (which engine, if any, to load
//! into the page) and [`crate::stage::stages::EquationNumberStage`] (how to
//! encode an equation number so that engine can typeset it). Parsing lives
//! here so neither can drift from the other.
//!
//! Both Quarto 1 / Pandoc shapes are accepted:
//!
//! ```yaml
//! html-math-method: katex
//! html-math-method:
//!   method: mathjax
//!   url: https://example.org/mathjax.js
//! ```
//!
//! An absent option means MathJax (Quarto 1's default). Values Quarto 1
//! recognizes but q2 does not implement (`webtex`, `gladtex`) and unknown
//! strings are kept verbatim as [`MathMethod::Unknown`] so callers can say
//! precisely what they were given.

use quarto_pandoc_types::config_value::ConfigValue;

/// Which math renderer a document asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathMethod {
    /// MathJax 3 in the browser (the default).
    Mathjax,
    /// KaTeX in the browser.
    Katex,
    /// Native MathML, converted at render time (bd-3evfzwal).
    MathMl,
    /// No renderer: the TeX is left as written.
    Plain,
    /// Anything else, kept verbatim (`webtex`, `gladtex`, typos).
    Unknown(String),
}

impl MathMethod {
    fn from_name(name: &str) -> Self {
        match name {
            "mathjax" => Self::Mathjax,
            "katex" => Self::Katex,
            "mathml" => Self::MathMl,
            "plain" => Self::Plain,
            other => Self::Unknown(other.to_string()),
        }
    }
}

/// The parsed option: the method plus the optional loader URL the object
/// form can carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathMethodConfig {
    pub method: MathMethod,
    /// `url:` from the object form, untouched. Only meaningful for the
    /// browser engines; ignored otherwise.
    pub url: Option<String>,
}

impl MathMethodConfig {
    /// Read `html-math-method` from (format-flattened) document metadata.
    /// Absent → MathJax with no URL override.
    pub fn from_meta(meta: &ConfigValue) -> Self {
        let Some(value) = meta.get("html-math-method") else {
            return Self {
                method: MathMethod::Mathjax,
                url: None,
            };
        };

        if value.is_map() {
            let method = value
                .get("method")
                .and_then(|v| v.as_plain_text())
                .map_or(MathMethod::Mathjax, |name| MathMethod::from_name(&name));
            let url = value.get("url").and_then(|v| v.as_plain_text());
            return Self { method, url };
        }

        let method = value
            .as_plain_text()
            .map_or(MathMethod::Mathjax, |name| MathMethod::from_name(&name));
        Self { method, url: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind, MergeOp};
    use quarto_source_map::SourceInfo;
    use yaml_rust2::Yaml;

    fn scalar(s: &str) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::scalar(Yaml::String(s.to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: MergeOp::Concat,
        }
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(
                entries
                    .into_iter()
                    .map(|(k, v)| ConfigMapEntry {
                        key: k.to_string(),
                        key_source: SourceInfo::for_test(),
                        value: v,
                    })
                    .collect(),
            ),
            source_info: SourceInfo::for_test(),
            merge_op: MergeOp::Concat,
        }
    }

    fn meta_string(s: &str) -> ConfigValue {
        map(vec![("html-math-method", scalar(s))])
    }

    #[test]
    fn absent_means_mathjax_without_url() {
        let cfg = MathMethodConfig::from_meta(&map(vec![]));
        assert_eq!(cfg.method, MathMethod::Mathjax);
        assert_eq!(cfg.url, None);
    }

    #[test]
    fn string_form_names_every_method() {
        for (name, expected) in [
            ("mathjax", MathMethod::Mathjax),
            ("katex", MathMethod::Katex),
            ("mathml", MathMethod::MathMl),
            ("plain", MathMethod::Plain),
            ("webtex", MathMethod::Unknown("webtex".to_string())),
            ("gladtex", MathMethod::Unknown("gladtex".to_string())),
            ("mathjx", MathMethod::Unknown("mathjx".to_string())),
        ] {
            let cfg = MathMethodConfig::from_meta(&meta_string(name));
            assert_eq!(cfg.method, expected, "{name}");
            assert_eq!(cfg.url, None, "{name}");
        }
    }

    #[test]
    fn object_form_carries_method_and_url() {
        let cfg = MathMethodConfig::from_meta(&map(vec![(
            "html-math-method",
            map(vec![
                ("method", scalar("katex")),
                ("url", scalar("https://example.org/katex/")),
            ]),
        )]));
        assert_eq!(cfg.method, MathMethod::Katex);
        assert_eq!(cfg.url.as_deref(), Some("https://example.org/katex/"));
    }

    #[test]
    fn object_form_without_method_defaults_to_mathjax() {
        let cfg = MathMethodConfig::from_meta(&map(vec![(
            "html-math-method",
            map(vec![("url", scalar("https://example.org/mj.js"))]),
        )]));
        assert_eq!(cfg.method, MathMethod::Mathjax);
        assert_eq!(cfg.url.as_deref(), Some("https://example.org/mj.js"));
    }

    #[test]
    fn object_form_with_unknown_method_keeps_the_name() {
        let cfg = MathMethodConfig::from_meta(&map(vec![(
            "html-math-method",
            map(vec![("method", scalar("webtex"))]),
        )]));
        assert_eq!(cfg.method, MathMethod::Unknown("webtex".to_string()));
    }
}
