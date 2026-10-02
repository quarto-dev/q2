//! R1 contract: the published JSON Schema, the hand-written golden request and
//! the Rust `PandocRequest` type agree.

use quarto_core::pandoc_request::{PandocRequest, REQUEST_SCHEMA_VERSION, RequestFile};
use serde_json::Value;

const SCHEMA: &str = include_str!("../../schemas/pandoc-request.schema.json");
const GOLDEN: &str = include_str!("../../schemas/pandoc-request.golden.json");
const CONSTANTS: &str = include_str!("../../../../resources/pandoc-wasm.json");

fn golden_value() -> Value {
    serde_json::from_str(GOLDEN).unwrap()
}

fn validate(instance: &Value) -> Vec<String> {
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("schema compiles");
    validator
        .iter_errors(instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect()
}

#[test]
fn golden_validates_against_the_schema() {
    let errors = validate(&golden_value());
    assert!(errors.is_empty(), "golden is invalid: {errors:#?}");
}

#[test]
fn golden_round_trips_through_the_rust_type() {
    let request: PandocRequest = serde_json::from_str(GOLDEN).unwrap();
    assert_eq!(request.schema_version, REQUEST_SCHEMA_VERSION);
    assert_eq!(serde_json::to_value(&request).unwrap(), golden_value());
}

#[test]
fn golden_job_id_is_the_computed_one() {
    let request: PandocRequest = serde_json::from_str(GOLDEN).unwrap();
    assert_eq!(request.job_id, request.compute_job_id());
}

#[test]
fn golden_pins_the_shared_constants() {
    let constants: Value = serde_json::from_str(CONSTANTS).unwrap();
    let request: PandocRequest = serde_json::from_str(GOLDEN).unwrap();
    assert_eq!(request.share_root, constants["share_root"]);
    assert_eq!(
        request.expected_pandoc_wasm_sha256,
        constants["wasm_sha256"].as_str().unwrap()
    );
    assert_eq!(request.argv[0], "pandoc");
    assert_eq!(
        request.share_tree_path,
        format!("{}/pandoc-share", request.share_root)
    );
    assert_eq!(request.env["QUARTO_SHARE_PATH"], request.share_tree_path);
}

#[test]
fn schema_rejects_malformed_requests() {
    let mutate = |f: &dyn Fn(&mut Value)| {
        let mut v = golden_value();
        f(&mut v);
        validate(&v)
    };
    assert!(!mutate(&|v| v["schema_version"] = 2.into()).is_empty());
    assert!(!mutate(&|v| v["argv"][0] = "pandoc.exe".into()).is_empty());
    assert!(!mutate(&|v| v["output_path"] = r"C:\proj\a.docx".into()).is_empty());
    assert!(!mutate(&|v| v["output_path"] = "proj/a.docx".into()).is_empty());
    assert!(!mutate(&|v| v["job_id"] = "XYZ".into()).is_empty());
    assert!(!mutate(&|v| v["extra"] = 1.into()).is_empty());
    assert!(
        !mutate(&|v| {
            v.as_object_mut().unwrap().remove("share_tree_version");
        })
        .is_empty()
    );
    // A native Windows request may use `X:/` paths.
    assert!(mutate(&|v| v["output_path"] = "C:/proj/a.docx".into()).is_empty());
}

#[test]
fn job_id_ignores_source_date_epoch_but_not_inputs() {
    let base: PandocRequest = serde_json::from_str(GOLDEN).unwrap();
    let id = base.compute_job_id();

    let mut other = base.clone();
    other.env.insert("SOURCE_DATE_EPOCH".into(), "1".into());
    assert_eq!(other.compute_job_id(), id);

    let mut other = base.clone();
    other.env.insert("EXTRA".into(), "1".into());
    assert_ne!(other.compute_job_id(), id);

    let mut other = base.clone();
    other.argv.push("--standalone".into());
    assert_ne!(other.compute_job_id(), id);

    let mut other = base.clone();
    other.files[0].bytes.push(b' ');
    assert_ne!(other.compute_job_id(), id);

    let mut other = base.clone();
    other.resource_refs[0] = RequestFile {
        path: other.resource_refs[0].path.clone(),
        bytes: b"different".to_vec(),
    };
    assert_ne!(other.compute_job_id(), id);

    let mut other = base.clone();
    other.share_tree_version = "0".repeat(64);
    assert_ne!(other.compute_job_id(), id);
}

mod transport {
    use std::path::PathBuf;
    use std::sync::Arc;

    use quarto_core::format::Format;
    use quarto_core::pandoc_request::PrepareOptions;
    use quarto_core::pipeline::{build_pandoc_pipeline_stages, run_pipeline};
    use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use quarto_core::render::{BinaryDependencies, RenderContext};
    use quarto_core::stage::PipelineStage;
    use quarto_core::stage::stages::{PandocPrepareStage, ParseDocumentStage};

    fn project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
            ..Default::default()
        }
    }

    /// The real pre-write stage list with `PandocPrepareStage` in place of
    /// `PandocWriteStage`.
    async fn run(
        options: Option<PrepareOptions>,
    ) -> Option<quarto_core::pandoc_request::PandocRequest> {
        let project = project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::docx();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.prepare_options = options;
        let mut stages = build_pandoc_pipeline_stages(quarto_core::format::FormatIdentifier::Docx);
        stages.pop();
        stages.push(Box::new(PandocPrepareStage::new()));
        let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
        run_pipeline(b"# Hi\n", "doc.qmd", &mut ctx, runtime, stages)
            .await
            .expect("pipeline runs");
        ctx.pandoc_request.take()
    }

    #[tokio::test]
    async fn request_crosses_the_stage_context_bridge() {
        let request = run(None).await.expect("PandocPrepareStage left a request");
        assert_eq!(request.writer, "docx");
        assert_eq!(request.argv[0], "pandoc");
        assert_eq!(request.job_id, request.compute_job_id());
    }

    #[tokio::test]
    async fn prepare_options_reach_prepare_through_the_bridge() {
        let request = run(Some(PrepareOptions {
            temp_root: PathBuf::from("/__q2_share__"),
            source_date_epoch: Some(1_700_000_000),
            collect_resources: true,
        }))
        .await
        .expect("request");
        assert_eq!(request.share_root, "/__q2_share__");
        assert_eq!(request.env["SOURCE_DATE_EPOCH"], "1700000000");
        assert!(request.dirs.iter().any(|d| d == "/tmp"));
    }

    #[tokio::test]
    async fn prepare_options_fix_the_temp_dir_before_any_stage_asks() {
        use quarto_core::stage::{PipelineData, PipelineDataKind, PipelineError, StageContext};

        struct TempDirProbe(Arc<std::sync::Mutex<Option<PathBuf>>>);
        #[async_trait::async_trait(?Send)]
        impl PipelineStage for TempDirProbe {
            fn name(&self) -> &str {
                "temp-dir-probe"
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
                *self.0.lock().unwrap() = Some(ctx.temp_dir()?.to_path_buf());
                Ok(input)
            }
        }

        let project = project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::docx();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.prepare_options = Some(PrepareOptions {
            temp_root: PathBuf::from("/__q2_share__"),
            source_date_epoch: Some(1_700_000_000),
            collect_resources: true,
        });
        let seen = Arc::new(std::sync::Mutex::new(None));
        let stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(ParseDocumentStage::new()),
            Box::new(TempDirProbe(seen.clone())),
        ];
        let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let _ = run_pipeline(b"# Hi\n", "doc.qmd", &mut ctx, runtime, stages).await;
        assert_eq!(
            seen.lock().unwrap().as_deref(),
            Some(std::path::Path::new("/__q2_share__"))
        );
    }
}
