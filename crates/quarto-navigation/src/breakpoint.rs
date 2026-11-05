/*
 * breakpoint.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Bootstrap 5 grid breakpoints, and the visibility classes that show
//! content only within a breakpoint range.
//!
//! The names and order match Bootstrap's `$grid-breakpoints` map
//! (`xs 0, sm 576px, md 768px, lg 992px, xl 1200px, xxl 1400px`). The
//! pixel values live in SCSS, not here: the classes emitted by
//! [`range_classes`] are Bootstrap's `d-{bp}-*` display utilities, which
//! compile from the same map as `navbar-expand-*`, so a theme that moves
//! a breakpoint keeps the two in step.

/// A Bootstrap grid breakpoint, in ascending width order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Breakpoint {
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
    Xxl,
}

impl Breakpoint {
    /// Every breakpoint, ascending.
    pub const ALL: [Breakpoint; 6] = [
        Breakpoint::Xs,
        Breakpoint::Sm,
        Breakpoint::Md,
        Breakpoint::Lg,
        Breakpoint::Xl,
        Breakpoint::Xxl,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Breakpoint::Xs => "xs",
            Breakpoint::Sm => "sm",
            Breakpoint::Md => "md",
            Breakpoint::Lg => "lg",
            Breakpoint::Xl => "xl",
            Breakpoint::Xxl => "xxl",
        }
    }

    // Deliberate Option-returning parser, matching `CollapseBelow`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|bp| bp.as_str() == s)
    }
}

/// Bootstrap display-utility classes that make an inline element
/// visible only for viewport widths in `[from, until)`.
///
/// `until: None` means "and every larger width". Bootstrap's utilities
/// are mobile-first (`min-width`) and `!important`, and larger
/// breakpoints are emitted later in the stylesheet, so
/// `d-none d-md-inline d-xl-none` shows the element from `md` up to,
/// but not including, `xl`.
///
/// `[xs, ∞)` needs no classes and yields an empty list.
pub fn range_classes(from: Breakpoint, until: Option<Breakpoint>) -> Vec<String> {
    let mut classes = Vec::new();
    if from != Breakpoint::Xs {
        classes.push("d-none".to_string());
        classes.push(format!("d-{}-inline", from.as_str()));
    }
    if let Some(until) = until {
        classes.push(format!("d-{}-none", until.as_str()));
    }
    classes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_roundtrips_every_name() {
        for bp in Breakpoint::ALL {
            assert_eq!(Breakpoint::from_str(bp.as_str()), Some(bp));
        }
        assert_eq!(Breakpoint::from_str("md2"), None);
        assert_eq!(Breakpoint::from_str("LG"), None);
    }

    #[test]
    fn ordering_is_ascending_width() {
        assert!(Breakpoint::Xs < Breakpoint::Sm);
        assert!(Breakpoint::Xl < Breakpoint::Xxl);
    }

    #[test]
    fn range_classes_cover_each_interval_shape() {
        assert!(range_classes(Breakpoint::Xs, None).is_empty());
        assert_eq!(
            range_classes(Breakpoint::Xs, Some(Breakpoint::Lg)),
            vec!["d-lg-none"]
        );
        assert_eq!(
            range_classes(Breakpoint::Lg, Some(Breakpoint::Xl)),
            vec!["d-none", "d-lg-inline", "d-xl-none"]
        );
        assert_eq!(
            range_classes(Breakpoint::Xxl, None),
            vec!["d-none", "d-xxl-inline"]
        );
    }
}
