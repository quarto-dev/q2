//! The math fixture corpus under `tests/fixtures/` and its loader.
//!
//! Every later snapshot test (CST, `MathAst`, OMML, Typst) iterates the same
//! corpus through [`load_all`], so the conventions live here:
//!
//! - one construct per file, `<group>/<name>.tex`;
//! - a `.display.tex` suffix means display math, otherwise inline;
//! - the file's bytes are the math text exactly as pampa would hand it over
//!   (no `$` delimiters), except that one trailing `\n` is stripped so files
//!   can end in a newline for the sake of editors;
//! - `errors/` holds inputs that must produce a diagnostic, never a panic.
//!
//! The guard test below keeps the corpus well-formed.

use std::fs;
use std::path::{Path, PathBuf};

pub const FIXTURE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

#[derive(Debug, Clone)]
pub struct Fixture {
    /// Directory name under `tests/fixtures/`, e.g. `fractions`.
    pub group: String,
    /// File stem without the `.display` marker, e.g. `frac-nested`.
    pub name: String,
    /// `true` for `<name>.display.tex`.
    pub display: bool,
    /// The math text with one trailing newline stripped.
    pub text: String,
    pub path: PathBuf,
}

impl Fixture {
    /// Stable id for snapshot names: `fractions/frac-nested` or
    /// `environments/aligned.display`.
    pub fn id(&self) -> String {
        if self.display {
            format!("{}/{}.display", self.group, self.name)
        } else {
            format!("{}/{}", self.group, self.name)
        }
    }
}

fn read_fixture(group: &str, path: &Path) -> Fixture {
    let file_name = path.file_name().unwrap().to_str().unwrap();
    let stem = file_name.strip_suffix(".tex").unwrap();
    let (name, display) = match stem.strip_suffix(".display") {
        Some(n) => (n, true),
        None => (stem, false),
    };
    let raw = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let text = raw.strip_suffix('\n').unwrap_or(&raw).to_string();
    Fixture {
        group: group.to_string(),
        name: name.to_string(),
        display,
        text,
        path: path.to_path_buf(),
    }
}

/// Load every fixture, sorted by `group/name` so iteration order is stable
/// across platforms.
pub fn load_all() -> Vec<Fixture> {
    let mut out = Vec::new();
    let mut groups: Vec<PathBuf> = fs::read_dir(FIXTURE_ROOT)
        .expect("tests/fixtures exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    groups.sort();
    for group_dir in groups {
        let group = group_dir.file_name().unwrap().to_str().unwrap().to_string();
        let mut files: Vec<PathBuf> = fs::read_dir(&group_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        for f in files {
            out.push(read_fixture(&group, &f));
        }
    }
    out
}

fn is_kebab(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[test]
fn corpus_is_well_formed() {
    let fixtures = load_all();
    assert!(
        fixtures.len() >= 150,
        "expected the full corpus, found {} fixtures",
        fixtures.len()
    );

    let mut ids = std::collections::BTreeSet::new();
    for fx in &fixtures {
        let file_name = fx.path.file_name().unwrap().to_str().unwrap();
        assert!(
            file_name.ends_with(".tex"),
            "{}: every fixture is a .tex file",
            fx.path.display()
        );
        assert!(
            is_kebab(&fx.group),
            "{}: group is kebab-case",
            fx.path.display()
        );
        assert!(
            is_kebab(&fx.name),
            "{}: name is kebab-case",
            fx.path.display()
        );
        assert!(
            ids.insert(fx.id()),
            "{}: duplicate fixture id",
            fx.path.display()
        );

        let raw = fs::read(&fx.path).unwrap();
        assert!(
            !raw.contains(&b'\r'),
            "{}: CRLF line endings are not allowed",
            fx.path.display()
        );
        assert!(
            !raw.ends_with(b"\n\n"),
            "{}: at most one trailing newline",
            fx.path.display()
        );
        // Only the two deliberately-empty error fixtures may have no content.
        let deliberately_blank =
            fx.group == "errors" && (fx.name == "empty" || fx.name == "only-whitespace");
        assert!(
            deliberately_blank || !fx.text.trim().is_empty(),
            "{}: fixture has no content",
            fx.path.display()
        );
    }

    // Every group the plan calls for is present.
    let groups: std::collections::BTreeSet<&str> =
        fixtures.iter().map(|f| f.group.as_str()).collect();
    for required in [
        "accents",
        "basic",
        "corpus",
        "delimiters",
        "environments",
        "errors",
        "fractions",
        "macros",
        "operators",
        "radicals",
        "scripts",
        "spacing",
        "symbols",
        "text-and-styles",
    ] {
        assert!(
            groups.contains(required),
            "missing fixture group {required}"
        );
    }
}

#[test]
fn display_marker_is_parsed_from_file_name() {
    let fixtures = load_all();
    let aligned = fixtures
        .iter()
        .find(|f| f.group == "environments" && f.name == "aligned")
        .expect("environments/aligned.display.tex exists");
    assert!(aligned.display);
    assert_eq!(aligned.id(), "environments/aligned.display");
    let frac = fixtures
        .iter()
        .find(|f| f.group == "fractions" && f.name == "frac")
        .expect("fractions/frac.tex exists");
    assert!(!frac.display);
    assert_eq!(frac.text, r"\frac{a}{b}");
}
