//! The founding-crash end-to-end pin (Plan 3 Phase 6e, T10).
//!
//! This file holds **two assertions of different kinds**, deliberately
//! kept in one test because they need the same fixture and the same
//! single render. Read the two halves separately:
//!
//! # Half 1 — the carets: a q2 regression guard (bound)
//!
//! `_quarto.yml:7:16` and `_quarto.yml:7:37` bind q2's own config-path
//! provenance — Plan 2 Phase 3's `content_source_info` consumption in the
//! **project-config** re-parse base:
//!
//! - `crates/quarto-core/src/transforms/config_markdown.rs:326`
//!   (`let base = content_source_info.as_ref().unwrap_or(&value.source_info);`)
//!
//! Reverting that base to the raw span (dropping `content_source_info` and
//! using `value.source_info` unconditionally) shifts **both** carets one
//! byte left — to `:7:15` (onto the `'` quote delimiter) and `:7:36`. The
//! second is the founding crash's own arithmetic: the correct end offset
//! sits at the `<` of `</span>`, and one byte left of it is byte 37, which
//! is *inside* `✨` (bytes 35..38). It renders as column 36 rather than
//! aborting only because `quarto-source-map`'s floor walks it back to the
//! start of the character. **Both caret assertions** go RED under this
//! revert, and the test was run both with the revert applied and without
//! it. ("Both" here means the two caret assertions in this file — *not*
//! both re-parse bases; see the next paragraph.) See the Phase 6e evidence
//! block in `claude-notes/plans/2026-08-20-provenance-3-audit-and-fix.md`.
//!
//! **This fixture does NOT bind the sibling base at
//! `crates/pampa/src/pandoc/meta.rs:255`** (`markdown_base`, the
//! front-matter path). Measured 2026-08-23: reverting `meta.rs:255` alone
//! leaves this test fully green, because the navbar `text:` value reaches
//! the markdown re-parse through `ConfigMarkdownTransform`, not through
//! `DocumentMetadata`. Plan 3's seam row predicted "either base" reddens
//! it; only one does. `meta.rs:255` is bound instead by
//! `json_errors::plain_scalar_raw_html_frontmatter_unaffected` and the
//! front-matter provenance tests beside it (Plan 3 T12 establishes the
//! one-guard-per-path split selectively).
//!
//! # Half 2 — the exit code: an UPSTREAM-BEHAVIOUR PIN (no q2 hunk)
//!
//! `assert exit 0` follows the T6 convention: **it does not guard any of
//! this plan's provenance work.** No revert of any provenance hunk in this
//! epic reddens it. (It is not literally unfalsifiable: any q2 change that
//! makes the render fail — a broken project loader, a panic anywhere in the
//! pipeline — reddens this assertion too. What it does not do is discriminate
//! a provenance regression, which is the only thing the rest of this file is
//! about.) Per recommendations § 4's measured config table, the abort returns
//! only if q2's mapping regresses **and** both upstream guards are gone:
//!
//! - `quarto-source-map`'s `FileInformation::offset_to_location` char-
//!   boundary floor (0.1.2, commit `8e07717`), and
//! - `quarto-error-reporting`'s `snap_span_to_char_boundaries` (0.2.2).
//!
//! With either guard present the render is clean (configs D and F);
//! with neither, ariadne 0.6.0 panics inside `write.rs` with
//! `end byte index 37 is not a char boundary; it is inside '✨'`, exit 101
//! (configs A, C and E). Its revert hunks live in two other repositories.
//!
//! It is asserted anyway because **it is the only witness of the founding
//! abort that exists anywhere**: the crash that opened this epic
//! (`bd-ariadne-config-span-char-boundary-panic-rkqmhzrg`) wrote `_site/`
//! and then aborted with exit 101 and a truncated log. Do not count it as
//! covering q2 code.
//!
//! # Fixture
//!
//! Written inline (no external repro path). Line endings are LF on every
//! platform: the `_quarto.yml` below is a Rust string literal with
//! explicit `\n`, so the byte offsets — and therefore the columns — do not
//! vary by host. The columns asserted are **character** columns, not byte
//! columns: `✨` occupies one column, not three.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// `_quarto.yml` line 7, verbatim, is:
///
/// ```text
///       - text: '<span id="x">Ask AI ✨</span>'
/// ```
///
/// Six leading spaces, then `- text: '` — fifteen characters before the
/// `<`. So (1-based character columns):
///
/// - cols 1-6 spaces, 7 `-`, 8 space, 9-12 `text`, 13 `:`, 14 space, 15 `'`
/// - col **16** = the `<` of `<span` → first `Q-2-9`
/// - `<span id="x">` spans cols 16-28; `Ask AI ` spans 29-35
/// - col 36 = `✨` (one character column, three bytes)
/// - col **37** = the `<` of `</span>` → second `Q-2-9`
///
/// Changing the indentation of this line changes both asserted columns.
const QUARTO_YML: &str = concat!(
    "project:\n",
    "  type: website\n",
    "website:\n",
    "  title: \"T\"\n",
    "  navbar:\n",
    "    left:\n",
    "      - text: '<span id=\"x\">Ask AI \u{2728}</span>'\n",
    "        href: index.qmd\n",
);

const INDEX_QMD: &str = "---\ntitle: \"Index\"\n---\n\nbody\n";

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Write the founding repro into `dir` and render it with the real binary.
fn render_founding_repro(dir: &Path) -> Output {
    write_file(&dir.join("_quarto.yml"), QUARTO_YML);
    write_file(&dir.join("index.qmd"), INDEX_QMD);

    Command::new(Q2_BIN)
        .current_dir(dir)
        .arg("render")
        .output()
        .expect("spawn q2 binary")
}

/// `TempDir::path` can be a symlink on macOS (`/var` → `/private/var`);
/// canonicalize so the path the diagnostic prints and the path we compare
/// against agree.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[test]
fn founding_repro_renders_clean_with_correct_carets() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());

    let output = render_founding_repro(&dir);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // --- Half 2: the upstream-behaviour pin (no q2 hunk) ---------------
    // Exit 101 here means the founding abort is back: ariadne panicked on
    // a span ending inside `✨`. See the module doc — this half binds
    // `quarto-source-map`'s floor and `quarto-error-reporting`'s snap, not
    // any code in this repository.
    assert!(
        output.status.success(),
        "founding repro must render cleanly (exit 0); got {:?}. \
         A panic here means BOTH upstream char-boundary guards are gone \
         (quarto-source-map's offset_to_location floor and \
         quarto-error-reporting's snap_span_to_char_boundaries) AND q2's \
         mapping regressed. stderr:\n{stderr}",
        output.status.code()
    );

    // --- Half 1: the carets — q2's own regression guard ----------------
    // Both `<span …>` and `</span>` in the navbar `text:` value are
    // converted to raw HTML, one Q-2-9 each.
    let q_2_9_count = stderr.matches("[Q-2-9]").count();
    assert_eq!(
        q_2_9_count, 2,
        "expected exactly two Q-2-9 warnings (the `<span …>` and the `</span>`); \
         got {q_2_9_count}. stderr:\n{stderr}"
    );

    // The `_quarto.yml:` prefix is separator-free, so this comparison is
    // host-independent even though the printed path is absolute.
    assert!(
        stderr.contains("_quarto.yml:7:16"),
        "first Q-2-9 must be anchored at the `<` of `<span` — _quarto.yml:7:16. \
         A `:7:15` here is the quote delimiter: the config re-parse base at \
         config_markdown.rs:326 was reverted to the raw span. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("_quarto.yml:7:37"),
        "second Q-2-9 must be anchored at the `<` of `</span>` — \
         _quarto.yml:7:37. A `:7:36` here is the documented degradation of \
         reverting the config re-parse base (config_markdown.rs:326) to the \
         raw span: one BYTE left, onto the interior of the multi-byte \
         character — the founding crash's own offset. stderr:\n{stderr}"
    );

    // The founding crash wrote `_site/` and *then* aborted. Confirm the
    // output actually landed, so a future regression that skips the render
    // entirely cannot pass this test by printing nothing.
    assert!(
        dir.join("_site").join("index.html").is_file(),
        "expected _site/index.html to be written"
    );
}
