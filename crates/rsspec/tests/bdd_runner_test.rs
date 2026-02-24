fn main() {
    let math_suite = rsspec::bdd_suite! {
        describe "Calculator" {
            it "adds two numbers" {
                assert_eq!(2 + 3, 5);
            }

            it "multiplies" {
                assert_eq!(3 * 4, 12);
            }

            context "with negative numbers" {
                it "handles negatives" {
                    assert_eq!(-1 + 3, 2);
                }
            }

            describe "Division" {
                it "divides evenly" {
                    assert_eq!(10 / 2, 5);
                }

                xit "handles division by zero" {
                    // pending test
                }
            }
        }
    };

    // Regression: BDD hook inheritance — outer before_each must be visible in nested it
    let hook_suite = rsspec::bdd_suite! {
        describe "Hook inheritance" {
            before_each {
                let x = 42;
            }

            it "uses before_each at same level" {
                assert_eq!(x, 42);
            }

            context "nested context" {
                before_each {
                    let y = x + 1;
                }

                it "inherits outer before_each" {
                    assert_eq!(x, 42);
                    assert_eq!(y, 43);
                }
            }
        }
    };

    let string_suite = rsspec::bdd_suite! {
        describe "String operations" {
            before_each {
                let greeting = String::from("hello");
            }

            it "has correct length" {
                assert_eq!(greeting.len(), 5);
            }

            it "can be uppercased" {
                assert_eq!(greeting.to_uppercase(), "HELLO");
            }
        }

        describe "Table-driven" {
            describe_table "arithmetic" (a: i32, b: i32, expected: i32) [
                "addition" (2, 3, 5),
                "large" (100, 200, 300),
            ] {
                assert_eq!(a + b, expected);
            }
        }
    };

    let subject_suite = rsspec::bdd_suite! {
        describe "Subject" {
            before_each {
                let x = 10;
                let y = 5;
            }

            subject {
                x + y
            }

            it "returns the sum" {
                assert_eq!(subject, 15);
            }

            it { assert!(subject > 0); }

            context "with override" {
                subject {
                    x * y
                }

                it "returns the product" {
                    assert_eq!(subject, 50);
                }
            }
        }
    };

    let suites = vec![
        rsspec::runner::Suite::new("math", file!(), math_suite),
        rsspec::runner::Suite::new("hooks", file!(), hook_suite),
        rsspec::runner::Suite::new("strings", file!(), string_suite),
        rsspec::runner::Suite::new("subject", file!(), subject_suite),
    ];

    let config = rsspec::runner::RunConfig::from_args();
    let result = rsspec::runner::run_suites(&suites, &config);
    if result.failed > 0 {
        std::process::exit(1);
    }
}
