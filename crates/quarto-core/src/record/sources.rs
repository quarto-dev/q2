//! Record sources (RIU-2): an ordered list of places a document's record can come from, and the
//! first-wins walk over it.
//!
//! Target-neutral (no I/O of its own): the walk calls a [`RecordLookup`] that the preview and
//! `provide-hub` each implement, because only what depends on *who is asking* differs (the
//! preview reads its cache and the `.quarto` copy and publishes a run itself; the provider's
//! engine step is `run_and_store`, which publishes as it runs). The Positron and directory
//! sources do not depend on the caller; their arms arrive with RIU-3a.
//!
//! The rules the walk encodes (epic decisions 3 to 5 and the fourth-review answers):
//!
//! - first source that **supplies** wins, with no merging; a source supplies only a run that
//!   [has content](CaptureRun::has_content);
//! - a run the gate-off writer could not publish ([`record_has_no_legacy_form`]) is **refused**
//!   with a message naming `QUARTO_ENGINE_RECORD_WRITE`, and the walk tries the next source;
//! - the engine runs only when `engine` is in the list **and** the policy allows it;
//! - the walk reports one outcome per source it considered, so a caller can say why nothing
//!   was found instead of "nothing found".

use std::path::PathBuf;

use async_trait::async_trait;

use crate::engine::capture_files::{PublishError, record_has_no_legacy_form};
use crate::engine::preview_record::{CaptureRun, SourceInputs};

/// One place a record can come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordSource {
    /// The preview's content-keyed capture cache.
    Cache,
    /// The `.quarto/execution-records/` copy a `record: true` render writes.
    Q2,
    /// The Positron inline-output cache (RIU-3a).
    Positron,
    /// A directory of Positron-format cache files (RIU-3a).
    Dir(PathBuf),
    /// Run the engine now.
    Engine,
}

impl RecordSource {
    /// The name a user types and a diagnostic prints.
    pub fn label(&self) -> String {
        match self {
            RecordSource::Cache => "cache".into(),
            RecordSource::Q2 => "q2".into(),
            RecordSource::Positron => "positron".into(),
            RecordSource::Dir(p) => p.display().to_string(),
            RecordSource::Engine => "engine".into(),
        }
    }
}

/// Why a `execute: record-sources` entry was not understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSourceError(pub String);

impl std::fmt::Display for ParseSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            return write!(
                f,
                "empty record source: use one of `cache`, `q2`, `positron`, `engine`, or a \
                 directory path"
            );
        }
        write!(
            f,
            "unknown record source `{0}`: use one of `cache`, `q2`, `positron`, `engine`, or a \
             directory path (starting with `/`, `./`, `../` or `~`, or containing a path \
             separator; write `./{0}` for a directory named `{0}`). A path containing a comma \
             cannot be given, because a comma separates entries",
            self.0
        )
    }
}

impl std::error::Error for ParseSourceError {}

/// The three classification rules of `execute: record-sources` (RIU-3a, kept by RIU-4): a known name, else a
/// path-like entry is a directory, else an error. No validation (the directory need not exist
/// here), no defaults and no conflicts; RIU-4 adds those.
impl std::str::FromStr for RecordSource {
    type Err = ParseSourceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cache" => Ok(RecordSource::Cache),
            "q2" => Ok(RecordSource::Q2),
            "positron" => Ok(RecordSource::Positron),
            "engine" => Ok(RecordSource::Engine),
            _ if is_path_like(s) => Ok(RecordSource::Dir(PathBuf::from(s))),
            _ => Err(ParseSourceError(s.to_string())),
        }
    }
}

fn is_path_like(s: &str) -> bool {
    !s.is_empty()
        && (s.starts_with('/')
            || s.starts_with("./")
            || s.starts_with("../")
            || s.starts_with('~')
            || s.contains('/')
            || s.contains('\\')
            || std::path::Path::new(s).is_absolute())
}

/// An ordered list of [`RecordSource`]s. Position is priority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSources(Vec<RecordSource>);

impl RecordSources {
    pub fn new(sources: Vec<RecordSource>) -> Self {
        Self(sources)
    }

    /// The preview default, `cache,q2,engine` (epic decisions 3 and 4).
    pub fn preview_default() -> Self {
        Self(vec![
            RecordSource::Cache,
            RecordSource::Q2,
            RecordSource::Engine,
        ])
    }

    /// The `provide-hub` default, `engine`: its engine step is always a fresh run.
    pub fn provider_default() -> Self {
        Self(vec![RecordSource::Engine])
    }

    pub fn iter(&self) -> impl Iterator<Item = &RecordSource> {
        self.0.iter()
    }

    pub fn as_slice(&self) -> &[RecordSource] {
        &self.0
    }

    /// Whether `engine` is in the list. Re-execute and the walk both ask this.
    pub fn contains_engine(&self) -> bool {
        self.0.contains(&RecordSource::Engine)
    }
}

/// Start-up checks on a parsed list (native only: they touch the filesystem).
#[cfg(not(target_arch = "wasm32"))]
impl RecordSources {
    /// Check every directory source once, at command start: expand a leading `~` to the home
    /// directory and require an existing directory. A relative path is left as given and so
    /// resolves against the working directory of the command. The parser never does this
    /// (it classifies by text alone).
    pub fn validate_directories(self) -> Result<Self, String> {
        let mut out = Vec::with_capacity(self.0.len());
        for source in self.0 {
            out.push(match source {
                RecordSource::Dir(p) => RecordSource::Dir(check_directory(p)?),
                other => other,
            });
        }
        Ok(Self(out))
    }

    /// `provide-hub` has no preview cache directory, so `cache` is a start-up error there.
    pub fn reject_cache(&self, command: &str) -> Result<(), String> {
        if self.0.contains(&RecordSource::Cache) {
            return Err(format!(
                "`cache` in `execute: record-sources` is not available for `{command}`: it has no preview cache \
                 directory. Use `q2`, `positron`, a directory path, or `engine`"
            ));
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn check_directory(path: PathBuf) -> Result<PathBuf, String> {
    let text = path.to_string_lossy().into_owned();
    let expanded = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            let home = dirs::home_dir().ok_or_else(|| {
                format!("record source `{text}`: no home directory to expand `~`")
            })?;
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => path,
    };
    if !expanded.exists() {
        return Err(format!(
            "record source `{}` does not exist (a relative path is resolved against the \
             working directory)",
            expanded.display()
        ));
    }
    if !expanded.is_dir() {
        return Err(format!(
            "record source `{}` is a file, not a directory",
            expanded.display()
        ));
    }
    Ok(expanded)
}

/// A lookup failed in a way the walk reports and steps past.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LookupError {
    /// The source exists but could not be read.
    #[error("{0}")]
    Unreadable(String),
}

/// Running the engine failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Failed(String),
}

/// What [`RecordLookup::run_engine`] hands back.
#[derive(Debug)]
pub enum EngineResult {
    /// A run for the caller to publish through the seam (the preview).
    Run(CaptureRun),
    /// The implementation already published it (the provider, whose engine step is
    /// `run_and_store`).
    Published,
}

/// The two operations whose meaning depends on who is asking. Both take the document's
/// [`SourceInputs`], so the pre-engine pipeline runs once per document and a cache key derives
/// from `inputs.input_qmd`.
#[async_trait(?Send)]
pub trait RecordLookup {
    /// The `Cache` and `Q2` sources: a run stored for this document, or `None`.
    async fn lookup(
        &self,
        source: &RecordSource,
        inputs: &SourceInputs,
    ) -> Result<Option<CaptureRun>, LookupError>;

    /// The `Engine` source: run it now.
    async fn run_engine(&self, inputs: &SourceInputs) -> Result<EngineResult, EngineError>;
}

/// What the `positron` and directory sources need to find and convert a document's cache
/// (RIU-3a). The caller must also have asked `compute_source_inputs_with` for the ownership map.
#[derive(Debug, Clone, Copy)]
pub struct ImportContext<'a> {
    /// The document's absolute path, which the locator turns into a `file://` URI.
    pub abs_path: &'a std::path::Path,
    /// The project-relative path recorded as the record's `quarto_source`.
    pub source: Option<&'a str>,
    /// Replaces the host's default Positron directories for the `positron` source (a test or
    /// an explicit override; a `Dir` source always names its own directory).
    pub positron_dir: Option<&'a std::path::Path>,
}

/// A lookup for a caller that wants only the imported sources (`positron`, a directory): the
/// cache sources have nothing and the engine is refused. `provide-hub` publishes an imported
/// record through the shared walk with this, so no engine is reachable from that path and the
/// consent gate (which only guards `run_and_store`) is never involved.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImportedOnly;

#[async_trait(?Send)]
impl RecordLookup for ImportedOnly {
    async fn lookup(
        &self,
        _source: &RecordSource,
        _inputs: &SourceInputs,
    ) -> Result<Option<CaptureRun>, LookupError> {
        Ok(None)
    }

    async fn run_engine(&self, _inputs: &SourceInputs) -> Result<EngineResult, EngineError> {
        Err(EngineError::Failed(
            "this path publishes imported records only and never runs an engine".into(),
        ))
    }
}

/// What the walk needs besides the list and the lookup.
#[derive(Debug, Clone, Copy)]
pub struct WalkContext<'a> {
    /// `None` makes the imported sources unavailable ("no document path").
    pub import: Option<ImportContext<'a>>,
    pub list: &'a RecordSources,
    /// Whether the policy lets the engine run (`preview.engine` is not `off`). The policy type
    /// lives in `quarto-preview`, which this crate cannot name, so the caller passes the answer.
    pub engine_enabled: bool,
    /// The writer gate, for the refusal check.
    pub record_gate: bool,
}

/// What one source did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutcomeKind {
    /// It supplied the record that was used.
    Supplied,
    /// It had nothing for this document.
    Nothing,
    /// It had a record the gate-off writer cannot publish; the reason names the variable.
    Refused(String),
    /// It could not be used (`engine` absent from the list or the policy is `off`).
    Unavailable(String),
    /// It failed (an unreadable file, an engine error).
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOutcome {
    pub source: RecordSource,
    pub kind: OutcomeKind,
}

/// The source that won.
#[derive(Debug)]
pub enum Chosen {
    /// A run for the caller to publish.
    Run {
        source: RecordSource,
        run: CaptureRun,
    },
    /// The engine ran and the implementation published it already.
    EnginePublished,
}

/// The result of a walk.
#[derive(Debug)]
pub struct Resolution {
    pub chosen: Option<Chosen>,
    /// One entry per source considered, in list order; an `engine` missing from the list adds an
    /// `Unavailable` entry at the end so a diagnostic can name it.
    pub outcomes: Vec<SourceOutcome>,
}

impl Resolution {
    /// The first failure message, if any source failed (an engine error is the one a caller
    /// must surface, since nothing came after it).
    pub fn first_failure(&self) -> Option<&str> {
        self.outcomes.iter().find_map(|o| match &o.kind {
            OutcomeKind::Failed(m) => Some(m.as_str()),
            _ => None,
        })
    }

    /// The reason the last refused source was refused, if the walk ended with a refusal and
    /// nothing chosen: the caller maps it to its error type (the message names
    /// `QUARTO_ENGINE_RECORD_WRITE`).
    pub fn refusal(&self) -> Option<&str> {
        if self.chosen.is_some() {
            return None;
        }
        self.outcomes.iter().rev().find_map(|o| match &o.kind {
            OutcomeKind::Refused(m) => Some(m.as_str()),
            _ => None,
        })
    }
}

/// Decide what a run from `source` is: chosen, or an outcome that sends the walk on.
fn judge(
    ctx: &WalkContext<'_>,
    source: &RecordSource,
    run: CaptureRun,
) -> Result<Chosen, OutcomeKind> {
    if !run.has_content() {
        return Err(OutcomeKind::Nothing);
    }
    // The one refusal predicate, shared with `encode_for_publish`.
    if record_has_no_legacy_form(ctx.record_gate, &run.captures, run.record.as_ref()) {
        return Err(OutcomeKind::Refused(
            PublishError::RecordWriterOff.to_string(),
        ));
    }
    Ok(Chosen::Run {
        source: source.clone(),
        run,
    })
}

/// The `positron` and directory sources: locate the document's cache file, import it and
/// convert it. A file that cannot be used, or two that fit equally, is `Unavailable` with the
/// reason, never an error: the walk goes on to the next source.
#[cfg(not(target_arch = "wasm32"))]
fn import_arm(
    ctx: &WalkContext<'_>,
    source: &RecordSource,
    inputs: &SourceInputs,
) -> Result<CaptureRun, OutcomeKind> {
    use crate::positron_cache::{HostEnv, HostOs, LocateError, default_directories, locate_cache};

    let Some(import) = ctx.import else {
        return Err(OutcomeKind::Unavailable(
            "no document path to look up".into(),
        ));
    };
    if inputs.ownership.is_none() {
        // A caller bug, not a condition of the document: with no owners no unit would take an
        // output and the source would silently look empty.
        return Err(OutcomeKind::Failed(
            "internal error: the source inputs carry no engine ownership".into(),
        ));
    }
    let directories: Vec<PathBuf> = match (source, import.positron_dir) {
        (RecordSource::Dir(dir), _) => vec![dir.clone()],
        (_, Some(dir)) => vec![dir.to_path_buf()],
        _ => default_directories(HostOs::current(), &HostEnv::from_process()),
    };
    let (_, authors) = super::positron_record::scan_inputs(inputs);
    let hashes: Vec<String> = authors.0.iter().map(|a| a.content_hash.clone()).collect();
    match locate_cache(&directories, import.abs_path, &hashes, HostOs::current()) {
        Ok(located) => Ok(CaptureRun {
            captures: Vec::new(),
            record: Some(super::import_record_source(
                &located.import,
                inputs,
                import.source.map(str::to_string),
            )),
        }),
        Err(LocateError::NotFound) => Err(OutcomeKind::Nothing),
        Err(e) => Err(OutcomeKind::Unavailable(e.to_string())),
    }
}

/// The WASM client has no Positron cache to read.
#[cfg(target_arch = "wasm32")]
fn import_arm(
    _ctx: &WalkContext<'_>,
    _source: &RecordSource,
    _inputs: &SourceInputs,
) -> Result<CaptureRun, OutcomeKind> {
    Err(OutcomeKind::Unavailable(
        "the Positron cache is not readable here".into(),
    ))
}

/// Walk `ctx.list` in order and return the first record a source supplies.
///
/// The engine is called only when it is in the list and `ctx.engine_enabled`; a lookup or
/// engine failure is an outcome, not an error, so the caller decides what a failure means (the
/// preview treats an engine failure as the document's error, `Resolution::first_failure`).
pub async fn resolve_records(
    ctx: &WalkContext<'_>,
    inputs: &SourceInputs,
    lookup: &dyn RecordLookup,
) -> Resolution {
    let mut outcomes = Vec::new();
    let mut engine_listed = false;
    for source in ctx.list.iter() {
        let kind = match source {
            RecordSource::Engine => {
                engine_listed = true;
                if !ctx.engine_enabled {
                    OutcomeKind::Unavailable("the engine policy is off".into())
                } else {
                    match lookup.run_engine(inputs).await {
                        Ok(EngineResult::Published) => {
                            outcomes.push(SourceOutcome {
                                source: source.clone(),
                                kind: OutcomeKind::Supplied,
                            });
                            return Resolution {
                                chosen: Some(Chosen::EnginePublished),
                                outcomes,
                            };
                        }
                        Ok(EngineResult::Run(run)) => match judge(ctx, source, run) {
                            Ok(chosen) => {
                                outcomes.push(SourceOutcome {
                                    source: source.clone(),
                                    kind: OutcomeKind::Supplied,
                                });
                                return Resolution {
                                    chosen: Some(chosen),
                                    outcomes,
                                };
                            }
                            Err(kind) => kind,
                        },
                        Err(e) => OutcomeKind::Failed(e.to_string()),
                    }
                }
            }
            // Shared code, no trait method: nothing here depends on who is asking.
            RecordSource::Positron | RecordSource::Dir(_) => {
                match import_arm(ctx, source, inputs).and_then(|run| judge(ctx, source, run)) {
                    Ok(chosen) => {
                        outcomes.push(SourceOutcome {
                            source: source.clone(),
                            kind: OutcomeKind::Supplied,
                        });
                        return Resolution {
                            chosen: Some(chosen),
                            outcomes,
                        };
                    }
                    Err(kind) => kind,
                }
            }
            RecordSource::Cache | RecordSource::Q2 => match lookup.lookup(source, inputs).await {
                Ok(Some(run)) => match judge(ctx, source, run) {
                    Ok(chosen) => {
                        outcomes.push(SourceOutcome {
                            source: source.clone(),
                            kind: OutcomeKind::Supplied,
                        });
                        return Resolution {
                            chosen: Some(chosen),
                            outcomes,
                        };
                    }
                    Err(kind) => kind,
                },
                Ok(None) => OutcomeKind::Nothing,
                Err(e) => OutcomeKind::Failed(e.to_string()),
            },
        };
        outcomes.push(SourceOutcome {
            source: source.clone(),
            kind,
        });
    }
    if !engine_listed {
        outcomes.push(SourceOutcome {
            source: RecordSource::Engine,
            kind: OutcomeKind::Unavailable("engine is not in the record sources".into()),
        });
    }
    Resolution {
        chosen: None,
        outcomes,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    fn inputs() -> SourceInputs {
        SourceInputs {
            input_qmd: b"x".to_vec(),
            metadata: Default::default(),
            sugared_blocks: Vec::new(),
            ownership: None,
        }
    }

    fn record() -> crate::record::Record {
        crate::record::read_record(include_str!("../../tests/fixtures/records/basic.json")).unwrap()
    }

    fn capture() -> quarto_trace::EngineCapture {
        serde_json::from_value(serde_json::json!({
            "engine_name": "jupyter", "input_qmd": "x", "result": {}
        }))
        .unwrap()
    }

    /// A run a gate-off writer can publish (captures beside the record).
    fn engine_run() -> CaptureRun {
        CaptureRun {
            captures: vec![capture()],
            record: Some(record()),
        }
    }

    /// A run with no legacy form: an import.
    fn record_only() -> CaptureRun {
        CaptureRun {
            captures: Vec::new(),
            record: Some(record()),
        }
    }

    enum Scripted {
        Run(CaptureRun),
        Err(&'static str),
    }

    /// A fake lookup that answers per source and records what it was asked.
    struct Fake {
        by_source: HashMap<String, Scripted>,
        engine: RefCell<Option<Result<EngineResult, EngineError>>>,
        asked: RefCell<Vec<String>>,
        engine_calls: RefCell<usize>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                by_source: HashMap::new(),
                engine: RefCell::new(None),
                asked: RefCell::new(Vec::new()),
                engine_calls: RefCell::new(0),
            }
        }
        fn source(mut self, name: &str, s: Scripted) -> Self {
            self.by_source.insert(name.to_string(), s);
            self
        }
        fn engine(self, r: Result<EngineResult, EngineError>) -> Self {
            *self.engine.borrow_mut() = Some(r);
            self
        }
    }

    #[async_trait(?Send)]
    impl RecordLookup for Fake {
        async fn lookup(
            &self,
            source: &RecordSource,
            _inputs: &SourceInputs,
        ) -> Result<Option<CaptureRun>, LookupError> {
            self.asked.borrow_mut().push(source.label());
            match self.by_source.get(&source.label()) {
                None => Ok(None),
                Some(Scripted::Run(r)) => Ok(Some(r.clone())),
                Some(Scripted::Err(m)) => Err(LookupError::Unreadable((*m).to_string())),
            }
        }
        async fn run_engine(&self, _inputs: &SourceInputs) -> Result<EngineResult, EngineError> {
            *self.engine_calls.borrow_mut() += 1;
            self.engine
                .borrow_mut()
                .take()
                .unwrap_or(Ok(EngineResult::Run(engine_run())))
        }
    }

    async fn walk(
        list: &RecordSources,
        engine_enabled: bool,
        gate: bool,
        fake: &Fake,
    ) -> Resolution {
        let ctx = WalkContext {
            list,
            engine_enabled,
            record_gate: gate,
            import: None,
        };
        resolve_records(&ctx, &inputs(), fake).await
    }

    fn kinds(r: &Resolution) -> Vec<(String, OutcomeKind)> {
        r.outcomes
            .iter()
            .map(|o| (o.source.label(), o.kind.clone()))
            .collect()
    }

    #[tokio::test]
    async fn the_defaults_are_in_one_place() {
        assert_eq!(
            RecordSources::preview_default().as_slice(),
            &[RecordSource::Cache, RecordSource::Q2, RecordSource::Engine]
        );
        assert_eq!(
            RecordSources::provider_default().as_slice(),
            &[RecordSource::Engine]
        );
        assert!(RecordSources::preview_default().contains_engine());
        assert!(!RecordSources::new(vec![RecordSource::Cache]).contains_engine());
    }

    /// First wins, and the sources after it are never asked (no merging).
    #[tokio::test]
    async fn the_first_source_that_supplies_wins() {
        let fake = Fake::new()
            .source("cache", Scripted::Run(engine_run()))
            .source("q2", Scripted::Run(engine_run()));
        let r = walk(&RecordSources::preview_default(), true, true, &fake).await;
        assert!(matches!(
            r.chosen,
            Some(Chosen::Run {
                source: RecordSource::Cache,
                ..
            })
        ));
        assert_eq!(*fake.asked.borrow(), vec!["cache".to_string()]);
        assert_eq!(*fake.engine_calls.borrow(), 0);
        assert_eq!(
            kinds(&r),
            vec![("cache".to_string(), OutcomeKind::Supplied)]
        );
    }

    #[tokio::test]
    async fn a_source_with_nothing_falls_through_to_the_next() {
        let fake = Fake::new().source("q2", Scripted::Run(engine_run()));
        let r = walk(&RecordSources::preview_default(), true, true, &fake).await;
        assert!(matches!(
            r.chosen,
            Some(Chosen::Run {
                source: RecordSource::Q2,
                ..
            })
        ));
        assert_eq!(*fake.engine_calls.borrow(), 0);
        assert_eq!(
            kinds(&r),
            vec![
                ("cache".to_string(), OutcomeKind::Nothing),
                ("q2".to_string(), OutcomeKind::Supplied)
            ]
        );
    }

    /// A run with no content (no captures, a record that describes nothing) is not a supply.
    #[tokio::test]
    async fn a_run_with_no_content_is_nothing() {
        let mut empty = record();
        empty.metadata.quarto_engines.clear();
        empty.cells.clear();
        let fake = Fake::new().source(
            "cache",
            Scripted::Run(CaptureRun {
                captures: Vec::new(),
                record: Some(empty),
            }),
        );
        let list = RecordSources::new(vec![RecordSource::Cache]);
        let r = walk(&list, true, true, &fake).await;
        assert!(r.chosen.is_none());
        assert_eq!(kinds(&r)[0], ("cache".to_string(), OutcomeKind::Nothing));
    }

    #[tokio::test]
    async fn the_engine_runs_last_and_its_run_is_chosen() {
        let fake = Fake::new();
        let r = walk(&RecordSources::preview_default(), true, true, &fake).await;
        assert!(matches!(
            r.chosen,
            Some(Chosen::Run {
                source: RecordSource::Engine,
                ..
            })
        ));
        assert_eq!(*fake.engine_calls.borrow(), 1);
    }

    /// Without `engine` in the list the engine is never called, and the outcomes say so.
    #[tokio::test]
    async fn an_absent_engine_is_never_run_and_is_reported_unavailable() {
        let fake = Fake::new();
        let list = RecordSources::new(vec![RecordSource::Cache, RecordSource::Q2]);
        let r = walk(&list, true, true, &fake).await;
        assert!(r.chosen.is_none());
        assert_eq!(*fake.engine_calls.borrow(), 0);
        let k = kinds(&r);
        assert_eq!(k[0], ("cache".to_string(), OutcomeKind::Nothing));
        assert_eq!(k[1], ("q2".to_string(), OutcomeKind::Nothing));
        assert!(matches!(&k[2], (s, OutcomeKind::Unavailable(_)) if s == "engine"));
    }

    /// `preview.engine: off` keeps the lookup sources and never runs the engine.
    #[tokio::test]
    async fn a_policy_that_forbids_the_engine_still_consults_the_other_sources() {
        let fake = Fake::new().source("cache", Scripted::Run(engine_run()));
        let r = walk(&RecordSources::preview_default(), false, true, &fake).await;
        assert!(matches!(
            r.chosen,
            Some(Chosen::Run {
                source: RecordSource::Cache,
                ..
            })
        ));
        let fake = Fake::new();
        let r = walk(&RecordSources::preview_default(), false, true, &fake).await;
        assert!(r.chosen.is_none());
        assert_eq!(*fake.engine_calls.borrow(), 0);
        assert!(matches!(
            kinds(&r).last().unwrap(),
            (s, OutcomeKind::Unavailable(_)) if s == "engine"
        ));
    }

    /// A fake engine that publishes by itself makes the walk say so and hand back no run.
    #[tokio::test]
    async fn a_published_engine_result_is_reported_as_published() {
        let fake = Fake::new().engine(Ok(EngineResult::Published));
        let r = walk(&RecordSources::provider_default(), true, false, &fake).await;
        assert!(matches!(r.chosen, Some(Chosen::EnginePublished)));
        assert_eq!(
            kinds(&r),
            vec![("engine".to_string(), OutcomeKind::Supplied)]
        );
    }

    /// The refusal predicate is the walk's: a record-only run with the gate off is refused
    /// (naming the variable) and the walk takes the next source.
    #[tokio::test]
    async fn a_refused_source_falls_through_and_names_the_variable() {
        let fake = Fake::new().source("cache", Scripted::Run(record_only()));
        let r = walk(&RecordSources::preview_default(), true, false, &fake).await;
        assert!(matches!(
            r.chosen,
            Some(Chosen::Run {
                source: RecordSource::Engine,
                ..
            })
        ));
        let k = kinds(&r);
        let (_, OutcomeKind::Refused(why)) = &k[0] else {
            panic!("expected Refused, got {k:?}");
        };
        assert!(why.contains(crate::engine::capture_files::RECORD_WRITING_ENV));
        assert!(r.refusal().is_none(), "something was chosen");
    }

    /// With the refused record the last source that could supply, the refusal stands.
    #[tokio::test]
    async fn a_refusal_stands_when_nothing_else_supplies() {
        let fake = Fake::new().source("cache", Scripted::Run(record_only()));
        let list = RecordSources::new(vec![RecordSource::Cache]);
        let r = walk(&list, true, false, &fake).await;
        assert!(r.chosen.is_none());
        assert!(
            r.refusal()
                .unwrap()
                .contains(crate::engine::capture_files::RECORD_WRITING_ENV)
        );
    }

    /// The walk and `encode_for_publish` give the same answer for each combination.
    #[tokio::test]
    async fn the_walk_refuses_exactly_what_publish_refuses() {
        for gate in [false, true] {
            for run in [engine_run(), record_only()] {
                let publish = crate::engine::capture_files::encode_for_publish(
                    &run.captures,
                    run.record.as_ref(),
                    gate,
                );
                let fake = Fake::new().source("cache", Scripted::Run(run));
                let list = RecordSources::new(vec![RecordSource::Cache]);
                let r = walk(&list, true, gate, &fake).await;
                assert_eq!(
                    publish.is_err(),
                    matches!(kinds(&r)[0].1, OutcomeKind::Refused(_)),
                    "gate={gate}"
                );
                assert!(matches!(
                    publish,
                    Ok(_) | Err(PublishError::RecordWriterOff)
                ));
            }
        }
    }

    /// A source that fails is reported and stepped past; the next source is tried.
    #[tokio::test]
    async fn a_failing_source_is_reported_and_skipped() {
        let fake = Fake::new()
            .source("cache", Scripted::Err("corrupt"))
            .source("q2", Scripted::Run(engine_run()));
        let r = walk(&RecordSources::preview_default(), true, true, &fake).await;
        assert!(matches!(
            r.chosen,
            Some(Chosen::Run {
                source: RecordSource::Q2,
                ..
            })
        ));
        assert!(matches!(
            &kinds(&r)[0].1,
            OutcomeKind::Failed(m) if m.contains("corrupt")
        ));
        assert_eq!(r.first_failure(), Some("corrupt"));
    }

    #[tokio::test]
    async fn an_engine_error_is_reported() {
        let fake = Fake::new().engine(Err(EngineError::Failed("kernel died".into())));
        let r = walk(&RecordSources::provider_default(), true, true, &fake).await;
        assert!(r.chosen.is_none());
        assert_eq!(r.first_failure(), Some("kernel died"));
    }

    /// Positron and directory sources resolve to nothing until RIU-3a supplies their arms, and
    /// the lookup is never asked about them.
    #[tokio::test]
    async fn positron_and_dir_resolve_to_nothing_for_now() {
        let fake = Fake::new();
        let list = RecordSources::new(vec![
            RecordSource::Positron,
            RecordSource::Dir("./x".into()),
        ]);
        let r = walk(&list, true, true, &fake).await;
        assert!(r.chosen.is_none());
        assert!(fake.asked.borrow().is_empty());
        assert_eq!(kinds(&r).len(), 3, "two sources and the absent engine");
    }

    #[test]
    fn record_source_error_lists_names_and_suggests_a_dot_slash_directory() {
        use std::str::FromStr;
        let err = RecordSource::from_str("postiron").unwrap_err().to_string();
        for name in ["cache", "q2", "positron", "engine"] {
            assert!(err.contains(name), "names the valid choices: {err}");
        }
        assert!(err.contains("./postiron"), "suggests ./name: {err}");
        assert!(
            err.contains("comma"),
            "says a comma path cannot be given: {err}"
        );
        let empty = RecordSource::from_str("").unwrap_err().to_string();
        assert!(
            empty.contains("empty"),
            "an empty entry is called empty: {empty}"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn directory_sources_are_checked_once_at_start() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f.ipynb");
        std::fs::write(&file, "{}").unwrap();
        let list = |p: &std::path::Path| RecordSources::new(vec![RecordSource::Dir(p.into())]);

        let ok = list(dir.path()).validate_directories().unwrap();
        assert_eq!(ok, list(dir.path()));

        let missing = dir.path().join("nope");
        let err = list(&missing).validate_directories().unwrap_err();
        assert!(
            err.contains("does not exist") && err.contains("nope"),
            "{err}"
        );

        let err = list(&file).validate_directories().unwrap_err();
        assert!(err.contains("is a file"), "{err}");

        // Non-directory sources pass untouched.
        let named = RecordSources::new(vec![RecordSource::Positron, RecordSource::Engine]);
        assert_eq!(named.clone().validate_directories().unwrap(), named);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_leading_tilde_expands_to_the_home_directory() {
        let Some(home) = dirs::home_dir() else { return };
        let got = RecordSources::new(vec![RecordSource::Dir("~".into())])
            .validate_directories()
            .unwrap();
        assert_eq!(got.as_slice(), [RecordSource::Dir(home)]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cache_is_rejected_for_provide_hub_only_when_listed() {
        let with = RecordSources::new(vec![RecordSource::Cache, RecordSource::Engine]);
        let err = with.reject_cache("q2 provide-hub").unwrap_err();
        assert!(
            err.contains("cache") && err.contains("provide-hub"),
            "{err}"
        );
        RecordSources::provider_default()
            .reject_cache("q2 provide-hub")
            .unwrap();
    }

    #[test]
    fn record_source_name_beats_a_same_named_directory_until_dot_slash() {
        use std::str::FromStr;
        assert_eq!(
            RecordSource::from_str("cache").unwrap(),
            RecordSource::Cache
        );
        assert_eq!(
            RecordSource::from_str("./cache").unwrap(),
            RecordSource::Dir("./cache".into())
        );
    }

    #[test]
    fn record_source_entries_classify_by_name_then_path_then_error() {
        use std::str::FromStr;
        for (text, want) in [
            ("cache", RecordSource::Cache),
            ("q2", RecordSource::Q2),
            ("positron", RecordSource::Positron),
            ("engine", RecordSource::Engine),
            ("/abs/dir", RecordSource::Dir("/abs/dir".into())),
            ("./rel", RecordSource::Dir("./rel".into())),
            ("../up", RecordSource::Dir("../up".into())),
            ("~/caches", RecordSource::Dir("~/caches".into())),
            ("a/b", RecordSource::Dir("a/b".into())),
            (r"C:\caches", RecordSource::Dir(r"C:\caches".into())),
        ] {
            assert_eq!(RecordSource::from_str(text).unwrap(), want, "{text}");
        }
        for bad in ["bogus", "", "Positron", "caches"] {
            let err = RecordSource::from_str(bad).unwrap_err();
            assert!(
                err.to_string().contains("positron"),
                "the message names the choices: {err}"
            );
        }
    }

    // The Positron and directory arms (RIU-3a) -------------------------------------------

    #[cfg(not(target_arch = "wasm32"))]
    mod imported {
        use super::*;
        use crate::record::content_hash;

        const DOC_URI: &str = "file:///virtual/doc.qmd";

        fn import_inputs() -> SourceInputs {
            SourceInputs {
                input_qmd: b"```{python}\nx = 1\n```\n".to_vec(),
                ownership: Some([("python".to_string(), "jupyter".to_string())].into()),
                ..Default::default()
            }
        }

        /// A cache directory holding one Positron-format file for `x = 1`, under an arbitrary
        /// name (so the `quarto_source` tier finds it).
        fn cache_dir() -> tempfile::TempDir {
            let dir = tempfile::tempdir().unwrap();
            let file = serde_json::json!({
                "nbformat": 4, "nbformat_minor": 5,
                "metadata": {"quarto_source": DOC_URI, "quarto_cache_version": 1},
                "cells": [{
                    "cell_type": "code", "execution_count": null,
                    "metadata": {"quarto_content_hash": content_hash("x = 1")},
                    "outputs": [{"output_type": "stream", "name": "stdout", "text": ["hi\n"]}],
                    "source": []
                }]
            });
            std::fs::write(dir.path().join("any-name.ipynb"), file.to_string()).unwrap();
            dir
        }

        async fn walk_imported(
            list: &RecordSources,
            dir_override: Option<&std::path::Path>,
            gate: bool,
            fake: &Fake,
            inputs: &SourceInputs,
        ) -> Resolution {
            let ctx = WalkContext {
                list,
                engine_enabled: true,
                record_gate: gate,
                import: Some(ImportContext {
                    abs_path: std::path::Path::new("/virtual/doc.qmd"),
                    source: Some("doc.qmd"),
                    positron_dir: dir_override,
                }),
            };
            resolve_records(&ctx, inputs, fake).await
        }

        #[tokio::test]
        async fn a_directory_source_supplies_an_imported_record_without_asking_the_lookup() {
            let dir = cache_dir();
            let list = RecordSources::new(vec![RecordSource::Dir(dir.path().to_path_buf())]);
            let fake = Fake::new();
            let r = walk_imported(&list, None, true, &fake, &import_inputs()).await;
            let Some(Chosen::Run { source, run }) = &r.chosen else {
                panic!("expected a run: {:?}", r.outcomes)
            };
            assert_eq!(source, &RecordSource::Dir(dir.path().to_path_buf()));
            assert!(run.captures.is_empty(), "an import has no legacy form");
            let record = run.record.as_ref().unwrap();
            assert_eq!(record.cells.len(), 1);
            assert_eq!(record.metadata.quarto_source.as_deref(), Some("doc.qmd"));
            assert!(fake.asked.borrow().is_empty() && *fake.engine_calls.borrow() == 0);
        }

        #[tokio::test]
        async fn the_positron_source_reads_the_directory_override_and_beats_the_engine() {
            let dir = cache_dir();
            let list = RecordSources::new(vec![RecordSource::Positron, RecordSource::Engine]);
            let fake = Fake::new();
            let r = walk_imported(&list, Some(dir.path()), true, &fake, &import_inputs()).await;
            assert!(matches!(
                r.chosen,
                Some(Chosen::Run {
                    source: RecordSource::Positron,
                    ..
                })
            ));
            assert_eq!(*fake.engine_calls.borrow(), 0, "no kernel is started");
        }

        #[tokio::test]
        async fn a_document_the_cache_does_not_cover_falls_through_to_the_engine() {
            let dir = tempfile::tempdir().unwrap();
            let list = RecordSources::new(vec![RecordSource::Positron, RecordSource::Engine]);
            let fake = Fake::new();
            let r = walk_imported(&list, Some(dir.path()), true, &fake, &import_inputs()).await;
            assert!(matches!(
                r.chosen,
                Some(Chosen::Run {
                    source: RecordSource::Engine,
                    ..
                })
            ));
            assert_eq!(
                kinds(&r),
                vec![
                    ("positron".to_string(), OutcomeKind::Nothing),
                    ("engine".to_string(), OutcomeKind::Supplied)
                ]
            );
        }

        #[tokio::test]
        async fn a_cache_whose_cells_match_no_unit_is_nothing() {
            let dir = cache_dir();
            let inputs = SourceInputs {
                input_qmd: b"```{python}\ny = 2\n```\n".to_vec(),
                ..import_inputs()
            };
            let list = RecordSources::new(vec![RecordSource::Dir(dir.path().to_path_buf())]);
            let r = walk_imported(&list, None, true, &Fake::new(), &inputs).await;
            assert!(r.chosen.is_none());
            assert_eq!(r.outcomes[0].kind, OutcomeKind::Nothing);
        }

        #[tokio::test]
        async fn an_unusable_cache_is_unavailable_with_the_reason_not_an_error() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("any-name.ipynb"),
                serde_json::json!({
                    "nbformat": 4, "metadata": {"quarto_source": DOC_URI, "quarto_cache_version": 2},
                    "cells": []
                })
                .to_string(),
            )
            .unwrap();
            let list = RecordSources::new(vec![RecordSource::Dir(dir.path().to_path_buf())]);
            let r = walk_imported(&list, None, true, &Fake::new(), &import_inputs()).await;
            assert!(r.chosen.is_none());
            let OutcomeKind::Unavailable(why) = &r.outcomes[0].kind else {
                panic!("{:?}", r.outcomes)
            };
            assert!(why.contains("version"), "{why}");
            assert!(
                r.first_failure().is_none(),
                "a bad cache is not the document's error"
            );
        }

        #[tokio::test]
        async fn with_the_writer_gate_off_an_import_is_refused_naming_the_variable() {
            let dir = cache_dir();
            let list = RecordSources::new(vec![RecordSource::Dir(dir.path().to_path_buf())]);
            let r = walk_imported(&list, None, false, &Fake::new(), &import_inputs()).await;
            assert!(r.chosen.is_none());
            assert!(r.refusal().unwrap().contains("QUARTO_ENGINE_RECORD_WRITE"));
        }

        #[tokio::test]
        async fn without_ownership_the_walk_reports_an_internal_failure_not_an_empty_cache() {
            let dir = cache_dir();
            let inputs = SourceInputs {
                ownership: None,
                ..import_inputs()
            };
            let list = RecordSources::new(vec![RecordSource::Dir(dir.path().to_path_buf())]);
            let r = walk_imported(&list, None, true, &Fake::new(), &inputs).await;
            assert!(r.first_failure().unwrap().contains("ownership"));
        }

        #[tokio::test]
        async fn without_a_document_path_the_imported_sources_are_unavailable() {
            let dir = cache_dir();
            let list = RecordSources::new(vec![RecordSource::Dir(dir.path().to_path_buf())]);
            let r = walk(&list, true, true, &Fake::new()).await;
            assert!(matches!(r.outcomes[0].kind, OutcomeKind::Unavailable(_)));
        }
    }
}
