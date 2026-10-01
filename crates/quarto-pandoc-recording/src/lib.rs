//! Dev tooling for recordings of native pandoc runs: the capture script
//! (`scripts/pandoc-capture.sh`) leaves a raw directory; [`rewrite::rewrite_run`]
//! normalizes it into a committed, machine-independent recording; and
//! [`replay::replay`] re-runs it with a native pandoc.

pub mod mapping;
pub mod replay;
pub mod rewrite;
pub mod tree;
