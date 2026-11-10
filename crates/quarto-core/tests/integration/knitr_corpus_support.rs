//! KNR-1 Phase 1D: shared support for the knitr corpus harness.
//!
//! The corpus is `tests/fixtures/knitr-corpus/manifest.toml` (see its header for the entry
//! kinds). [`CorpusManifest::generate`] turns it into [`Fixture`]s deterministically: one
//! manifest and one seed give byte-equal fixtures. The seed reaches only the trailing
//! `Filler:` paragraph and [`CorpusManifest::sample`]'s choice of a subset.
//!
//! Everything here runs without R. The R-driving pieces (control run, normalizer, runner)
//! live in sibling modules.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The corpus directory (`tests/fixtures/knitr-corpus`).
pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("knitr-corpus")
}

/// One document of the corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    /// Unique, `/`-separated: `options/echo/header/00_true`, `s1/01_plain`.
    pub name: String,
    /// The manifest group (`options`, `markdown-stress`, `s1`, ...).
    pub group: String,
    /// File extension to write it under: `qmd` generated, `Rmd` for the committed S-spike files.
    pub ext: &'static str,
    pub text: String,
    /// Target formats to run it under.
    pub formats: Vec<String>,
    /// Committed fixtures only: the directory whose other files (child documents) sit beside it.
    pub sibling_dir: Option<PathBuf>,
    /// The render is expected to abort (an `error=FALSE` chunk that fails).
    pub expect_abort: bool,
}

#[derive(Debug, Deserialize)]
struct Raw {
    defaults: Defaults,
    #[serde(default)]
    sweep: Vec<Sweep>,
    #[serde(default)]
    document: Vec<Document>,
    #[serde(default)]
    handwritten: Vec<Handwritten>,
}

#[derive(Debug, Deserialize)]
struct Defaults {
    body: String,
    plot_body: String,
    single_body: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Value {
    Same(String),
    Split { r: String, yaml: String },
}

#[derive(Debug, Deserialize)]
struct Sweep {
    group: String,
    option: String,
    values: Vec<Value>,
    #[serde(default)]
    forms: Option<Vec<String>>,
    body: Option<String>,
    body_ref: Option<String>,
    #[serde(default)]
    formats: Option<Vec<String>>,
    /// Options set to the same value in every fixture of the sweep (R literal values).
    #[serde(default)]
    fixed: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Document {
    group: String,
    name: String,
    text: String,
    #[serde(default)]
    formats: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct Handwritten {
    dir: String,
    #[serde(default)]
    skip: Vec<String>,
    #[serde(default)]
    expect: Option<Expect>,
}

#[derive(Debug, Deserialize)]
struct Expect {
    #[serde(default)]
    abort: Vec<String>,
}

pub struct CorpusManifest {
    raw: Raw,
    dir: PathBuf,
}

impl CorpusManifest {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("manifest.toml");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let raw = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            raw,
            dir: dir.to_path_buf(),
        })
    }

    /// Every fixture, in manifest order. `seed` only changes each generated fixture's filler.
    pub fn generate(&self, seed: u64) -> Vec<Fixture> {
        let mut out = Vec::new();
        for sweep in &self.raw.sweep {
            self.generate_sweep(sweep, seed, &mut out);
        }
        for doc in &self.raw.document {
            let name = format!("{}/{}", doc.group, doc.name);
            let mut text = doc.text.clone();
            append_filler(&mut text, seed, &name);
            out.push(Fixture {
                name,
                group: doc.group.clone(),
                ext: "qmd",
                text,
                formats: formats_or_html(&doc.formats),
                sibling_dir: None,
                expect_abort: false,
            });
        }
        for hw in &self.raw.handwritten {
            self.load_handwritten(hw, &mut out);
        }
        out
    }

    /// A seed-chosen subset of `n` fixtures (the quick run), in corpus order.
    pub fn sample(&self, seed: u64, n: usize) -> Vec<Fixture> {
        let all = self.generate(seed);
        let mut ranked: Vec<(u64, usize)> = all
            .iter()
            .enumerate()
            .map(|(i, f)| (splitmix64(seed ^ fnv1a(&f.name)), i))
            .collect();
        ranked.sort_unstable();
        let mut keep: Vec<usize> = ranked.into_iter().take(n).map(|(_, i)| i).collect();
        keep.sort_unstable();
        keep.into_iter().map(|i| all[i].clone()).collect()
    }

    fn generate_sweep(&self, sweep: &Sweep, seed: u64, out: &mut Vec<Fixture>) {
        let body = match (&sweep.body, sweep.body_ref.as_deref()) {
            (Some(b), _) => b.as_str(),
            (None, Some("plot_body")) => self.raw.defaults.plot_body.as_str(),
            (None, Some("single_body")) => self.raw.defaults.single_body.as_str(),
            (None, Some(other)) => panic!("unknown body_ref {other}"),
            (None, None) => self.raw.defaults.body.as_str(),
        }
        .trim_end();
        let forms = sweep
            .forms
            .clone()
            .unwrap_or_else(|| vec!["header".into(), "yaml".into()]);
        for form in &forms {
            for (idx, value) in sweep.values.iter().enumerate() {
                let (r_value, yaml_value) = match value {
                    Value::Same(v) => (v.as_str(), Some(yaml_scalar(v))),
                    Value::Split { r, yaml } => {
                        (r.as_str(), (!yaml.is_empty()).then(|| yaml.clone()))
                    }
                };
                let chunk = match form.as_str() {
                    "header" => {
                        let fixed: String = sweep
                            .fixed
                            .iter()
                            .map(|(k, v)| format!(", {k}={v}"))
                            .collect();
                        format!(
                            "```{{r c1, {}={}{fixed}}}\n{body}\n```",
                            sweep.option, r_value
                        )
                    }
                    "yaml" => {
                        let Some(y) = yaml_value else { continue };
                        let fixed: String = sweep
                            .fixed
                            .iter()
                            .map(|(k, v)| format!("#| {k}: {}\n", yaml_scalar(v)))
                            .collect();
                        format!("```{{r c1}}\n#| {}: {y}\n{fixed}{body}\n```", sweep.option)
                    }
                    other => panic!("unknown form {other}"),
                };
                let name = format!(
                    "{}/{}/{form}/{idx:02}_{}",
                    sweep.group,
                    sweep.option,
                    slug(r_value)
                );
                let mut text = format!("Prose before.\n\n{chunk}\n\nProse after.\n");
                append_filler(&mut text, seed, &name);
                out.push(Fixture {
                    name,
                    group: sweep.group.clone(),
                    ext: "qmd",
                    text,
                    formats: formats_or_html(&sweep.formats),
                    sibling_dir: None,
                    expect_abort: false,
                });
            }
        }
    }

    fn load_handwritten(&self, hw: &Handwritten, out: &mut Vec<Fixture>) {
        let dir = self.dir.join(&hw.dir);
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "Rmd"))
            .collect();
        files.sort();
        for path in files {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            if hw.skip.contains(&stem) {
                continue;
            }
            out.push(Fixture {
                name: format!("{}/{stem}", hw.dir),
                group: hw.dir.clone(),
                ext: "Rmd",
                text: std::fs::read_to_string(&path).unwrap(),
                formats: vec!["html".into()],
                sibling_dir: Some(dir.clone()),
                expect_abort: hw.expect.as_ref().is_some_and(|e| e.abort.contains(&stem)),
            });
        }
    }
}

fn formats_or_html(formats: &Option<Vec<String>>) -> Vec<String> {
    formats.clone().unwrap_or_else(|| vec!["html".into()])
}

/// The R literal as a YAML scalar: `TRUE`/`FALSE` become `true`/`false`; quoted strings and
/// numbers are valid YAML as written.
fn yaml_scalar(r: &str) -> String {
    match r {
        "TRUE" => "true".into(),
        "FALSE" => "false".into(),
        other => other.into(),
    }
}

fn slug(r_value: &str) -> String {
    let s: String = r_value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "empty".into() } else { s }
}

const FILLER_WORDS: &[&str] = &[
    "amber", "basalt", "cedar", "dune", "ember", "fjord", "glacier", "harbor", "iris", "juniper",
    "kelp", "lagoon", "marsh", "nectar", "orchid", "pebble", "quartz", "reef", "sable", "tundra",
    "umber", "valley", "willow", "yarrow", "zephyr", "anchor", "bramble", "canyon", "delta",
    "estuary",
];

fn append_filler(text: &mut String, seed: u64, name: &str) {
    let mut state = seed ^ fnv1a(name);
    let words: Vec<&str> = (0..12)
        .map(|_| {
            state = splitmix64(state);
            FILLER_WORDS[(state % FILLER_WORDS.len() as u64) as usize]
        })
        .collect();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("\nFiller: {}.\n", words.join(" ")));
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
