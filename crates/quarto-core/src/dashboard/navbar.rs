/*
 * dashboard/navbar.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The dashboard navbar's metadata-driven parts: `nav-buttons` (Quarto 1's
//! `dashboardMeta` + `processNavButtons`) and the light/dark `logo` pair
//! (`format-dashboard.ts` `resolveLogo`). Q1 built both in its DOM pass;
//! q2 resolves them into `rendered.dashboard.nav-buttons` and
//! `rendered.dashboard.logo`, which `DASHBOARD_TITLE_BLOCK_PARTIAL` renders.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue};
use quarto_source_map::SourceInfo;

fn generated() -> SourceInfo {
    SourceInfo::generated(quarto_source_map::By::unknown())
}

/// One navbar link (Q1 `NavButton`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NavButton {
    pub href: String,
    pub text: Option<String>,
    pub icon: Option<String>,
    pub rel: Option<String>,
    pub target: Option<String>,
    pub title: Option<String>,
    pub aria_label: Option<String>,
}

/// The share-link aliases Q1 knows (`kNavButtonAliases`), minus `github`,
/// which needs the repository URL (bd-q04oxic2).
const ALIASES: &[&str] = &["linkedin", "facebook", "reddit", "twitter"];

/// JavaScript `encodeURI`: escape everything but unreserved and reserved
/// URI characters.
fn encode_uri(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric() || ";,/?:@&=+$-_.!~*'()#".contains(c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Q1 appends `&title=<encoded title>` (or `&text=` for Twitter) even
/// without a title, producing `title=undefined`. D5-Q18: q2 leaves the
/// parameter off when the document has no title.
fn share_param(key: &str, title: Option<&str>) -> String {
    match title {
        Some(t) => format!("&{key}={}", encode_uri(t)),
        None => String::new(),
    }
}

fn alias(name: &str, title: Option<&str>) -> Option<NavButton> {
    let (icon, label, href) = match name {
        "linkedin" => (
            "linkedin",
            "LinkedIn",
            format!(
                "https://www.linkedin.com/sharing/share-offsite/?url=|url|{}",
                share_param("title", title)
            ),
        ),
        "facebook" => (
            "facebook",
            "Facebook",
            "https://www.facebook.com/sharer/sharer.php?u=|url|".to_string(),
        ),
        "reddit" => (
            "reddit",
            "Reddit",
            format!(
                "https://reddit.com/submit?url=|url|{}",
                share_param("title", title)
            ),
        ),
        "twitter" => (
            "twitter",
            "Twitter",
            format!(
                "https://twitter.com/intent/tweet?url=|url|{}",
                share_param("text", title)
            ),
        ),
        _ => return None,
    };
    Some(NavButton {
        href,
        icon: Some(icon.to_string()),
        title: Some(label.to_string()),
        ..Default::default()
    })
}

fn unknown_alias(name: &str, source: &SourceInfo) -> DiagnosticMessage {
    let hint = if name == "github" {
        "The `github` alias is not supported yet. Give the repository's URL instead: \
         `{icon: github, href: https://github.com/<owner>/<repo>}`."
            .to_string()
    } else {
        format!(
            "The known aliases are {}. For any other link, give a map with `href` (and \
             optionally `icon`, `text`, `title`, `rel`, `target`, `aria-label`).",
            ALIASES
                .iter()
                .map(|a| format!("`{a}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    DiagnosticMessageBuilder::warning("Unknown nav-button")
        .with_code("Q-27-3")
        .with_location(source.clone())
        .problem(format!(
            "`nav-buttons` lists `{name}`, which is not a nav-button alias, so it was left out."
        ))
        .add_hint(hint)
        .build()
}

/// Read `nav-buttons` (a list, or a single entry): alias strings and
/// `{href, text, icon, rel, target, title, aria-label}` maps.
pub fn nav_buttons(
    meta: &ConfigValue,
    title: Option<&str>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<NavButton> {
    let Some(raw) = meta.get("nav-buttons") else {
        return Vec::new();
    };
    let entries: Vec<&ConfigValue> = match raw.as_array() {
        Some(items) => items.iter().collect(),
        None => vec![raw],
    };
    let mut out = Vec::new();
    for entry in entries {
        if let Some(name) = entry.as_plain_text()
            && entry.get("href").is_none()
        {
            match alias(&name, title) {
                Some(btn) => out.push(btn),
                None => diagnostics.push(unknown_alias(&name, &entry.source_info)),
            }
            continue;
        }
        let field = |k: &str| entry.get(k).and_then(|v| v.as_plain_text());
        if let Some(href) = field("href") {
            out.push(NavButton {
                href,
                text: field("text"),
                icon: field("icon"),
                rel: field("rel"),
                target: field("target"),
                title: field("title"),
                aria_label: field("aria-label"),
            });
        }
    }
    out
}

fn entry(k: &str, v: ConfigValue) -> ConfigMapEntry {
    ConfigMapEntry {
        key: k.to_string(),
        key_source: generated(),
        value: v,
    }
}

fn string(s: &str) -> ConfigValue {
    ConfigValue::new_string(s.to_string(), generated())
}

/// `rendered.dashboard.nav-buttons`: one map per button, with only the keys
/// that are set (the template tests each).
pub fn nav_buttons_value(buttons: &[NavButton]) -> ConfigValue {
    let items = buttons
        .iter()
        .map(|b| {
            let mut entries = vec![entry("href", string(&b.href))];
            for (k, v) in [
                ("text", &b.text),
                ("icon", &b.icon),
                ("rel", &b.rel),
                ("target", &b.target),
                ("title", &b.title),
                ("aria-label", &b.aria_label),
            ] {
                if let Some(v) = v {
                    entries.push(entry(k, string(v)));
                }
            }
            ConfigValue::new_map(entries, generated())
        })
        .collect();
    ConfigValue::new_array(items, generated())
}

/// `rendered.dashboard.logo`: `{light: {path, alt}, dark: {path, alt}}`
/// from `logo` / `logo-alt` (Q1 always emits both images, each shown in
/// its color mode, even for a single logo).
pub fn logo_value(meta: &ConfigValue) -> Option<ConfigValue> {
    let logo_alt = meta.get("logo-alt").and_then(|v| v.as_plain_text());
    let logo = quarto_navigation::navbar::parse_logo(meta.get("logo")?, logo_alt.as_deref())?;
    let variant = |v: &quarto_navigation::navbar::LogoVariant| {
        let mut entries = vec![entry("path", string(&v.path))];
        if let Some(alt) = &v.alt {
            entries.push(entry("alt", string(alt)));
        }
        ConfigValue::new_map(entries, generated())
    };
    Some(ConfigValue::new_map(
        vec![
            entry("light", variant(&logo.light)),
            entry("dark", variant(&logo.dark)),
        ],
        generated(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(yaml: &str) -> ConfigValue {
        let parsed = quarto_yaml::parse(yaml).unwrap();
        let mut diagnostics = pampa::utils::diagnostic_collector::DiagnosticCollector::new();
        pampa::pandoc::yaml_to_config_value(
            parsed,
            quarto_config::InterpretationContext::DocumentMetadata,
            &mut diagnostics,
        )
    }

    #[test]
    fn aliases_and_custom_buttons() {
        let m = meta(
            "nav-buttons:\n  - linkedin\n  - twitter\n  - icon: gitlab\n    href: https://gitlab.com/\n    text: G\n",
        );
        let mut diags = Vec::new();
        let b = nav_buttons(&m, Some("My Dash"), &mut diags);
        assert!(diags.is_empty());
        assert_eq!(b.len(), 3);
        assert_eq!(
            b[0].href,
            "https://www.linkedin.com/sharing/share-offsite/?url=|url|&title=My%20Dash"
        );
        assert_eq!(b[0].title.as_deref(), Some("LinkedIn"));
        assert_eq!(
            b[1].href,
            "https://twitter.com/intent/tweet?url=|url|&text=My%20Dash"
        );
        assert_eq!(b[2].href, "https://gitlab.com/");
        assert_eq!(b[2].icon.as_deref(), Some("gitlab"));
        assert_eq!(b[2].text.as_deref(), Some("G"));
    }

    /// D5-Q18: no `title=undefined` without a document title.
    #[test]
    fn share_links_without_a_title_omit_the_parameter() {
        let m = meta("nav-buttons: reddit\n");
        let b = nav_buttons(&m, None, &mut Vec::new());
        assert_eq!(b[0].href, "https://reddit.com/submit?url=|url|");
    }

    #[test]
    fn unknown_alias_warns_q_27_3() {
        let m = meta("nav-buttons: [github, myspace]\n");
        let mut diags = Vec::new();
        assert!(nav_buttons(&m, None, &mut diags).is_empty());
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().all(|d| d.code.as_deref() == Some("Q-27-3")));
    }

    #[test]
    fn encode_uri_matches_javascript() {
        assert_eq!(encode_uri("a b/é?&"), "a%20b/%C3%A9?&");
    }

    #[test]
    fn single_logo_fills_both_variants() {
        let v = logo_value(&meta("logo: who.jpg\nlogo-alt: WHO\n")).unwrap();
        for mode in ["light", "dark"] {
            let m = v.get(mode).unwrap();
            assert_eq!(
                m.get("path").unwrap().as_plain_text().as_deref(),
                Some("who.jpg")
            );
            assert_eq!(
                m.get("alt").unwrap().as_plain_text().as_deref(),
                Some("WHO")
            );
        }
        assert!(logo_value(&meta("title: x\n")).is_none());
    }
}
