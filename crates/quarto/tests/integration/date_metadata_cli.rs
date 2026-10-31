/*
 * date_metadata_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `{{< meta date >}}` sees the same resolved, formatted date as the title
 * block, and date keywords resolve in the machine's local timezone
 * (bd-meta-date-local-time-bj4w929l). The expected values match Quarto 1.
 */

use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// Render `qmd` to HTML under `tz` and return the output.
fn render_in_tz(qmd: &str, tz: &str) -> String {
    let dir = TempDir::new().unwrap();
    let dir = dir.path();
    std::fs::write(dir.join("doc.qmd"), qmd).unwrap();
    let output = Command::new(Q2_BIN)
        .current_dir(dir)
        .env("TZ", tz)
        .args(["render", "doc.qmd", "--to", "html"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read_to_string(dir.join("doc.html")).unwrap()
}

fn render(qmd: &str) -> String {
    render_in_tz(qmd, "UTC")
}

#[test]
fn meta_shortcode_sees_formatted_keyword_date() {
    let html = render(
        "---\ntitle: T\ndate: today\ndate-format: \"YYYY\"\n---\n\nbody: {{< meta date >}}\n",
    );
    assert!(!html.contains("body: today"), "{html}");
    assert!(html.contains("body: 20"), "{html}");
}

#[test]
fn meta_shortcode_gets_html_default_long_style() {
    let html = render("---\ntitle: T\ndate: 2026-07-01\n---\n\nbody: {{< meta date >}}\n");
    assert!(html.contains("body: July 1, 2026"), "{html}");
}

#[test]
fn meta_shortcode_stays_iso_without_a_styled_title_block() {
    let html = render(
        "---\ntitle: T\ndate: 2026-07-01\ntitle-block-style: none\n---\n\nbody: {{< meta date >}}\n",
    );
    assert!(html.contains("body: 2026-07-01"), "{html}");
}

#[test]
fn meta_shortcode_sees_date_modified_and_field_format_map() {
    let html = render(
        "---\ntitle: T\ndate:\n  value: 2026-07-01\n  format: \"MMM D\"\ndate-modified: 2026-03-07\ndate-format: long\n---\n\nbody: {{< meta date >}} / {{< meta date-modified >}}\n",
    );
    assert!(html.contains("body: Jul 1 / March 7, 2026"), "{html}");
}

#[test]
fn metadata_value_referencing_date_sees_formatted_value() {
    let html = render(
        "---\ntitle: T\ndate: 2026-07-01\nsubtitle: \"Built {{< meta date >}}\"\n---\n\nx\n",
    );
    assert!(html.contains("Built July 1, 2026"), "{html}");
}

#[test]
fn date_written_with_a_shortcode_still_resolves() {
    let html = render(
        "---\ntitle: T\nbase: 2026-07-01\ndate: \"{{< meta base >}}\"\n---\n\nbody: {{< meta date >}}\n",
    );
    assert!(html.contains("body: July 1, 2026"), "{html}");
}

#[test]
fn unparseable_date_is_left_as_written() {
    let html = render("---\ntitle: T\ndate: not a date\n---\n\nbody: {{< meta date >}}\n");
    assert!(html.contains("body: not a date"), "{html}");
}

/// Timezones with no daylight saving, so the offsets are fixed. Windows
/// ignores `TZ`, so only the portable assertions above run there.
#[test]
fn keywords_resolve_in_the_local_timezone() {
    if !cfg!(unix) {
        return;
    }
    for (tz, offset) in [
        ("Pacific/Kiritimati", "+14:00"),
        ("Pacific/Pago_Pago", "-11:00"),
        ("UTC", "+00:00"),
    ] {
        let now = render_in_tz(
            "---\ntitle: T\ndate: now\ndate-format: \"Z\"\n---\n\nbody: {{< meta date >}}\n",
            tz,
        );
        assert!(now.contains(&format!("body: {offset}")), "{tz}: {now}");

        let today = render_in_tz(
            "---\ntitle: T\ndate: today\ndate-format: \"HH:mm Z\"\n---\n\nbody: {{< meta date >}}\n",
            tz,
        );
        assert!(
            today.contains(&format!("body: 00:00 {offset}")),
            "{tz}: {today}"
        );
        assert!(
            today.contains(&format!("{offset}\">")),
            "{tz}: dcterms: {today}"
        );
    }
}
