//! Closure-based BDD API — Context, ItBuilder, SuiteBuilder, and `run()`.

use crate::runner::{self, RunConfig, Suite, TestNode};
use std::cell::RefCell;

// ============================================================================
// Thread-local suite builder
// ============================================================================

thread_local! {
    static BUILDER: RefCell<Option<SuiteBuilder>> = const { RefCell::new(None) };
}

pub(crate) struct SuiteBuilder {
    stack: Vec<GroupFrame>,
}

struct GroupFrame {
    name: String,
    focused: bool,
    pending: bool,
    labels: Vec<String>,
    before_each: Vec<Box<dyn Fn()>>,
    after_each: Vec<Box<dyn Fn()>>,
    before_all: Vec<Box<dyn Fn()>>,
    after_all: Vec<Box<dyn Fn()>>,
    just_before_each: Vec<Box<dyn Fn()>>,
    children: Vec<TestNode>,
}

impl GroupFrame {
    fn root() -> Self {
        GroupFrame {
            name: String::new(),
            focused: false,
            pending: false,
            labels: Vec::new(),
            before_each: Vec::new(),
            after_each: Vec::new(),
            before_all: Vec::new(),
            after_all: Vec::new(),
            just_before_each: Vec::new(),
            children: Vec::new(),
        }
    }
}

impl SuiteBuilder {
    fn new() -> Self {
        SuiteBuilder {
            stack: vec![GroupFrame::root()],
        }
    }

    pub(crate) fn push_group(&mut self, name: String, focused: bool, pending: bool) {
        self.stack.push(GroupFrame {
            name,
            focused,
            pending,
            labels: Vec::new(),
            before_each: Vec::new(),
            after_each: Vec::new(),
            before_all: Vec::new(),
            after_all: Vec::new(),
            just_before_each: Vec::new(),
            children: Vec::new(),
        });
    }

    pub(crate) fn pop_group(&mut self) {
        let frame = self.stack.pop().expect("rsspec: unbalanced group push/pop");
        let node = TestNode::Describe {
            name: frame.name,
            focused: frame.focused,
            pending: frame.pending,
            labels: frame.labels,
            before_each: frame.before_each,
            after_each: frame.after_each,
            before_all: frame.before_all,
            after_all: frame.after_all,
            just_before_each: frame.just_before_each,
            children: frame.children,
        };
        self.current_frame_mut().children.push(node);
    }

    pub(crate) fn add_node(&mut self, node: TestNode) {
        self.current_frame_mut().children.push(node);
    }

    fn add_before_each(&mut self, hook: Box<dyn Fn()>) {
        self.current_frame_mut().before_each.push(hook);
    }

    fn add_after_each(&mut self, hook: Box<dyn Fn()>) {
        self.current_frame_mut().after_each.push(hook);
    }

    fn add_before_all(&mut self, hook: Box<dyn Fn()>) {
        self.current_frame_mut().before_all.push(hook);
    }

    fn add_after_all(&mut self, hook: Box<dyn Fn()>) {
        self.current_frame_mut().after_all.push(hook);
    }

    fn add_just_before_each(&mut self, hook: Box<dyn Fn()>) {
        self.current_frame_mut().just_before_each.push(hook);
    }

    fn set_labels(&mut self, labels: Vec<String>) {
        self.current_frame_mut().labels = labels;
    }

    fn current_frame_mut(&mut self) -> &mut GroupFrame {
        self.stack.last_mut().expect("rsspec: empty builder stack")
    }

    fn into_nodes(mut self) -> Vec<TestNode> {
        assert_eq!(
            self.stack.len(),
            1,
            "rsspec: unbalanced group push/pop at finalization"
        );
        self.stack.pop().unwrap().children
    }
}

/// Access the thread-local builder.
pub(crate) fn with_builder<R>(f: impl FnOnce(&mut SuiteBuilder) -> R) -> R {
    BUILDER.with(|cell| {
        let mut opt = cell.borrow_mut();
        let builder = opt
            .as_mut()
            .expect("rsspec: Context used outside of rsspec::run()");
        f(builder)
    })
}

// ============================================================================
// Context — the user-facing handle
// ============================================================================

/// A lightweight handle for defining BDD test structure.
///
/// All methods delegate to a thread-local builder. `Context` is `Copy` so it
/// can be passed into nested closures without ceremony.
///
/// # Example
/// ```rust,no_run
/// fn main() {
///     rsspec::run(|ctx| {
///         ctx.describe("Calculator", |ctx| {
///             ctx.it("adds", || { assert_eq!(2 + 3, 5); });
///         });
///     });
/// }
/// ```
#[derive(Copy, Clone)]
pub struct Context;

impl Context {
    // ---- Describe / Context / When -------------------------------------------

    pub fn describe(&self, name: &str, body: impl FnOnce(Context)) {
        self.describe_impl(name, false, false, body);
    }

    pub fn fdescribe(&self, name: &str, body: impl FnOnce(Context)) {
        self.describe_impl(name, true, false, body);
    }

    pub fn xdescribe(&self, name: &str, body: impl FnOnce(Context)) {
        self.describe_impl(name, false, true, body);
    }

    pub fn context(&self, name: &str, body: impl FnOnce(Context)) {
        self.describe(name, body);
    }

    pub fn fcontext(&self, name: &str, body: impl FnOnce(Context)) {
        self.fdescribe(name, body);
    }

    pub fn xcontext(&self, name: &str, body: impl FnOnce(Context)) {
        self.xdescribe(name, body);
    }

    pub fn when(&self, name: &str, body: impl FnOnce(Context)) {
        self.describe(name, body);
    }

    pub fn fwhen(&self, name: &str, body: impl FnOnce(Context)) {
        self.fdescribe(name, body);
    }

    pub fn xwhen(&self, name: &str, body: impl FnOnce(Context)) {
        self.xdescribe(name, body);
    }

    fn describe_impl(&self, name: &str, focused: bool, pending: bool, body: impl FnOnce(Context)) {
        with_builder(|b| b.push_group(name.to_string(), focused, pending));
        body(Context);
        with_builder(|b| b.pop_group());
    }

    // ---- It / Specify --------------------------------------------------------

    /// Define a test case. Returns an [`ItBuilder`] for optional decorators.
    ///
    /// ```rust,no_run
    /// # fn main() { rsspec::run(|ctx| {
    /// ctx.it("works", || { assert!(true); });
    ///
    /// ctx.it("slow test", || { /* ... */ })
    ///     .labels(&["slow"])
    ///     .retries(3)
    ///     .timeout(5000);
    /// # }); }
    /// ```
    pub fn it(&self, name: &str, body: impl Fn() + 'static) -> ItBuilder {
        ItBuilder::new(name.to_string(), body, false, false)
    }

    pub fn fit(&self, name: &str, body: impl Fn() + 'static) -> ItBuilder {
        ItBuilder::new(name.to_string(), body, true, false)
    }

    pub fn xit(&self, name: &str, body: impl Fn() + 'static) -> ItBuilder {
        ItBuilder::new(name.to_string(), body, false, true)
    }

    pub fn specify(&self, name: &str, body: impl Fn() + 'static) -> ItBuilder {
        self.it(name, body)
    }

    pub fn fspecify(&self, name: &str, body: impl Fn() + 'static) -> ItBuilder {
        self.fit(name, body)
    }

    pub fn xspecify(&self, name: &str, body: impl Fn() + 'static) -> ItBuilder {
        self.xit(name, body)
    }

    // ---- Hooks ---------------------------------------------------------------

    pub fn before_each(&self, hook: impl Fn() + 'static) {
        with_builder(|b| b.add_before_each(Box::new(hook)));
    }

    pub fn after_each(&self, hook: impl Fn() + 'static) {
        with_builder(|b| b.add_after_each(Box::new(hook)));
    }

    pub fn before_all(&self, hook: impl Fn() + 'static) {
        with_builder(|b| b.add_before_all(Box::new(hook)));
    }

    pub fn after_all(&self, hook: impl Fn() + 'static) {
        with_builder(|b| b.add_after_all(Box::new(hook)));
    }

    pub fn just_before_each(&self, hook: impl Fn() + 'static) {
        with_builder(|b| b.add_just_before_each(Box::new(hook)));
    }

    // ---- Labels on current describe ------------------------------------------

    /// Set labels on the current describe scope.
    ///
    /// ```rust,no_run
    /// # fn main() { rsspec::run(|ctx| {
    /// ctx.describe("integration tests", |ctx| {
    ///     ctx.labels(&["integration", "slow"]);
    ///     ctx.it("test", || { /* ... */ });
    /// });
    /// # }); }
    /// ```
    pub fn labels(&self, labels: &[&str]) {
        let labels: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
        with_builder(|b| b.set_labels(labels));
    }

    // ---- Table-driven --------------------------------------------------------

    /// Start building a table-driven test.
    ///
    /// ```rust,no_run
    /// # fn main() { rsspec::run(|ctx| {
    /// ctx.describe_table("arithmetic")
    ///     .case("addition", (2i32, 3i32, 5i32))
    ///     .case("subtraction", (5, 3, 2))
    ///     .run(|(a, b, expected): &(i32, i32, i32)| {
    ///         assert_eq!(a + b, *expected);
    ///     });
    /// # }); }
    /// ```
    pub fn describe_table(&self, name: &str) -> crate::table::TableBuilder {
        crate::table::TableBuilder::new(name.to_string())
    }

    // ---- Ordered -------------------------------------------------------------

    /// Define an ordered sequence of steps that run as a single test.
    ///
    /// If any step fails, subsequent steps are skipped (fail-fast).
    ///
    /// ```rust,no_run
    /// # fn main() { rsspec::run(|ctx| {
    /// ctx.ordered("workflow", |oct| {
    ///     oct.step("step 1", || { /* ... */ });
    ///     oct.step("step 2", || { /* ... */ });
    /// });
    /// # }); }
    /// ```
    pub fn ordered(&self, name: &str, body: impl FnOnce(&mut crate::ordered::OrderedContext)) {
        let mut oct = crate::ordered::OrderedContext::new(name.to_string(), false);
        body(&mut oct);
        with_builder(|b| b.add_node(oct.into_node()));
    }

    /// Like [`ordered`](Self::ordered), but continues running steps even if one fails.
    pub fn ordered_continue_on_failure(
        &self,
        name: &str,
        body: impl FnOnce(&mut crate::ordered::OrderedContext),
    ) {
        let mut oct = crate::ordered::OrderedContext::new(name.to_string(), true);
        body(&mut oct);
        with_builder(|b| b.add_node(oct.into_node()));
    }
}

// ============================================================================
// ItBuilder — fluent decorator API, registers test on Drop
// ============================================================================

/// Builder returned by [`Context::it`]. Supports chaining decorators and
/// registers the test node when dropped.
///
/// ```rust,no_run
/// # fn main() { rsspec::run(|ctx| {
/// // Simple (drops immediately, registered at semicolon):
/// ctx.it("simple", || { assert!(true); });
///
/// // With decorators:
/// ctx.it("complex", || { /* ... */ })
///     .labels(&["integration"])
///     .retries(3)
///     .timeout(5000);
/// # }); }
/// ```
pub struct ItBuilder {
    name: String,
    body: Option<Box<dyn Fn()>>,
    focused: bool,
    pending: bool,
    labels: Vec<String>,
    retries: Option<u32>,
    timeout_ms: Option<u64>,
    must_pass_repeatedly: Option<u32>,
}

impl ItBuilder {
    fn new(name: String, body: impl Fn() + 'static, focused: bool, pending: bool) -> Self {
        ItBuilder {
            name,
            body: Some(Box::new(body)),
            focused,
            pending,
            labels: Vec::new(),
            retries: None,
            timeout_ms: None,
            must_pass_repeatedly: None,
        }
    }

    /// Add labels for filtering via `RSSPEC_LABEL_FILTER`.
    pub fn labels(mut self, labels: &[&str]) -> Self {
        self.labels = labels.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Retry the test up to `n` additional times on failure.
    pub fn retries(mut self, n: u32) -> Self {
        self.retries = Some(n);
        self
    }

    /// Fail the test if it exceeds `ms` milliseconds.
    pub fn timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// Require the test to pass `n` consecutive times.
    pub fn must_pass_repeatedly(mut self, n: u32) -> Self {
        self.must_pass_repeatedly = Some(n);
        self
    }
}

impl Drop for ItBuilder {
    fn drop(&mut self) {
        let node = TestNode::It {
            name: std::mem::take(&mut self.name),
            focused: self.focused,
            pending: self.pending,
            labels: std::mem::take(&mut self.labels),
            retries: self.retries,
            timeout_ms: self.timeout_ms,
            must_pass_repeatedly: self.must_pass_repeatedly,
            test_fn: self.body.take().unwrap(),
        };
        with_builder(|b| b.add_node(node));
    }
}

// ============================================================================
// run() — entry point
// ============================================================================

/// Build and run a BDD test suite.
///
/// This is the main entry point for rsspec. Call it from `fn main()` in a
/// test target with `harness = false`.
///
/// # Example
///
/// ```rust,no_run
/// fn main() {
///     rsspec::run(|ctx| {
///         ctx.describe("Calculator", |ctx| {
///             ctx.it("adds", || { assert_eq!(2 + 3, 5); });
///         });
///     });
/// }
/// ```
pub fn run(body: impl FnOnce(Context)) {
    // Phase 1: build the tree
    BUILDER.with(|cell| {
        *cell.borrow_mut() = Some(SuiteBuilder::new());
    });

    body(Context);

    let nodes = BUILDER.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("rsspec: builder missing after run")
            .into_nodes()
    });

    // Phase 2: execute the tree
    let config = RunConfig::from_args();
    let suite = Suite::new("", "", nodes);
    let result = runner::run_suites(&[suite], &config);

    if result.failed > 0 {
        std::process::exit(1);
    }
}
