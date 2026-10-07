//! Inputs to `PandocWriteStage::prepare()` that differ between hosts.

use std::path::PathBuf;

use super::types::RequestPost;

/// Host-dependent inputs to `prepare()`.
///
/// Native renders leave `RenderContext::prepare_options` unset and `prepare()`
/// uses [`PrepareOptions::native`] (the per-render temp dir, no
/// `SOURCE_DATE_EPOCH`, no resource bytes). The wasm entry point sets it
/// before the pipeline runs: the temp root must be fixed up front because
/// absolute temp paths are baked into `QUARTO_FILTER_PARAMS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareOptions {
    /// Root of the per-render scratch files (input JSON, dependency file,
    /// the share tree under `<temp_root>/pandoc-share`). On wasm the
    /// constant share root.
    pub temp_root: PathBuf,
    /// Set as `SOURCE_DATE_EPOCH` in the request env when `Some` (click
    /// time in production, fixed in tests); natively `None`.
    pub source_date_epoch: Option<i64>,
    /// Copy resource bytes into the request (`resource_refs`) and add `/tmp`
    /// to `dirs`; natively the files already exist where pandoc runs.
    pub collect_resources: bool,
    /// The `typst-available-fonts` filter param (typst formats only): what
    /// the host's typst compiler can load, in place of `typst fonts`.
    /// `None` leaves the param unset, which the Lua consumer treats as
    /// permissive.
    pub typst_available_fonts: Option<Vec<String>>,
    /// What the host runs after pandoc. `CompileTypst` (the `pdf` request)
    /// also collects what the compile reads and pandoc does not: typst
    /// images and brand fonts and logos go into `resource_refs`. A `.typ`
    /// download leaves them out, since they would only count against the
    /// size limits.
    pub post: RequestPost,
}

impl PrepareOptions {
    /// What a native render uses: `temp_root` is the pipeline's temp dir.
    pub fn native(temp_root: PathBuf) -> Self {
        Self {
            temp_root,
            source_date_epoch: None,
            collect_resources: false,
            typst_available_fonts: None,
            post: RequestPost::None,
        }
    }
}
