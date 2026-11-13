//! KNR-1 Phase 1D: the corpus generator (`knitr_corpus_support`). No R needed.

use std::collections::BTreeMap;

use super::knitr_corpus_support::{CorpusManifest, Fixture, corpus_dir};

fn manifest() -> CorpusManifest {
    CorpusManifest::load(&corpus_dir()).expect("manifest.toml loads")
}

fn by_name(fixtures: &[Fixture]) -> BTreeMap<&str, &Fixture> {
    fixtures.iter().map(|f| (f.name.as_str(), f)).collect()
}

fn without_filler(text: &str) -> String {
    text.lines()
        .filter(|l| !l.starts_with("Filler: "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn corpus_generator_is_deterministic() {
    let m = manifest();
    let a = m.generate(7);
    let b = m.generate(7);
    assert!(!a.is_empty());
    assert_eq!(a, b, "one manifest and one seed give byte-equal fixtures");

    // A different seed changes only the filler text.
    let c = m.generate(8);
    assert_eq!(
        a.iter().map(|f| &f.name).collect::<Vec<_>>(),
        c.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    let mut any_filler_differs = false;
    for (x, y) in a.iter().zip(&c) {
        assert_eq!(
            without_filler(&x.text),
            without_filler(&y.text),
            "{}",
            x.name
        );
        any_filler_differs |= x.text != y.text;
    }
    assert!(any_filler_differs, "the seed must reach the filler text");
}

#[test]
fn sampled_subset_depends_on_seed_and_is_stable() {
    let m = manifest();
    let all = m.generate(1);
    let s1 = m.sample(1, 12);
    assert_eq!(s1, m.sample(1, 12));
    assert_eq!(s1.len(), 12);
    assert!(
        s1.iter()
            .all(|f| by_name(&all).contains_key(f.name.as_str()))
    );
    let s2 = m.sample(2, 12);
    assert_ne!(
        s1.iter().map(|f| &f.name).collect::<Vec<_>>(),
        s2.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

#[test]
fn every_axis_group_and_every_s1_fixture_is_present() {
    let all = manifest().generate(1);
    let groups: std::collections::BTreeSet<&str> = all.iter().map(|f| f.group.as_str()).collect();
    for g in [
        "options",
        "quarto-cell-options",
        "setup-state",
        "markdown-stress",
        "structure",
        "rich-output",
        "engines",
        "s1",
        "s2",
        "s5",
        "s7",
        "s8",
        "s9",
    ] {
        assert!(groups.contains(g), "missing group {g}");
    }
    let s1 = std::fs::read_dir(corpus_dir().join("s1")).unwrap();
    for entry in s1.flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "Rmd") {
            let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
            assert!(
                all.iter().any(|f| f.name == format!("s1/{stem}")),
                "S1 fixture {stem} missing from the corpus"
            );
        }
    }
}

#[test]
fn sweep_emits_header_and_yaml_forms() {
    let all = manifest().generate(1);
    let by = by_name(&all);
    let header = by
        .keys()
        .find(|n| n.starts_with("options/echo/header/"))
        .expect("header echo fixture");
    let yaml = by
        .keys()
        .find(|n| n.starts_with("options/echo/yaml/"))
        .expect("yaml echo fixture");
    assert!(
        by[header].text.contains("```{r c1, echo="),
        "{}",
        by[header].text
    );
    assert!(by[yaml].text.contains("#| echo: "), "{}", by[yaml].text);
    // `NA` has no YAML form.
    assert!(
        !by.keys()
            .any(|n| n.starts_with("options/warning/yaml/") && n.ends_with("_na"))
    );
    assert!(
        by.keys()
            .any(|n| n.starts_with("options/warning/header/") && n.ends_with("_na"))
    );
}

#[test]
fn sweep_fixed_options_appear_in_both_forms() {
    let all = manifest().generate(1);
    let by = by_name(&all);
    let header = by
        .keys()
        .find(|n| n.starts_with("options/class.error/header/"))
        .unwrap();
    let yaml = by
        .keys()
        .find(|n| n.starts_with("options/class.error/yaml/"))
        .unwrap();
    assert!(
        by[header].text.contains(", error=TRUE}"),
        "{}",
        by[header].text
    );
    assert!(
        by[yaml].text.contains("#| error: true\n"),
        "{}",
        by[yaml].text
    );
}
