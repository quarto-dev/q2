/*
 * engine/jupyter/router.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-2 Phase 5: the `kernel_for(unit)` seam and the pure planning/splicing
 * helpers the Jupyter text executor's loop is built from.
 */

//! The kernel-routing seam of the Jupyter text executor.
//!
//! [`KernelRouter::kernel_for`] answers "which kernel runs this unit?" for one
//! [`Unit`]. Today every unit goes to the one kernel chosen for the document
//! ([`SingleKernelRouter`]); JOL-2a/2b supply the kernel choice behind it and a
//! reopened JOL-3 (several kernels per document, deferred) can vary the answer
//! per unit without a change to the execution loop.
//!
//! [`plan_units`] and [`splice_units`] are the two pure halves of that loop,
//! kept free of the daemon so they can be tested without a kernel.

use std::ops::Range;

use crate::engine::error::ExecutionError;
use crate::engine::units::Unit;

/// A Jupyter kernel name (`python3`, `julia`, `ir`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KernelName(pub String);

impl KernelName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Chooses the kernel that runs one unit.
pub trait KernelRouter {
    fn kernel_for(&self, unit: &Unit) -> Result<KernelName, ExecutionError>;
}

/// The default router: one kernel for every unit, chosen once for the document
/// (today's behavior).
#[derive(Debug, Clone)]
pub struct SingleKernelRouter {
    kernel: KernelName,
}

impl SingleKernelRouter {
    pub fn new(kernel: impl Into<String>) -> Self {
        Self {
            kernel: KernelName(kernel.into()),
        }
    }
}

impl KernelRouter for SingleKernelRouter {
    fn kernel_for(&self, _unit: &Unit) -> Result<KernelName, ExecutionError> {
        Ok(self.kernel.clone())
    }
}

/// Ask the router once per unit, in document order. Each entry is the unit's
/// position in `units` and the kernel that runs it. The first router error
/// stops the plan.
pub fn plan_units(
    units: &[Unit],
    router: &dyn KernelRouter,
) -> Result<Vec<(usize, KernelName)>, ExecutionError> {
    units
        .iter()
        .enumerate()
        .map(|(position, unit)| Ok((position, router.kernel_for(unit)?)))
        .collect()
}

/// Replace byte ranges of `input` with rendered text, copying the gaps between
/// them verbatim. `replacements` are in document order and do not overlap.
///
/// A replacement is a `::: {.cell}` fenced div, which cannot interrupt a
/// paragraph, so when the copied gap ends in a single newline a blank line is
/// added to keep the opener in its own block even when the source had no blank
/// line before the cell.
pub fn splice_units(input: &str, replacements: &[(Range<usize>, String)]) -> String {
    let mut output = String::with_capacity(input.len());
    let mut last_end = 0;
    for (range, text) in replacements {
        output.push_str(&input[last_end..range.start]);
        if output.ends_with('\n') && !output.ends_with("\n\n") {
            output.push('\n');
        }
        output.push_str(text);
        last_end = range.end;
    }
    output.push_str(&input[last_end..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::units::scan_units;
    use std::cell::RefCell;

    /// Routes `python` to one kernel and everything else to another, and
    /// records every unit it was asked about.
    struct StubRouter {
        asked: RefCell<Vec<String>>,
    }

    impl KernelRouter for StubRouter {
        fn kernel_for(&self, unit: &Unit) -> Result<KernelName, ExecutionError> {
            self.asked.borrow_mut().push(unit.lang().to_string());
            Ok(KernelName(
                if unit.lang() == "python" {
                    "python3"
                } else {
                    "julia"
                }
                .to_string(),
            ))
        }
    }

    const DOC: &str = "```{python}\na\n```\n\n```{julia}\nb\n```\n\n`{python} 1`\n";

    #[test]
    fn plan_units_asks_the_router_once_per_unit_in_document_order() {
        let units = scan_units(DOC);
        let router = StubRouter {
            asked: RefCell::new(Vec::new()),
        };
        let plan = plan_units(&units, &router).unwrap();
        assert_eq!(*router.asked.borrow(), ["python", "julia", "python"]);
        assert_eq!(
            plan,
            [
                (0, KernelName("python3".into())),
                (1, KernelName("julia".into())),
                (2, KernelName("python3".into())),
            ]
        );
    }

    #[test]
    fn single_kernel_router_gives_every_unit_the_one_kernel() {
        let units = scan_units(DOC);
        let plan = plan_units(&units, &SingleKernelRouter::new("python3")).unwrap();
        assert_eq!(plan.len(), 3);
        assert!(plan.iter().all(|(_, k)| k.as_str() == "python3"));
    }

    #[test]
    fn plan_units_stops_at_the_first_router_error() {
        struct Failing;
        impl KernelRouter for Failing {
            fn kernel_for(&self, unit: &Unit) -> Result<KernelName, ExecutionError> {
                if unit.lang() == "julia" {
                    Err(ExecutionError::no_handler_for_language("jupyter", "julia"))
                } else {
                    Ok(KernelName("python3".into()))
                }
            }
        }
        let err = plan_units(&scan_units(DOC), &Failing).unwrap_err();
        assert!(matches!(err, ExecutionError::NoHandlerForLanguage { .. }));
    }

    #[test]
    fn splice_units_with_no_replacements_is_the_identity() {
        assert_eq!(splice_units(DOC, &[]), DOC);
    }

    #[test]
    fn splice_units_copies_the_gaps_and_the_tail() {
        let input = "before\n\nCELL\n\nafter\n";
        let start = input.find("CELL").unwrap();
        let out = splice_units(input, &[(start..start + 4, "::: {.cell}\n:::".to_string())]);
        assert_eq!(out, "before\n\n::: {.cell}\n:::\n\nafter\n");
    }

    #[test]
    fn splice_units_keeps_a_fenced_div_in_its_own_block() {
        // No blank line before the cell in the source: one is added.
        let input = "text\nCELL\nmore\n";
        let start = input.find("CELL").unwrap();
        let out = splice_units(input, &[(start..start + 4, "::: {.cell}\n:::".to_string())]);
        assert_eq!(out, "text\n\n::: {.cell}\n:::\nmore\n");
        // Already separated: nothing extra.
        let input = "text\n\nCELL\n";
        let start = input.find("CELL").unwrap();
        let out = splice_units(input, &[(start..start + 4, "X".to_string())]);
        assert_eq!(out, "text\n\nX\n");
    }

    #[test]
    fn splice_units_handles_several_replacements_in_order() {
        let input = "a\n\n1\n\nb\n\n2\n\nc\n";
        let one = input.find('1').unwrap();
        let two = input.find('2').unwrap();
        let out = splice_units(
            input,
            &[
                (one..one + 1, "ONE".to_string()),
                (two..two + 1, "TWO".to_string()),
            ],
        );
        assert_eq!(out, "a\n\nONE\n\nb\n\nTWO\n\nc\n");
    }
}
