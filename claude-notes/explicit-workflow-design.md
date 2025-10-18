# Explicit Workflow Design for Quarto Rendering

**Date:** 2025-10-12
**Purpose:** Design data structures and systems to make rendering dependencies explicit
**Status:** Draft

## Executive Summary

The current Quarto-CLI rendering system has **implicit data dependencies** that exist through file I/O and global state:
- Intermediate markdown written to disk, then read by next stage
- Execution results frozen to `_freeze/`, read on subsequent renders
- Navigation state built in pre-render, accessed during per-file rendering
- Search index/sitemap updated post-render by reading all HTML outputs

This analysis proposes **explicit workflow representations** that would enable:
1. **Parallelization**: Safe concurrent execution where dependencies allow
2. **Reconfiguration**: Users can reorder pipeline stages (e.g., filters before engines)
3. **Caching**: Automatic memoization based on input changes
4. **Debugging**: Clear visibility into what depends on what

## Core Concept: Workflow as DAG of Steps

A rendering workflow is a **directed acyclic graph (DAG)** where:
- **Nodes** are processing steps (engine execution, pandoc, postprocessing)
- **Edges** are data dependencies (step B needs output from step A)
- **Artifacts** are the data flowing between steps

```rust
pub struct Workflow {
    steps: HashMap<StepId, Step>,
    dependencies: HashMap<StepId, Vec<StepId>>,
}

pub struct Step {
    id: StepId,
    kind: StepKind,
    inputs: Vec<Artifact>,
    outputs: Vec<Artifact>,
    executor: Box<dyn StepExecutor>,
}

pub enum Artifact {
    Markdown(MarkdownArtifact),
    Html(HtmlArtifact),
    Metadata(MetadataArtifact),
    Resources(ResourcesArtifact),
    Index(SearchIndexArtifact),
}
```

### Example: Single Document Rendering as DAG

```
┌─────────────────┐
│ ParseYAML       │
│ input: doc.qmd  │
│ output: Meta    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌──────────────────┐
│ ValidateYAML    │     │ ExtractMarkdown  │
│ input: Meta     │     │ input: doc.qmd   │
│ output: Meta'   │     │ output: Markdown │
└────────┬────────┘     └────────┬─────────┘
         │                       │
         └───────┬───────────────┘
                 ▼
         ┌──────────────────┐
         │ ExecuteEngine    │
         │ input: Markdown  │
         │ output: Markdown'│
         └────────┬─────────┘
                  │
                  ▼
         ┌──────────────────┐
         │ HandleLanguages  │
         │ input: Markdown' │
         │ output: Markdown"│
         └────────┬─────────┘
                  │
                  ▼
         ┌──────────────────┐
         │ RunPandoc        │
         │ input: Markdown" │
         │ output: HTML     │
         └────────┬─────────┘
                  │
                  ▼
         ┌──────────────────┐
         │ PostprocessHTML  │
         │ input: HTML      │
         │ output: HTML'    │
         └──────────────────┘
```

This DAG makes dependencies explicit:
- `ExecuteEngine` cannot run until both `ValidateYAML` and `ExtractMarkdown` complete
- `PostprocessHTML` must wait for `RunPandoc`
- No other implicit ordering constraints

## Data Structure Design

### Step Identification

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StepId(uuid::Uuid);

impl StepId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
```

### Step Kinds (Coarse-Grained)

```rust
pub enum StepKind {
    // Input processing
    ParseYAML,
    ValidateYAML,
    ExtractMarkdown,

    // Metadata operations
    MergeMetadata,
    ResolveFormats,

    // Execution
    ExecuteEngine { engine: String },

    // Language handlers
    HandleLanguageCells { stage: HandlerStage },

    // Pandoc
    RunPandoc { format: String },

    // Postprocessing
    PostprocessHTML { postprocessor: String },
    EnginePostprocess,

    // Project-level
    BuildNavigation,
    GenerateSitemap,
    GenerateSearchIndex,

    // Custom (for extensions)
    Custom { name: String },
}

pub enum HandlerStage {
    PreEngine,
    PostEngine,
}
```

### Artifacts (Typed Data)

```rust
pub enum Artifact {
    // File-based
    SourceFile(PathBuf),
    OutputFile(PathBuf),

    // Structured data
    Markdown(MarkdownArtifact),
    Metadata(MetadataArtifact),
    Html(HtmlArtifact),
    ExecuteResult(ExecuteResultArtifact),

    // Collections
    Resources(Vec<PathBuf>),
    SupportingFiles(Vec<PathBuf>),

    // Project-level
    NavigationState(NavigationArtifact),
    SearchIndex(SearchIndexArtifact),
    Sitemap(SitemapArtifact),

    // Caching
    FrozenExecution(PathBuf),
}

pub struct MarkdownArtifact {
    pub content: String,
    pub source_info: SourceInfo,
}

pub struct MetadataArtifact {
    pub metadata: Metadata,
    pub source_info: SourceInfo,
}

pub struct HtmlArtifact {
    pub content: String,
    pub supporting: Vec<PathBuf>,
    pub resources: Vec<PathBuf>,
}

pub struct ExecuteResultArtifact {
    pub markdown: String,
    pub supporting: Vec<PathBuf>,
    pub filters: Vec<String>,
    pub includes: PandocIncludes,
    pub dependencies: EngineDependencies,
}

pub struct NavigationArtifact {
    pub navbar: Option<Navbar>,
    pub sidebars: Vec<Sidebar>,
    pub footer: Option<Footer>,
}

pub struct SearchIndexArtifact {
    pub entries: Vec<SearchEntry>,
}
```

### Step Execution

```rust
#[async_trait]
pub trait StepExecutor: Send + Sync {
    /// Execute this step with given inputs
    async fn execute(
        &self,
        inputs: Vec<Artifact>,
        context: &ExecutionContext,
    ) -> Result<Vec<Artifact>>;

    /// Can this step be cached?
    fn cacheable(&self) -> bool {
        true
    }

    /// Compute cache key from inputs
    fn cache_key(&self, inputs: &[Artifact]) -> Option<String> {
        None
    }
}

pub struct ExecutionContext {
    pub project: Option<ProjectContext>,
    pub flags: RenderFlags,
    pub temp_dir: PathBuf,
}
```

### Workflow Builder

```rust
pub struct WorkflowBuilder {
    steps: HashMap<StepId, Step>,
    dependencies: HashMap<StepId, Vec<StepId>>,
}

impl WorkflowBuilder {
    pub fn new() -> Self {
        Self {
            steps: HashMap::new(),
            dependencies: HashMap::new(),
        }
    }

    pub fn add_step(&mut self, step: Step) -> StepId {
        let id = step.id;
        self.steps.insert(id, step);
        self.dependencies.insert(id, Vec::new());
        id
    }

    pub fn add_dependency(&mut self, dependent: StepId, dependency: StepId) {
        self.dependencies
            .entry(dependent)
            .or_default()
            .push(dependency);
    }

    pub fn build(self) -> Result<Workflow> {
        // Validate: no cycles
        self.check_cycles()?;

        Ok(Workflow {
            steps: self.steps,
            dependencies: self.dependencies,
        })
    }

    fn check_cycles(&self) -> Result<()> {
        // Tarjan's algorithm or similar
        // ...
        Ok(())
    }
}
```

### Workflow Execution

```rust
pub struct WorkflowExecutor {
    workflow: Workflow,
    parallelism: usize,
}

impl WorkflowExecutor {
    pub async fn execute(&self, context: ExecutionContext) -> Result<WorkflowResult> {
        // Topological sort to find execution order
        let execution_order = self.topological_sort()?;

        // Group independent steps for parallel execution
        let execution_groups = self.group_independent_steps(&execution_order);

        // Storage for intermediate artifacts
        let mut artifact_store: HashMap<StepId, Vec<Artifact>> = HashMap::new();

        // Execute groups sequentially, steps within group in parallel
        for group in execution_groups {
            let tasks: Vec<_> = group.iter().map(|&step_id| {
                let step = &self.workflow.steps[&step_id];
                let inputs = self.collect_inputs(step, &artifact_store);
                let context = context.clone();

                async move {
                    let outputs = step.executor.execute(inputs, &context).await?;
                    Ok::<_, Error>((step_id, outputs))
                }
            }).collect();

            // Execute all steps in group concurrently
            let results = futures::future::try_join_all(tasks).await?;

            // Store outputs
            for (step_id, outputs) in results {
                artifact_store.insert(step_id, outputs);
            }
        }

        Ok(WorkflowResult {
            artifacts: artifact_store,
        })
    }

    fn topological_sort(&self) -> Result<Vec<StepId>> {
        // Kahn's algorithm
        let mut in_degree: HashMap<StepId, usize> = HashMap::new();

        // Calculate in-degrees
        for step_id in self.workflow.steps.keys() {
            in_degree.insert(*step_id, 0);
        }
        for deps in self.workflow.dependencies.values() {
            for dep in deps {
                *in_degree.get_mut(dep).unwrap() += 1;
            }
        }

        // Process nodes with in-degree 0
        let mut queue: VecDeque<StepId> = in_degree
            .iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut sorted = Vec::new();

        while let Some(step_id) = queue.pop_front() {
            sorted.push(step_id);

            // Reduce in-degree of dependents
            if let Some(deps) = self.workflow.dependencies.get(&step_id) {
                for dep_id in deps {
                    let degree = in_degree.get_mut(dep_id).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(*dep_id);
                    }
                }
            }
        }

        if sorted.len() != self.workflow.steps.len() {
            return Err(anyhow!("Cycle detected in workflow"));
        }

        Ok(sorted)
    }

    fn group_independent_steps(&self, sorted: &[StepId]) -> Vec<Vec<StepId>> {
        let mut groups = Vec::new();
        let mut remaining: HashSet<StepId> = sorted.iter().copied().collect();
        let mut completed: HashSet<StepId> = HashSet::new();

        while !remaining.is_empty() {
            let mut group = Vec::new();

            // Find all steps whose dependencies are satisfied
            for &step_id in sorted {
                if remaining.contains(&step_id) {
                    let deps = &self.workflow.dependencies[&step_id];
                    if deps.iter().all(|dep| completed.contains(dep)) {
                        group.push(step_id);
                    }
                }
            }

            if group.is_empty() {
                panic!("Cannot make progress - cycle detected");
            }

            // Move group from remaining to completed
            for &step_id in &group {
                remaining.remove(&step_id);
                completed.insert(step_id);
            }

            groups.push(group);
        }

        groups
    }

    fn collect_inputs(
        &self,
        step: &Step,
        artifact_store: &HashMap<StepId, Vec<Artifact>>,
    ) -> Vec<Artifact> {
        let deps = &self.workflow.dependencies[&step.id];

        deps.iter()
            .flat_map(|dep_id| {
                artifact_store.get(dep_id).cloned().unwrap_or_default()
            })
            .collect()
    }
}

pub struct WorkflowResult {
    pub artifacts: HashMap<StepId, Vec<Artifact>>,
}
```

## Example: Single Document Workflow Construction

```rust
pub fn build_single_document_workflow(
    input_file: &Path,
    format: &Format,
) -> Result<Workflow> {
    let mut builder = WorkflowBuilder::new();

    // Step 1: Parse YAML front matter
    let parse_yaml = builder.add_step(Step {
        id: StepId::new(),
        kind: StepKind::ParseYAML,
        inputs: vec![Artifact::SourceFile(input_file.to_path_buf())],
        outputs: vec![Artifact::Metadata(/* placeholder */)],
        executor: Box::new(ParseYAMLExecutor),
    });

    // Step 2: Validate YAML
    let validate_yaml = builder.add_step(Step {
        id: StepId::new(),
        kind: StepKind::ValidateYAML,
        inputs: vec![/* metadata from parse_yaml */],
        outputs: vec![Artifact::Metadata(/* placeholder */)],
        executor: Box::new(ValidateYAMLExecutor),
    });
    builder.add_dependency(validate_yaml, parse_yaml);

    // Step 3: Extract markdown
    let extract_md = builder.add_step(Step {
        id: StepId::new(),
        kind: StepKind::ExtractMarkdown,
        inputs: vec![Artifact::SourceFile(input_file.to_path_buf())],
        outputs: vec![Artifact::Markdown(/* placeholder */)],
        executor: Box::new(ExtractMarkdownExecutor),
    });

    // Step 4: Execute engine
    let execute = builder.add_step(Step {
        id: StepId::new(),
        kind: StepKind::ExecuteEngine {
            engine: "jupyter".to_string(),
        },
        inputs: vec![/* markdown + metadata */],
        outputs: vec![Artifact::ExecuteResult(/* placeholder */)],
        executor: Box::new(JupyterEngineExecutor),
    });
    builder.add_dependency(execute, validate_yaml);
    builder.add_dependency(execute, extract_md);

    // Step 5: Handle language cells
    let handle_langs = builder.add_step(Step {
        id: StepId::new(),
        kind: StepKind::HandleLanguageCells {
            stage: HandlerStage::PostEngine,
        },
        inputs: vec![/* execute result */],
        outputs: vec![Artifact::Markdown(/* placeholder */)],
        executor: Box::new(LanguageCellHandler),
    });
    builder.add_dependency(handle_langs, execute);

    // Step 6: Run Pandoc
    let run_pandoc = builder.add_step(Step {
        id: StepId::new(),
        kind: StepKind::RunPandoc {
            format: format.identifier.target_format.clone(),
        },
        inputs: vec![/* markdown + metadata */],
        outputs: vec![Artifact::Html(/* placeholder */)],
        executor: Box::new(PandocExecutor),
    });
    builder.add_dependency(run_pandoc, handle_langs);

    // Step 7: Postprocess HTML
    let postprocess = builder.add_step(Step {
        id: StepId::new(),
        kind: StepKind::PostprocessHTML {
            postprocessor: "bootstrap".to_string(),
        },
        inputs: vec![/* html */],
        outputs: vec![Artifact::Html(/* placeholder */)],
        executor: Box::new(BootstrapPostprocessor),
    });
    builder.add_dependency(postprocess, run_pandoc);

    builder.build()
}
```

## Example: Website Project Workflow

```rust
pub fn build_website_workflow(
    project: &ProjectContext,
) -> Result<Workflow> {
    let mut builder = WorkflowBuilder::new();

    // Pre-render: Build navigation (runs once)
    let build_nav = builder.add_step(Step {
        id: StepId::new(),
        kind: StepKind::BuildNavigation,
        inputs: vec![],
        outputs: vec![Artifact::NavigationState(/* placeholder */)],
        executor: Box::new(BuildNavigationExecutor),
    });

    // For each input file: create rendering workflow
    let mut file_workflows = Vec::new();
    for input_file in &project.files.input {
        let file_workflow = build_single_document_workflow(input_file, &format)?;

        // First step of file workflow depends on navigation
        let first_step = file_workflow.find_initial_steps()[0];
        builder.add_dependency(first_step, build_nav);

        file_workflows.push(file_workflow);
    }

    // Collect all final steps (HTML outputs)
    let html_outputs: Vec<StepId> = file_workflows
        .iter()
        .flat_map(|w| w.find_final_steps())
        .collect();

    // Post-render: Generate sitemap (depends on all HTML outputs)
    let gen_sitemap = builder.add_step(Step {
        id: StepId::new(),
        kind: StepKind::GenerateSitemap,
        inputs: vec![/* all HTML artifacts */],
        outputs: vec![Artifact::Sitemap(/* placeholder */)],
        executor: Box::new(GenerateSitemapExecutor),
    });
    for &html_output in &html_outputs {
        builder.add_dependency(gen_sitemap, html_output);
    }

    // Post-render: Generate search index (depends on all HTML outputs)
    let gen_search = builder.add_step(Step {
        id: StepId::new(),
        kind: StepKind::GenerateSearchIndex,
        inputs: vec![/* all HTML artifacts */],
        outputs: vec![Artifact::SearchIndex(/* placeholder */)],
        executor: Box::new(GenerateSearchIndexExecutor),
    });
    for &html_output in &html_outputs {
        builder.add_dependency(gen_search, html_output);
    }

    builder.build()
}
```

## Parallelization Analysis

With explicit dependencies, the executor can identify parallelization opportunities:

### Single Document (Limited Parallelism)

```
ParseYAML ────┐
              ├──> ValidateYAML ──┐
ExtractMd ────┘                   ├──> ExecuteEngine ──> ... ──> Output
                                  │
                                  └──> (Must wait)
```

**Parallelism**: `ParseYAML` and `ExtractMd` can run concurrently, but most pipeline is sequential.

### Website (High Parallelism)

```
BuildNavigation
      │
      ├──> File1: Parse ──> Execute ──> Pandoc ──> HTML1 ──┐
      │                                                     │
      ├──> File2: Parse ──> Execute ──> Pandoc ──> HTML2 ──┤
      │                                                     ├──> GenerateSitemap
      ├──> File3: Parse ──> Execute ──> Pandoc ──> HTML3 ──┤
      │                                                     ├──> GenerateSearchIndex
      └──> File4: Parse ──> Execute ──> Pandoc ──> HTML4 ──┘
```

**Parallelism**: All files can render concurrently (up to CPU limit), then post-render runs once.

### Website with Cross-References (Medium Parallelism)

```
BuildNavigation
      │
      ├──> File1 ──> HTML1 ──┐
      │                      │
      ├──> File2 ──> HTML2 ──┼──> (File3 references File1)
      │                      │
      └──> File3 ──> HTML3 ──┼──> GenerateSitemap
                             │
                             └──> GenerateSearchIndex
```

**Parallelism**: File1 and File2 can render concurrently, File3 must wait for File1.

## Reconfiguration: User-Specified Pipeline Order

### Default Pipeline

```rust
pub fn default_rendering_pipeline() -> Vec<StepKind> {
    vec![
        StepKind::ParseYAML,
        StepKind::ValidateYAML,
        StepKind::ExtractMarkdown,
        StepKind::HandleLanguageCells { stage: HandlerStage::PreEngine },
        StepKind::ExecuteEngine { engine: "auto".into() },
        StepKind::HandleLanguageCells { stage: HandlerStage::PostEngine },
        StepKind::RunPandoc { format: "auto".into() },
        StepKind::PostprocessHTML { postprocessor: "auto".into() },
    ]
}
```

### User Configuration (Filters Before Engine)

User wants to run Lua filters to inject code cells before engine execution:

```yaml
# _quarto.yml
pipeline:
  order:
    - parse-yaml
    - validate-yaml
    - extract-markdown
    - run-filters:  # NEW: Run filters early
        filters:
          - inject-cells.lua
    - execute-engine
    - handle-language-cells
    - run-pandoc
    - postprocess-html
```

```rust
pub fn build_custom_pipeline(config: &PipelineConfig) -> Result<Workflow> {
    let mut builder = WorkflowBuilder::new();
    let mut prev_step: Option<StepId> = None;

    for stage_config in &config.order {
        let step = match stage_config {
            StageConfig::ParseYAML => {
                builder.add_step(Step {
                    kind: StepKind::ParseYAML,
                    // ...
                })
            }
            StageConfig::RunFilters { filters } => {
                // NEW: Allow filters at any point in pipeline
                builder.add_step(Step {
                    kind: StepKind::RunPandocFilters {
                        filters: filters.clone(),
                    },
                    // ...
                })
            }
            // ... other stages
        };

        // Add dependency on previous step
        if let Some(prev) = prev_step {
            builder.add_dependency(step, prev);
        }

        prev_step = Some(step);
    }

    builder.build()
}
```

### Example: Filters Before and After Engine

```yaml
pipeline:
  order:
    - parse-yaml
    - extract-markdown
    - run-filters:  # Pre-engine filters
        filters:
          - inject-cells.lua
          - modify-metadata.lua
    - execute-engine
    - run-filters:  # Post-engine filters
        filters:
          - process-outputs.lua
    - run-pandoc
```

This enables advanced use cases like:
- Injecting code cells dynamically based on metadata
- Pre-processing markdown before engine sees it
- Post-processing engine outputs before Pandoc
- Inserting custom transformation stages

## Caching and Memoization

With explicit artifacts and dependencies, automatic caching becomes possible:

```rust
pub struct CachingWorkflowExecutor {
    inner: WorkflowExecutor,
    cache: Arc<dyn Cache>,
}

#[async_trait]
impl Cache for FileSystemCache {
    async fn get(&self, key: &str) -> Option<Vec<Artifact>> {
        let path = self.cache_dir.join(key);
        if path.exists() {
            let data = tokio::fs::read(&path).await.ok()?;
            bincode::deserialize(&data).ok()
        } else {
            None
        }
    }

    async fn put(&self, key: &str, artifacts: &[Artifact]) -> Result<()> {
        let path = self.cache_dir.join(key);
        let data = bincode::serialize(artifacts)?;
        tokio::fs::write(&path, data).await?;
        Ok(())
    }
}

impl CachingWorkflowExecutor {
    async fn execute_step(
        &self,
        step: &Step,
        inputs: Vec<Artifact>,
        context: &ExecutionContext,
    ) -> Result<Vec<Artifact>> {
        // Check if step is cacheable
        if !step.executor.cacheable() {
            return step.executor.execute(inputs, context).await;
        }

        // Compute cache key
        let cache_key = step.executor.cache_key(&inputs)
            .unwrap_or_else(|| {
                // Default: hash step kind + input artifacts
                let mut hasher = blake3::Hasher::new();
                hasher.update(format!("{:?}", step.kind).as_bytes());
                for artifact in &inputs {
                    hasher.update(&bincode::serialize(artifact).unwrap());
                }
                hasher.finalize().to_hex().to_string()
            });

        // Check cache
        if let Some(cached) = self.cache.get(&cache_key).await {
            return Ok(cached);
        }

        // Execute and cache
        let outputs = step.executor.execute(inputs, context).await?;
        self.cache.put(&cache_key, &outputs).await?;

        Ok(outputs)
    }
}
```

This enables:
- **Engine execution caching**: Same as current `freeze` system, but automatic
- **Pandoc caching**: If markdown + format unchanged, reuse output
- **Postprocessor caching**: If HTML unchanged, skip postprocessing
- **Incremental project renders**: Only re-render changed files

## Error Handling and Debugging

Explicit workflows enable better error messages:

```rust
impl WorkflowExecutor {
    pub async fn execute_with_tracing(
        &self,
        context: ExecutionContext,
    ) -> Result<WorkflowResult> {
        let mut trace = ExecutionTrace::new();

        for group in self.execution_groups() {
            for step_id in group {
                let step = &self.workflow.steps[&step_id];

                trace.start_step(step_id, step.kind.clone());

                match self.execute_step(step, &context).await {
                    Ok(outputs) => {
                        trace.complete_step(step_id, outputs);
                    }
                    Err(e) => {
                        trace.fail_step(step_id, e.clone());

                        // Print dependency chain
                        eprintln!("Error in step: {:?}", step.kind);
                        eprintln!("Dependency chain:");
                        for ancestor in self.ancestors(step_id) {
                            let ancestor_step = &self.workflow.steps[&ancestor];
                            eprintln!("  <- {:?}", ancestor_step.kind);
                        }

                        return Err(e);
                    }
                }
            }
        }

        Ok(WorkflowResult { trace })
    }
}
```

Example error message:

```
Error: Pandoc execution failed
  Step: RunPandoc { format: "html" }
  Input artifacts:
    - Markdown (2847 lines)
    - Metadata (23 keys)

  Dependency chain:
    <- HandleLanguageCells { stage: PostEngine }
    <- ExecuteEngine { engine: "jupyter" }
    <- ExtractMarkdown
    <- ParseYAML

  To debug:
    1. Check intermediate markdown: /tmp/workflow-abc123/step-5-output.md
    2. Run pandoc manually: pandoc /tmp/workflow-abc123/step-5-output.md
    3. Enable tracing: QUARTO_TRACE=1 quarto render
```

## Integration with Current Architecture

### Compatibility Layer

For gradual migration, provide a compatibility layer:

```rust
pub struct LegacyRenderer {
    workflow_executor: WorkflowExecutor,
}

impl LegacyRenderer {
    pub async fn render(
        &self,
        input: &Path,
        options: &RenderOptions,
    ) -> Result<RenderedFile> {
        // Build workflow from legacy configuration
        let workflow = build_single_document_workflow(input, &options.format)?;

        // Execute workflow
        let result = self.workflow_executor.execute(ExecutionContext {
            project: options.project.clone(),
            flags: options.flags.clone(),
            temp_dir: options.temp.clone(),
        }).await?;

        // Extract final HTML artifact and convert to RenderedFile
        let html_artifact = result.artifacts
            .values()
            .find_map(|artifacts| {
                artifacts.iter().find_map(|a| match a {
                    Artifact::Html(html) => Some(html),
                    _ => None,
                })
            })
            .ok_or_else(|| anyhow!("No HTML output found"))?;

        Ok(RenderedFile {
            input: input.to_path_buf(),
            file: html_artifact.output_path.clone(),
            supporting: html_artifact.supporting.clone(),
            resources: html_artifact.resources.clone(),
            // ...
        })
    }
}
```

### Extension API

Allow extensions to contribute workflow steps:

```rust
pub trait WorkflowExtension {
    fn extend_workflow(
        &self,
        builder: &mut WorkflowBuilder,
        context: &ExtensionContext,
    ) -> Result<()>;
}

// Example: Custom diagram renderer
pub struct MermaidExtension;

impl WorkflowExtension for MermaidExtension {
    fn extend_workflow(
        &self,
        builder: &mut WorkflowBuilder,
        context: &ExtensionContext,
    ) -> Result<()> {
        // Find the "HandleLanguageCells" step
        let handler_step = context.find_step(StepKind::HandleLanguageCells {
            stage: HandlerStage::PostEngine,
        })?;

        // Insert a custom mermaid rendering step before it
        let mermaid_step = builder.add_step(Step {
            id: StepId::new(),
            kind: StepKind::Custom {
                name: "render-mermaid".to_string(),
            },
            executor: Box::new(MermaidRenderer),
            // ...
        });

        // Update dependencies
        builder.add_dependency(handler_step, mermaid_step);

        Ok(())
    }
}
```

## Benefits Summary

### Parallelization
- **Automatic**: Executor identifies independent steps
- **Safe**: Dependencies prevent race conditions
- **Configurable**: Control parallelism level

### Reconfiguration
- **User control**: Specify pipeline order in config
- **Advanced use cases**: Filters before/after engines
- **Extension points**: Third parties can insert steps

### Caching
- **Transparent**: Based on input hashes
- **Incremental**: Only re-run changed steps
- **Efficient**: Reuse expensive computations

### Debugging
- **Visibility**: See full dependency chain
- **Tracing**: Track execution flow
- **Isolation**: Test individual steps

### Testing
- **Unit tests**: Test step executors independently
- **Integration tests**: Test workflow construction
- **Mocking**: Replace steps with test doubles

## Implementation Phases

### Phase 1: Core Infrastructure
1. Define `Step`, `Artifact`, `Workflow` types
2. Implement `WorkflowBuilder`
3. Implement `WorkflowExecutor` (sequential first)
4. Add basic error handling

### Phase 2: Single Document Rendering
1. Port single document pipeline to workflow
2. Implement step executors (parse, execute, pandoc, etc.)
3. Test against current quarto-cli output
4. Add compatibility layer

### Phase 3: Parallelization
1. Implement parallel execution in `WorkflowExecutor`
2. Add dependency analysis
3. Test with website projects (multiple files)
4. Benchmark performance improvements

### Phase 4: Caching
1. Design cache key generation
2. Implement `Cache` trait and file-based implementation
3. Add `CachingWorkflowExecutor`
4. Integrate with existing freeze system

### Phase 5: Reconfiguration
1. Design pipeline configuration format
2. Implement custom workflow builder
3. Add validation and error checking
4. Document advanced use cases

### Phase 6: Extensions
1. Design extension API
2. Implement workflow extension points
3. Create example extensions
4. Document extension development

## Open Questions

1. **Artifact Serialization**: How to efficiently serialize/cache arbitrary artifacts?
   - Option A: Require all artifacts to be `serde::Serialize`
   - Option B: Use trait objects with custom serialization
   - Option C: Only cache file-based artifacts

2. **Incremental Execution**: How to handle file changes?
   - Track file modification times?
   - Use content hashing (like Bazel)?
   - Let users specify cache invalidation rules?

3. **Error Recovery**: Should workflows support retries?
   - Retry individual steps?
   - Retry entire workflow?
   - User-configurable retry policies?

4. **Distributed Execution**: Could workflows run across machines?
   - Remote execution of steps?
   - Distributed caching?
   - Coordination protocol?

5. **Dynamic Workflows**: Should workflow structure be mutable?
   - Add steps during execution?
   - Conditional step execution?
   - Loop constructs?

## Related Work

- **Bazel/Buck**: Build systems with explicit dependencies and caching
- **Apache Airflow**: Workflow orchestration with DAGs
- **Luigi**: Python workflow framework
- **Dask**: Parallel computing with task graphs
- **Makefiles**: Classic dependency-based execution

## Conclusion

Explicit workflow representation transforms Quarto's rendering from implicit sequential execution to a flexible, parallel, cacheable system. The DAG-based approach enables:

1. **Performance**: Automatic parallelization where safe
2. **Flexibility**: User-configurable pipeline order
3. **Reliability**: Clear error messages and debugging
4. **Efficiency**: Smart caching and incremental updates

This design provides a solid foundation for the Rust port while enabling features difficult or impossible in the current TypeScript implementation.
