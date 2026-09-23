//! Which documents may execute code during a render (bd-sl79jjiq,
//! plan `claude-notes/plans/2026-09-22-q2-preview-static.md` § Lazy
//! code execution).
//!
//! `q2 preview --static` renders a whole site eagerly but only wants to
//! *execute* the pages the user is looking at: 300 markdown pages are
//! cheap, 300 Jupyter pages are not, 299 markdown pages plus the one
//! Jupyter page on screen is fine. The policy is consulted by
//! `EngineExecutionStage` right after engine resolution: a document it
//! excludes that would have run a non-markdown engine passes through
//! with its code cells inert, emits no diagnostic, and is marked
//! `execution_skipped` on its output so the caller knows which pages
//! still owe an execution.
//!
//! This is deliberately a first-class knob rather than a registry
//! without engines: an unregistered engine is a *warning* ("not
//! available in this build"), and a skipped one must be silent.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Which documents may execute code during this render.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ExecutionPolicy {
    /// Every document — what `q2 render` does.
    #[default]
    All,
    /// No document. Code cells pass through inert, silently.
    None,
    /// Only documents whose input path is in the set (absolute paths,
    /// as `DocumentInfo::input` carries them).
    Only(BTreeSet<PathBuf>),
}

impl ExecutionPolicy {
    /// May `input` execute its code under this policy?
    pub fn allows(&self, input: &Path) -> bool {
        match self {
            ExecutionPolicy::All => true,
            ExecutionPolicy::None => false,
            ExecutionPolicy::Only(inputs) => inputs.contains(input),
        }
    }
}
