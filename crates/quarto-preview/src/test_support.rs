//! Shared test support (RIU-2): a recording engine that counts executions, for tests that need
//! a record (the passthrough engines build none) across `cache` and `capture_driver`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError,
};

/// A recording engine that counts executions (the passthrough engines build no record).
pub(crate) struct CountingRecordingEngine {
    calls: Arc<AtomicUsize>,
}

impl ExecutionEngine for CountingRecordingEngine {
    fn name(&self) -> &str {
        "test-recording"
    }
    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        use quarto_core::record::{EngineEntryInput, RecordBuilder, UnitResult};
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut result = ExecuteResult::passthrough(input);
        if ctx.want_record {
            let fail = |e: &dyn std::fmt::Display| {
                ExecutionError::execution_failed("test-recording", e.to_string())
            };
            let def = self.hash_definition();
            let mut b = RecordBuilder::new(self.name(), &def, &ctx.units, &ctx.author_texts)
                .map_err(|e| fail(&e))?;
            for (i, unit) in ctx.units.iter().enumerate() {
                if ctx.owned_languages.iter().any(|l| l == unit.lang()) {
                    b.push(i, UnitResult::default()).map_err(|e| fail(&e))?;
                }
            }
            result.record = Some(
                b.finish(EngineEntryInput {
                    owned_languages: ctx.owned_languages.clone(),
                    document_metadata: ctx.document_metadata_json(),
                    ..Default::default()
                })
                .map_err(|e| fail(&e))?,
            );
        }
        Ok(result)
    }
    fn claims_language(
        &self,
        language: &str,
        _first_class: Option<&str>,
    ) -> quarto_core::engine::LanguageClaim {
        if language == "test-recording" {
            quarto_core::engine::LanguageClaim::Primary(1)
        } else {
            quarto_core::engine::LanguageClaim::None
        }
    }
}

pub(crate) fn counting_recording_registry() -> (Arc<EngineRegistry>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut reg = EngineRegistry::new();
    reg.register(Arc::new(CountingRecordingEngine {
        calls: calls.clone(),
    }));
    (Arc::new(reg), calls)
}

/// A document with one cell of the recording engine.
pub(crate) const RECORDING_DOC: &str =
    "---\nengine: test-recording\n---\n\n```{test-recording}\nSENTINEL\n```\n";
