/*
 * project/listing/placeholders.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Stable hex tokens for description / image placeholders, shared
//! between the L3 listing render transform (which emits them) and
//! the L7 post-render upgrade (which consumes them).
//!
//! These come verbatim from Q1's `website-listing-template.ts` so
//! the L7 regex can be ported as-is when L7 is implemented.

/// Hex token used in description placeholder comments:
/// `<!-- desc(5A0113B34292)[max=<n>]:<output-href> -->`
pub const DESC_TOKEN: &str = "5A0113B34292";

/// Hex token used in image placeholder comments:
/// `<!-- img(9CEB782EFEE6)[<attrs>]:<id>:<output-href> -->`
pub const IMG_TOKEN: &str = "9CEB782EFEE6";

/// Build a description-placeholder comment for a given listing id,
/// max-description-length, and target output href. The L7 step
/// matches this exact shape with a regex.
pub fn description_placeholder(_listing_id: &str, max_length: u32, output_href: &str) -> String {
    // Q1 carries the listing id outside the parens (in the
    // surrounding markup); the placeholder's parentheses always
    // contain the magic token. We mirror that contract.
    format!(
        "<!-- desc({})[max={}]:{} -->",
        DESC_TOKEN, max_length, output_href
    )
}

/// Build an image-placeholder comment. `attrs` is the
/// already-rendered `<img>` attributes the L7 substitution should
/// preserve (e.g. `class="thumbnail-image" loading="lazy"`).
pub fn image_placeholder(item_index: usize, output_href: &str, attrs: &str) -> String {
    format!(
        "<!-- img({})[{}]:{}:{} -->",
        IMG_TOKEN, attrs, item_index, output_href
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_placeholder_matches_q1_shape() {
        let s = description_placeholder("my-listing", 175, "posts/foo.html");
        assert_eq!(s, "<!-- desc(5A0113B34292)[max=175]:posts/foo.html -->");
    }

    #[test]
    fn image_placeholder_matches_q1_shape() {
        let s = image_placeholder(0, "posts/foo.html", r#"class="thumbnail-image""#);
        assert_eq!(
            s,
            r#"<!-- img(9CEB782EFEE6)[class="thumbnail-image"]:0:posts/foo.html -->"#
        );
    }
}
