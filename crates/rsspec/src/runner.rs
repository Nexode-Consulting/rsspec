//! BDD-style test runner with colored, indented tree output.
//!
//! Used with `harness = false` test targets to get Ginkgo-like output:
//!
//! ```text
//! Calculator
//!   ✓ adds two numbers
//!   when negative
//!     ✓ handles negatives
//!     ✗ fails on overflow
//! ```

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

// ============================================================================
// Test tree types
// ============================================================================

/// A step in an ordered test sequence.
pub struct OrderedStep {
    pub name: String,
    pub body: Box<dyn Fn()>,
}

/// A node in the BDD test tree.
pub enum TestNode {
    /// A describe/context/when container.
    Describe {
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
    },
    /// An individual test case.
    It {
        name: String,
        focused: bool,
        pending: bool,
        labels: Vec<String>,
        retries: Option<u32>,
        timeout_ms: Option<u64>,
        must_pass_repeatedly: Option<u32>,
        test_fn: Box<dyn Fn()>,
    },
    /// An ordered sequence of steps that run as a single test.
    Ordered {
        name: String,
        labels: Vec<String>,
        continue_on_failure: bool,
        steps: Vec<OrderedStep>,
    },
}

impl TestNode {
    /// Create a describe/context container with child nodes.
    pub fn describe(name: impl Into<String>, children: Vec<TestNode>) -> Self {
        TestNode::Describe {
            name: name.into(),
            focused: false,
            pending: false,
            labels: Vec::new(),
            before_each: Vec::new(),
            after_each: Vec::new(),
            before_all: Vec::new(),
            after_all: Vec::new(),
            just_before_each: Vec::new(),
            children,
        }
    }

    /// Create a normal test case.
    pub fn it(name: impl Into<String>, f: impl Fn() + 'static) -> Self {
        TestNode::It {
            name: name.into(),
            focused: false,
            pending: false,
            labels: Vec::new(),
            retries: None,
            timeout_ms: None,
            must_pass_repeatedly: None,
            test_fn: Box::new(f),
        }
    }

    /// Create a focused test case — when any node is focused, only focused nodes run.
    pub fn fit(name: impl Into<String>, f: impl Fn() + 'static) -> Self {
        TestNode::It {
            name: name.into(),
            focused: true,
            pending: false,
            labels: Vec::new(),
            retries: None,
            timeout_ms: None,
            must_pass_repeatedly: None,
            test_fn: Box::new(f),
        }
    }

    /// Create a pending (skipped) test case.
    pub fn xit(name: impl Into<String>, f: impl Fn() + 'static) -> Self {
        TestNode::It {
            name: name.into(),
            focused: false,
            pending: true,
            labels: Vec::new(),
            retries: None,
            timeout_ms: None,
            must_pass_repeatedly: None,
            test_fn: Box::new(f),
        }
    }
}

// ============================================================================
// Hook chain — accumulates hooks from ancestor Describe nodes
// ============================================================================

#[derive(Default, Clone)]
struct HookChain<'a> {
    before_each: Vec<&'a dyn Fn()>,
    after_each: Vec<&'a dyn Fn()>,
    just_before_each: Vec<&'a dyn Fn()>,
    labels: Vec<&'a str>,
}

impl<'a> HookChain<'a> {
    fn with_describe(&self, node: &'a TestNode) -> HookChain<'a> {
        if let TestNode::Describe {
            before_each,
            after_each,
            just_before_each,
            labels,
            ..
        } = node
        {
            let mut chain = self.clone();
            for hook in before_each {
                chain.before_each.push(hook.as_ref());
            }
            for hook in after_each {
                chain.after_each.push(hook.as_ref());
            }
            for hook in just_before_each {
                chain.just_before_each.push(hook.as_ref());
            }
            for label in labels {
                chain.labels.push(label.as_str());
            }
            chain
        } else {
            self.clone()
        }
    }
}

// ============================================================================
// ANSI color helpers
// ============================================================================

fn use_color() -> bool {
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

fn green(s: &str) -> String {
    if use_color() {
        format!("\x1b[32m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn red(s: &str) -> String {
    if use_color() {
        format!("\x1b[31m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn yellow(s: &str) -> String {
    if use_color() {
        format!("\x1b[33m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn bold(s: &str) -> String {
    if use_color() {
        format!("\x1b[1m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn dim(s: &str) -> String {
    if use_color() {
        format!("\x1b[2m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

// ============================================================================
// Runner
// ============================================================================

/// Results from running a test tree.
#[derive(Default)]
pub struct RunResult {
    pub passed: usize,
    pub failed: usize,
    pub pending: usize,
    pub skipped: usize,
    pub failures: Vec<String>,
}

/// Configuration parsed from command-line args.
pub struct RunConfig {
    /// Filter string — only run tests whose full path contains this.
    pub filter: Option<String>,
    /// Only list tests, don't run them.
    pub list: bool,
    /// Include ignored/pending tests in the run.
    pub include_ignored: bool,
}

impl RunConfig {
    /// Parse from the process args (compatible with `cargo test -- <args>`).
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut filter = None;
        let mut list = false;
        let mut include_ignored = false;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--list" => list = true,
                "--include-ignored" | "--ignored" => include_ignored = true,
                arg if !arg.starts_with('-') => {
                    filter = Some(arg.to_string());
                }
                _ => {}
            }
            i += 1;
        }

        RunConfig {
            filter,
            list,
            include_ignored,
        }
    }
}

/// A named suite with its source location, for multi-suite runs.
pub struct Suite {
    pub name: String,
    pub file: String,
    pub nodes: Vec<TestNode>,
}

impl Suite {
    pub fn new(name: impl Into<String>, file: impl Into<String>, nodes: Vec<TestNode>) -> Self {
        Suite {
            name: name.into(),
            file: file.into(),
            nodes,
        }
    }
}

/// Run a single test tree and print BDD-formatted output.
pub fn run_tree(nodes: &[TestNode], config: &RunConfig) -> RunResult {
    let focus_mode = tree_has_focus(nodes);
    let mut result = RunResult::default();
    let start = Instant::now();

    if config.list {
        list_tree(nodes, &[], config);
        return result;
    }

    println!();
    let hooks = HookChain::default();
    run_nodes(nodes, 0, &[], &hooks, focus_mode, false, config, &mut result);
    print_summary(&result, start.elapsed());

    result
}

/// Run multiple named suites, printing a header per suite and a combined summary.
pub fn run_suites(suites: &[Suite], config: &RunConfig) -> RunResult {
    let focus_mode = suites.iter().any(|s| tree_has_focus(&s.nodes));
    let mut result = RunResult::default();
    let start = Instant::now();

    if config.list {
        for suite in suites {
            list_tree(&suite.nodes, &[], config);
        }
        return result;
    }

    println!();

    for suite in suites {
        let header = match (suite.name.as_str(), suite.file.as_str()) {
            ("", "") => String::new(),
            (name, "") => name.to_string(),
            ("", file) => file.to_string(),
            (name, file) => format!("{name} ({file})"),
        };
        if !header.is_empty() {
            println!("{}", dim(&format!("--- {} ---", header)));
            println!();
        }

        let hooks = HookChain::default();
        run_nodes(
            &suite.nodes,
            0,
            &[],
            &hooks,
            focus_mode,
            false,
            config,
            &mut result,
        );

        if suites.len() > 1 {
            println!();
        }
    }

    print_summary(&result, start.elapsed());

    result
}

#[allow(clippy::too_many_arguments)]
fn run_nodes(
    nodes: &[TestNode],
    depth: usize,
    path: &[String],
    hooks: &HookChain,
    focus_mode: bool,
    force_focused: bool,
    config: &RunConfig,
    result: &mut RunResult,
) {
    for node in nodes {
        run_node(node, depth, path, hooks, focus_mode, force_focused, config, result);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_node(
    node: &TestNode,
    depth: usize,
    path: &[String],
    hooks: &HookChain,
    focus_mode: bool,
    force_focused: bool,
    config: &RunConfig,
    result: &mut RunResult,
) {
    match node {
        TestNode::Describe {
            name,
            focused,
            pending,
            children,
            before_all,
            after_all,
            ..
        } => {
            let indent = "  ".repeat(depth);
            println!("{indent}{}", bold(name));

            let mut child_path = path.to_vec();
            child_path.push(name.clone());

            // If this describe is pending, mark all children as pending
            if *pending {
                run_nodes_pending(children, depth + 1, result);
                return;
            }

            let child_hooks = hooks.with_describe(node);
            let child_force_focused = force_focused || *focused;

            // Run before_all once at scope entry
            for hook in before_all {
                hook();
            }

            run_nodes(
                children,
                depth + 1,
                &child_path,
                &child_hooks,
                focus_mode,
                child_force_focused,
                config,
                result,
            );

            // Run after_all once at scope exit
            for hook in after_all {
                hook();
            }
        }
        TestNode::It {
            name,
            focused,
            pending,
            labels,
            retries,
            timeout_ms,
            must_pass_repeatedly,
            test_fn,
        } => {
            let indent = "  ".repeat(depth);
            let full_path = {
                let mut p = path.to_vec();
                p.push(name.clone());
                p.join(" > ")
            };

            // Filter check
            if let Some(ref f) = config.filter {
                if !full_path.to_lowercase().contains(&f.to_lowercase()) {
                    return;
                }
            }

            // Pending
            if *pending {
                println!("{indent}{} {}", yellow("-"), dim(name));
                result.pending += 1;
                return;
            }

            // Focus mode: skip non-focused
            let effectively_focused = *focused || force_focused;
            if focus_mode && !effectively_focused && !config.include_ignored {
                result.skipped += 1;
                return;
            }

            // Fail-on-focus CI check
            if effectively_focused && focus_mode {
                crate::check_fail_on_focus();
            }

            // Label check (merge accumulated + own)
            let all_labels: Vec<&str> = hooks
                .labels
                .iter()
                .copied()
                .chain(labels.iter().map(|s| s.as_str()))
                .collect();
            if !crate::check_labels(&all_labels) {
                return;
            }

            // Execute the test
            let start = Instant::now();

            let test_body = || {
                // before_each (outermost first)
                for hook in &hooks.before_each {
                    hook();
                }
                // just_before_each (outermost first)
                for hook in &hooks.just_before_each {
                    hook();
                }

                // Run test with catch_unwind to guarantee after_each
                let body_result = catch_unwind(AssertUnwindSafe(|| {
                    test_fn();
                }));

                // after_each (innermost first)
                for hook in hooks.after_each.iter().rev() {
                    hook();
                }

                // Deferred cleanups
                crate::run_deferred_cleanups();

                if let Err(e) = body_result {
                    std::panic::resume_unwind(e);
                }
            };

            // Apply decorators compositionally so combinations behave as expected:
            // retries -> must_pass_repeatedly -> timeout (outermost)
            let with_retries = || {
                if let Some(n) = *retries {
                    crate::with_retries(n, test_body);
                } else {
                    test_body();
                }
            };

            let with_must_pass_repeatedly = || {
                if let Some(n) = *must_pass_repeatedly {
                    crate::must_pass_repeatedly(n, with_retries);
                } else {
                    with_retries();
                }
            };

            let outcome = if let Some(ms) = *timeout_ms {
                run_with_timeout(ms, &with_must_pass_repeatedly)
            } else {
                catch_unwind(AssertUnwindSafe(with_must_pass_repeatedly))
            };

            report_outcome(&indent, name, &full_path, outcome, start, result);
        }
        TestNode::Ordered {
            name,
            labels,
            continue_on_failure,
            steps,
        } => {
            let indent = "  ".repeat(depth);
            let full_path = {
                let mut p = path.to_vec();
                p.push(name.clone());
                p.join(" > ")
            };

            // Filter check
            if let Some(ref f) = config.filter {
                if !full_path.to_lowercase().contains(&f.to_lowercase()) {
                    return;
                }
            }

            // Focus mode: skip non-focused ordered tests unless include_ignored is set.
            if focus_mode && !force_focused && !config.include_ignored {
                result.skipped += 1;
                return;
            }

            // Fail-on-focus CI check for ordered tests inside focused containers.
            if force_focused && focus_mode {
                crate::check_fail_on_focus();
            }

            // Label check
            let all_labels: Vec<&str> = hooks
                .labels
                .iter()
                .copied()
                .chain(labels.iter().map(|s| s.as_str()))
                .collect();
            if !crate::check_labels(&all_labels) {
                return;
            }

            let start = Instant::now();

            let outcome = catch_unwind(AssertUnwindSafe(|| {
                // Run before_each
                for hook in &hooks.before_each {
                    hook();
                }
                for hook in &hooks.just_before_each {
                    hook();
                }

                let mut failures: Vec<Box<dyn std::any::Any + Send>> = Vec::new();

                for step in steps {
                    crate::by(&step.name);
                    if *continue_on_failure {
                        if let Err(e) = catch_unwind(AssertUnwindSafe(|| (step.body)())) {
                            failures.push(e);
                        }
                    } else {
                        (step.body)();
                    }
                }

                // Run after_each
                for hook in hooks.after_each.iter().rev() {
                    hook();
                }

                crate::run_deferred_cleanups();

                if !failures.is_empty() {
                    panic!(
                        "{} of {} ordered steps failed",
                        failures.len(),
                        steps.len()
                    );
                }
            }));

            report_outcome(&indent, name, &full_path, outcome, start, result);
        }
    }
}

/// Mark all descendant It nodes as pending (for xdescribe).
fn run_nodes_pending(nodes: &[TestNode], depth: usize, result: &mut RunResult) {
    let indent = "  ".repeat(depth);
    for node in nodes {
        match node {
            TestNode::Describe { name, children, .. } => {
                println!("{indent}{}", bold(&dim(name)));
                run_nodes_pending(children, depth + 1, result);
            }
            TestNode::It { name, .. } => {
                println!("{indent}{} {}", yellow("-"), dim(name));
                result.pending += 1;
            }
            TestNode::Ordered { name, .. } => {
                println!("{indent}{} {}", yellow("-"), dim(name));
                result.pending += 1;
            }
        }
    }
}

fn report_outcome(
    indent: &str,
    name: &str,
    full_path: &str,
    outcome: Result<(), Box<dyn std::any::Any + Send>>,
    start: Instant,
    result: &mut RunResult,
) {
    let elapsed = start.elapsed();
    let ms = elapsed.as_millis();
    let time_str = if ms > 100 {
        format!(" {}", dim(&format!("({ms}ms)")))
    } else {
        String::new()
    };

    match outcome {
        Ok(()) => {
            println!("{indent}{} {}{}", green("✓"), name, time_str);
            result.passed += 1;
        }
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic".to_string()
            };
            println!("{indent}{} {}{}", red("✗"), red(name), time_str);
            println!("{indent}  {}", red(&format!("Error: {msg}")));
            result.failed += 1;
            result.failures.push(format!("{full_path}: {msg}"));
        }
    }
}

/// Run a closure with a timeout.
///
/// The closure runs on the current thread. A separate timer thread signals
/// if the deadline is exceeded. Since we can't abort the current thread,
/// the closure must finish before we can check the result — but if it takes
/// too long, we report a timeout failure.
fn run_with_timeout(
    ms: u64,
    f: &dyn Fn(),
) -> Result<(), Box<dyn std::any::Any + Send>> {
    use std::time::Duration;

    let start = Instant::now();
    let deadline = Duration::from_millis(ms);

    // Run the closure on the current thread
    let _cleanup_guard = crate::Guard::new(crate::run_deferred_cleanups);
    let result = catch_unwind(AssertUnwindSafe(|| {
        f();
    }));

    // Check if the closure exceeded the deadline
    if start.elapsed() > deadline {
        Err(Box::new(format!("test timed out after {ms}ms")))
    } else {
        result
    }
}

fn print_summary(result: &RunResult, elapsed: std::time::Duration) {
    let elapsed_str = format!("{:.3}s", elapsed.as_secs_f64());

    let parts: Vec<String> = [
        (result.passed > 0).then(|| green(&format!("{} passed", result.passed))),
        (result.failed > 0).then(|| red(&format!("{} failed", result.failed))),
        (result.pending > 0).then(|| yellow(&format!("{} pending", result.pending))),
        (result.skipped > 0).then(|| dim(&format!("{} skipped", result.skipped))),
    ]
    .into_iter()
    .flatten()
    .collect();

    let summary = format!("{} ({})", parts.join(", "), dim(&elapsed_str));

    println!();
    if result.failed > 0 {
        println!("{}", red("FAIL"));
        println!("{summary}");
        println!();
        println!("Failures:");
        for (i, failure) in result.failures.iter().enumerate() {
            println!("  {}. {}", i + 1, failure);
        }
        println!();
    } else {
        println!("{}", green("PASS"));
        println!("{summary}");
    }
}

fn list_tree(nodes: &[TestNode], path: &[String], config: &RunConfig) {
    for node in nodes {
        match node {
            TestNode::Describe { name, children, .. } => {
                let mut child_path = path.to_vec();
                child_path.push(name.clone());
                list_tree(children, &child_path, config);
            }
            TestNode::It { name, pending, .. } => {
                let full_path = {
                    let mut p = path.to_vec();
                    p.push(name.clone());
                    p.join(" > ")
                };

                if let Some(ref f) = config.filter {
                    if !full_path.to_lowercase().contains(&f.to_lowercase()) {
                        continue;
                    }
                }

                if *pending {
                    println!("{full_path} (pending)");
                } else {
                    println!("{full_path}");
                }
            }
            TestNode::Ordered { name, .. } => {
                let full_path = {
                    let mut p = path.to_vec();
                    p.push(name.clone());
                    p.join(" > ")
                };

                if let Some(ref f) = config.filter {
                    if !full_path.to_lowercase().contains(&f.to_lowercase()) {
                        continue;
                    }
                }

                println!("{full_path}");
            }
        }
    }
}

fn tree_has_focus(nodes: &[TestNode]) -> bool {
    nodes.iter().any(|node| match node {
        TestNode::It { focused, .. } => *focused,
        TestNode::Describe {
            focused, children, ..
        } => *focused || tree_has_focus(children),
        TestNode::Ordered { .. } => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::Duration;

    #[test]
    fn ordered_is_skipped_when_focus_mode_is_active() {
        static ORDERED_RAN: AtomicBool = AtomicBool::new(false);
        ORDERED_RAN.store(false, Ordering::SeqCst);

        let nodes = vec![TestNode::describe(
            "root",
            vec![
                TestNode::fit("focused", || assert!(true)),
                TestNode::Ordered {
                    name: "ordered".to_string(),
                    labels: Vec::new(),
                    continue_on_failure: false,
                    steps: vec![OrderedStep {
                        name: "step".to_string(),
                        body: Box::new(|| {
                            ORDERED_RAN.store(true, Ordering::SeqCst);
                        }),
                    }],
                },
            ],
        )];

        let config = RunConfig {
            filter: None,
            list: false,
            include_ignored: false,
        };
        let result = run_tree(&nodes, &config);

        assert_eq!(result.failed, 0);
        assert_eq!(result.passed, 1);
        assert_eq!(result.skipped, 1);
        assert!(!ORDERED_RAN.load(Ordering::SeqCst));
    }

    #[test]
    fn retries_and_timeout_compose() {
        static ATTEMPTS: AtomicU32 = AtomicU32::new(0);
        ATTEMPTS.store(0, Ordering::SeqCst);

        let nodes = vec![TestNode::It {
            name: "combined".to_string(),
            focused: false,
            pending: false,
            labels: Vec::new(),
            retries: Some(2),
            timeout_ms: Some(5),
            must_pass_repeatedly: None,
            test_fn: Box::new(|| {
                let n = ATTEMPTS.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                assert!(n >= 2, "attempt {n}");
            }),
        }];

        let config = RunConfig {
            filter: None,
            list: false,
            include_ignored: false,
        };
        let result = run_tree(&nodes, &config);

        assert_eq!(ATTEMPTS.load(Ordering::SeqCst), 3);
        assert_eq!(result.failed, 1);
    }

    #[test]
    fn retries_and_must_pass_repeatedly_compose() {
        static ATTEMPTS: AtomicU32 = AtomicU32::new(0);
        ATTEMPTS.store(0, Ordering::SeqCst);

        let nodes = vec![TestNode::It {
            name: "combined".to_string(),
            focused: false,
            pending: false,
            labels: Vec::new(),
            retries: Some(1),
            timeout_ms: None,
            must_pass_repeatedly: Some(2),
            test_fn: Box::new(|| {
                let n = ATTEMPTS.fetch_add(1, Ordering::SeqCst);
                assert!(n > 0, "first call should fail and retry");
            }),
        }];

        let config = RunConfig {
            filter: None,
            list: false,
            include_ignored: false,
        };
        let result = run_tree(&nodes, &config);

        assert_eq!(ATTEMPTS.load(Ordering::SeqCst), 3);
        assert_eq!(result.failed, 0);
        assert_eq!(result.passed, 1);
    }
}
