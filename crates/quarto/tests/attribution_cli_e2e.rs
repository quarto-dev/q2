/*
 * attribution_cli_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! End-to-end regression test for `q2 render --attribution=git`.
//!
//! Builds a temp git repo on every invocation (`tempdir` + `git init`
//! + two scripted commits by distinct authors), with
//! `GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE` / author identities pinned
//! so the porcelain output and commit hashes are bit-deterministic.
//! Copies `crates/quarto-core/tests/fixtures/attribution-blame/doc.qmd`
//! into the tempdir, then runs
//! `q2 render <tempdir>/doc.qmd --to html --attribution=git` and
//! asserts the full `data-attr-*` contract on the produced HTML:
//!
//! * `data-attr-actor` — the author email (per-commit blame credit).
//! * `data-attr-time`  — Unix epoch **seconds** for the git provider.
//!                       (Automerge / hub-client uses ms; the unit is
//!                       part of the wire contract — see
//!                       `docs/authoring/attribution.qmd`.)
//!
//! Identity (display name + colour) is **not** emitted per-node;
//! `AttributionViewerTransform` injects one `[data-attr-actor="…"]`
//! CSS rule per distinct actor into `<head>`, exposing the values as
//! `--attr-name` / `--attr-color` custom properties. The browser paints
//! the colour via the cascade, and `viewer.js` reads the custom
//! properties when building the hover badge.
//!
//! This is the one test that exercises the live `git blame --porcelain`
//! shell-out (`GitBlameProvider::build` in
//! `crates/quarto-core/src/attribution/git_blame.rs`); the fixture-
//! based unit tests in `attribution_gitblame.rs` only cover the
//! parser. Any regression in CLI flag wiring, working-directory
//! resolution, or porcelain handling on real git output should surface
//! here first.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn run_git(cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd);
    // Use mutable args so we can prefix `-c commit.gpgsign=false` for
    // commit operations; it's harmless for other subcommands.
    cmd.args(["-c", "commit.gpgsign=false"]);
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.env("GIT_CONFIG_NOSYSTEM", "1");
    // Cross-platform "no global config" — /dev/null on unix, NUL on
    // windows. Using a missing path also works.
    #[cfg(unix)]
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
    #[cfg(windows)]
    cmd.env("GIT_CONFIG_GLOBAL", "NUL");
    cmd.output().expect("spawn git")
}

const ALICE_EMAIL: &str = "alice@example.com";
const BOB_EMAIL: &str = "bob@example.com";

/// Build a deterministic two-author git history under `dir`.
///
/// The first commit (Alice) contains everything up to and including
/// the first body paragraph; the second commit (Bob) appends the
/// rest. Splitting on a line boundary guarantees `git blame` credits
/// at least one rendered-body line to each author — splitting
/// mid-line would let Bob's "completion" of a partial line absorb
/// what was nominally Alice's contribution.
fn scripted_repo(dir: &Path, doc_qmd: &str) {
    let split = doc_qmd
        .match_indices('\n')
        .map(|(i, _)| i + 1)
        .find(|&i| i >= doc_qmd.len() / 2)
        .expect("doc.qmd must have at least one newline past its midpoint");
    write_file(&dir.join("doc.qmd"), &doc_qmd[..split]);

    let init = run_git(dir, &["init", "-q", "-b", "main"], &[]);
    assert!(init.status.success(), "git init failed: {:?}", init);
    let add = run_git(dir, &["add", "doc.qmd"], &[]);
    assert!(add.status.success());
    let alice_env = [
        ("GIT_AUTHOR_NAME", "Alice"),
        ("GIT_AUTHOR_EMAIL", ALICE_EMAIL),
        ("GIT_COMMITTER_NAME", "Alice"),
        ("GIT_COMMITTER_EMAIL", ALICE_EMAIL),
        ("GIT_AUTHOR_DATE", "@1700000000 +0000"),
        ("GIT_COMMITTER_DATE", "@1700000000 +0000"),
    ];
    let commit = run_git(dir, &["commit", "-q", "-m", "alice: initial"], &alice_env);
    assert!(commit.status.success(), "git commit failed: {:?}", commit);

    // Second commit: full doc, attributed to Bob.
    write_file(&dir.join("doc.qmd"), doc_qmd);
    run_git(dir, &["add", "doc.qmd"], &[]);
    let bob_env = [
        ("GIT_AUTHOR_NAME", "Bob"),
        ("GIT_AUTHOR_EMAIL", BOB_EMAIL),
        ("GIT_COMMITTER_NAME", "Bob"),
        ("GIT_COMMITTER_EMAIL", BOB_EMAIL),
        ("GIT_AUTHOR_DATE", "@1700100000 +0000"),
        ("GIT_COMMITTER_DATE", "@1700100000 +0000"),
    ];
    let commit = run_git(dir, &["commit", "-q", "-m", "bob: append"], &bob_env);
    assert!(
        commit.status.success(),
        "second commit failed: {:?}",
        commit
    );
}

fn locate_fixture() -> PathBuf {
    // Resolve relative to this test's source position: we're in
    // `crates/quarto/tests/`, the fixture is in
    // `crates/quarto-core/tests/fixtures/`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join("quarto-core")
        .join("tests")
        .join("fixtures")
        .join("attribution-blame")
        .join("doc.qmd")
}

#[test]
fn cli_attribution_git_emits_data_attr_actor_for_both_authors() {
    let fixture = locate_fixture();
    assert!(
        fixture.exists(),
        "expected fixture at {}",
        fixture.display()
    );
    let doc_qmd = std::fs::read_to_string(&fixture).expect("read fixture");

    let tmp = TempDir::new().expect("tempdir");
    scripted_repo(tmp.path(), &doc_qmd);

    let output = Command::new(Q2_BIN)
        .arg("render")
        .arg(tmp.path().join("doc.qmd"))
        .args(["--to", "html"])
        .arg("--attribution=git")
        .output()
        .expect("spawn q2");

    assert!(
        output.status.success(),
        "q2 render --attribution=git must succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The output html lives next to the input by default; find it.
    let html_path = tmp.path().join("doc.html");
    let html = std::fs::read_to_string(&html_path).expect("read rendered html");

    // data-attr-actor — author email per commit blame credit.
    assert!(
        html.contains(&format!("data-attr-actor=\"{}\"", ALICE_EMAIL)),
        "alice's email must appear as data-attr-actor; html:\n{}",
        html
    );
    assert!(
        html.contains(&format!("data-attr-actor=\"{}\"", BOB_EMAIL)),
        "bob's email must appear as data-attr-actor; html:\n{}",
        html
    );

    // data-attr-time — Unix epoch SECONDS for the git provider. The
    // scripted commit times (@1700000000, @1700100000) flow through
    // `git blame --porcelain`'s committer-time and must arrive
    // verbatim. The fixture pins author-time = committer-time so this
    // particular assertion would pass regardless of which time the
    // provider reads; the committer-time semantics are pinned
    // separately by `cli_attribution_git_emits_committer_time_for_backdated_commit`.
    // A regression to milliseconds would shift to 13-digit values
    // (1_700_000_000_000) and fail this assertion.
    assert!(
        html.contains("data-attr-time=\"1700000000\""),
        "alice's commit time (seconds) must appear as data-attr-time; html:\n{}",
        html
    );
    assert!(
        html.contains("data-attr-time=\"1700100000\""),
        "bob's commit time (seconds) must appear as data-attr-time; html:\n{}",
        html
    );

    // Identity (display name + colour) is render-time CSS, not
    // per-node attrs. There must be exactly one rule per actor that
    // names that actor on the left-hand side and carries both
    // custom properties on the right.
    let alice_rule = extract_actor_css_rule(&html, ALICE_EMAIL)
        .expect("alice's [data-attr-actor=...] rule must be in <head>");
    let bob_rule = extract_actor_css_rule(&html, BOB_EMAIL)
        .expect("bob's [data-attr-actor=...] rule must be in <head>");

    // The display name pins the mail-local-part derivation that
    // `docs/authoring/attribution.qmd` advertises. CSS string-valued
    // custom properties are quoted, so the test looks for the quoted
    // form.
    assert!(
        alice_rule.contains("--attr-name: \"alice\""),
        "alice's CSS rule must carry --attr-name; got: {alice_rule}"
    );
    assert!(
        bob_rule.contains("--attr-name: \"bob\""),
        "bob's CSS rule must carry --attr-name; got: {bob_rule}"
    );

    // Colour is a deterministic hex entry from the Tol Muted palette
    // (see `crates/quarto-core/src/attribution/palette.rs`). We don't
    // pin a specific entry — the palette may evolve — but the wire
    // format `#` + 6 hex chars is part of the contract, and distinct
    // authors at this scale (2) must land on distinct buckets.
    let alice_color =
        extract_css_var_value(&alice_rule, "--attr-color").expect("alice's --attr-color value");
    let bob_color =
        extract_css_var_value(&bob_rule, "--attr-color").expect("bob's --attr-color value");
    assert!(
        alice_color.starts_with('#') && alice_color.len() == 7,
        "alice's --attr-color must be a 7-char hex string; got {alice_color}"
    );
    assert!(
        bob_color.starts_with('#') && bob_color.len() == 7,
        "bob's --attr-color must be a 7-char hex string; got {bob_color}"
    );
    assert_ne!(
        alice_color, bob_color,
        "per-actor colour derivation must yield distinct palette entries"
    );
}

/// Phase B test from `2026-05-14-attribution-auto-viewer.md`: the
/// default-on viewer auto-injects an inline `<style>` (carrying
/// `q2-attr-badge` selectors) into `<head>` and an inline `<script>`
/// (binding to `data-attr-actor` elements) before `</body>`. Both
/// must reach the rendered HTML through the live pipeline, not just
/// the transform-level tests.
#[test]
fn cli_attribution_git_auto_injects_viewer_css_and_js() {
    let fixture = locate_fixture();
    let doc_qmd = std::fs::read_to_string(&fixture).expect("read fixture");
    let tmp = TempDir::new().expect("tempdir");
    scripted_repo(tmp.path(), &doc_qmd);

    let output = Command::new(Q2_BIN)
        .arg("render")
        .arg(tmp.path().join("doc.qmd"))
        .args(["--to", "html"])
        .arg("--attribution=git")
        .output()
        .expect("spawn q2");
    assert!(
        output.status.success(),
        "q2 render --attribution=git must succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let html = std::fs::read_to_string(tmp.path().join("doc.html")).expect("read rendered html");

    // CSS: a `<style>` containing the badge class lifted from the
    // hub-client framework so both surfaces share the contract.
    assert!(
        html.contains("q2-attr-badge"),
        "rendered HTML must contain the viewer CSS class; html:\n{}",
        html
    );
    assert!(
        html.contains("<!-- quarto-attribution-viewer-css -->"),
        "viewer CSS must carry its dedup sentinel; html:\n{}",
        html
    );

    // JS: a `<script>` binding to `data-attr-actor` for hover.
    assert!(
        html.contains("<!-- quarto-attribution-viewer-js -->"),
        "viewer JS must carry its dedup sentinel; html:\n{}",
        html
    );
    // The auto-injected JS hooks on `[data-attr-actor]` (the same
    // attribute the writer emits per wrapper). A regression that ships
    // CSS but loses the JS hook would still satisfy the badge-class
    // assertion above — this one pins the listener path.
    let script_idx = html
        .find("<!-- quarto-attribution-viewer-js -->")
        .expect("sentinel present");
    let script_tail = &html[script_idx..];
    assert!(
        script_tail.contains("data-attr-actor"),
        "viewer JS body must reference data-attr-actor; got tail:\n{}",
        &script_tail[..script_tail.len().min(400)]
    );
}

/// Regression: when a commit is back-dated (its `author-time` is far
/// in the past but its `committer-time` is `now`), the rendered
/// wrapper's `data-attr-time` must carry the committer-time. The git
/// blame parser used to read only `author-time`, which made any
/// `--date=PAST` commit render as "910 days ago" in the viewer
/// regardless of when it actually landed in the branch. Pins the
/// committer-time semantics end-to-end.
#[test]
fn cli_attribution_git_emits_committer_time_for_backdated_commit() {
    let fixture = locate_fixture();
    let doc_qmd = std::fs::read_to_string(&fixture).expect("read fixture");

    let tmp = TempDir::new().expect("tempdir");
    write_file(&tmp.path().join("doc.qmd"), &doc_qmd);

    // The author-time is back-dated to 2023; the committer-time is set
    // to a clearly distinct, recent-ish value (well into 2024). The
    // two values must not collide. Both env vars are pinned so the
    // assertion is deterministic across machines and clocks.
    let init = run_git(tmp.path(), &["init", "-q", "-b", "main"], &[]);
    assert!(init.status.success(), "git init failed: {:?}", init);
    run_git(tmp.path(), &["add", "doc.qmd"], &[]);
    let env = [
        ("GIT_AUTHOR_NAME", "Alice"),
        ("GIT_AUTHOR_EMAIL", ALICE_EMAIL),
        ("GIT_COMMITTER_NAME", "Alice"),
        ("GIT_COMMITTER_EMAIL", ALICE_EMAIL),
        // Author-time: back-dated to 2023.
        ("GIT_AUTHOR_DATE", "@1700000000 +0000"),
        // Committer-time: distinct, "recent" relative to the author
        // date. The wrapper must surface this value.
        ("GIT_COMMITTER_DATE", "@1900000000 +0000"),
    ];
    let commit = run_git(tmp.path(), &["commit", "-q", "-m", "backdated"], &env);
    assert!(commit.status.success(), "commit failed: {:?}", commit);

    let output = Command::new(Q2_BIN)
        .arg("render")
        .arg(tmp.path().join("doc.qmd"))
        .args(["--to", "html"])
        .arg("--attribution=git")
        .output()
        .expect("spawn q2");
    assert!(
        output.status.success(),
        "q2 render must succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let html = std::fs::read_to_string(tmp.path().join("doc.html")).expect("read rendered html");
    assert!(
        html.contains("data-attr-time=\"1900000000\""),
        "rendered wrapper must carry committer-time (1900000000), not \
         the back-dated author-time; html:\n{}",
        html
    );
    assert!(
        !html.contains("data-attr-time=\"1700000000\""),
        "back-dated author-time (1700000000) must NOT appear in \
         data-attr-time — that would make recent commits render as \
         900+ days old in the viewer. html:\n{}",
        html
    );
}

/// Phase B test: YAML opt-out via `attribution: { viewer: false }`
/// suppresses the auto-injection while keeping the wrappers
/// themselves (attribution is activated by `--attribution=git` here;
/// the `viewer: false` knob is the orthogonal opt-out the plan
/// introduced).
#[test]
fn cli_attribution_git_yaml_viewer_opt_out_suppresses_css_and_js() {
    let fixture = locate_fixture();
    let doc_qmd = std::fs::read_to_string(&fixture).expect("read fixture");

    // Prepend a YAML block carrying the opt-out and strip the
    // fixture's own front matter so the result has a single,
    // well-formed YAML header. The CLI `--attribution=git` flag
    // activates wrapping; the YAML `viewer: false` only governs
    // whether the auto-injected CSS/JS ships.
    let augmented = format!(
        "---\ntitle: \"Attribution Test Document\"\nattribution:\n  viewer: false\n---\n\n{}",
        strip_front_matter(&doc_qmd)
    );

    let tmp = TempDir::new().expect("tempdir");
    scripted_repo(tmp.path(), &augmented);

    let output = Command::new(Q2_BIN)
        .arg("render")
        .arg(tmp.path().join("doc.qmd"))
        .args(["--to", "html"])
        .arg("--attribution=git")
        .output()
        .expect("spawn q2");
    assert!(
        output.status.success(),
        "q2 render must succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let html = std::fs::read_to_string(tmp.path().join("doc.html")).expect("read rendered html");

    // Wrappers still present (attribution itself is on via CLI).
    assert!(
        html.contains("data-attr-actor=\""),
        "opt-out must keep wrappers; html:\n{}",
        html
    );

    // But the viewer scaffolding is gone.
    assert!(
        !html.contains("q2-attr-badge"),
        "viewer: false must suppress the badge CSS; html:\n{}",
        html
    );
    assert!(
        !html.contains("<!-- quarto-attribution-viewer-css -->"),
        "viewer: false must suppress the CSS sentinel; html:\n{}",
        html
    );
    assert!(
        !html.contains("<!-- quarto-attribution-viewer-js -->"),
        "viewer: false must suppress the JS sentinel; html:\n{}",
        html
    );
}

/// Phase B test: when attribution is off the new transform must not
/// leak any wrapper / viewer scaffolding into the output. Pins
/// "no incidental whitespace from the new transform on the off path"
/// against future regressions (e.g. unconditionally creating the
/// `rendered.includes` slot even when bailing).
#[test]
fn cli_attribution_off_emits_no_viewer_artifacts() {
    let fixture = locate_fixture();
    let doc_qmd = std::fs::read_to_string(&fixture).expect("read fixture");
    let tmp = TempDir::new().expect("tempdir");
    scripted_repo(tmp.path(), &doc_qmd);

    let output = Command::new(Q2_BIN)
        .arg("render")
        .arg(tmp.path().join("doc.qmd"))
        .args(["--to", "html"])
        .output()
        .expect("spawn q2");
    assert!(
        output.status.success(),
        "q2 render (no --attribution) must succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let html = std::fs::read_to_string(tmp.path().join("doc.html")).expect("read rendered html");
    assert!(
        !html.contains("data-attr-actor"),
        "off path must produce no wrappers; html:\n{}",
        html
    );
    assert!(
        !html.contains("q2-attr-badge"),
        "off path must produce no viewer CSS; html:\n{}",
        html
    );
    assert!(
        !html.contains("quarto-attribution-viewer-"),
        "off path must produce neither viewer sentinel; html:\n{}",
        html
    );
}

/// Strip a single leading `---`-delimited YAML front-matter block. Used
/// by the opt-out test to wrap the fixture with its own front matter
/// without producing two YAML blocks.
fn strip_front_matter(qmd: &str) -> String {
    let trimmed = qmd.trim_start();
    if !trimmed.starts_with("---") {
        return qmd.to_string();
    }
    // Find the closing `---` line.
    let after_open = &trimmed[3..];
    if let Some(close_at) = after_open.find("\n---") {
        // Body starts after the closing `---` and its newline.
        let body_start = 3 + close_at + "\n---".len();
        let rest = &trimmed[body_start..];
        return rest.trim_start_matches('\n').to_string();
    }
    qmd.to_string()
}

/// Locate the per-actor CSS rule generated by
/// `AttributionViewerTransform` for `actor_email`. Returns the rule
/// body (everything between `{` and `}`) or `None` if the rule is
/// missing.
fn extract_actor_css_rule(html: &str, actor_email: &str) -> Option<String> {
    let selector = format!("[data-attr-actor=\"{}\"]", actor_email);
    let selector_at = html.find(&selector)?;
    let open = html[selector_at..].find('{')? + selector_at;
    let close = html[open..].find('}')? + open;
    Some(html[open + 1..close].trim().to_string())
}

/// Pull the value of a CSS custom property declaration from a rule
/// body. Looks for `<name>:` and returns the text up to the next `;`,
/// trimmed.
fn extract_css_var_value(rule_body: &str, name: &str) -> Option<String> {
    let needle = format!("{}:", name);
    let at = rule_body.find(&needle)?;
    let after = &rule_body[at + needle.len()..];
    let end = after.find(';').unwrap_or(after.len());
    Some(after[..end].trim().to_string())
}
