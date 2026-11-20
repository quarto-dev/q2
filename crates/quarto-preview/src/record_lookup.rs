//! The preview's [`RecordLookup`] (RIU-2): what `Cache`, `Q2` and `Engine` mean when the preview
//! is the one asking.
//!
//! - `Cache`: the content-keyed capture cache under `<data_dir>/captures/` (key from
//!   `inputs.input_qmd`, so the pre-engine pipeline has run once for the document).
//! - `Q2`: the `.quarto/execution-records/<doc>.ipynb` copy a `record: true` render writes,
//!   at the path the writer uses ([`quarto_core::record::execution_record_path`]).
//! - `Engine`: run the engine now and store the result in the cache. The preview publishes the
//!   run itself, so it returns [`EngineResult::Run`].
//!
//! `provide-hub` implements the trait differently (its engine step is `run_and_store`), in RIU-4.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::{CaptureRun, SourceInputs, record_capture_run};
use quarto_core::project::ProjectContext;
use quarto_core::record::{
    EngineError, EngineResult, LookupError, RecordLookup, RecordSource, execution_record_path,
    read_record,
};
use quarto_system_runtime::SystemRuntime;

use crate::cache::{lookup_cached_run, session_cache_key, store_run};

pub struct PreviewLookup<'a> {
    pub abs_path: &'a Path,
    pub project: &'a ProjectContext,
    pub runtime: Arc<dyn SystemRuntime>,
    pub registry: Option<Arc<EngineRegistry>>,
    pub cache_dir: &'a Path,
}

#[async_trait(?Send)]
impl RecordLookup for PreviewLookup<'_> {
    async fn lookup(
        &self,
        source: &RecordSource,
        inputs: &SourceInputs,
    ) -> Result<Option<CaptureRun>, LookupError> {
        match source {
            RecordSource::Cache => Ok(lookup_cached_run(
                self.cache_dir,
                &session_cache_key(&inputs.input_qmd),
            )),
            RecordSource::Q2 => {
                let path = execution_record_path(&self.project.dir, self.abs_path);
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(e) => {
                        return Err(LookupError::Unreadable(format!(
                            "reading {}: {e}",
                            path.display()
                        )));
                    }
                };
                let record = read_record(&text).map_err(|e| {
                    LookupError::Unreadable(format!("{} is not a record: {e}", path.display()))
                })?;
                Ok(Some(CaptureRun {
                    captures: Vec::new(),
                    record: Some(record),
                }))
            }
            // The walk answers these itself; asking here is a bug in the caller.
            RecordSource::Positron | RecordSource::Dir(_) | RecordSource::Engine => Ok(None),
        }
    }

    async fn run_engine(&self, inputs: &SourceInputs) -> Result<EngineResult, EngineError> {
        let run = record_capture_run(
            self.abs_path,
            self.project,
            self.runtime.clone(),
            self.registry.clone(),
        )
        .await
        .map_err(|e| EngineError::Failed(e.to_string()))?;
        let run = overlay_free(run, inputs).map_err(EngineError::Failed)?;
        // An engine run is stored whenever there is a cache directory, whether or not `Cache` is
        // in the list: storing is what makes the next lookup cheap.
        let key = session_cache_key(&inputs.input_qmd);
        store_run(self.cache_dir, &key, &run);
        Ok(EngineResult::Run(run))
    }
}

/// Make a run executed under the session's `-M` overlay describe the source, as `provide-hub`
/// does (F8-D9): the staleness check recomputes on the overlay-free document, so the stored
/// hashes must be overlay-free too. The outputs stay as the overlay produced them. A session
/// without `-M` returns the run untouched.
pub(crate) fn overlay_free(run: CaptureRun, source: &SourceInputs) -> Result<CaptureRun, String> {
    match crate::render_flags::get().metadata {
        Some(metadata) => quarto_core::record::overlay_free_run(
            run,
            source,
            &quarto_core::overlay_runtime::RenderOverlay::with_metadata(metadata),
        ),
        None => Ok(run),
    }
}
