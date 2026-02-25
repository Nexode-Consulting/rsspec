use std::sync::atomic::{AtomicU32, Ordering};

fn main() {
    rsspec::run(|ctx| {
        // =================================================================
        // Basic describe / context / it
        // =================================================================
        ctx.describe("Calculator", |ctx| {
            ctx.it("adds two numbers", || {
                let (a, b) = (2, 3);
                assert_eq!(a + b, 5);
            });

            ctx.it("multiplies", || {
                let (a, b) = (3, 4);
                assert_eq!(a * b, 12);
            });

            ctx.context("with negative numbers", |ctx| {
                ctx.it("handles negatives", || {
                    let (a, b) = (-1, 3);
                    assert_eq!(a + b, 2);
                });
            });

            ctx.when("dividing", |ctx| {
                ctx.it("divides evenly", || {
                    let (a, b) = (10, 2);
                    assert_eq!(a / b, 5);
                });
            });
        });

        // =================================================================
        // Hooks: before_each / after_each
        // =================================================================
        ctx.describe("Hooks", |ctx| {
            ctx.describe("before_each and after_each", |ctx| {
                static BE_COUNTER: AtomicU32 = AtomicU32::new(0);
                static AE_COUNTER: AtomicU32 = AtomicU32::new(0);

                ctx.before_each(|| {
                    BE_COUNTER.fetch_add(1, Ordering::SeqCst);
                });

                ctx.after_each(|| {
                    AE_COUNTER.fetch_add(1, Ordering::SeqCst);
                });

                ctx.it("runs before_each before test 1", || {
                    assert!(BE_COUNTER.load(Ordering::SeqCst) >= 1);
                });

                ctx.it("runs before_each before test 2", || {
                    assert!(BE_COUNTER.load(Ordering::SeqCst) >= 2);
                });
            });

            ctx.describe("before_all and after_all", |ctx| {
                static BA_COUNTER: AtomicU32 = AtomicU32::new(0);
                static AA_COUNTER: AtomicU32 = AtomicU32::new(0);

                ctx.before_all(|| {
                    BA_COUNTER.fetch_add(1, Ordering::SeqCst);
                });

                ctx.after_all(|| {
                    AA_COUNTER.fetch_add(1, Ordering::SeqCst);
                });

                ctx.it("before_all ran once", || {
                    assert_eq!(BA_COUNTER.load(Ordering::SeqCst), 1);
                });

                ctx.it("before_all still only ran once", || {
                    assert_eq!(BA_COUNTER.load(Ordering::SeqCst), 1);
                });
            });

            ctx.describe("just_before_each", |ctx| {
                static ORDER: AtomicU32 = AtomicU32::new(0);

                ctx.before_each(|| {
                    // This should run first
                    ORDER.store(1, Ordering::SeqCst);
                });

                ctx.just_before_each(|| {
                    // This should run after before_each
                    assert_eq!(ORDER.load(Ordering::SeqCst), 1);
                    ORDER.store(2, Ordering::SeqCst);
                });

                ctx.it("just_before_each runs after before_each", || {
                    assert_eq!(ORDER.load(Ordering::SeqCst), 2);
                });
            });

            ctx.describe("nested hook inheritance", |ctx| {
                static OUTER_BE: AtomicU32 = AtomicU32::new(0);
                static INNER_BE: AtomicU32 = AtomicU32::new(0);

                ctx.before_each(|| {
                    OUTER_BE.fetch_add(1, Ordering::SeqCst);
                });

                ctx.context("inner", |ctx| {
                    ctx.before_each(|| {
                        INNER_BE.fetch_add(1, Ordering::SeqCst);
                    });

                    ctx.it("both hooks run", || {
                        assert!(OUTER_BE.load(Ordering::SeqCst) >= 1);
                        assert!(INNER_BE.load(Ordering::SeqCst) >= 1);
                    });
                });
            });

            ctx.describe("after_each guaranteed execution", |ctx| {
                static AE_RAN: AtomicU32 = AtomicU32::new(0);

                ctx.after_each(|| {
                    AE_RAN.fetch_add(1, Ordering::SeqCst);
                });

                ctx.it("after_each runs on normal completion", || {
                    assert!(true);
                });

                ctx.it("after_each counter incremented", || {
                    assert!(AE_RAN.load(Ordering::SeqCst) >= 1);
                });
            });
        });

        // =================================================================
        // Pending / xdescribe / xit
        // =================================================================
        ctx.describe("Pending", |ctx| {
            ctx.xit("not yet implemented", || {
                panic!("should never run");
            });

            ctx.xdescribe("pending container", |ctx| {
                ctx.it("also pending", || {
                    panic!("should never run");
                });
            });
        });

        // =================================================================
        // Decorators: labels, retries, timeout, must_pass_repeatedly
        // =================================================================
        ctx.describe("Decorators", |ctx| {
            ctx.it("with labels", || {
                assert!(true);
            })
            .labels(&["smoke", "fast"]);

            static RETRY_COUNT: AtomicU32 = AtomicU32::new(0);

            ctx.it("with retries", || {
                let n = RETRY_COUNT.fetch_add(1, Ordering::SeqCst);
                assert!(n >= 2, "should fail first 2 attempts");
            })
            .retries(3);

            ctx.it("must pass repeatedly", || {
                assert!(true);
            })
            .must_pass_repeatedly(3);

            ctx.it("with timeout", || {
                // Should complete well within 5 seconds
                assert!(true);
            })
            .timeout(5000);
        });

        // =================================================================
        // Table-driven tests
        // =================================================================
        ctx.describe("Table-driven", |ctx| {
            ctx.describe_table("addition")
                .case("positive", (2i32, 3i32, 5i32))
                .case("large", (100i32, 200i32, 300i32))
                .case("negative", (-1i32, 1i32, 0i32))
                .run(|(a, b, expected): &(i32, i32, i32)| {
                    assert_eq!(a + b, *expected);
                });
        });

        // =================================================================
        // Ordered tests
        // =================================================================
        ctx.describe("Ordered", |ctx| {
            static STEPS: AtomicU32 = AtomicU32::new(0);

            ctx.ordered("sequential workflow", |oct| {
                oct.step("step 1", || {
                    STEPS.fetch_add(1, Ordering::SeqCst);
                });
                oct.step("step 2", || {
                    STEPS.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(STEPS.load(Ordering::SeqCst), 2);
                });
            });
        });

        // =================================================================
        // Describe-level labels
        // =================================================================
        ctx.describe("Labelled container", |ctx| {
            ctx.labels(&["integration"]);

            ctx.it("inherits container labels", || {
                assert!(true);
            });
        });

        // =================================================================
        // defer_cleanup
        // =================================================================
        ctx.describe("defer_cleanup", |ctx| {
            static CLEANUP_RAN: AtomicU32 = AtomicU32::new(0);

            ctx.it("registers cleanup", || {
                rsspec::defer_cleanup(|| {
                    CLEANUP_RAN.fetch_add(1, Ordering::SeqCst);
                });
            });

            ctx.it("cleanup ran after previous test", || {
                assert!(CLEANUP_RAN.load(Ordering::SeqCst) >= 1);
            });
        });

        // =================================================================
        // by() step documentation
        // =================================================================
        ctx.describe("by()", |ctx| {
            ctx.it("documents steps", || {
                rsspec::by("setting up");
                let x = 42;
                rsspec::by("verifying");
                assert_eq!(x, 42);
            });
        });

        // =================================================================
        // specify (alias for it)
        // =================================================================
        ctx.describe("specify", |ctx| {
            ctx.specify("works as alias for it", || {
                assert!(true);
            });
        });
    });
}
