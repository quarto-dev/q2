//! `xmllint`-backed schema validation of writer output.
//!
//! The OMML and WordprocessingML schemas live in `tests/schemas/ooxml/`
//! (see the README there for provenance and the one local patch). Writer
//! tests call [`assert_valid_omml`] on every emitted `m:oMath`; when
//! `xmllint` is not installed the check is skipped with a visible message
//! rather than failing, so the suite stays green on Windows CI while the
//! Unix legs keep the schema gate.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Directory holding the vendored schema closure.
pub const SCHEMA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/schemas/ooxml");

/// Outcome of a schema check.
#[derive(Debug)]
pub enum Validation {
    /// `xmllint` accepted the document.
    Valid,
    /// `xmllint` rejected it; the payload is its stderr.
    Invalid(String),
    /// The check could not run (no `xmllint` on `PATH`).
    Skipped(String),
}

/// Whether `xmllint` can be spawned. Probed once per test process.
pub fn xmllint_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("xmllint")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// Validate `xml` against `<SCHEMA_DIR>/<schema>` (e.g. `"shared-math.xsd"`).
pub fn validate_against(schema: &str, xml: &str) -> Validation {
    if !xmllint_available() {
        return Validation::Skipped("xmllint is not on PATH".to_string());
    }
    let schema_path = format!("{SCHEMA_DIR}/{schema}");
    let mut child = Command::new("xmllint")
        .arg("--noout")
        .arg("--schema")
        .arg(&schema_path)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("xmllint spawns after --version succeeded");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(xml.as_bytes())
        .expect("write xml to xmllint");
    let output = child.wait_with_output().expect("xmllint exits");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Validation::Valid
    } else {
        Validation::Invalid(stderr)
    }
}

/// Validate a standalone `m:oMath` / `m:oMathPara` fragment against the OMML
/// schema. The fragment must declare the `m:` namespace itself.
pub fn validate_omml(xml: &str) -> Validation {
    validate_against("shared-math.xsd", xml)
}

/// Assert that an OMML fragment validates. Skips (with a message on stderr)
/// when `xmllint` is unavailable.
pub fn assert_valid_omml(xml: &str) {
    match validate_omml(xml) {
        Validation::Valid => {}
        Validation::Invalid(err) => {
            panic!("OMML failed schema validation:\n{err}\n--- xml ---\n{xml}")
        }
        Validation::Skipped(why) => eprintln!("OMML schema check skipped: {why}"),
    }
}

const M_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn omath(body: &str) -> String {
    format!(r#"<m:oMath xmlns:m="{M_NS}">{body}</m:oMath>"#)
}

// ---------------------------------------------------------------------------
// Guard tests for the vendored schema set and the helper itself
// ---------------------------------------------------------------------------

#[test]
fn schema_set_accepts_a_correct_fragment() {
    let xml = omath(concat!(
        "<m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>",
        "<m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>",
        "<m:nary><m:naryPr><m:chr m:val=\"∑\"/><m:limLoc m:val=\"undOvr\"/></m:naryPr>",
        "<m:sub><m:r><m:t>i=1</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup>",
        "<m:e><m:r><m:t>i</m:t></m:r></m:e></m:nary>",
    ));
    match validate_omml(&xml) {
        Validation::Valid => {}
        Validation::Invalid(err) => panic!("expected the fragment to validate:\n{err}"),
        Validation::Skipped(why) => eprintln!("skipped: {why}"),
    }
}

#[test]
fn schema_set_rejects_swapped_fraction_children() {
    // `m:den` before `m:num`: the child order Word enforces.
    let xml = omath(
        "<m:f><m:den><m:r><m:t>b</m:t></m:r></m:den><m:num><m:r><m:t>a</m:t></m:r></m:num></m:f>",
    );
    match validate_omml(&xml) {
        Validation::Invalid(err) => assert!(
            err.contains("den") && err.contains("num"),
            "xmllint should name the misplaced/expected elements, got:\n{err}"
        ),
        Validation::Valid => panic!("swapped num/den must not validate"),
        Validation::Skipped(why) => eprintln!("skipped: {why}"),
    }
}

#[test]
fn schema_set_rejects_an_unknown_element() {
    let xml = omath("<m:bogus/>");
    match validate_omml(&xml) {
        Validation::Invalid(err) => assert!(err.contains("bogus"), "got:\n{err}"),
        Validation::Valid => panic!("unknown element must not validate"),
        Validation::Skipped(why) => eprintln!("skipped: {why}"),
    }
}

#[test]
fn wml_schema_accepts_a_minimal_document_with_math() {
    let xml = format!(
        r#"<w:document xmlns:w="{W_NS}" xmlns:m="{M_NS}"><w:body><w:p><w:r><w:t xml:space="preserve">hi </w:t></w:r><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></w:p><w:sectPr/></w:body></w:document>"#
    );
    match validate_against("wml.xsd", &xml) {
        Validation::Valid => {}
        Validation::Invalid(err) => panic!("expected the document to validate:\n{err}"),
        Validation::Skipped(why) => eprintln!("skipped: {why}"),
    }
}

#[test]
fn skipped_reports_a_reason_when_xmllint_is_missing() {
    // Only meaningful where xmllint is absent; elsewhere it documents the
    // contract that `Skipped` carries a human-readable reason.
    if !xmllint_available() {
        match validate_omml(&omath("<m:r><m:t>x</m:t></m:r>")) {
            Validation::Skipped(why) => assert!(why.contains("xmllint")),
            other => panic!("expected Skipped without xmllint, got {other:?}"),
        }
    }
}
