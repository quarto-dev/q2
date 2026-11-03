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

/// What the walk needs besides the list and the lookup.
#[derive(Debug, Clone, Copy)]
pub struct WalkContext<'a> {
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
            // RIU-3a supplies these arms; until then they have nothing, and the caller's
            // lookup is never asked about them.
            RecordSource::Positron | RecordSource::Dir(_) => OutcomeKind::Nothing,
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
}
