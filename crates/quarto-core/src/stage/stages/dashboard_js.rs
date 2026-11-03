/*
 * dashboard_js.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pipeline stage that ships the dashboard runtime scripts.
 */

//! Ships the scripts a `format: dashboard` page needs, in Quarto 1's order
//! (`format-dashboard.ts:200-283`):
//!
//! 1. `quarto-dashboard.js` — page and hash navigation, widget fill,
//!    sticky table headers;
//! 2. `stickythead.js` — the sticky-header helper it calls;
//! 3. bslib `web-components.js` (`<bslib-tooltip>`, …), an ES module;
//! 4. bslib `components.js` — card full screen, sidebar collapse.
//!
//! They are linked after every other page script (`link_order` 10–13; the
//! other `js:*` artifacts use the default 0). Native-only, like
//! [`super::TabsetsJsStage`]: the hub preview drops `ApplyTemplateStage`,
//! so the artifacts would never become tags there. DataTables is not
//! shipped (deferred, bd-6f0kslxu). Plan:
//! `claude-notes/plans/2026-11-03-dashboard-format-port.md`, Phase 2.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::artifact::{Artifact, ArtifactScope};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// One dashboard script: artifact key, file name, bytes, link order and
/// whether it is an ES module.
struct DashboardScript {
    key: &'static str,
    filename: &'static str,
    bytes: &'static [u8],
    order: i32,
    module: bool,
}

const DASHBOARD_SCRIPTS: &[DashboardScript] = &[
    DashboardScript {
        key: "js:dashboard",
        filename: "quarto-dashboard.js",
        bytes: include_bytes!("../../../../../resources/js/dashboard/quarto-dashboard.js"),
        order: 10,
        module: false,
    },
    DashboardScript {
        key: "js:dashboard-stickythead",
        filename: "stickythead.js",
        bytes: include_bytes!("../../../../../resources/js/dashboard/stickythead.js"),
        order: 11,
        module: false,
    },
    DashboardScript {
        key: "js:bslib-web-components",
        filename: "web-components.js",
        bytes: include_bytes!("../../../../../resources/js/bslib/web-components.js"),
        order: 12,
        module: true,
    },
    DashboardScript {
        key: "js:bslib-components",
        filename: "components.js",
        bytes: include_bytes!("../../../../../resources/js/bslib/components.js"),
        order: 13,
        module: false,
    },
];

pub struct DashboardJsStage;

impl DashboardJsStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DashboardJsStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for DashboardJsStage {
    fn name(&self) -> &str {
        "dashboard-js"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        if ctx.format.base_format() != "dashboard" {
            return Ok(input);
        }
        for script in DASHBOARD_SCRIPTS {
            let path = if ctx.project.is_single_file {
                PathBuf::from(script.filename)
            } else {
                PathBuf::from(format!("quarto/{}", script.filename))
            };
            let mut artifact = Artifact::from_bytes(script.bytes.to_vec(), "text/javascript")
                .with_path(path)
                .with_scope(ArtifactScope::Project)
                .with_link_order(script.order);
            if script.module {
                artifact = artifact.with_link_attribs([("type", "module")]);
            }
            ctx.artifacts.store(script.key, artifact);
        }
        trace_event!(
            ctx,
            EventLevel::Debug,
            "shipped {} dashboard scripts",
            DASHBOARD_SCRIPTS.len()
        );
        Ok(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_ordered_and_only_web_components_is_a_module() {
        let orders: Vec<i32> = DASHBOARD_SCRIPTS.iter().map(|s| s.order).collect();
        assert!(orders.windows(2).all(|w| w[0] < w[1]), "{orders:?}");
        let modules: Vec<&str> = DASHBOARD_SCRIPTS
            .iter()
            .filter(|s| s.module)
            .map(|s| s.filename)
            .collect();
        assert_eq!(modules, ["web-components.js"]);
        for s in DASHBOARD_SCRIPTS {
            assert!(!s.bytes.is_empty(), "{} is empty", s.filename);
        }
    }
}
