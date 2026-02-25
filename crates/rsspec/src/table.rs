//! Table-driven tests — parameterized test cases via a builder.

use crate::context::with_builder;
use crate::runner::TestNode;
use std::rc::Rc;

/// Builder for table-driven (parameterized) tests.
///
/// Each `.case()` becomes a separate `It` test node inside a `Describe` group.
///
/// # Example
///
/// ```rust,no_run
/// # fn main() { rsspec::run(|ctx| {
/// ctx.describe_table("arithmetic")
///     .case("addition", (2i32, 3i32, 5i32))
///     .case("large numbers", (100, 200, 300))
///     .run(|(a, b, expected): &(i32, i32, i32)| {
///         assert_eq!(a + b, *expected);
///     });
/// # }); }
/// ```
pub struct TableBuilder {
    name: String,
    cases: Vec<(String, Box<dyn std::any::Any>)>,
    auto_index: usize,
}

impl TableBuilder {
    pub(crate) fn new(name: String) -> Self {
        TableBuilder {
            name,
            cases: Vec::new(),
            auto_index: 0,
        }
    }

    /// Add a named test case with parameter data.
    pub fn case<T: 'static>(mut self, label: &str, data: T) -> Self {
        self.cases.push((label.to_string(), Box::new(data)));
        self
    }

    /// Add an unnamed test case (auto-named `case_1`, `case_2`, ...).
    pub fn case_unnamed<T: 'static>(mut self, data: T) -> Self {
        self.auto_index += 1;
        let label = format!("case_{}", self.auto_index);
        self.cases.push((label, Box::new(data)));
        self
    }

    /// Run all cases. Each case becomes a separate test node.
    ///
    /// The test function receives a reference to the data for each case.
    /// `T` must be `'static` (required for type-erased storage).
    pub fn run<T: 'static>(self, test_fn: impl Fn(&T) + 'static) {
        with_builder(|b| b.push_group(self.name, false, false));

        let test_fn = Rc::new(test_fn);

        for (label, data) in self.cases {
            let data = *data.downcast::<T>().unwrap_or_else(|_| {
                panic!("rsspec: table case type mismatch in case '{label}'");
            });
            let test_fn = test_fn.clone();

            // Data is owned by the closure and passed by reference to test_fn.
            // This makes the closure Fn() — callable multiple times (for retries).
            let body = move || {
                test_fn(&data);
            };

            with_builder(|b| {
                b.add_node(TestNode::It {
                    name: label,
                    focused: false,
                    pending: false,
                    labels: Vec::new(),
                    retries: None,
                    timeout_ms: None,
                    must_pass_repeatedly: None,
                    test_fn: Box::new(body),
                });
            });
        }

        with_builder(|b| b.pop_group());
    }
}
