/*
 * tests/integration/positron_conformance.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-16 Phase A/B: Positron conformance fixtures and their generator.
 */

//! The cases below are written to `tests/fixtures/positron-conformance/` as
//! `<name>.qmd` plus `<name>.q2.json` (the units q2 finds, with content hashes,
//! labels and ids). Positron's observed side lives in the hand-reviewed
//! `positron-expected.json`. The format and the commands are in that
//! directory's README.
//!
//! Regenerate after a deliberate change with
//! `UPDATE_POSITRON_FIXTURES=1 cargo nextest run -p quarto-core -E
//! 'test(positron_conformance::)'`, then review the diff by hand.

use std::path::{Path, PathBuf};

use quarto_core::engine::units::{Unit, scan_units};
use quarto_core::record::{AuthorTexts, author_label, fill_ids, scanned_author_text};
use serde_json::{Value, json};

/// `(name, document text)`. Bytes are written exactly as given (the CRLF case matters).
const CASES: &[(&str, &str)] = &[
    ("plain", "```{python}\nx = 1\n```\n"),
    (
        "label_comment",
        "```{python}\n#| label: fig-a\nx = 1\n```\n",
    ),
    ("r_label_comment", "```{r}\n#| label: fig-r\nx <- 1\n```\n"),
    ("brace_label_r", "```{r, label=foo}\nx <- 1\n```\n"),
    ("brace_label_bare", "```{python, fig-x}\nx = 1\n```\n"),
    ("brace_id", "```{python #bar}\nx = 1\n```\n"),
    ("dotted_fence", "```{.python}\nx = 1\n```\n"),
    ("marimo_spaced", "```{python .marimo}\nx = 1\n```\n"),
    ("marimo_dotted", "```{python.marimo}\nx = 1\n```\n"),
    ("lang_case", "```{Python}\nx = 1\n```\n"),
    ("blockquote", "> ```{python}\n> x = 1\n> ```\n"),
    ("list", "- item\n\n  ```{python}\n  x = 1\n  ```\n"),
    (
        "four_backtick",
        "````markdown\n```{python}\nx = 1\n```\n````\n",
    ),
    ("display_fence", "```text\n```{python}\nx = 1\n```\n```\n"),
    ("tilde_fence", "~~~{python}\nx = 1\n~~~\n"),
    ("crlf", "```{python}\r\nx = 1\r\n```\r\n"),
    ("raw_html", "```{=html}\n<b>x</b>\n```\n"),
    (
        "two_cells",
        "```{python}\na = 1\n```\n\ntext\n\n```{r}\nb <- 2\n```\n",
    ),
    (
        "duplicate_pair",
        "```{python}\nx = 1\n```\n\ntext\n\n```{python}\nx = 1\n```\n",
    ),
    (
        "span_before_cell",
        "Value: `{python} 1 + 1`.\n\n```{python}\nx = 1\n```\n",
    ),
    (
        "unicode_cell",
        "```{python}\n#| label: fig-é\nx = 'é😀'\n```\n",
    ),
];

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/positron-conformance")
}

fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].bytes().filter(|&b| b == b'\n').count() + 1
}

/// q2's side for one document: every unit with the values Positron could share.
fn q2_json(text: &str) -> Value {
    let mut units = scan_units(text);
    let authors = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &authors).unwrap();
    Value::Array(
        units
            .iter()
            .zip(&authors.0)
            .map(|(u, a)| match u {
                Unit::Cell(c) => json!({
                    "kind": "cell",
                    "lang": c.lang,
                    "index": c.index,
                    "line": line_of(text, c.start),
                    "content_hash": a.content_hash,
                    "label": author_label(&scanned_author_text(u), &c.lang),
                    "cell_id": c.id.as_ref().unwrap().0,
                }),
                Unit::Inline(i) => json!({
                    "kind": "span",
                    "lang": i.lang,
                    "span_index": i.span_index,
                    "line": line_of(text, i.start),
                    "content_hash": a.content_hash,
                    "cell_id": i.id.as_ref().unwrap().0,
                }),
            })
            .collect(),
    )
}

fn write_json(path: &Path, v: &Value) {
    let mut s = serde_json::to_string_pretty(v).unwrap();
    s.push('\n');
    std::fs::write(path, s).unwrap();
}

#[test]
fn fixtures_match_the_generator() {
    let update = std::env::var_os("UPDATE_POSITRON_FIXTURES").is_some();
    let dir = fixture_dir();
    if update {
        std::fs::create_dir_all(&dir).unwrap();
    }
    for (name, text) in CASES {
        let qmd = dir.join(format!("{name}.qmd"));
        let json_path = dir.join(format!("{name}.q2.json"));
        let got = q2_json(text);
        if update {
            std::fs::write(&qmd, text).unwrap();
            write_json(&json_path, &got);
            continue;
        }
        let on_disk = std::fs::read(&qmd).unwrap_or_else(|_| panic!("missing {}", qmd.display()));
        assert_eq!(
            on_disk,
            text.as_bytes(),
            "{name}.qmd differs from the table"
        );
        let want: Value = serde_json::from_str(
            &std::fs::read_to_string(&json_path)
                .unwrap_or_else(|_| panic!("missing {}", json_path.display())),
        )
        .unwrap();
        assert_eq!(got, want, "{name}: q2 side");
    }
}

#[test]
fn no_stray_fixture_files() {
    let known: Vec<String> = CASES
        .iter()
        .flat_map(|(n, _)| [format!("{n}.qmd"), format!("{n}.q2.json")])
        .chain(["positron-expected.json".into(), "README.md".into()])
        .collect();
    for e in std::fs::read_dir(fixture_dir()).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().to_string();
        assert!(known.contains(&name), "unlisted fixture file {name}");
    }
}

/// The ids Positron would compute from its own cell list must follow the shared format;
/// where the cell lists agree the ids agree. Divergences are recorded, never hidden.
#[test]
fn positron_expected_covers_every_case_and_states_divergences() {
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture_dir().join("positron-expected.json")).unwrap(),
    )
    .unwrap();
    for (name, text) in CASES {
        let entry = expected
            .get(*name)
            .unwrap_or_else(|| panic!("positron-expected.json lacks `{name}`"));
        let positron_cells = entry["cells"].as_array().unwrap();
        let q2 = q2_json(text);
        let q2_cells: Vec<&Value> = q2
            .as_array()
            .unwrap()
            .iter()
            .filter(|u| u["kind"] == "cell")
            .collect();
        let same_shape = positron_cells.len() == q2_cells.len()
            && positron_cells
                .iter()
                .zip(&q2_cells)
                .all(|(p, q)| p["id"] == q["cell_id"] && p["content_hash"] == q["content_hash"]);
        let note = entry["divergence"].as_str();
        assert_eq!(
            same_shape,
            note.is_none(),
            "{name}: ids/hashes {} but divergence note is {:?}",
            if same_shape { "agree" } else { "differ" },
            note
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Round-trip safety and trust (EXR-16 Phase A).
// ---------------------------------------------------------------------------------------------

/// What Positron's cache loader (`quartoOutputCacheService.ts:218-298`, read 2026-10-28) does
/// with a file: a fixture-level reimplementation of its structure checks. It validates a
/// numeric `nbformat`, a `cells` array, an object `metadata` and a numeric
/// `metadata.quarto_cache_version`; deletes the file on a version other than 1; and deletes it
/// when anything throws, for example a code cell without an `outputs` array. Spike S2a ran Positron's
/// real service on these cases (branch `spike/q2-s2-read-record`, `0bfcb904c1`) and confirmed all
/// three; see `claude-notes/research/2026-11-01-exr-16-s2a-read-record-findings.md`. This copy
/// stays because it is the only check that runs in q2's CI.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LoaderVerdict {
    Keeps,
    Deletes(&'static str),
}

pub(crate) fn positron_loader(file: &Value) -> LoaderVerdict {
    if !file["nbformat"].is_number() || !file["cells"].is_array() || !file["metadata"].is_object() {
        return LoaderVerdict::Deletes("invalid structure");
    }
    let Some(version) = file["metadata"]["quarto_cache_version"].as_u64() else {
        return LoaderVerdict::Deletes("missing or non-numeric quarto_cache_version");
    };
    if version != 1 {
        return LoaderVerdict::Deletes("quarto_cache_version is not 1");
    }
    for cell in file["cells"].as_array().unwrap() {
        if cell["cell_type"] == "code" && !cell["outputs"].is_array() {
            return LoaderVerdict::Deletes("a code cell without an outputs array throws");
        }
    }
    LoaderVerdict::Keeps
}

fn golden_record() -> Value {
    serde_json::from_str(include_str!("../fixtures/records/golden.ipynb")).unwrap()
}

#[test]
fn a_q2_record_passes_positrons_loader_checks_only_with_cache_version_one() {
    let golden = golden_record();
    assert!(
        golden["cells"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["cell_type"] == "code"),
        "the golden record must hold a code cell for this test to mean anything"
    );
    assert_eq!(positron_loader(&golden), LoaderVerdict::Keeps);

    // Any other value, or a missing key, and Positron deletes the file. q2 never writes such a
    // file into Positron's directory; this documents why the key stays at 1 or is dropped.
    for other in [json!(2), json!(0), json!("1")] {
        let mut f = golden.clone();
        f["metadata"]["quarto_cache_version"] = other.clone();
        assert!(
            matches!(positron_loader(&f), LoaderVerdict::Deletes(_)),
            "version {other}"
        );
    }
    let mut f = golden.clone();
    f["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("quarto_cache_version");
    assert!(matches!(positron_loader(&f), LoaderVerdict::Deletes(_)));
}

#[test]
fn positrons_loader_deletes_a_code_cell_without_outputs() {
    let mut f = golden_record();
    let cell = f["cells"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["cell_type"] == "code")
        .unwrap();
    cell.as_object_mut().unwrap().remove("outputs");
    assert!(matches!(positron_loader(&f), LoaderVerdict::Deletes(_)));
}

/// q2 never emits `quarto_cache_version` with a value other than Positron's. The key is
/// defensive only (q2 never writes into Positron's cache directory); see the schema reference
/// in `claude-notes/plans/2026-10-28-exr-04-record-format.md`.
#[test]
fn q2_only_ever_emits_cache_version_one() {
    use quarto_core::record::{POSITRON_CACHE_VERSION, Record};
    assert_eq!(Record::CACHE_VERSION, 1);
    assert_eq!(u64::from(Record::CACHE_VERSION), POSITRON_CACHE_VERSION);
    assert_eq!(golden_record()["metadata"]["quarto_cache_version"], 1);
}

/// Trust: imported Positron outputs are display-only. They convert to no type a reuse or
/// freeze decision takes, checked at compile time with an inherent-method-wins probe: the
/// inherent `converts` exists only when the `Into` bound holds, otherwise the blanket trait
/// method answers `false`.
#[test]
fn imported_outputs_convert_to_nothing_a_reuse_decision_takes() {
    use quarto_core::record::{DisplayOnlyOutputs, Provenance, RecordCell, UnitResult};
    use std::marker::PhantomData;

    struct Probe<T>(PhantomData<T>);
    trait Fallback {
        fn converts(&self) -> bool {
            false
        }
    }
    impl<T> Fallback for Probe<T> {}
    impl<T: Into<UnitResult>> Probe<T> {
        #[allow(dead_code)]
        fn converts(&self) -> bool {
            true
        }
    }
    struct CellProbe<T>(PhantomData<T>);
    trait CellFallback {
        fn converts(&self) -> bool {
            false
        }
    }
    impl<T> CellFallback for CellProbe<T> {}
    impl<T: Into<RecordCell>> CellProbe<T> {
        #[allow(dead_code)]
        fn converts(&self) -> bool {
            true
        }
    }

    assert!(!Probe::<DisplayOnlyOutputs>(PhantomData).converts());
    assert!(!CellProbe::<DisplayOnlyOutputs>(PhantomData).converts());
    // The probe is not vacuous: it does see an `Into` that exists.
    assert!(Probe::<UnitResult>(PhantomData).converts());
    // And the marker says where outputs came from.
    assert_eq!(Provenance::Positron, Provenance::Positron);
}
