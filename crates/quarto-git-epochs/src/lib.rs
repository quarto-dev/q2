//! Git epochs: the Rust half of the shared logic.
//!
//! The same six modules exist in the TypeScript package `@quarto/quarto-git-epochs` (`classify`, `arrival`,
//! `startup`, `registry`, `probe`, `controller`), built against one contract
//! (`claude-notes/designs/git-epoch-registry-contract.md`) and one set of fixtures
//! (`ts-packages/quarto-git-epochs/fixtures/`). The pure modules take and return plain data. The probe runs
//! git as a subprocess (plumbing only). The controller is a state machine driven by explicit ticks and an
//! explicit clock; it never touches Automerge or the hub.

pub mod arrival;
pub mod classify;
pub mod controller;
pub mod numbers;
pub mod probe;
pub mod registry;
pub mod startup;
pub mod vocabulary;
