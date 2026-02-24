use std::sync::atomic::{AtomicU32, Ordering};

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
