//! # rsspec — A Ginkgo/RSpec-inspired BDD testing framework for Rust
//!
//! Write expressive, structured tests using a familiar BDD syntax with
//! `describe`, `context`, `it`, lifecycle hooks, table-driven tests, and more.
//!
//! ## Three ways to run tests
//!
//! - **[`suite!`]** — generates `#[test]` functions, works with `cargo test`
//! - **[`bdd!`]** — generates a `main()` with colored tree output (`harness = false`)
//! - **[`bdd_suite!`]** — returns test nodes for combining multiple suites
//!
//! ## Quick example
//!
//! ```rust
//! rsspec::suite! {
//!     describe "Calculator" {
//!         before_each {
//!             let a = 2;
//!             let b = 3;
//!         }
//!
//!         subject { a + b }
//!
//!         it "adds two numbers" {
//!             assert_eq!(subject, 5);
//!         }
//!
//!         context "with negative numbers" {
//!             it "handles negatives" {
//!                 assert_eq!(-1 + b, 2);
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! ## Features
//!
//! - `macros` *(default)* — enables `suite!`, `bdd!`, `bdd_suite!` macros
//! - `googletest` — re-exports `googletest` matchers via `rsspec::matchers`
//!
//! See the [`suite!`] macro documentation for the full DSL reference.

pub mod runner;

/// Re-export of the [`googletest`] crate. Available with the `googletest` feature.
#[cfg(feature = "googletest")]
pub use googletest;

/// Composable matchers re-exported from [`googletest::prelude`].
///
/// Enable with `features = ["googletest"]`, then:
///
/// ```rust,ignore
/// use rsspec::matchers::*;
///
/// assert_that!(vec![1, 2, 3], contains(eq(2)));
/// ```
#[cfg(feature = "googletest")]
pub mod matchers {
    pub use googletest::prelude::*;
}

#[cfg(feature = "macros")]
pub use rsspec_macros::suite;

#[cfg(feature = "macros")]
pub use rsspec_macros::bdd;

#[cfg(feature = "macros")]
pub use rsspec_macros::bdd_suite;

use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::cell::RefCell;

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

/// Require a test to pass `n` consecutive times. If any run fails, the test fails.
///
/// This is the inverse of `with_retries` — useful to verify that a previously flaky
/// test is truly fixed.
pub fn must_pass_repeatedly(n: u32, f: impl Fn()) {
    for attempt in 1..=n {
        if let Err(e) = catch_unwind(AssertUnwindSafe(&f)) {
            eprintln!("  must_pass_repeatedly: failed on attempt {attempt}/{n}");
            resume_unwind(e);
        }
    }
}

/// Run a test with a timeout (in milliseconds).
///
/// If the test does not complete within the given duration, it panics.
/// Note: the test body runs on a separate thread.
pub fn with_timeout(timeout_ms: u64, f: impl FnOnce() + Send + 'static) {
    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        // Guard ensures deferred cleanups run on this thread even on panic,
        // since the cleanup stack is thread-local and the outer thread can't drain it.
        let _cleanup_guard = Guard::new(run_deferred_cleanups);
        f();
        let _ = tx.send(());
    });

    match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(()) => {
            handle.join().expect("test thread panicked");
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // We can't kill the thread, but we can fail the test.
            panic!("test timed out after {timeout_ms}ms");
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            // The thread panicked before sending — propagate the panic.
            if let Err(e) = handle.join() {
                std::panic::resume_unwind(e);
            }
        }
    }
}

/// Panics if `RSSPEC_FAIL_ON_FOCUS` is set and focus mode is active.
///
/// This is used in CI to prevent accidentally committing focused tests.
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
/// Cleanup functions run in LIFO (last-registered-first) order, similar to Go's
/// `defer` or Ginkgo's `DeferCleanup`.
///
/// # Example
/// ```rust,no_run
/// # fn teardown_database() {}
/// # fn main() {
/// rsspec::defer_cleanup(|| {
///     teardown_database();
/// });
/// # }
/// ```
pub fn defer_cleanup(f: impl FnOnce() + 'static) {
    CLEANUP_STACK.with(|stack| {
        stack.borrow_mut().push(Box::new(f));
    });
}

/// Run all deferred cleanup functions. Called automatically by generated test code.
pub fn run_deferred_cleanups() {
    CLEANUP_STACK.with(|stack| {
        let mut cleanups: Vec<Box<dyn FnOnce()>> = stack.borrow_mut().drain(..).collect();
        // LIFO order
        cleanups.reverse();
        for cleanup in cleanups {
            cleanup();
        }
    });
}

// ============================================================================
// AfterAll — counter-based "run once after all tests complete"
// ============================================================================

/// Helper for after_all: tracks how many tests in a scope have completed.
/// When the count reaches `total`, the cleanup function runs.
pub struct AfterAllGuard {
    counter: &'static std::sync::atomic::AtomicU32,
    total: u32,
    body: Option<Box<dyn FnOnce()>>,
}

impl AfterAllGuard {
    pub fn new(
        counter: &'static std::sync::atomic::AtomicU32,
        total: u32,
        body: impl FnOnce() + 'static,
    ) -> Self {
        AfterAllGuard {
            counter,
            total,
            body: Some(Box::new(body)),
        }
    }
}

impl Drop for AfterAllGuard {
    fn drop(&mut self) {
        let count = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        if count >= self.total {
            if let Some(f) = self.body.take() {
                f();
            }
        }
    }
}

// ============================================================================
// By — step documentation
// ============================================================================

/// Document a step within a test. Prints the step description to stderr.
///
/// Equivalent to Ginkgo's `By("description")`.
///
/// # Example
/// ```rust,no_run
/// # fn main() {
/// rsspec::by("setting up the database");
/// // ...setup...
/// rsspec::by("inserting the user");
/// // ...insert...
/// # }
/// ```
pub fn by(description: &str) {
    eprintln!("  STEP: {description}");
}

// ============================================================================
// Skip — runtime test skipping
// ============================================================================

/// Skip the current test at runtime with a reason.
///
/// This prints the skip reason and returns early from the test.
/// Must be used with `return` at the call site (generated by the `skip!` helper).
///
/// # Example
/// ```rust,no_run
/// # fn database_available() -> bool { false }
/// # fn test() {
/// if !database_available() {
///     rsspec::skip("database not available");
///     return;
/// }
/// # }
/// # fn main() {}
/// ```
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
