/*
 * engine/jupyter/kernel_select.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Which Jupyter kernel runs a document, and which owned languages have one.
 */

//! Kernel selection policy (JOL-2a, JOL-2b).
//!
//! Pure functions over an injected kernelspec list, so the policy is tested
//! without spawning a process or reading the machine's Jupyter directories.
//!
//! The rules follow Quarto 1 (`src/core/jupyter/kernels.ts`,
//! `jupyter.ts`) with two deliberate differences:
//!
//! - Quarto 1 compares the fence language to `kernelspec.language` with a
//!   case-sensitive `===`. This module compares case-insensitively, because
//!   the Ark kernelspec declares `"language": "R"` and an `{r}` fence must
//!   find it.
//! - Quarto 1 has no alias table. `py`, `jl`, `rs`, `rb`, `ts`, `js` and
//!   `python3` as fence names are a Quarto 2 invention; they still work for
//!   one release and warn (see [`deprecated_alias`]), then go away.
//!
//! One kernel serves the document. An explicit kernel (the `jupyter:` key,
//! then `engine: [{jupyter: {kernel: x}}]`) takes every owned cell and nothing
//! is checked against the kernelspec list. Without one, the kernel is the
//! first language present, in document order, that has a kernelspec, then
//! Python. An owned language with no kernelspec is *unrunnable*.

use std::collections::HashMap;
use std::path::PathBuf;

use quarto_pandoc_types::ConfigValue;

use super::kernelspec::{ResolvedKernel, kernel_from_ts_metadata};
use crate::engine::ts_protocol::TsMetadataValue;

/// The fields of an installed kernelspec the policy reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelSpecSummary {
    /// Kernel name (`python3`, `ir`, `ggsql`).
    pub name: String,
    /// The `language` field of `kernel.json`, as written.
    pub language: String,
    /// The kernelspec directory.
    pub path: PathBuf,
}

impl From<&ResolvedKernel> for KernelSpecSummary {
    fn from(k: &ResolvedKernel) -> Self {
        Self {
            name: k.name.clone(),
            language: k.language.clone(),
            path: k.path.clone(),
        }
    }
}

/// A fence name that Quarto 2 invented and is removing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeprecatedAlias {
    /// The alias as it is lowercased for lookup (`py`).
    pub alias: &'static str,
    /// The canonical language to write instead (`python`).
    pub canonical: &'static str,
}

const ALIASES: &[DeprecatedAlias] = &[
    DeprecatedAlias {
        alias: "py",
        canonical: "python",
    },
    DeprecatedAlias {
        alias: "python3",
        canonical: "python",
    },
    DeprecatedAlias {
        alias: "jl",
        canonical: "julia",
    },
    DeprecatedAlias {
        alias: "rb",
        canonical: "ruby",
    },
    DeprecatedAlias {
        alias: "rs",
        canonical: "rust",
    },
    DeprecatedAlias {
        alias: "ts",
        canonical: "typescript",
    },
    DeprecatedAlias {
        alias: "js",
        canonical: "javascript",
    },
];

/// The deprecated alias a fence language is, if it is one.
pub fn deprecated_alias(fence_language: &str) -> Option<DeprecatedAlias> {
    let lower = fence_language.to_ascii_lowercase();
    ALIASES.iter().find(|a| a.alias == lower).copied()
}

/// The language used for kernelspec lookup: lowercased, with a deprecated
/// alias resolved to its canonical name.
pub fn lookup_language(fence_language: &str) -> String {
    match deprecated_alias(fence_language) {
        Some(a) => a.canonical.to_string(),
        None => fence_language.to_ascii_lowercase(),
    }
}

/// The first kernelspec whose `language` equals `language` (already
/// lowercased), ignoring case. For Python, a kernelspec whose path contains
/// `virtual_env` wins (Q1's venv-first rule, `kernels.ts:41-46`).
pub fn kernel_for_language<'a>(
    specs: &'a [KernelSpecSummary],
    language: &str,
    virtual_env: Option<&str>,
) -> Option<&'a KernelSpecSummary> {
    let mut matching = specs
        .iter()
        .filter(|s| s.language.to_ascii_lowercase() == language);
    if language == "python"
        && let Some(venv) = virtual_env.filter(|v| !v.is_empty())
    {
        let all: Vec<&KernelSpecSummary> = matching.collect();
        return all
            .iter()
            .find(|s| s.path.to_string_lossy().contains(venv))
            .or(all.first())
            .copied();
    }
    matching.next()
}

/// The explicit kernel name, if the document names one: the `jupyter:` key,
/// then `engine_config.kernel`.
pub fn explicit_kernel(
    metadata: &HashMap<String, TsMetadataValue>,
    engine_config: Option<&ConfigValue>,
) -> Option<String> {
    kernel_from_ts_metadata(metadata).or_else(|| {
        engine_config
            .and_then(|c| c.get("kernel"))
            .and_then(|k| k.as_plain_text())
    })
}

/// The outcome of kernel selection for one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelChoice {
    /// The kernel that runs the document. `None` only when there is no
    /// explicit kernel and no owned language to choose from.
    pub kernel: Option<String>,
    /// Owned languages (lookup form, document order, distinct) with no
    /// kernelspec. Always empty with an explicit kernel.
    pub unrunnable: Vec<String>,
}

/// Choose the document's kernel.
///
/// `owned_languages` are the languages of the document's owned cells in
/// document order, in lookup form (see [`lookup_language`]); repeats are fine.
pub fn choose_kernel(
    explicit: Option<String>,
    owned_languages: &[String],
    specs: &[KernelSpecSummary],
    virtual_env: Option<&str>,
) -> KernelChoice {
    if let Some(kernel) = explicit {
        return KernelChoice {
            kernel: Some(kernel),
            unrunnable: Vec::new(),
        };
    }
    let mut distinct: Vec<&String> = Vec::new();
    for lang in owned_languages {
        if !distinct.contains(&lang) {
            distinct.push(lang);
        }
    }
    if distinct.is_empty() {
        return KernelChoice {
            kernel: None,
            unrunnable: Vec::new(),
        };
    }
    let mut kernel = None;
    let mut unrunnable = Vec::new();
    for lang in distinct {
        match kernel_for_language(specs, lang, virtual_env) {
            Some(spec) => {
                kernel.get_or_insert_with(|| spec.name.clone());
            }
            None => unrunnable.push(lang.clone()),
        }
    }
    // Q1 then tries Python. With owned cells present but no language of
    // theirs runnable the choice cannot run anything; the name still resolves
    // so the caller reports the unrunnable languages rather than a missing
    // kernel.
    let kernel = kernel.or_else(|| {
        Some(
            kernel_for_language(specs, "python", virtual_env)
                .map_or_else(|| "python3".to_string(), |s| s.name.clone()),
        )
    });
    KernelChoice { kernel, unrunnable }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;

    fn spec(name: &str, language: &str, path: &str) -> KernelSpecSummary {
        KernelSpecSummary {
            name: name.to_string(),
            language: language.to_string(),
            path: PathBuf::from(path),
        }
    }

    fn langs(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    fn meta(name: &str) -> HashMap<String, TsMetadataValue> {
        HashMap::from([(
            "jupyter".to_string(),
            TsMetadataValue::String(name.to_string()),
        )])
    }

    fn kernel_config(name: &str) -> ConfigValue {
        use quarto_pandoc_types::config_value::ConfigMapEntry;
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "kernel".to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_string(name, SourceInfo::for_test()),
            }],
            SourceInfo::for_test(),
        )
    }

    fn installed() -> Vec<KernelSpecSummary> {
        vec![
            spec("python3", "python", "/usr/share/jupyter/kernels/python3"),
            spec(
                "julia-1.11",
                "julia",
                "/usr/share/jupyter/kernels/julia-1.11",
            ),
            // The Ark kernelspec declares a capital R.
            spec("ark", "R", "/usr/share/jupyter/kernels/ark"),
            spec("ggsql", "ggsql", "/usr/share/jupyter/kernels/ggsql"),
        ]
    }

    // ── explicit kernel ──────────────────────────────────────────────────

    #[test]
    fn explicit_jupyter_key_wins_and_nothing_is_checked() {
        let ex = explicit_kernel(&meta("sos"), None);
        let c = choose_kernel(ex, &langs(&["python", "made-up"]), &installed(), None);
        assert_eq!(c.kernel.as_deref(), Some("sos"));
        assert!(c.unrunnable.is_empty(), "{c:?}");
    }

    #[test]
    fn explicit_engine_config_kernel_and_precedence() {
        let cfg = kernel_config("sos");
        assert_eq!(
            explicit_kernel(&Default::default(), Some(&cfg)).as_deref(),
            Some("sos")
        );
        assert_eq!(
            explicit_kernel(&meta("julia"), Some(&cfg)).as_deref(),
            Some("julia")
        );
        assert_eq!(explicit_kernel(&Default::default(), None), None);
    }

    #[test]
    fn explicit_kernel_with_no_cells_is_kept() {
        let c = choose_kernel(Some("python3".into()), &[], &installed(), None);
        assert_eq!(c.kernel.as_deref(), Some("python3"));
    }

    // ── lookup by kernelspec language ────────────────────────────────────

    #[test]
    fn lookup_ignores_case_in_both_directions() {
        // `{r}` finds the Ark kernelspec whose language is "R".
        let specs = installed();
        assert_eq!(
            kernel_for_language(&specs, "r", None).map(|s| s.name.as_str()),
            Some("ark")
        );
        // A kernelspec declaring "Python" is found for a `python` fence.
        let mixed = vec![spec("py", "Python", "/k/py")];
        assert!(kernel_for_language(&mixed, "python", None).is_some());
        // And a `{Python}` fence is lowercased before lookup.
        assert_eq!(lookup_language("Python"), "python");
    }

    #[test]
    fn lookup_is_by_language_not_kernel_name() {
        // `ir` is a kernel *name*; no kernelspec has that language.
        let specs = vec![spec("ir", "R", "/k/ir")];
        assert!(kernel_for_language(&specs, "ir", None).is_none());
        assert!(kernel_for_language(&specs, "r", None).is_some());
    }

    #[test]
    fn unknown_language_has_no_kernel_and_no_alias_passthrough() {
        let specs = installed();
        assert!(kernel_for_language(&specs, "fortran", None).is_none());
        // `rust` is not mapped to any kernel name any more.
        assert!(kernel_for_language(&specs, "rust", None).is_none());
    }

    #[test]
    fn first_matching_kernelspec_wins() {
        let specs = vec![
            spec("julia-1.10", "julia", "/k/a"),
            spec("julia-1.11", "julia", "/k/b"),
        ];
        assert_eq!(
            kernel_for_language(&specs, "julia", None).unwrap().name,
            "julia-1.10"
        );
    }

    #[test]
    fn python_prefers_the_virtual_env_kernelspec() {
        let specs = vec![
            spec("python3", "python", "/usr/share/jupyter/kernels/python3"),
            spec(
                "venv-py",
                "python",
                "/home/u/proj/.venv/share/jupyter/kernels/python3",
            ),
        ];
        let got = kernel_for_language(&specs, "python", Some("/home/u/proj/.venv")).unwrap();
        assert_eq!(got.name, "venv-py");
        // No match under the venv: first python kernelspec.
        let got = kernel_for_language(&specs, "python", Some("/elsewhere")).unwrap();
        assert_eq!(got.name, "python3");
        // The venv rule is Python-only.
        let julia = vec![
            spec("a", "julia", "/home/u/proj/.venv/k/a"),
            spec("b", "julia", "/other/b"),
        ];
        let got = kernel_for_language(&julia, "julia", Some("/other")).unwrap();
        assert_eq!(got.name, "a");
    }

    // ── choosing the document kernel ─────────────────────────────────────

    #[test]
    fn first_language_with_a_kernelspec_wins() {
        // `fortran` has none; julia is the first that does.
        let c = choose_kernel(
            None,
            &langs(&["fortran", "julia", "python"]),
            &installed(),
            None,
        );
        assert_eq!(c.kernel.as_deref(), Some("julia-1.11"));
        assert_eq!(c.unrunnable, ["fortran"]);
    }

    #[test]
    fn r_fence_selects_the_ark_kernel() {
        let c = choose_kernel(None, &langs(&["r"]), &installed(), None);
        assert_eq!(c.kernel.as_deref(), Some("ark"));
        assert!(c.unrunnable.is_empty());
    }

    #[test]
    fn made_up_language_is_unrunnable_and_falls_back_to_python() {
        let c = choose_kernel(None, &langs(&["made-up", "made-up"]), &installed(), None);
        assert_eq!(c.kernel.as_deref(), Some("python3"));
        // Reported once.
        assert_eq!(c.unrunnable, ["made-up"]);
    }

    #[test]
    fn python_failsafe_name_when_no_python_kernelspec_is_installed() {
        let c = choose_kernel(None, &langs(&["made-up"]), &[], None);
        assert_eq!(c.kernel.as_deref(), Some("python3"));
    }

    #[test]
    fn no_cells_no_key_is_none() {
        let c = choose_kernel(None, &[], &installed(), None);
        assert_eq!(c.kernel, None);
        assert!(c.unrunnable.is_empty());
    }

    #[test]
    fn ggsql_and_bash_are_runnable_when_a_kernelspec_declares_them() {
        let mut specs = installed();
        specs.push(spec("bash", "bash", "/k/bash"));
        let c = choose_kernel(None, &langs(&["ggsql", "bash"]), &specs, None);
        assert_eq!(c.kernel.as_deref(), Some("ggsql"));
        assert!(c.unrunnable.is_empty());
    }

    // ── aliases ──────────────────────────────────────────────────────────

    #[test]
    fn aliases_resolve_to_canonical_and_are_flagged() {
        for (alias, canonical) in [
            ("py", "python"),
            ("python3", "python"),
            ("jl", "julia"),
            ("rb", "ruby"),
            ("rs", "rust"),
            ("ts", "typescript"),
            ("js", "javascript"),
        ] {
            assert_eq!(lookup_language(alias), canonical);
            assert_eq!(deprecated_alias(alias).unwrap().canonical, canonical);
        }
        assert_eq!(deprecated_alias("PY").unwrap().canonical, "python");
        assert!(deprecated_alias("python").is_none());
        assert!(deprecated_alias("r").is_none());
    }

    #[test]
    fn py_fence_finds_the_python_kernel_through_the_alias() {
        let c = choose_kernel(None, &langs(&[&lookup_language("py")]), &installed(), None);
        assert_eq!(c.kernel.as_deref(), Some("python3"));
    }
}
