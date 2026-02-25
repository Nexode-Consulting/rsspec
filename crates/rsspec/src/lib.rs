//! # rsspec — A Ginkgo/RSpec-inspired BDD testing framework for Rust
//!
//! Write expressive, structured tests using a closure-based API with
//! `describe`, `context`, `it`, lifecycle hooks, table-driven tests, and more.
//!
//! ## Quick example
//!
//! ```rust,no_run
//! fn main() {
//!     rsspec::run(|ctx| {
//!         ctx.describe("Calculator", |ctx| {
//!             ctx.it("adds two numbers", || {
//!                 assert_eq!(2 + 3, 5);
//!             });
//!
//!             ctx.context("with negative numbers", |ctx| {
//!                 ctx.it("handles negatives", || {
//!                     assert_eq!(-1 + 1, 0);
//!                 });
//!             });
//!         });
//!     });
//! }
//! ```
//!
//! ## Features
//!
//! - `googletest` — re-exports `googletest` matchers via `rsspec::matchers`

pub mod runner;
mod context;
pub mod ordered;
pub mod table;

pub use context::{Context, ItBuilder, run};

/// Re-export of the [`googletest`] crate. Available with the `googletest` feature.
#[cfg(feature = "googletest")]
pub use googletest;

/// Composable matchers re-exported from [`googletest::prelude`].
#[cfg(feature = "googletest")]
pub mod matchers {
    pub use googletest::prelude::*;
}

use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::cell::RefCell;

/// A drop guard that runs cleanup code even if the test panics.
pub struct Guard<F: FnOnce()> {
    f: Option<F>,
}

impl<F: FnOnce()> Guard<F> {
    pub fn new(f: F) -> Self {
        Guard { f: Some(f) }
    }
}

impl<F: FnOnce()> Drop for Guard<F> {
    fn drop(&mut self) {
        if let Some(f) = self.f.take() {
            f();
        }
    }
}

/// Check if the current test's labels match the `RSSPEC_LABEL_FILTER` env var.
///
/// Filter syntax:
/// - `integration` — matches if any label equals "integration"
/// - `!slow` — matches if no label equals "slow"
/// - `integration,smoke` — OR: matches if any label matches any filter term
/// - `integration+fast` — AND: matches if labels include all filter terms
///
/// Returns `true` (run the test) if no filter is set.
pub fn check_labels(labels: &[&str]) -> bool {
    let filter = match std::env::var("RSSPEC_LABEL_FILTER") {
        Ok(f) if !f.is_empty() => f,
        _ => return true,
    };

    // AND filter: "a+b" means all must match
    if filter.contains('+') {
        return filter
            .split('+')
            .all(|term| labels.contains(&term.trim()));
    }

    // OR filter: "a,b" means any must match
    filter.split(',').any(|term| {
        let term = term.trim();
        if let Some(negated) = term.strip_prefix('!') {
            !labels.contains(&negated)
        } else {
            labels.contains(&term)
        }
    })
}

/// Retry a test function up to `retries` additional times on failure.
pub fn with_retries(retries: u32, f: impl Fn()) {
    let max_attempts = retries + 1;
    let mut last_panic = None;

    for attempt in 1..=max_attempts {
        match catch_unwind(AssertUnwindSafe(&f)) {
            Ok(()) => return,
            Err(e) => {
                if attempt < max_attempts {
                    eprintln!("  attempt {attempt}/{max_attempts} failed, retrying...");
                }
                last_panic = Some(e);
            }
        }
    }

    if let Some(e) = last_panic {
        resume_unwind(e);
    }
}

/// Require a test to pass `n` consecutive times.
pub fn must_pass_repeatedly(n: u32, f: impl Fn()) {
    for attempt in 1..=n {
        if let Err(e) = catch_unwind(AssertUnwindSafe(&f)) {
            eprintln!("  must_pass_repeatedly: failed on attempt {attempt}/{n}");
            resume_unwind(e);
        }
    }
}

/// Panics if `RSSPEC_FAIL_ON_FOCUS` is set and focus mode is active.
pub fn check_fail_on_focus() {
    if let Ok(val) = std::env::var("RSSPEC_FAIL_ON_FOCUS") {
        if val == "1" || val.eq_ignore_ascii_case("true") {
            panic!(
                "rsspec: focused tests detected but RSSPEC_FAIL_ON_FOCUS is set. \
                 Remove fit/fdescribe/fcontext before pushing."
            );
        }
    }
}

// ============================================================================
// DeferCleanup — LIFO cleanup stack
// ============================================================================

thread_local! {
    static CLEANUP_STACK: RefCell<Vec<Box<dyn FnOnce()>>> = RefCell::new(Vec::new());
}

/// Register a cleanup function that will run after the current test completes.
///
/// Cleanup functions run in LIFO (last-registered-first) order.
pub fn defer_cleanup(f: impl FnOnce() + 'static) {
    CLEANUP_STACK.with(|stack| {
        stack.borrow_mut().push(Box::new(f));
    });
}

/// Run all deferred cleanup functions.
pub fn run_deferred_cleanups() {
    CLEANUP_STACK.with(|stack| {
        let mut cleanups: Vec<Box<dyn FnOnce()>> = stack.borrow_mut().drain(..).collect();
        cleanups.reverse();
        for cleanup in cleanups {
            cleanup();
        }
    });
}

// ============================================================================
// By — step documentation
// ============================================================================

/// Document a step within a test. Prints the step description to stderr.
pub fn by(description: &str) {
    eprintln!("  STEP: {description}");
}

// ============================================================================
// Skip — runtime test skipping
// ============================================================================

/// Skip the current test at runtime with a reason.
pub fn skip(reason: &str) {
    eprintln!("  SKIPPED: {reason}");
}

/// Skip the current test at runtime. Prints the reason and returns from the test.
#[macro_export]
macro_rules! skip {
    ($reason:expr) => {{
        rsspec::skip($reason);
        return;
    }};
}

/// Document a step within a test (macro form).
#[macro_export]
macro_rules! by {
    ($description:expr) => {
        rsspec::by($description);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guard_runs_on_success() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static RAN: AtomicBool = AtomicBool::new(false);

        {
            let _g = Guard::new(|| RAN.store(true, Ordering::SeqCst));
        }
        assert!(RAN.load(Ordering::SeqCst));
    }

    #[test]
    fn test_guard_runs_on_panic() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static RAN: AtomicBool = AtomicBool::new(false);

        let result = catch_unwind(AssertUnwindSafe(|| {
            let _g = Guard::new(|| RAN.store(true, Ordering::SeqCst));
            panic!("boom");
        }));
        assert!(result.is_err());
        assert!(RAN.load(Ordering::SeqCst));
    }

    #[test]
    fn test_check_labels_no_filter() {
        std::env::remove_var("RSSPEC_LABEL_FILTER");
        assert!(check_labels(&["integration"]));
        assert!(check_labels(&[]));
    }

    #[test]
    fn test_with_retries_success_first_try() {
        with_retries(3, || {
            assert_eq!(1, 1);
        });
    }

    #[test]
    fn test_with_retries_eventual_success() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static ATTEMPTS: AtomicU32 = AtomicU32::new(0);
        ATTEMPTS.store(0, Ordering::SeqCst);

        with_retries(3, || {
            let n = ATTEMPTS.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                panic!("not yet");
            }
        });

        assert_eq!(ATTEMPTS.load(Ordering::SeqCst), 3);
    }
}
