/*
 * transforms/image_dimensions.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Turn an image's `width` / `height` key-values into valid HTML.
//!
//! The HTML `width` and `height` attributes take a bare non-negative
//! integer (a pixel count). Authors write `{width=60%}`, `{width=750px}`,
//! `{width=3in}`, and pampa's HTML writer prints key-values verbatim, so
//! without this pass q2 emitted `width="60%"` / `width="750px"`: invalid
//! HTML that browsers happen to tolerate (bd-html-image-dims-t5ogxjxd).
//!
//! Quarto 1 gets valid output from Pandoc's HTML writer
//! (`imgAttrsToHtml` / `dimensionsToAttrList`, `Writers/HTML.hs`): a
//! pixel or unitless value stays an attribute, floored to an integer;
//! anything with a unit moves into `style`, appended after the author's
//! own `style`. This pass does the same, with one deliberate difference:
//!
//! - **Values Pandoc cannot parse are kept, not dropped.** Pandoc only
//!   knows `px`, `cm`, `mm`, `in`, `pt`, `pc`, `%` and `em`, and silently
//!   deletes anything else (`auto`, `50vw`, `calc(…)`). Here the rule is
//!   simply "only `N` / `Npx` stays an attribute": every other value
//!   moves into `style` exactly as written, so `width:50vw` works and a
//!   unit CSS adds later works without a code change. Writing the value
//!   verbatim also means `60%` stays `60%` rather than Pandoc's `60.0%`.
//!
//! **Why a transform, not the writer.** Pandoc lifts at write time, after
//! every filter. Doing it here, in the Finalization Phase, means it runs
//! inside `AstTransformsStage` and therefore *before*
//! `UserFiltersStage::post`: a Lua post filter sees the lifted form
//! (`style="width:60%"`, `width="750"`) and can still change it. A post
//! filter that writes `width="60%"` back gets exactly what it wrote; the
//! writer stays a faithful attribute printer. This diverges from Quarto 1,
//! where every filter saw the author's raw values; the Lua filter guide
//! documents it.
//!
//! **Placement:** last of the content transforms in the Finalization
//! Phase. `responsive-image` and revealjs `auto-stretch` decide on the
//! *presence* of the `height` / `width` key-values, and `llms-capture`
//! should keep the author's `{width=60%}` in its markdown, so all of them
//! must see the unlifted attr. Quarto 1 runs those decisions before the
//! writer too, so this is the parity order.
//!
//! Gated to the HTML family (html, revealjs, dashboard, and their preview
//! pseudo-formats). Pandoc-writer formats do their own sizing. The
//! q2-preview runs this pass as well, and its `Image` component applies
//! the `style` key-value, so a filter sees the same AST in preview and
//! render.
//!
//! Scope is `Image` only, like Pandoc: `Div` / `Span` widths (revealjs
//! columns, for example) are untouched.

use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

use super::image_walk::for_each_image_mut;

/// The dimension keys, in the order their declarations are appended to
/// `style` (Pandoc's order).
const DIMENSIONS: [&str; 2] = ["width", "height"];

const STYLE_ATTR: &str = "style";

pub struct ImageDimensionsTransform;

impl ImageDimensionsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ImageDimensionsTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ImageDimensionsTransform {
    fn name(&self) -> &str {
        "image-dimensions"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !ctx.format.identifier.is_html_based() {
            return Ok(());
        }
        for_each_image_mut(&mut ast.blocks, &mut |img| lift_dimensions(&mut img.attr));
        Ok(())
    }
}

/// Rewrite one image's `width` / `height` into valid HTML.
///
/// `N` / `Npx` becomes the integer attribute `floor(N)`; any other
/// non-empty value is removed and appended to `style` as
/// `<dim>:<value>`. An empty value is left alone. Idempotent.
pub(crate) fn lift_dimensions(attr: &mut Attr) {
    // Values are updated through `get_mut`, never re-`insert`ed:
    // `LinkedHashMap::insert` on an existing key moves it to the back,
    // which would reorder the printed attributes on every run.
    let mut lifted: Vec<String> = Vec::new();
    for dim in DIMENSIONS {
        let Some(raw) = attr.2.get_mut(dim) else {
            continue;
        };
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        match pixel_count(value) {
            Some(px) => *raw = px,
            None => {
                lifted.push(format!("{dim}:{value}"));
                attr.2.remove(dim);
            }
        }
    }
    if lifted.is_empty() {
        return;
    }
    match attr.2.get_mut(STYLE_ATTR) {
        Some(existing) => {
            let kept = existing.trim().trim_end_matches(';').trim_end();
            let mut declarations: Vec<String> = Vec::new();
            if !kept.is_empty() {
                declarations.push(kept.to_string());
            }
            declarations.extend(lifted);
            *existing = declarations.join(";");
        }
        None => {
            attr.2.insert(STYLE_ATTR.to_string(), lifted.join(";"));
        }
    }
}

/// `Some(integer string)` when `value` is a pixel length: a non-negative
/// decimal number, optionally followed by `px` (any case). Fractional
/// pixels are floored, as Pandoc does (`750.7px` → `750`).
fn pixel_count(value: &str) -> Option<String> {
    let number = match value.len().checked_sub(2) {
        Some(split)
            if value.is_char_boundary(split) && value[split..].eq_ignore_ascii_case("px") =>
        {
            &value[..split]
        }
        _ => value,
    };
    let (int_part, frac_part) = match number.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (number, None),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    // At least one digit overall; `.` alone, `5.` and `.5` are numbers
    // (`.5` floors to 0), `1.2.3` is not.
    let valid = digits(int_part)
        && frac_part.is_none_or(digits)
        && (!int_part.is_empty() || frac_part.is_some_and(|f| !f.is_empty()));
    if !valid {
        return None;
    }
    let int_part = int_part.trim_start_matches('0');
    Some(if int_part.is_empty() {
        "0".to_string()
    } else {
        int_part.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;

    fn attr(kvs: &[(&str, &str)]) -> Attr {
        let mut map = LinkedHashMap::new();
        for (k, v) in kvs {
            map.insert(k.to_string(), v.to_string());
        }
        (String::new(), vec![], map).into()
    }

    fn lifted(kvs: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut a = attr(kvs);
        lift_dimensions(&mut a);
        a.2.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn percent_moves_to_style_verbatim() {
        assert_eq!(lifted(&[("width", "60%")]), kv(&[("style", "width:60%")]));
        assert_eq!(
            lifted(&[("width", "60.0%")]),
            kv(&[("style", "width:60.0%")]),
            "values are copied as written, not reformatted"
        );
    }

    #[test]
    fn px_and_unitless_stay_attributes_as_integers() {
        assert_eq!(lifted(&[("width", "750px")]), kv(&[("width", "750")]));
        assert_eq!(lifted(&[("width", "750PX")]), kv(&[("width", "750")]));
        assert_eq!(lifted(&[("width", "750")]), kv(&[("width", "750")]));
        assert_eq!(lifted(&[("width", " 750 ")]), kv(&[("width", "750")]));
        assert_eq!(lifted(&[("width", "750.7px")]), kv(&[("width", "750")]));
        assert_eq!(lifted(&[("height", "200.5")]), kv(&[("height", "200")]));
        assert_eq!(lifted(&[("height", ".5")]), kv(&[("height", "0")]));
        assert_eq!(lifted(&[("height", "0")]), kv(&[("height", "0")]));
        assert_eq!(lifted(&[("height", "007")]), kv(&[("height", "7")]));
    }

    #[test]
    fn other_units_move_to_style() {
        for v in [
            "3in", "2.5cm", "40mm", "12pt", "3pc", "20em", "50vw", "10rem",
        ] {
            assert_eq!(
                lifted(&[("width", v)]),
                kv(&[("style", &format!("width:{v}"))]),
                "{v}"
            );
        }
    }

    #[test]
    fn values_pandoc_would_drop_are_kept_in_style() {
        for v in [
            "auto",
            "calc(100% - 2em)",
            "fit-content",
            "-5",
            "1.2.3",
            "px",
        ] {
            assert_eq!(
                lifted(&[("width", v)]),
                kv(&[("style", &format!("width:{v}"))]),
                "{v}"
            );
        }
    }

    #[test]
    fn both_dimensions_lift_in_width_then_height_order() {
        assert_eq!(
            lifted(&[("height", "2in"), ("width", "50%")]),
            kv(&[("style", "width:50%;height:2in")])
        );
        assert_eq!(
            lifted(&[("width", "50%"), ("height", "300px")]),
            kv(&[("height", "300"), ("style", "width:50%")])
        );
    }

    #[test]
    fn appends_after_existing_style_keeping_its_position() {
        assert_eq!(
            lifted(&[
                ("style", "border: 1px solid"),
                ("width", "50%"),
                ("alt", "x")
            ]),
            kv(&[("style", "border: 1px solid;width:50%"), ("alt", "x")])
        );
        assert_eq!(
            lifted(&[("style", "border: 1px solid; "), ("width", "50%")]),
            kv(&[("style", "border: 1px solid;width:50%")]),
            "a trailing `;` must not produce `;;`"
        );
        assert_eq!(
            lifted(&[("style", "  "), ("width", "50%")]),
            kv(&[("style", "width:50%")])
        );
    }

    #[test]
    fn style_untouched_when_nothing_lifts() {
        assert_eq!(
            lifted(&[("style", "border: 1px solid;"), ("width", "10px")]),
            kv(&[("style", "border: 1px solid;"), ("width", "10")])
        );
    }

    #[test]
    fn empty_value_is_left_alone() {
        assert_eq!(lifted(&[("width", "")]), kv(&[("width", "")]));
    }

    #[test]
    fn idempotent() {
        let mut a = attr(&[
            ("style", "color:red;"),
            ("width", "50%"),
            ("height", "20px"),
        ]);
        lift_dimensions(&mut a);
        let once = a.clone();
        lift_dimensions(&mut a);
        assert_eq!(a, once);
    }

    #[test]
    fn unrelated_attributes_are_untouched() {
        assert_eq!(
            lifted(&[("fig-align", "center"), ("data-width", "60%")]),
            kv(&[("fig-align", "center"), ("data-width", "60%")])
        );
    }
}
