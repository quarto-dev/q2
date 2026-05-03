//! GitHub Pages provider.
//!
//! **Phase 0:** trait shape only. `publish_record` and
//! `authorize_token` are real (Phase 1 fills them in fully); the
//! `prepare` and `commit` methods `unimplemented!()` so any
//! attempt to drive an actual publish lights up the unfinished
//! work clearly.

pub mod provider;

pub use provider::GhPagesProvider;
