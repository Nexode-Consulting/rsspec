//! # rsspec — A Ginkgo/RSpec-inspired BDD testing framework for Rust
//!
//! Write your tests in a familiar BDD style:
//!
//! ```rust
//! rsspec::suite! {
//!     describe "Calculator" {
//!         before_each {
//!             let result: i32;
//!         }
//!
//!         it "adds two numbers" {
//!             let result = 2 + 3;
//!             assert_eq!(result, 5);
//!         }
//!
//!         context "with negative numbers" {
//!             it "handles negatives" {
//!                 let result = -1 + 3;
//!                 assert_eq!(result, 2);
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! See the [`suite!`] macro documentation for the full DSL reference.

#[cfg(feature = "macros")]
pub use rsspec_macros::suite;

use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

/// A drop guard that runs cleanup code (after_each) even if the test panics.
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
        _ => return true, // No filter → run everything
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
///
/// The test passes if any attempt succeeds. If all attempts fail,
/// the panic from the last attempt is propagated.
pub fn with_retries(retries: u32, f: impl Fn()) {
    let max_attempts = retries + 1;
    let mut last_panic = None;

    for attempt in 1..=max_attempts {
        match catch_unwind(AssertUnwindSafe(&f)) {
            Ok(()) => return, // Success
            Err(e) => {
                if attempt < max_attempts {
                    eprintln!(
                        "  attempt {attempt}/{max_attempts} failed, retrying..."
                    );
                }
                last_panic = Some(e);
            }
        }
    }

    if let Some(e) = last_panic {
        resume_unwind(e);
    }
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
        // No env var set → always true
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
