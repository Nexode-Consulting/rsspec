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

    let suites = vec![
        rsspec::runner::Suite::new("math", file!(), math_suite),
        rsspec::runner::Suite::new("strings", file!(), string_suite),
    ];

    let config = rsspec::runner::RunConfig::from_args();
    let result = rsspec::runner::run_suites(&suites, &config);
    if result.failed > 0 {
        std::process::exit(1);
    }
}
