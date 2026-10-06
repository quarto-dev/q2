/*
 * tests/integration/shortcode_all_contexts.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Detection test: a live `{{< meta author >}}` shortcode in every
 * qmd-reachable AST context, rendered to native HTML (bd-xjg7vl6c).
 */

//! Each case renders a separate document whose front matter defines
//! `author: Ann` and whose body places `{{< meta author >}}` in one
//! context. A case passes when every `needles` entry appears in the
//! HTML, no `forbid` entry appears, and (unless `unresolved_ok`) the
//! page carries no `quarto-unresolved-shortcode` / `?meta` marker.
//!
//! Cases carry an optional `known_gap` marker. Today the only marker is
//! `Q1_LITERAL`: contexts where Quarto 1 does not expand shortcodes either,
//! asserted here as "stays unexpanded":
//!
//! - an unmarked case that fails         => test fails (regression/new gap)
//! - a `known_gap` case that passes      => test fails (q2 now differs from Q1)
//!
//! All cases are rendered and reported together; the full results
//! table is printed on any failure.
//!
//! Not reachable from qmd syntax (no case here; covered only by
//! building the AST directly): `TableBody.head` rows (q2 rejects grid
//! tables, Q-2-39), `CaptionBlock`, `BlockMetadata`, attr values on
//! `Row` / `Cell`, and indented multi-paragraph footnote definitions
//! (indented text is a code block in q2, Q-2-35).

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Contexts where Quarto 1 does NOT expand shortcodes (verified against Q1's
/// `shortcodes.lua` and renders, 2026-10-04; see
/// claude-notes/plans/2026-10-04-shortcode-footnote-defs-lstcap.md), so q2
/// leaves them unexpanded for parity. The case must keep failing; if it
/// starts passing, q2 diverged from Q1 and that should be a deliberate choice.
const Q1_LITERAL: Option<&str> = Some("q1-literal");

struct Case {
    name: &'static str,
    /// Extra front-matter lines (after `author: Ann`).
    meta: &'static str,
    body: &'static str,
    /// Substrings that must all appear in the HTML.
    needles: &'static [&'static str],
    /// Substrings that must not appear in the HTML.
    forbid: &'static [&'static str],
    /// Skip the generic "no unresolved marker" check (unknown-shortcode case).
    unresolved_ok: bool,
    /// Q-code that must appear among the render diagnostics.
    expect_diag: Option<&'static str>,
    known_gap: Option<&'static str>,
}

const BASE: Case = Case {
    name: "",
    meta: "",
    body: "",
    needles: &[],
    forbid: &[],
    unresolved_ok: false,
    expect_diag: None,
    known_gap: None,
};

fn cases() -> Vec<Case> {
    vec![
        // ── Block text contexts ─────────────────────────────────────
        Case {
            name: "paragraph (control)",
            body: "P-CTL {{< meta author >}}.\n",
            needles: &["P-CTL Ann."],
            ..BASE
        },
        Case {
            name: "heading text",
            body: "## H-TXT {{< meta author >}}\n\nBody.\n",
            needles: &["H-TXT Ann"],
            ..BASE
        },
        Case {
            name: "heading attr value",
            body: "## Head {data-x=\"H-ATTR {{< meta author >}}\"}\n\nBody.\n",
            needles: &["data-x=\"H-ATTR Ann\""],
            ..BASE
        },
        Case {
            name: "bullet list item",
            body: "- LI-B {{< meta author >}}\n- other\n",
            needles: &["LI-B Ann"],
            ..BASE
        },
        Case {
            name: "ordered list item",
            body: "1. LI-O {{< meta author >}}\n2. other\n",
            needles: &["LI-O Ann"],
            ..BASE
        },
        Case {
            name: "task list item",
            body: "- [ ] LI-T {{< meta author >}}\n- [x] other\n",
            needles: &["LI-T Ann"],
            ..BASE
        },
        Case {
            name: "blockquote",
            body: "> BQ {{< meta author >}}\n",
            needles: &["BQ Ann"],
            ..BASE
        },
        Case {
            name: "definition list term",
            body: "::: {.definition-list}\n- DL-TERM {{< meta author >}}\n  - Definition body.\n:::\n",
            needles: &["DL-TERM Ann"],
            ..BASE
        },
        Case {
            name: "definition list definition",
            body: "::: {.definition-list}\n- Term\n  - DL-DEF {{< meta author >}}\n:::\n",
            needles: &["DL-DEF Ann"],
            ..BASE
        },
        Case {
            name: "line block",
            body: "| LB-1 {{< meta author >}}\n| second line\n",
            needles: &["LB-1 Ann"],
            ..BASE
        },
        Case {
            name: "div content",
            body: "::: {.note}\nDIV-C {{< meta author >}}\n:::\n",
            needles: &["DIV-C Ann"],
            ..BASE
        },
        Case {
            name: "div attr value",
            body: "::: {data-x=\"DIV-A {{< meta author >}}\"}\nInside.\n:::\n",
            needles: &["data-x=\"DIV-A Ann\""],
            ..BASE
        },
        // ── Inline contexts ─────────────────────────────────────────
        Case {
            name: "span content",
            body: "A [SPAN-C {{< meta author >}}]{.hl} b.\n",
            needles: &["SPAN-C Ann"],
            ..BASE
        },
        Case {
            name: "span attr value",
            body: "A [word]{data-x=\"SPAN-A {{< meta author >}}\"} b.\n",
            needles: &["data-x=\"SPAN-A Ann\""],
            ..BASE
        },
        Case {
            name: "emph",
            body: "*EMPH {{< meta author >}}*\n",
            needles: &["EMPH Ann"],
            ..BASE
        },
        Case {
            name: "strong",
            body: "**STRONG {{< meta author >}}**\n",
            needles: &["STRONG Ann"],
            ..BASE
        },
        Case {
            name: "strikeout",
            body: "~~STRIKE {{< meta author >}}~~\n",
            needles: &["STRIKE Ann"],
            ..BASE
        },
        Case {
            name: "superscript",
            body: "x^SUP {{< meta author >}}^\n",
            needles: &["SUP Ann"],
            ..BASE
        },
        Case {
            name: "subscript",
            body: "x~SUB {{< meta author >}}~\n",
            needles: &["SUB Ann"],
            ..BASE
        },
        Case {
            name: "underline span",
            body: "[UND {{< meta author >}}]{.underline}\n",
            needles: &["UND Ann"],
            ..BASE
        },
        Case {
            name: "smallcaps span",
            body: "[SMC {{< meta author >}}]{.smallcaps}\n",
            needles: &["SMC Ann"],
            ..BASE
        },
        Case {
            name: "quoted",
            body: "He said \"QUO {{< meta author >}}\" today.\n",
            needles: &["QUO Ann"],
            ..BASE
        },
        Case {
            name: "link text",
            body: "[LNK-T {{< meta author >}}](https://example.com/x)\n",
            needles: &["LNK-T Ann"],
            ..BASE
        },
        Case {
            name: "link URL",
            body: "[t](https://example.com/{{< meta author >}})\n",
            needles: &["https://example.com/Ann"],
            ..BASE
        },
        Case {
            name: "link title",
            body: "[t](https://example.com/x \"LNK-TTL {{< meta author >}}\")\n",
            needles: &["title=\"LNK-TTL Ann\""],
            known_gap: Q1_LITERAL,
            ..BASE
        },
        Case {
            name: "link attr value",
            body: "[t](https://example.com/x){data-x=\"LNK-A {{< meta author >}}\"}\n",
            needles: &["data-x=\"LNK-A Ann\""],
            ..BASE
        },
        Case {
            name: "image alt",
            body: "![IMG-ALT {{< meta author >}}](img.png)\n",
            needles: &["IMG-ALT Ann"],
            ..BASE
        },
        Case {
            name: "image title",
            body: "![a](img.png \"IMG-TTL {{< meta author >}}\")\n",
            needles: &["IMG-TTL Ann"],
            known_gap: Q1_LITERAL,
            ..BASE
        },
        Case {
            name: "image attr value",
            body: "![a](img.png){data-x=\"IMG-A {{< meta author >}}\"}\n",
            needles: &["data-x=\"IMG-A Ann\""],
            ..BASE
        },
        Case {
            name: "image src",
            body: "![a]({{< meta author >}}.png)\n",
            needles: &["src=\"Ann.png\""],
            ..BASE
        },
        Case {
            name: "bracketed span attr",
            body: "[plain]{#sp data-x=\"BSP {{< meta author >}}\"}\n",
            needles: &["data-x=\"BSP Ann\""],
            ..BASE
        },
        // ── Figures / tables ────────────────────────────────────────
        Case {
            name: "figure caption",
            body: "![FIG-CAP {{< meta author >}}](img.png){#fig-a}\n",
            needles: &["FIG-CAP Ann"],
            ..BASE
        },
        Case {
            name: "figure attr value",
            body: "![cap](img.png){#fig-b data-x=\"FIG-A {{< meta author >}}\"}\n",
            needles: &["data-x=\"FIG-A Ann\""],
            ..BASE
        },
        Case {
            name: "div fig- id caption",
            body: "::: {#fig-c}\n![](img.png)\n\nDFIG-CAP {{< meta author >}}\n:::\n",
            needles: &["DFIG-CAP Ann"],
            ..BASE
        },
        Case {
            name: "pipe table cell",
            body: "| H |\n|---|\n| TBL-CELL {{< meta author >}} |\n",
            needles: &["TBL-CELL Ann"],
            ..BASE
        },
        Case {
            name: "pipe table header",
            body: "| TBL-HDR {{< meta author >}} |\n|---|\n| x |\n",
            needles: &["TBL-HDR Ann"],
            ..BASE
        },
        Case {
            name: "table caption",
            body: "| H |\n|---|\n| x |\n\n: TBL-CAP {{< meta author >}}\n",
            needles: &["TBL-CAP Ann"],
            ..BASE
        },
        Case {
            name: "table caption (tbl- id)",
            body: "| H |\n|---|\n| x |\n\n: TBL-CAP2 {{< meta author >}} {#tbl-a}\n",
            needles: &["TBL-CAP2 Ann"],
            ..BASE
        },
        Case {
            name: "table attr value",
            body: "| H |\n|---|\n| x |\n\n: Cap {data-x=\"TBL-A {{< meta author >}}\"}\n",
            needles: &["data-x=\"TBL-A Ann\""],
            known_gap: Q1_LITERAL,
            ..BASE
        },
        // ── Code / raw / math ───────────────────────────────────────
        Case {
            name: "code block text",
            body: "```\nCB {{< meta author >}}\n```\n",
            needles: &["CB Ann"],
            ..BASE
        },
        Case {
            name: "code block attr value",
            body: "```{.txt data-x=\"CB-A {{< meta author >}}\"}\nx\n```\n",
            needles: &["data-x=\"CB-A Ann\""],
            known_gap: Q1_LITERAL,
            ..BASE
        },
        Case {
            name: "code block opt-out (shortcodes=false)",
            body: "```{.markdown shortcodes=\"false\"}\nCB-OFF {{< meta author >}}\n```\n",
            needles: &["CB-OFF {{&lt; meta author &gt;}}"],
            forbid: &["CB-OFF Ann"],
            ..BASE
        },
        Case {
            name: "code block opt-out (cell-code)",
            body: "```{.cell-code}\nCB-CELL {{< meta author >}}\n```\n",
            needles: &["CB-CELL {{&lt; meta author &gt;}}"],
            forbid: &["CB-CELL Ann"],
            ..BASE
        },
        Case {
            name: "inline code",
            body: "Run `IC {{< meta author >}}` now.\n",
            needles: &["IC Ann"],
            ..BASE
        },
        Case {
            name: "inline code attr value",
            body: "Run `x`{data-x=\"IC-A {{< meta author >}}\"} now.\n",
            needles: &["data-x=\"IC-A Ann\""],
            known_gap: Q1_LITERAL,
            ..BASE
        },
        Case {
            name: "raw block",
            body: "```{=html}\n<i>RAW-B {{< meta author >}}</i>\n```\n",
            needles: &["<i>RAW-B Ann</i>"],
            ..BASE
        },
        Case {
            name: "raw inline",
            body: "x `<b>RAW-I {{< meta author >}}</b>`{=html} y\n",
            needles: &["<b>RAW-I Ann</b>"],
            ..BASE
        },
        Case {
            name: "inline math",
            body: "Val $MI = {{< meta author >}}$ here.\n",
            needles: &["MI = Ann"],
            ..BASE
        },
        Case {
            name: "display math",
            body: "$$\nMD = {{< meta author >}}\n$$\n",
            needles: &["MD = Ann"],
            ..BASE
        },
        // ── Callouts / tabsets / cells ──────────────────────────────
        Case {
            name: "callout title attr",
            body: "::: {.callout-note title=\"CO-T {{< meta author >}}\"}\nBody.\n:::\n",
            needles: &["CO-T Ann"],
            ..BASE
        },
        Case {
            name: "callout title heading",
            body: "::: {.callout-note}\n## CO-H {{< meta author >}}\n\nBody.\n:::\n",
            needles: &["CO-H Ann"],
            ..BASE
        },
        Case {
            name: "callout body",
            body: "::: {.callout-note}\nCO-B {{< meta author >}}\n:::\n",
            needles: &["CO-B Ann"],
            ..BASE
        },
        Case {
            name: "tabset title",
            body: "::: {.panel-tabset}\n## TAB-T {{< meta author >}}\n\nContent A.\n\n## Other\n\nContent B.\n:::\n",
            needles: &["TAB-T Ann"],
            ..BASE
        },
        Case {
            name: "tabset body",
            body: "::: {.panel-tabset}\n## A\n\nTAB-B {{< meta author >}}\n\n## B\n\nContent B.\n:::\n",
            needles: &["TAB-B Ann"],
            ..BASE
        },
        Case {
            name: "listing caption attr (lst-cap)",
            body: "```{#lst-a .python lst-cap=\"LST-CAP {{< meta author >}}\"}\nx = 1\n```\n",
            needles: &["LST-CAP Ann"],
            ..BASE
        },
        Case {
            name: "code block attr value (lst- listing id)",
            body: "```{#lst-b .python data-x=\"CELL-A {{< meta author >}}\"}\nx = 1\n```\n",
            needles: &["data-x=\"CELL-A Ann\""],
            known_gap: Q1_LITERAL,
            ..BASE
        },
        // ── Cite ────────────────────────────────────────────────────
        Case {
            name: "cite prefix",
            body: "[see CITE-P {{< meta author >}} @nokey]\n",
            needles: &["CITE-P Ann"],
            known_gap: Q1_LITERAL,
            ..BASE
        },
        Case {
            name: "cite suffix",
            body: "[@nokey, p. 3 CITE-S {{< meta author >}}]\n",
            needles: &["CITE-S Ann"],
            known_gap: Q1_LITERAL,
            ..BASE
        },
        // ── Footnotes ───────────────────────────────────────────────
        Case {
            name: "inline footnote ^[...]",
            body: "Text.^[FN-INL {{< meta author >}}.]\n",
            needles: &["FN-INL Ann."],
            ..BASE
        },
        Case {
            name: "footnote definition (single para)",
            body: "Text.[^1]\n\n[^1]: FN-DEF {{< meta author >}}.\n",
            needles: &["FN-DEF Ann."],
            ..BASE
        },
        Case {
            name: "footnote definition (fenced block)",
            body: "Text.[^my]\n\n::: ^my\nFN-FB {{< meta author >}}.\n\nSecond.\n:::\n",
            needles: &["FN-FB Ann."],
            ..BASE
        },
        Case {
            name: "footnote definition (fenced, 2nd para)",
            body: "Text.[^my]\n\n::: ^my\nFirst.\n\nFN-FB2 {{< meta author >}}.\n:::\n",
            needles: &["FN-FB2 Ann."],
            ..BASE
        },
        Case {
            name: "footnote definition: unknown shortcode",
            body: "Text.[^1]\n\n[^1]: FN-UNK {{< nosuchthing >}}.\n",
            needles: &["?nosuchthing"],
            unresolved_ok: true,
            expect_diag: Some("Q-16-3"),
            ..BASE
        },
        Case {
            name: "body: unknown shortcode (control)",
            body: "UNK-CTL {{< nosuchthing >}}.\n",
            needles: &["?nosuchthing"],
            unresolved_ok: true,
            expect_diag: Some("Q-16-3"),
            ..BASE
        },
        Case {
            name: "footnote definition: unknown meta key",
            body: "Text.[^1]\n\n[^1]: FN-KEY {{< meta nosuchkey >}}.\n",
            needles: &["?meta:nosuchkey"],
            unresolved_ok: true,
            expect_diag: Some("Q-16-5"),
            ..BASE
        },
        Case {
            name: "body: unknown meta key (control)",
            body: "KEY-CTL {{< meta nosuchkey >}}.\n",
            needles: &["?meta:nosuchkey"],
            unresolved_ok: true,
            expect_diag: Some("Q-16-5"),
            ..BASE
        },
        // ── YAML metadata values (rendered into the page) ───────────
        Case {
            name: "yaml title",
            meta: "title: \"YT {{< meta author >}}\"\n",
            body: "Body.\n",
            needles: &["YT Ann"],
            ..BASE
        },
        Case {
            name: "yaml subtitle",
            meta: "subtitle: \"YS {{< meta author >}}\"\n",
            body: "Body.\n",
            needles: &["YS Ann"],
            ..BASE
        },
        Case {
            name: "yaml abstract",
            meta: "abstract: \"YA {{< meta author >}}\"\n",
            body: "Body.\n",
            needles: &["YA Ann"],
            ..BASE
        },
        // These two redefine `author:` as a list/map, so the shortcode
        // reads a separate `who: Ann` key instead.
        Case {
            name: "yaml author field (list of strings)",
            meta: "who: Ann\nauthor:\n  - Ann\n  - \"YL {{< meta who >}}\"\n",
            body: "Body.\n",
            needles: &["YL Ann"],
            ..BASE
        },
        Case {
            name: "yaml author nested map (affiliation name)",
            meta: "who: Ann\nauthor:\n  - name: \"Bob\"\n    affiliation:\n      - name: \"YN {{< meta who >}}\"\n",
            body: "Body.\n",
            needles: &["YN Ann"],
            ..BASE
        },
        Case {
            name: "yaml description",
            meta: "description: \"YD {{< meta author >}}\"\n",
            body: "Body.\n",
            needles: &["YD Ann"],
            ..BASE
        },
        Case {
            name: "yaml date",
            meta: "date: \"YDT {{< meta author >}}\"\n",
            body: "Body.\n",
            needles: &["YDT Ann"],
            ..BASE
        },
    ]
}

fn render(case: &Case) -> Result<(String, Vec<String>), String> {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    // A case's `meta` may itself define `author`; only add the default
    // when it does not.
    let author = if case.meta.contains("author:") {
        ""
    } else {
        "author: Ann\n"
    };
    let title = if case.meta.contains("title:") {
        ""
    } else {
        "title: All Contexts\n"
    };
    std::fs::write(
        &qmd_path,
        format!("---\n{title}{author}{}---\n\n{}", case.meta, case.body),
    )
    .unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&qmd_path, "html", &RenderToFileOptions::default(), runtime)
        .map_err(|e| format!("render error: {e}"))?;
    let html =
        std::fs::read_to_string(&result.output_path).map_err(|e| format!("read output: {e}"))?;
    let codes = result
        .render_output
        .diagnostics
        .iter()
        .filter_map(|d| d.code.clone())
        .collect();
    Ok((html, codes))
}

/// `Ok(())` when the case's expectation holds, otherwise why not.
fn check(case: &Case) -> Result<(), String> {
    let (html, codes) = render(case)?;
    let mut problems = Vec::new();
    for n in case.needles {
        if !html.contains(n) {
            problems.push(format!("missing {n:?}"));
        }
    }
    for f in case.forbid {
        if html.contains(f) {
            problems.push(format!("unexpected {f:?}"));
        }
    }
    if !case.unresolved_ok
        && (html.contains("quarto-unresolved-shortcode") || html.contains("?meta"))
    {
        problems.push("unresolved-shortcode marker present".to_string());
    }
    if let Some(code) = case.expect_diag
        && !codes.iter().any(|c| c == code)
    {
        problems.push(format!("missing diagnostic {code} (got {codes:?})"));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

#[test]
fn shortcode_expands_in_every_context() {
    let mut rows = Vec::new();
    let mut bad = Vec::new();
    for case in cases() {
        let result = check(&case);
        let status = match (&result, case.known_gap) {
            (Ok(()), None) => "PASS".to_string(),
            (Err(_), Some(id)) => format!("KNOWN-GAP({id})"),
            (Err(why), None) => {
                bad.push(format!("{}: FAILED: {why}", case.name));
                "FAIL".to_string()
            }
            (Ok(()), Some(id)) => {
                bad.push(format!(
                    "{}: known_gap {id} now PASSES; q2 now expands where Q1 does not \
                     (decide deliberately, then remove the marker)",
                    case.name
                ));
                "UNEXPECTED-PASS".to_string()
            }
        };
        let detail = match &result {
            Err(why) if case.known_gap.is_some() || status == "FAIL" => format!("  [{why}]"),
            _ => String::new(),
        };
        rows.push(format!("{status:<24} {}{detail}", case.name));
    }
    assert!(
        bad.is_empty(),
        "\n{} problem(s):\n  {}\n\nFull results:\n{}\n",
        bad.len(),
        bad.join("\n  "),
        rows.join("\n")
    );
}
