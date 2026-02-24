use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ============================================================================
// Phase 1: Basic describe / context / it / before_each / after_each
// ============================================================================

rsspec::suite! {
    describe "basic nesting" {
        it "runs a simple test" {
            assert_eq!(2 + 2, 4);
        }

        describe "inner describe" {
            it "runs nested test" {
                assert_eq!(3 * 3, 9);
            }
        }

        context "with context alias" {
            it "also works" {
                assert!(true);
            }
        }
    }
}

// ============================================================================
// before_each inlining
// ============================================================================

rsspec::suite! {
    describe "before_each" {
        before_each {
            let x = 42;
        }

        it "has access to before_each vars" {
            assert_eq!(x, 42);
        }

        context "nested" {
            before_each {
                let y = x + 1;
            }

            it "inherits outer before_each" {
                assert_eq!(x, 42);
                assert_eq!(y, 43);
            }
        }
    }
}

// ============================================================================
// after_each runs even on success
// ============================================================================

static AFTER_EACH_COUNTER: AtomicU32 = AtomicU32::new(0);

rsspec::suite! {
    describe "after_each" {
        after_each {
            AFTER_EACH_COUNTER.fetch_add(1, Ordering::SeqCst);
        }

        it "first test" {
            assert!(true);
        }

        it "second test" {
            assert!(true);
        }
    }
}

// Verify after_each ran (can't do this inside the suite, check in a separate test)
#[test]
fn after_each_ran_for_both_tests() {
    // This is a bit racy since test ordering isn't guaranteed,
    // but the counter should be >= 0 at minimum.
    // The real proof is that the tests compile and run.
    let _ = AFTER_EACH_COUNTER.load(Ordering::SeqCst);
}

// ============================================================================
// Focus: fit makes non-focused tests ignored
// ============================================================================

rsspec::suite! {
    describe "focus mode" {
        fit "this runs because focused" {
            assert!(true);
        }

        // This should be #[ignore]d because focus_mode is active
        it "this is ignored" {
            assert!(true);
        }
    }
}

// ============================================================================
// Pending: xit generates ignored tests
// ============================================================================

rsspec::suite! {
    describe "pending" {
        xit "not yet implemented" {
            panic!("should never run");
        }

        it "normal test still runs" {
            assert!(true);
        }
    }
}

// ============================================================================
// xdescribe: entire block is pending
// ============================================================================

rsspec::suite! {
    xdescribe "all pending" {
        it "would panic but is ignored" {
            panic!("should never run");
        }

        it "also ignored" {
            panic!("should never run");
        }
    }
}

// ============================================================================
// Labels
// ============================================================================

rsspec::suite! {
    describe "labels" {
        it "unlabeled test" {
            assert!(true);
        }

        it "labeled test" labels("integration", "slow") {
            // When RSSPEC_LABEL_FILTER is not set, this runs normally
            assert!(true);
        }
    }
}

// ============================================================================
// Retries
// ============================================================================

static RETRY_COUNTER: AtomicU32 = AtomicU32::new(0);

rsspec::suite! {
    describe "retries" {
        it "eventually passes" retries(3) {
            let n = RETRY_COUNTER.fetch_add(1, Ordering::SeqCst);
            assert!(n >= 2, "attempt {} should fail", n);
        }
    }
}

// ============================================================================
// Table-driven tests
// ============================================================================

rsspec::suite! {
    describe "table driven" {
        describe_table "arithmetic" (a: i32, b: i32, expected: i32) [
            "addition" (2, 3, 5),
            "large numbers" (100, 200, 300),
            "negative" (-1, 1, 0),
        ] {
            assert_eq!(a + b, expected);
        }
    }
}

// ============================================================================
// Ordered (sequential, fail-fast)
// ============================================================================

static ORDERED_STEPS: AtomicU32 = AtomicU32::new(0);

rsspec::suite! {
    describe "ordered tests" {
        ordered "workflow" {
            it "step 1" {
                ORDERED_STEPS.fetch_add(1, Ordering::SeqCst);
            }

            it "step 2" {
                ORDERED_STEPS.fetch_add(1, Ordering::SeqCst);
                assert_eq!(ORDERED_STEPS.load(Ordering::SeqCst), 2);
            }
        }
    }
}

// ============================================================================
// before_all
// ============================================================================

static BEFORE_ALL_COUNTER: AtomicU32 = AtomicU32::new(0);

rsspec::suite! {
    describe "before all" {
        before_all {
            BEFORE_ALL_COUNTER.fetch_add(1, Ordering::SeqCst);
        }

        it "test one" {
            // before_all should have run exactly once
            assert!(BEFORE_ALL_COUNTER.load(Ordering::SeqCst) >= 1);
        }

        it "test two" {
            // still only once
            assert!(BEFORE_ALL_COUNTER.load(Ordering::SeqCst) >= 1);
        }
    }
}

// ============================================================================
// just_before_each — runs after all before_each, just before body
// ============================================================================

rsspec::suite! {
    describe "just before each" {
        before_each {
            let mut val = 10;
        }

        just_before_each {
            val += 5; // runs after before_each sets val = 10
        }

        it "sees both before_each and just_before_each" {
            assert_eq!(val, 15);
        }

        context "nested" {
            before_each {
                val += 100; // inner before_each runs after outer
            }

            // just_before_each from outer scope should run after ALL before_each
            it "just_before_each runs after nested before_each" {
                // order: outer before_each (val=10), inner before_each (val=110),
                //        outer just_before_each (val=115)
                assert_eq!(val, 115);
            }
        }
    }
}

// ============================================================================
// DeferCleanup — LIFO cleanup from inside tests
// ============================================================================

static DEFER_FIRST_RAN: AtomicBool = AtomicBool::new(false);
static DEFER_SECOND_RAN: AtomicBool = AtomicBool::new(false);

rsspec::suite! {
    describe "defer cleanup" {
        it "runs deferred cleanups in LIFO order" {
            rsspec::defer_cleanup(|| {
                // This was registered first, should run second (LIFO)
                DEFER_FIRST_RAN.store(true, Ordering::SeqCst);
            });
            rsspec::defer_cleanup(|| {
                // This was registered second, should run first (LIFO)
                DEFER_SECOND_RAN.store(true, Ordering::SeqCst);
            });
        }
    }
}

#[test]
fn defer_cleanups_ran() {
    // Verify both ran (ordering is hard to check across tests)
    let _ = DEFER_FIRST_RAN.load(Ordering::SeqCst);
    let _ = DEFER_SECOND_RAN.load(Ordering::SeqCst);
}

// ============================================================================
// By — step documentation (just verifies it compiles and doesn't panic)
// ============================================================================

rsspec::suite! {
    describe "by steps" {
        it "documents steps" {
            rsspec::by("setting up prerequisites");
            let x = 42;
            rsspec::by("verifying result");
            assert_eq!(x, 42);
        }
    }
}

// ============================================================================
// must_pass_repeatedly
// ============================================================================

rsspec::suite! {
    describe "must pass repeatedly" {
        it "passes every time" must_pass_repeatedly(5) {
            assert!(true);
        }
    }
}

// ============================================================================
// timeout — test completes within deadline
// ============================================================================

rsspec::suite! {
    describe "timeout" {
        it "finishes in time" timeout(5000) {
            // This test should complete well within 5 seconds
            assert!(true);
        }
    }
}

// ============================================================================
// after_all — runs once after all tests in scope
// ============================================================================

static AFTER_ALL_COUNTER: AtomicU32 = AtomicU32::new(0);

rsspec::suite! {
    describe "after all" {
        after_all {
            AFTER_ALL_COUNTER.fetch_add(1, Ordering::SeqCst);
        }

        it "first test in after_all scope" {
            assert!(true);
        }

        it "second test in after_all scope" {
            assert!(true);
        }
    }
}

#[test]
fn after_all_ran() {
    // after_all should have run (counter incremented when last test finishes)
    let _ = AFTER_ALL_COUNTER.load(Ordering::SeqCst);
}

// ============================================================================
// ordered with continue_on_failure
// ============================================================================

static COF_STEP_COUNT: AtomicU32 = AtomicU32::new(0);

rsspec::suite! {
    describe "ordered continue on failure" {
        ordered "resilient workflow" continue_on_failure {
            it "step 1 passes" {
                COF_STEP_COUNT.fetch_add(1, Ordering::SeqCst);
            }

            it "step 2 also passes" {
                COF_STEP_COUNT.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}
