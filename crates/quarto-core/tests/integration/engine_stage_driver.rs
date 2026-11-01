/*
 * tests/integration/engine_stage_driver.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Shared driver for tests of `EngineExecutionStage`'s observer surface (EXR-5): run
 * the HTML pipeline up through `engine-execution` on one document with a
 * caller-supplied observer and engine registry. Mirrors `record_capture`
 * (`engine/preview_record.rs`), whose collector is private and always wants records.
 */

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use quarto_core::Format;
use quarto_core::engine::EngineRegistry;
use quarto_core::pipeline::build_html_pipeline_stages_with_options;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::record::{Record, read_record_value};
use quarto_core::stage::stages::ENGINE_RECORD_KIND;
use quarto_core::stage::{
    LoadedSource, Pipeline, PipelineData, PipelineError, PipelineObserver, StageContext,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Records every aux event and answers `wants_execution_record` as configured.
pub struct AuxRecorder {
    wants_record: bool,
    events: Mutex<Vec<(usize, String, Value)>>,
}

impl AuxRecorder {
    pub fn arc(wants_record: bool) -> Arc<Self> {
        Arc::new(Self {
            wants_record,
            events: Mutex::new(Vec::new()),
        })
    }

    pub fn events_of(&self, kind: &str) -> Vec<(usize, Value)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, k, _)| k == kind)
            .map(|(i, _, v)| (*i, v.clone()))
            .collect()
    }

    /// The one coalesced record the stage emitted.
    pub fn record(&self) -> Record {
        let events = self.events_of(ENGINE_RECORD_KIND);
        assert_eq!(events.len(), 1, "one coalesced record per run: {events:?}");
        read_record_value(events[0].1.clone()).expect("the payload is a record notebook")
    }
}

impl PipelineObserver for AuxRecorder {
    fn on_auxiliary_data(&self, _stage: &str, index: usize, kind: &str, data: &Value) {
        self.events
            .lock()
            .unwrap()
            .push((index, kind.to_string(), data.clone()));
    }

    fn wants_execution_record(&self) -> bool {
        self.wants_record
    }
}

/// Run the HTML pipeline up through `engine-execution` on `input` with `observer`
/// watching and `registry` (when given) standing in for the default engines.
pub fn run_engine_stage(
    input: &Path,
    observer: Arc<dyn PipelineObserver>,
    registry: Option<Arc<EngineRegistry>>,
) -> Result<(), PipelineError> {
    run_engine_stage_ast(input, observer, registry).map(|_| ())
}

/// As [`run_engine_stage`], returning the document the stage produced (the executed
/// markdown, parsed).
pub fn run_engine_stage_ast(
    input: &Path,
    observer: Arc<dyn PipelineObserver>,
    registry: Option<Arc<EngineRegistry>>,
) -> Result<pampa::pandoc::Pandoc, PipelineError> {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(input, runtime.as_ref()).expect("project discovery");
    let content = runtime.file_read(input).expect("read the input");
    let document = DocumentInfo::from_path(input);
    let format = Format::from_format_string("html").expect("html format");

    let mut ctx = StageContext::new(runtime, format, project, document)?.with_observer(observer);
    if let Some(registry) = registry {
        ctx.registry = registry;
    }
    let mut stages = build_html_pipeline_stages_with_options(None);
    if let Some(idx) = stages.iter().position(|s| s.name() == "engine-execution") {
        stages.truncate(idx + 1);
    }
    let pipeline = Pipeline::new(stages).expect("pipeline stages are compatible");
    let data = PipelineData::LoadedSource(LoadedSource::new(input.to_path_buf(), content));
    match pollster::block_on(pipeline.run(data, &mut ctx))? {
        PipelineData::DocumentAst(doc) => Ok(doc.ast),
        _ => panic!("the engine stage ends in a DocumentAst"),
    }
}
