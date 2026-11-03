---
title: "revealjs: extension formats take the Reveal.js path"
date: 2026-11-03
description: "Make is_revealjs_target recognize extension formats whose base is revealjs (GH #1034)."
---

Strand: `bd-eco47h7r` · Issue: <https://github.com/quarto-dev/q2/issues/1034>

## Overview

**Before.** `is_revealjs_target` (`crates/quarto-core/src/format.rs`) matched
only `revealjs` and `q2-slides`. A Reveal.js format extension such as
`demo-revealjs` therefore fell into the Bootstrap branch of
`CompileThemeCssStage` (`compile_theme_css.rs`): a reveal theme name like
`default` failed with Q-14-2, and without `theme` the deck never registered
reveal.js and its CSS. The other callers (`metadata_merge.rs`,
`panel_tabset.rs`, `responsive_image.rs`, `announcement_render.rs`,
`draft_alert.rs`, `Format::lua_format`) had the same blind spot.

**After.** `is_revealjs_target` also accepts any target whose
`parse_format_descriptor` base format is `revealjs`, the same mapping
`PipelineProfile::from_format` already applies. Every caller picks this up.

## Work items

- [x] Unit test `test_is_revealjs_target_extension_formats` (fails before).
- [x] Integration test `revealjs_extension_format_uses_reveal_theme_and_assets`
      (fails before with Q-14-2).
- [x] Extend `is_revealjs_target`; refresh the gate comment in the theme stage.
- [x] Verify: quarto-core nextest, clippy (host + Windows cross), CLI repro.
