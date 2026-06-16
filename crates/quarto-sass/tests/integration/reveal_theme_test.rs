//! RevealJS theme assembly + compilation tests (Stage A of bd-r9mkybwl).
//!
//! Verifies that `assemble_reveal_scss` produces a SCSS bundle that:
//! - compiles cleanly through `grass` (our native SCSS compiler),
//! - emits the reveal `--r-*` custom properties carrying Quarto's overridden
//!   values (not reveal's defaults), and
//! - includes the look-fixing rules that distinguish a Quarto deck from a stock
//!   reveal deck (left-aligned slides, non-uppercase headings, Quarto title slide).

use quarto_sass::bundle::{
    REVEAL_BUILTIN_THEMES, assemble_reveal_scss, load_quarto_reveal_layer, load_reveal_framework,
    load_reveal_theme_layer, resolve_reveal_theme_name,
};

/// Compile assembled reveal SCSS through grass (expanded output for assertions).
fn compile(scss: &str) -> String {
    let options = grass::Options::default();
    grass::from_string(scss, &options).unwrap_or_else(|e| panic!("reveal SCSS should compile: {e}"))
}

#[test]
fn reveal_framework_layer_loads() {
    let fw = load_reveal_framework().unwrap();
    assert!(fw.uses.contains("sass:color"), "framework uses sass:color");
    assert!(
        fw.defaults.contains("$heading-text-transform"),
        "framework defaults declare reveal vars"
    );
    assert!(
        fw.rules.contains("--r-main-color"),
        "framework rules include the :root --r-* emitter"
    );
    assert!(
        fw.rules.contains("var(--r-main-color)"),
        "framework rules include the theme rule set"
    );
    assert!(
        fw.mixins.contains("dark-bg-text-color"),
        "framework mixins include reveal mixins"
    );
}

#[test]
fn quarto_reveal_layer_parses() {
    let q = load_quarto_reveal_layer().unwrap();
    assert!(
        q.defaults.contains("$presentation-slide-text-align"),
        "quarto layer declares the presentation vocabulary"
    );
    assert!(
        q.rules.contains("#title-slide"),
        "quarto layer rules include title-slide layout"
    );
}

#[test]
fn reveal_theme_compiles() {
    let scss = assemble_reveal_scss(&[]).unwrap();
    let css = compile(&scss);
    assert!(!css.is_empty(), "compiled reveal CSS is non-empty");
    // sanity: the reveal base rule set made it through
    assert!(
        css.contains(".reveal-viewport"),
        "includes reveal base rules"
    );
}

#[test]
fn quarto_values_flow_into_custom_properties() {
    let css = compile(&assemble_reveal_scss(&[]).unwrap());

    // Quarto's body color (#222) must win over reveal's default (#eee).
    assert!(
        css.contains("--r-main-color: #222"),
        "Quarto body color should drive --r-main-color\n{css}"
    );
    // Quarto's background (white) must win over reveal's default (#2b2b2b / #bbb).
    assert!(
        css.contains("--r-background-color: #fff"),
        "Quarto body-bg should drive --r-background-color"
    );
    // Collision case: reveal's own $link-color is set from Quarto's $primary.
    assert!(
        css.contains("--r-link-color: #2a76dd"),
        "Quarto primary should drive --r-link-color"
    );
}

#[test]
fn headings_are_not_uppercased() {
    let css = compile(&assemble_reveal_scss(&[]).unwrap());
    // Quarto turns OFF reveal's default uppercase headings.
    assert!(
        css.contains("--r-heading-text-transform: none"),
        "heading text-transform should be none"
    );
    assert!(
        !css.contains("uppercase"),
        "no uppercase anywhere in the compiled Quarto reveal theme\n{css}"
    );
}

#[test]
fn slides_are_left_aligned() {
    let css = compile(&assemble_reveal_scss(&[]).unwrap());
    // The Quarto rule `.reveal .slides { text-align: left }` must be present.
    assert!(
        css.contains("text-align: left"),
        "slides should be left aligned\n{css}"
    );
}

#[test]
fn title_slide_is_centered_and_resized() {
    let css = compile(&assemble_reveal_scss(&[]).unwrap());
    assert!(css.contains("#title-slide"), "title-slide rules present");
    assert!(
        css.contains("text-align: center"),
        "title slide centered\n{css}"
    );
}

// ── Stage B: built-in theme set + aliases ────────────────────────────────

#[test]
fn theme_name_aliases_resolve() {
    assert_eq!(resolve_reveal_theme_name("white"), Some("default"));
    assert_eq!(resolve_reveal_theme_name("black"), Some("dark"));
    assert_eq!(resolve_reveal_theme_name("default"), Some("default"));
    assert_eq!(resolve_reveal_theme_name("dark"), Some("dark"));
    assert_eq!(resolve_reveal_theme_name("dracula"), Some("dracula"));
    assert_eq!(resolve_reveal_theme_name("nope"), None);
}

#[test]
fn all_builtin_themes_compile() {
    // Every shipped theme must assemble + compile through grass.
    for name in REVEAL_BUILTIN_THEMES {
        let layer =
            load_reveal_theme_layer(name).unwrap_or_else(|e| panic!("load theme {name}: {e}"));
        let css = compile(&assemble_reveal_scss(&[layer]).unwrap());
        assert!(
            css.contains(".reveal-viewport"),
            "theme {name} should produce reveal base rules"
        );
        // No theme should leak reveal's uppercase default unless it opted in
        // (beige/league/moon/solarized/blood/sky set uppercase deliberately).
        let opts_in_uppercase = matches!(
            *name,
            "beige" | "league" | "moon" | "solarized" | "blood" | "sky"
        );
        if !opts_in_uppercase {
            assert!(
                !css.contains("uppercase"),
                "theme {name} should not uppercase headings"
            );
        }
    }
}

#[test]
fn dark_theme_sets_dark_background() {
    let layer = load_reveal_theme_layer("dark").unwrap();
    let css = compile(&assemble_reveal_scss(&[layer]).unwrap());
    assert!(
        css.contains("--r-background-color: #191919"),
        "dark theme should set the dark background\n{css}"
    );
    assert!(
        css.contains("--r-main-color: #fff"),
        "dark theme should use light text"
    );
    // `black` is an alias for `dark` — same output.
    let black =
        compile(&assemble_reveal_scss(&[load_reveal_theme_layer("black").unwrap()]).unwrap());
    assert!(black.contains("--r-background-color: #191919"));
}

#[test]
fn dracula_theme_distinctive_colors() {
    let layer = load_reveal_theme_layer("dracula").unwrap();
    let css = compile(&assemble_reveal_scss(&[layer]).unwrap());
    // Dracula background + purple headings + its custom-property effects.
    assert!(
        css.contains("--r-background-color: #282a36"),
        "dracula background\n{css}"
    );
    assert!(
        css.contains("--r-heading-color: #bd93f9"),
        "dracula purple headings"
    );
    assert!(
        css.contains("--r-bold-color: #ffb86c"),
        "dracula bold-color effect from its rules"
    );
}

#[test]
fn beige_theme_has_radial_background() {
    // The bodyBackground()/radial-gradient mixin was ported to a $background
    // gradient (reveal 6 dropped the mixin).
    let layer = load_reveal_theme_layer("beige").unwrap();
    let css = compile(&assemble_reveal_scss(&[layer]).unwrap());
    assert!(
        css.contains("--r-background: radial-gradient"),
        "beige should set a radial-gradient background\n{css}"
    );
}

#[test]
fn unknown_theme_errors() {
    assert!(load_reveal_theme_layer("no-such-theme").is_err());
}

// ── Stage C quick-wins (the ported quarto.scss systems) ──────────────────────

#[test]
fn quick_wins_compile_and_emit() {
    let css = compile(&assemble_reveal_scss(&[]).unwrap());

    // per-background text recoloring
    assert!(
        css.contains("section.has-dark-background"),
        "has-dark-background rule present"
    );
    assert!(
        css.contains("section.has-light-background"),
        "has-light-background rule present"
    );
    // code blocks: bordered + scrollable
    assert!(css.contains("div.sourceCode"), "sourceCode border rule");
    assert!(
        css.contains("max-height: 500px"),
        "code block scroll max-height\n{css}"
    );
    // blockquote restyle (left accent border, not italic-centered)
    assert!(
        css.contains("border-left: 0.25rem"),
        "blockquote accent border"
    );
    // .smaller system
    assert!(css.contains(".reveal.smaller"), "global .smaller rule");
    // kbd
    assert!(css.contains("kbd {") || css.contains("kbd{"), "kbd rule");
    // task lists
    assert!(css.contains("task-list"), "task-list rule");
    // code-font custom properties
    assert!(
        css.contains("--r-inline-code-font:"),
        "inline code font custom property"
    );
}

#[test]
fn callouts_compile_and_emit_per_type() {
    let css = compile(&assemble_reveal_scss(&[]).unwrap());
    assert!(css.contains(".reveal div.callout"), "callout base rules");
    assert!(
        css.contains(".callout-note") && css.contains(".callout-warning"),
        "per-type callout rules"
    );
    // Per-type icon as an inlined, URL-encoded SVG with the accent color.
    assert!(
        css.contains("background-image: url('data:image/svg+xml,"),
        "callout icon SVG data URI\n{}",
        &css[css.len().saturating_sub(600)..]
    );
    // The icon fill is recolored per type and URL-encoded (# → %23).
    assert!(css.contains("%23"), "callout icon color URL-encoded");
    assert!(css.contains("fill:"), "callout icon fill set");
    // callout-style-simple / -default specific rules present
    assert!(css.contains(".callout-style-simple"));
    assert!(css.contains(".callout-style-default"));
}

#[test]
fn shift_to_dark_picks_dark_value_on_dark_theme() {
    // kbd uses `shift_to_dark`, which depends on the slide background's
    // blackness. On the `dark` theme (bg #191919) the dark branch is chosen
    // (a shifted background), proving the helper + blackness threshold work.
    let dark = compile(&assemble_reveal_scss(&[load_reveal_theme_layer("dark").unwrap()]).unwrap());
    let light = compile(&assemble_reveal_scss(&[]).unwrap());
    // The kbd background-color differs between dark and light themes.
    let kbd_bg = |css: &str| {
        let i = css
            .find("kbd {")
            .or_else(|| css.find("kbd{"))
            .expect("kbd rule");
        let seg = &css[i..(i + 200).min(css.len())];
        seg.find("background-color:")
            .map(|j| seg[j..].split(';').next().unwrap_or("").to_string())
            .expect("kbd background-color")
    };
    assert_ne!(
        kbd_bg(&dark),
        kbd_bg(&light),
        "shift_to_dark should yield different kbd backgrounds on dark vs light themes"
    );
}
