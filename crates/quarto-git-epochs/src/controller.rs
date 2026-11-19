//! The gate and quiescence controller (G1, G3, G32). `check()` is the synchronous entry every sync path calls
//! first; `tick()` resolves HEAD and emits a decision once the repository has been quiet. All time is the
//! `now_ms` the caller passes (milliseconds since the Unix epoch, comparable to marker mtimes); the only I/O is
//! through the probe. See the contract, "Module map" and "Numbers".
//!
//! The decision carries the reflog subjects, the held paths, and the tracked files at HEAD. The verdict itself
//! is the caller's to compute with [`crate::classify::classify`], because it needs disk and doc contents that
//! the controller does not see; under colocated jj the caller applies the step 5 fallback (`Decision::jj`).

use crate::numbers::{
    CONTROLLER_TICK_PENDING_MS, DECISION_CAP_MS, IDLE_TICK_MS, QUIET_WINDOW_MS, STALE_LOCK_MS,
};
use crate::probe::{
    CheapState, ControllerProbe, MarkerKind, ProbeError, ReflogMark, ReflogStat, TrackedFile,
};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct ControllerOptions {
    /// Paths whose disk content differs from their last synced content. Read once, when the gate goes pending.
    pub disk_differs_from_synced: Option<Box<dyn Fn() -> Vec<String> + Send>>,
    /// True for a path the peer itself just wrote; such events do not restart the quiet window.
    pub is_own_write: Option<Box<dyn Fn(&str) -> bool + Send>>,
    /// One-line diagnostics (stale lock, git off). Default: `tracing::warn!`.
    pub log: Option<Box<dyn Fn(&str) + Send>>,
    /// Tick interval while nothing is pending. Default [`IDLE_TICK_MS`].
    pub idle_tick_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateReason {
    GitMissing,
    Unborn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateMode {
    /// Sync as usual.
    Open,
    /// A decision is pending; sync nothing.
    Closed,
    /// Git-aware logic is inactive; sync as usual.
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateState {
    pub mode: GateMode,
    pub pending: bool,
    /// Names of the markers holding the gate.
    pub markers: Vec<String>,
    /// Files held since the gate went pending (disk differed from the last synced content).
    pub held: Vec<String>,
    pub reason: Option<GateReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub head: Option<String>,
    /// Reflog subjects since the previous decision, or `None` when there is no reflog (the content rule alone).
    pub reflog: Option<Vec<String>>,
    pub held: Vec<String>,
    /// True when the 30 s cap forced the decision.
    pub capped: bool,
    /// Tracked regular files under the project prefix at the new HEAD.
    pub tracked: Vec<TrackedFile>,
    /// Colocated jj: the caller applies the step 5 fallback verdict to every changed file.
    pub jj: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Decision(Decision),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickResult {
    pub actions: Vec<Action>,
    pub next_tick_ms: u64,
}

pub struct Controller<P: ControllerProbe> {
    probe: P,
    options: ControllerOptions,
    off: Option<GateReason>,
    unborn: bool,
    initialized: bool,
    last_head: Option<String>,
    seen_head: Option<String>,
    last_mark: Option<ReflogMark>,
    seen_sig: Option<ReflogStat>,
    pending: bool,
    pending_since: u64,
    quiet_since: u64,
    held: Vec<String>,
    blocking_key: String,
    stale_logged: BTreeSet<String>,
    newly_stale: bool,
}

impl<P: ControllerProbe> Controller<P> {
    pub fn new(probe: P, options: ControllerOptions) -> Self {
        Controller {
            probe,
            options,
            off: None,
            unborn: false,
            initialized: false,
            last_head: None,
            seen_head: None,
            last_mark: None,
            seen_sig: None,
            pending: false,
            pending_since: 0,
            quiet_since: 0,
            held: Vec::new(),
            blocking_key: String::new(),
            stale_logged: BTreeSet::new(),
            newly_stale: false,
        }
    }

    fn log(&self, message: &str) {
        match &self.options.log {
            Some(f) => f(message),
            None => tracing::warn!("{message}"),
        }
    }

    fn idle(&self) -> u64 {
        self.options.idle_tick_ms.unwrap_or(IDLE_TICK_MS)
    }

    fn go_off(&mut self, why: &str) {
        if self.off.is_some() {
            return;
        }
        self.off = Some(GateReason::GitMissing);
        self.log(&format!("git epochs off: {why}"));
    }

    fn enter_pending(&mut self, now: u64) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.pending_since = now;
        self.quiet_since = now;
        self.held = self
            .options
            .disk_differs_from_synced
            .as_ref()
            .map_or_else(Vec::new, |f| f());
    }

    /// Reads the cheap state and updates pending, the quiet window, and the stale-lock bookkeeping.
    fn observe(&mut self, now: u64) -> (Vec<String>, Option<ReflogStat>) {
        let cs: CheapState = self.probe.cheap_state();
        let mut blocking = Vec::new();
        for m in &cs.markers {
            if m.kind == MarkerKind::Lock && now.saturating_sub(m.mtime_ms) > STALE_LOCK_MS {
                if self.stale_logged.insert(m.name.clone()) {
                    self.newly_stale = true;
                    self.log(&format!(
                        "ignoring stale lock {} (older than {} s); resuming",
                        m.name,
                        STALE_LOCK_MS / 1000
                    ));
                }
                continue;
            }
            blocking.push(m.name.clone());
        }
        self.stale_logged
            .retain(|n| cs.markers.iter().any(|m| &m.name == n));
        let sig = cs.reflog;
        if !blocking.is_empty() || sig != self.seen_sig {
            self.enter_pending(now);
        }
        let key = blocking.join("\n");
        if self.pending {
            if key.is_empty() && !self.blocking_key.is_empty() {
                self.pending_since = now; // the cap counts from the resume, not from the pause
            }
            if key != self.blocking_key || sig != self.seen_sig || self.newly_stale {
                self.quiet_since = now;
            }
        }
        self.blocking_key = key;
        self.seen_sig = sig;
        (blocking, sig)
    }

    fn state(&self, blocking: Vec<String>) -> GateState {
        if let Some(reason) = self.off {
            return GateState {
                mode: GateMode::Off,
                pending: false,
                markers: vec![],
                held: vec![],
                reason: Some(reason),
            };
        }
        if self.unborn {
            return GateState {
                mode: GateMode::Off,
                pending: false,
                markers: vec![],
                held: vec![],
                reason: Some(GateReason::Unborn),
            };
        }
        GateState {
            mode: if self.pending {
                GateMode::Closed
            } else {
                GateMode::Open
            },
            pending: self.pending,
            markers: blocking,
            held: if self.pending {
                self.held.clone()
            } else {
                vec![]
            },
            reason: None,
        }
    }

    fn baseline(&mut self) -> Result<(), ProbeError> {
        let head = self.probe.head_sha()?;
        self.last_head = head.clone();
        self.seen_head = head.clone();
        self.unborn = head.is_none();
        self.last_mark = self.probe.reflog_since(None)?.and_then(|s| s.tail);
        self.seen_sig = self.probe.cheap_state().reflog;
        self.initialized = true;
        Ok(())
    }

    fn try_baseline(&mut self) {
        match self.baseline() {
            Ok(()) => {}
            Err(ProbeError::Unavailable(m)) => self.go_off(&m),
            // Cannot resolve HEAD right now; the next tick retries.
            Err(ProbeError::Failed(_)) => {}
        }
    }

    /// Records the baseline: HEAD, the reflog tail, and the reflog signature.
    pub fn init(&mut self) {
        self.try_baseline();
    }

    /// Synchronous: marker stats and the reflog file only, no subprocess. Call before every sync.
    pub fn check(&mut self, now_ms: u64) -> GateState {
        if self.off.is_some() || self.unborn || !self.initialized {
            return self.state(vec![]);
        }
        let (blocking, _) = self.observe(now_ms);
        self.state(blocking)
    }

    /// A file system event. The peer's own writes are ignored; any other event restarts the quiet window.
    pub fn note_activity(&mut self, path: &str, now_ms: u64) {
        let own = self.options.is_own_write.as_ref().is_some_and(|f| f(path));
        if self.pending && !own {
            self.quiet_since = now_ms;
        }
    }

    pub fn tick(&mut self, now_ms: u64) -> TickResult {
        let none = |ms: u64| TickResult {
            actions: vec![],
            next_tick_ms: ms,
        };
        if self.off.is_some() {
            return none(self.idle());
        }
        if !self.initialized {
            self.try_baseline();
            return none(if self.off.is_some() {
                self.idle()
            } else {
                CONTROLLER_TICK_PENDING_MS
            });
        }
        let head = match self.probe.head_sha() {
            Ok(h) => h,
            Err(ProbeError::Unavailable(m)) => {
                self.go_off(&m);
                return none(self.idle());
            }
            Err(ProbeError::Failed(_)) => {
                // HEAD cannot be resolved right now: no change, keep re-checking.
                if !self.unborn {
                    self.observe(now_ms);
                }
                return none(CONTROLLER_TICK_PENDING_MS);
            }
        };
        if self.unborn {
            if head.is_none() {
                return none(self.idle());
            }
            self.initialized = false;
            self.unborn = false;
            self.try_baseline(); // first commit: adopt it without a decision
            return none(self.idle());
        }
        self.newly_stale = false;
        let (blocking, sig) = self.observe(now_ms);
        if head != self.last_head {
            self.enter_pending(now_ms);
            if head != self.seen_head {
                self.quiet_since = now_ms; // a new HEAD is activity; the same HEAD seen again is not
            }
        }
        self.seen_head = head.clone();
        if !self.pending {
            return none(self.idle());
        }
        let quiet =
            blocking.is_empty() && now_ms.saturating_sub(self.quiet_since) >= QUIET_WINDOW_MS;
        let capped =
            blocking.is_empty() && now_ms.saturating_sub(self.pending_since) >= DECISION_CAP_MS;
        if self.newly_stale || !(quiet || capped) {
            return none(CONTROLLER_TICK_PENDING_MS);
        }
        let slice = match self.probe.reflog_since(self.last_mark.as_ref()) {
            Ok(s) => s,
            Err(_) => return none(CONTROLLER_TICK_PENDING_MS),
        };
        let tracked = match self.probe.ls_tree() {
            Ok(t) => t,
            Err(_) => return none(CONTROLLER_TICK_PENDING_MS),
        };
        let decision = Decision {
            head: head.clone(),
            reflog: slice.as_ref().map(|s| s.subjects.clone()),
            held: std::mem::take(&mut self.held),
            capped: capped && !quiet,
            tracked,
            jj: self.probe.is_colocated_jj(),
        };
        self.last_head = head;
        if let Some(tail) = slice.and_then(|s| s.tail) {
            self.last_mark = Some(tail);
        }
        self.seen_sig = sig;
        self.pending = false;
        TickResult {
            actions: vec![Action::Decision(decision)],
            next_tick_ms: self.idle(),
        }
    }
}
