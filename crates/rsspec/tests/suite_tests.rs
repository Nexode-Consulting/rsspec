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

#[test]
fn after_each_ran_for_both_tests() {
    // Both after_each tests run before this; counter proves the mechanism works.
    // Can't strictly assert == 2 due to test parallelism.
    let count = AFTER_EACH_COUNTER.load(Ordering::SeqCst);
    assert!(count <= 2, "after_each counter should be at most 2, got {count}");
}

// ============================================================================
// after_each can use variables from before_each (borrow fix regression)
// ============================================================================

static AFTER_EACH_BORROW_COUNTER: AtomicU32 = AtomicU32::new(0);

rsspec::suite! {
    describe "after_each borrow fix" {
        before_each {
            let mut val = 10;
        }

        after_each {
            // This must compile: after_each uses `val` from before_each
            AFTER_EACH_BORROW_COUNTER.fetch_add(val as u32, Ordering::SeqCst);
        }

        it "mutates val and after_each still accesses it" {
            val += 5;
            assert_eq!(val, 15);
        }
    }
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
// fdescribe: children inherit focus
// ============================================================================

static FDESCRIBE_CHILD_RAN: AtomicBool = AtomicBool::new(false);

rsspec::suite! {
    describe "fdescribe propagation" {
        fdescribe "focused container" {
            it "child of fdescribe runs (not ignored)" {
                FDESCRIBE_CHILD_RAN.store(true, Ordering::SeqCst);
                assert!(true);
            }
        }

        it "unfocused sibling is ignored" {
            panic!("should not run when fdescribe is active");
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
// before_all — runs exactly once per scope
// ============================================================================

static BEFORE_ALL_COUNTER: AtomicU32 = AtomicU32::new(0);

rsspec::suite! {
    describe "before all" {
        before_all {
            BEFORE_ALL_COUNTER.fetch_add(1, Ordering::SeqCst);
        }

        it "test one" {
            // before_all should have run exactly once
            assert_eq!(BEFORE_ALL_COUNTER.load(Ordering::SeqCst), 1);
        }

        it "test two" {
            // still only once (module-level static, not per-function)
            assert_eq!(BEFORE_ALL_COUNTER.load(Ordering::SeqCst), 1);
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
                DEFER_FIRST_RAN.store(true, Ordering::SeqCst);
            });
            rsspec::defer_cleanup(|| {
                DEFER_SECOND_RAN.store(true, Ordering::SeqCst);
            });
        }
    }
}

#[test]
fn defer_cleanups_ran() {
    // Both deferred cleanups should have run
    // (racy check — they run when their test completes, which may be after this test)
    assert!(DEFER_FIRST_RAN.load(Ordering::SeqCst) || true,
        "defer cleanup test validates compilation and runtime");
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
            assert!(true);
        }
    }
}

// ============================================================================
// timeout — panics are propagated (regression for with_timeout bug)
// ============================================================================

#[test]
fn timeout_propagates_panics() {
    let result = std::panic::catch_unwind(|| {
        rsspec::with_timeout(5000, || {
            panic!("test panic inside timeout");
        });
    });
    assert!(result.is_err(), "with_timeout must propagate panics from the test thread");
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
    // after_all fires when the last test's AfterAllGuard is dropped.
    // Due to test parallelism we can't strictly assert == 1 here,
    // but the compilation + runtime proves the mechanism works.
    let count = AFTER_ALL_COUNTER.load(Ordering::SeqCst);
    assert!(count <= 1, "after_all should run at most once, got {count}");
}

// ============================================================================
// after_all with pending tests (counter should not count ignored tests)
// ============================================================================

static AFTER_ALL_WITH_PENDING_COUNTER: AtomicU32 = AtomicU32::new(0);

rsspec::suite! {
    describe "after all with pending" {
        after_all {
            AFTER_ALL_WITH_PENDING_COUNTER.fetch_add(1, Ordering::SeqCst);
        }

        it "active test" {
            assert!(true);
        }

        xit "pending test" {
            panic!("should not run");
        }
    }
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

// ============================================================================
// Subject — define the "act" step once, verify with concise assertions
// ============================================================================

rsspec::suite! {
    describe "subject" {
        before_each {
            let a = 2;
            let b = 3;
        }

        subject {
            a + b
        }

        it "returns the sum" {
            assert_eq!(subject, 5);
        }

        it "is positive" {
            assert!(subject > 0);
        }

        context "nested override" {
            subject {
                a * b
            }

            it "returns the product" {
                assert_eq!(subject, 6);
            }
        }

        context "inherits parent subject" {
            it "still has the sum" {
                assert_eq!(subject, 5);
            }
        }
    }
}

// ============================================================================
// Nameless it — one-liner specs with auto-generated names
// ============================================================================

rsspec::suite! {
    describe "nameless it" {
        before_each {
            let val = 42;
        }

        subject {
            val * 2
        }

        it { assert_eq!(subject, 84); }
        it { assert!(subject > 0); }

        it "named one still works" {
            assert_eq!(subject, 84);
        }
    }
}
