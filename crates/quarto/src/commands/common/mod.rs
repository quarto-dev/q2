//! Machinery shared by the file-producing commands (`q2 create`,
//! `q2 use`).
//!
//! Extracted from `commands/create/` when `q2 use brand` landed
//! (bd-1vlw8). Both commands resolve user intent into a [`FilePlan`]
//! and hand it to one [`writer::execute_plan`], so they cannot drift on
//! the questions that matter to users: what counts as "already exists",
//! whether a dry run reports the same thing a real run does, and how a
//! failure is rendered on the human and JSON front doors.
//!
//! The plan is deliberately declarative — preconditions, files, and
//! edits are *data*, not callbacks — which is what lets `--dry-run`
//! compute the identical answer without a parallel code path.

pub mod plan;
pub mod prompter;
pub mod writer;
