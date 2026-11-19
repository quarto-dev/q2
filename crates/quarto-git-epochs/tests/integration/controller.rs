//! The gate and quiescence controller against a fake probe and explicit time: no sleeps.

use quarto_git_epochs::controller::{
    Action, Controller, ControllerOptions, GateMode, GateReason, GateState,
};
use quarto_git_epochs::probe::{
    CheapState, ControllerProbe, MarkerKind, MarkerState, ProbeError, ReflogMark, ReflogSlice,
    ReflogStat, TrackedFile,
};
use std::cell::RefCell;
use std::rc::Rc;

const T0: u64 = 1_000_000;

struct Inner {
    head: Result<Option<String>, ProbeError>,
    markers: Vec<MarkerState>,
    reflog: Option<ReflogStat>,
    subjects: Vec<String>,
    head_calls: u32,
    tracked: Vec<TrackedFile>,
}

#[derive(Clone)]
struct FakeProbe(Rc<RefCell<Inner>>);

impl FakeProbe {
    fn new() -> Self {
        FakeProbe(Rc::new(RefCell::new(Inner {
            head: Ok(Some("h1".into())),
            markers: vec![],
            reflog: Some(ReflogStat {
                size: 100,
                mtime_ms: 1,
            }),
            subjects: vec![],
            head_calls: 0,
            tracked: vec![],
        })))
    }
    fn set_head(&self, h: &str) {
        self.0.borrow_mut().head = Ok(Some(h.into()));
    }
    fn set_markers(&self, m: Vec<MarkerState>) {
        self.0.borrow_mut().markers = m;
    }
    /// Simulates git appending a reflog entry.
    fn append_reflog(&self, subject: &str) {
        let mut i = self.0.borrow_mut();
        let r = i.reflog.unwrap_or(ReflogStat {
            size: 0,
            mtime_ms: 0,
        });
        i.reflog = Some(ReflogStat {
            size: r.size + 80,
            mtime_ms: r.mtime_ms + 1,
        });
        i.subjects.push(subject.to_string());
    }
    fn head_calls(&self) -> u32 {
        self.0.borrow().head_calls
    }
}

impl ControllerProbe for FakeProbe {
    fn head_sha(&self) -> Result<Option<String>, ProbeError> {
        let mut i = self.0.borrow_mut();
        i.head_calls += 1;
        i.head.clone()
    }
    fn cheap_state(&self) -> CheapState {
        let i = self.0.borrow();
        CheapState {
            markers: i.markers.clone(),
            reflog: i.reflog,
        }
    }
    fn reflog_since(&self, _mark: Option<&ReflogMark>) -> Result<Option<ReflogSlice>, ProbeError> {
        let i = self.0.borrow();
        if i.reflog.is_none() {
            return Ok(None);
        }
        Ok(Some(ReflogSlice {
            subjects: i.subjects.clone(),
            tail: Some(ReflogMark {
                sha: format!("{:?}", i.head),
                lines: Some(1),
            }),
            mark_found: true,
        }))
    }
    fn ls_tree(&self) -> Result<Vec<TrackedFile>, ProbeError> {
        Ok(self.0.borrow().tracked.clone())
    }
}

fn lock(now: u64, age: u64) -> MarkerState {
    MarkerState {
        name: "index.lock".into(),
        kind: MarkerKind::Lock,
        mtime_ms: now - age,
    }
}
fn state_marker(now: u64, name: &str) -> MarkerState {
    MarkerState {
        name: name.into(),
        kind: MarkerKind::State,
        mtime_ms: now,
    }
}

struct Rig {
    probe: FakeProbe,
    c: Controller<FakeProbe>,
    logs: Rc<RefCell<Vec<String>>>,
    now: u64,
}

impl Rig {
    fn new() -> Self {
        Self::with(|_| {})
    }
    fn with(f: impl FnOnce(&mut ControllerOptions)) -> Self {
        let probe = FakeProbe::new();
        let logs = Rc::new(RefCell::new(Vec::new()));
        let l = logs.clone();
        let mut opts = ControllerOptions {
            log: Some(Box::new(move |m| l.borrow_mut().push(m.to_string()))),
            ..Default::default()
        };
        f(&mut opts);
        let mut c = Controller::new(probe.clone(), opts);
        c.init();
        Rig {
            probe,
            c,
            logs,
            now: T0,
        }
    }
    fn advance(&mut self, ms: u64) {
        self.now += ms;
    }
    fn check(&mut self) -> GateState {
        self.c.check(self.now)
    }
    fn tick(&mut self) -> quarto_git_epochs::controller::TickResult {
        self.c.tick(self.now)
    }
    fn stale_logs(&self) -> usize {
        self.logs
            .borrow()
            .iter()
            .filter(|l| l.to_lowercase().contains("stale"))
            .count()
    }
}

fn open() -> GateState {
    GateState {
        mode: GateMode::Open,
        pending: false,
        markers: vec![],
        held: vec![],
        reason: None,
    }
}

#[test]
fn is_open_and_idle_with_no_markers_and_no_change() {
    let mut r = Rig::new();
    assert_eq!(r.check(), open());
}

#[test]
fn closes_on_each_marker() {
    for (name, kind) in [
        ("index.lock", MarkerKind::Lock),
        ("MERGE_HEAD", MarkerKind::State),
        ("rebase-merge", MarkerKind::State),
        (".jj/working_copy/working_copy.lock", MarkerKind::Lock),
    ] {
        let mut r = Rig::new();
        r.probe.set_markers(vec![MarkerState {
            name: name.into(),
            kind,
            mtime_ms: r.now,
        }]);
        let g = r.check();
        assert_eq!(g.mode, GateMode::Closed, "{name}");
        assert!(g.pending);
        assert_eq!(g.markers, vec![name.to_string()]);
    }
}

#[test]
fn check_sees_a_changed_reflog_tail_before_any_tick() {
    let mut r = Rig::new();
    r.probe.append_reflog("checkout: moving from a to b");
    assert_eq!(r.probe.head_calls(), 1, "only init");
    assert_eq!(r.check().mode, GateMode::Closed);
    assert_eq!(r.probe.head_calls(), 1, "check() ran no subprocess");
}

#[test]
fn a_missing_reflog_is_no_reflog_not_an_error() {
    let mut r = Rig::new();
    r.probe.0.borrow_mut().reflog = None;
    assert_eq!(
        r.check().mode,
        GateMode::Closed,
        "reflog to none is a change"
    );

    let probe = FakeProbe::new();
    probe.0.borrow_mut().reflog = None;
    let mut c = Controller::new(probe, ControllerOptions::default());
    c.init();
    assert_eq!(c.check(T0), open());
}

#[test]
fn a_head_change_seen_by_a_tick_makes_the_gate_pending() {
    let mut r = Rig::new();
    r.probe.set_head("h2");
    r.tick();
    assert!(r.check().pending);
}

#[test]
fn holds_files_whose_disk_differs_frozen_at_pending_entry() {
    let differing = Rc::new(RefCell::new(vec!["a.qmd".to_string(), "b.qmd".to_string()]));
    let d = differing.clone();
    let mut r =
        Rig::with(|o| o.disk_differs_from_synced = Some(Box::new(move || d.borrow().clone())));
    r.probe.set_markers(vec![lock(r.now, 0)]);
    assert_eq!(r.check().held, vec!["a.qmd", "b.qmd"]);
    differing.borrow_mut().push("c.qmd".into()); // git rewrote another file meanwhile
    assert_eq!(r.check().held, vec!["a.qmd", "b.qmd"]);
}

#[test]
fn decides_only_after_one_second_of_quiet() {
    let mut r = Rig::new();
    r.probe.append_reflog("checkout: moving from a to b");
    r.probe.set_head("h2");
    r.check();
    r.tick(); // observes the new HEAD at the start of the window
    r.advance(999);
    assert!(r.tick().actions.is_empty());
    r.advance(1);
    let res = r.tick();
    assert_eq!(res.actions.len(), 1);
    let Action::Decision(d) = &res.actions[0];
    assert_eq!(d.head.as_deref(), Some("h2"));
    assert_eq!(
        d.reflog.as_deref(),
        Some(&["checkout: moving from a to b".to_string()][..])
    );
    assert!(d.held.is_empty());
    assert!(!d.capped);
    assert!(!r.check().pending);
}

#[test]
fn a_further_reflog_change_restarts_the_quiet_window() {
    let mut r = Rig::new();
    r.probe.append_reflog("checkout: moving from a to b");
    r.check();
    r.advance(800);
    r.probe.append_reflog("commit: x");
    r.check();
    r.advance(800);
    assert!(r.tick().actions.is_empty());
    r.advance(200);
    assert_eq!(r.tick().actions.len(), 1);
}

#[test]
fn own_writes_do_not_restart_the_quiet_window_a_foreign_write_does() {
    let mut r = Rig::with(|o| o.is_own_write = Some(Box::new(|p| p == "mine.qmd")));
    r.probe.append_reflog("checkout: moving from a to b");
    r.check();
    r.advance(900);
    r.c.note_activity("mine.qmd", r.now);
    r.advance(100);
    assert_eq!(r.tick().actions.len(), 1);

    r.probe.append_reflog("checkout: moving from b to a");
    r.check();
    r.advance(900);
    r.c.note_activity("theirs.qmd", r.now);
    r.advance(100);
    assert!(r.tick().actions.is_empty());
    r.advance(900);
    assert_eq!(r.tick().actions.len(), 1);
}

#[test]
fn decides_at_the_thirty_second_cap_even_when_state_keeps_changing() {
    let mut r = Rig::new();
    r.probe.append_reflog("checkout: moving from a to b");
    r.check();
    let mut decided = false;
    let mut capped = false;
    for _ in 0..80 {
        r.advance(400);
        r.probe.append_reflog("rebase (pick): x");
        r.check();
        let res = r.tick();
        if let Some(Action::Decision(d)) = res.actions.first() {
            decided = true;
            capped = d.capped;
            break;
        }
        assert!(r.now - T0 < 30_400);
    }
    assert!(decided);
    assert!(capped);
    assert!(r.now - T0 >= 30_000);
}

#[test]
fn a_lock_older_than_sixty_seconds_is_stale_and_resumes_with_one_warning() {
    let mut r = Rig::new();
    r.probe.set_markers(vec![lock(r.now, 0)]);
    r.check();
    r.advance(61_000);
    r.probe.set_markers(vec![lock(r.now, 61_000)]);
    assert!(
        r.tick().actions.is_empty(),
        "the quiet window starts when the lock is judged stale"
    );
    assert_eq!(r.stale_logs(), 1);
    r.advance(1_000);
    assert_eq!(r.tick().actions.len(), 1);
    assert_eq!(r.stale_logs(), 1);
}

#[test]
fn state_markers_pause_indefinitely_and_resume_after_one_quiet_window_once_gone() {
    let mut r = Rig::new();
    r.probe
        .set_markers(vec![state_marker(r.now, "rebase-merge")]);
    r.check();
    r.advance(300_000);
    assert!(r.tick().actions.is_empty());
    assert_eq!(r.check().mode, GateMode::Closed);
    r.probe.set_markers(vec![]);
    r.check();
    r.advance(500);
    assert!(r.tick().actions.is_empty());
    r.advance(500);
    assert_eq!(r.tick().actions.len(), 1);
}

#[test]
fn asks_for_a_250_ms_tick_while_pending() {
    let mut r = Rig::new();
    r.probe.set_markers(vec![lock(r.now, 0)]);
    r.check();
    assert_eq!(r.tick().next_tick_ms, 250);
}

#[test]
fn an_unresolvable_head_means_no_change_and_is_rechecked_each_tick() {
    let mut r = Rig::new();
    r.probe.append_reflog("checkout: moving from a to b");
    r.check();
    r.probe.0.borrow_mut().head = Err(ProbeError::Failed("fatal: bad object HEAD".into()));
    r.advance(5_000);
    let res = r.tick();
    assert!(res.actions.is_empty());
    assert_eq!(res.next_tick_ms, 250);
    r.probe.set_head("h2");
    assert!(
        r.tick().actions.is_empty(),
        "the new HEAD is activity: one quiet window first"
    );
    r.advance(1_000);
    assert_eq!(r.tick().actions.len(), 1);
}

#[test]
fn the_decision_carries_the_tracked_files_at_head() {
    let mut r = Rig::new();
    r.probe.0.borrow_mut().tracked = vec![TrackedFile {
        path: "a.qmd".into(),
        blob: "b1".into(),
        mode: "100644".into(),
    }];
    r.probe.append_reflog("checkout: moving from a to b");
    r.check();
    r.advance(1_000);
    let res = r.tick();
    let Action::Decision(d) = &res.actions[0];
    assert_eq!(d.tracked.len(), 1);
    assert!(!d.jj);
}

#[test]
fn goes_off_with_one_message_when_git_is_missing_or_refused() {
    let probe = FakeProbe::new();
    probe.0.borrow_mut().head = Err(ProbeError::Unavailable("git: command not found".into()));
    let logs = Rc::new(RefCell::new(Vec::new()));
    let l = logs.clone();
    let mut c = Controller::new(
        probe,
        ControllerOptions {
            log: Some(Box::new(move |m| l.borrow_mut().push(m.to_string()))),
            ..Default::default()
        },
    );
    c.init();
    let g = c.check(T0);
    assert_eq!(g.mode, GateMode::Off);
    assert_eq!(g.reason, Some(GateReason::GitMissing));
    c.tick(T0);
    c.tick(T0);
    assert_eq!(logs.borrow().len(), 1);
}

#[test]
fn refuses_an_unborn_repository_and_recovers_without_a_decision() {
    let probe = FakeProbe::new();
    probe.0.borrow_mut().head = Ok(None);
    probe.0.borrow_mut().reflog = None;
    let mut c = Controller::new(probe.clone(), ControllerOptions::default());
    c.init();
    let g = c.check(T0);
    assert_eq!(g.mode, GateMode::Off);
    assert_eq!(g.reason, Some(GateReason::Unborn));
    probe.set_head("h1");
    probe.0.borrow_mut().reflog = Some(ReflogStat {
        size: 80,
        mtime_ms: 5,
    });
    assert!(c.tick(T0).actions.is_empty());
    let g = c.check(T0);
    assert_eq!(g.mode, GateMode::Open);
    assert!(!g.pending);
}
