/*
 * images.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Chapter-relative image resolution for the single-file book merge.
 */

//! Chapter-relative image targets in a merged single-file book.
//!
//! A chapter's image paths are written relative to the chapter's own
//! directory (`sub/two.qmd` with `![](local.png)` means `sub/local.png`).
//! After the merge, every chapter body sits in one document whose source
//! anchor is the project root, so those targets must be re-anchored
//! before anything downstream interprets them.
//!
//! This pass walks the merged document, tracks the current chapter's
//! `resourceDir` positionally from the merge's file-metadata markers
//! (as [`super::links`] does), and rewrites each relative image target
//! to the site-root form `/<project-relative path>`. That is the form
//! the existing static-resource machinery already understands:
//! `LinkRewriteTransform` turns it into the correct page-relative URL
//! and the resource collector copies the file to the matching place in
//! the output tree. Site-root (`/x.png`), external, and `data:` targets
//! are already anchored and are left alone.
//!
//! The vendored `resourcerefs.lua` skips images for single-file books
//! (it would otherwise prepend the chapter directory a second time).

use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;

use super::links::{block_marker_resource_dir, is_relative_ref, normalize_book_path};

/// Re-anchor every chapter-relative image target in a merged book
/// document at the project root, in place. Returns the number of
/// targets rewritten.
pub fn resolve_chapter_image_targets(ast: &mut Pandoc) -> usize {
    let mut current_dir: Option<String> = None;
    let mut rewritten = 0;
    for block in ast.blocks.iter_mut() {
        if let Some(dir) = block_marker_resource_dir(block) {
            current_dir = Some(dir);
            continue;
        }
        crate::ast_walk::for_each_inline_mut(std::slice::from_mut(block), &mut |inline| {
            if let Inline::Image(img) = inline
                && let Some(rooted) = rebase_image_target(&img.target.0, current_dir.as_deref())
            {
                img.target.0 = rooted;
                rewritten += 1;
            }
        });
    }
    rewritten
}

/// `Some("/<project-relative>")` when `target` is a chapter-relative
/// path that needs re-anchoring; `None` to leave it as written (already
/// anchored, external, empty, or escaping the project root).
fn rebase_image_target(target: &str, resource_dir: Option<&str>) -> Option<String> {
    if target.is_empty() || !is_relative_ref(target) {
        return None;
    }
    let joined = match resource_dir {
        Some(dir) => format!("{dir}/{target}"),
        None => target.to_string(),
    };
    // A path that climbs out of the project root has no project-relative
    // form; leave it for the downstream diagnostics.
    let mut depth: i32 = 0;
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => depth -= 1,
            _ => depth += 1,
        }
        if depth < 0 {
            return None;
        }
    }
    Some(format!("/{}", normalize_book_path(&joined)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebases_against_chapter_dir() {
        assert_eq!(
            rebase_image_target("local.png", Some("sub")).as_deref(),
            Some("/sub/local.png")
        );
        assert_eq!(
            rebase_image_target("../top.png", Some("sub")).as_deref(),
            Some("/top.png")
        );
        assert_eq!(
            rebase_image_target("img/a.png", Some(".")).as_deref(),
            Some("/img/a.png")
        );
        assert_eq!(
            rebase_image_target("a.png", None).as_deref(),
            Some("/a.png")
        );
    }

    #[test]
    fn leaves_anchored_and_external_targets() {
        for t in [
            "/img/a.png",
            "https://x.org/a.png",
            "data:image/png;base64,AA",
            "",
        ] {
            assert_eq!(rebase_image_target(t, Some("sub")), None, "{t}");
        }
    }

    #[test]
    fn leaves_targets_escaping_the_project_root() {
        assert_eq!(rebase_image_target("../../x.png", Some("sub")), None);
    }
}
