//! Ordered test sequences — steps that run sequentially as a single test.

use crate::runner::{OrderedStep, TestNode};

/// Context for defining steps in an ordered test sequence.
///
/// # Example
///
/// ```rust,no_run
/// # fn main() { rsspec::run(|ctx| {
/// ctx.ordered("user workflow", |oct| {
///     oct.step("create account", || { /* ... */ });
///     oct.step("verify email", || { /* ... */ });
///     oct.step("login", || { /* ... */ });
/// });
/// # }); }
/// ```
pub struct OrderedContext {
    name: String,
    continue_on_failure: bool,
    steps: Vec<OrderedStep>,
    labels: Vec<String>,
}

impl OrderedContext {
    pub(crate) fn new(name: String, continue_on_failure: bool) -> Self {
        OrderedContext {
            name,
            continue_on_failure,
            steps: Vec::new(),
            labels: Vec::new(),
        }
    }

    /// Add a named step to the sequence.
    pub fn step(&mut self, name: &str, body: impl Fn() + 'static) {
        self.steps.push(OrderedStep {
            name: name.to_string(),
            body: Box::new(body),
        });
    }

    /// Set labels on this ordered test.
    pub fn labels(&mut self, labels: &[&str]) {
        self.labels = labels.iter().map(|s| s.to_string()).collect();
    }

    pub(crate) fn into_node(self) -> TestNode {
        TestNode::Ordered {
            name: self.name,
            labels: self.labels,
            continue_on_failure: self.continue_on_failure,
            steps: self.steps,
        }
    }
}
