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

/// A node in the BDD test tree.
pub enum TestNode {
    /// A describe/context/when container.
    Describe {
        name: String,
        children: Vec<TestNode>,
    },
    /// An individual test case.
    It {
        name: String,
        focused: bool,
        pending: bool,
        test_fn: Box<dyn Fn() + Send + Sync>,
    },
}

impl TestNode {
    pub fn describe(name: impl Into<String>, children: Vec<TestNode>) -> Self {
        TestNode::Describe {
            name: name.into(),
            children,
        }
    }

    pub fn it(name: impl Into<String>, f: impl Fn() + Send + Sync + 'static) -> Self {
        TestNode::It {
            name: name.into(),
            focused: false,
            pending: false,
            test_fn: Box::new(f),
        }
    }

    pub fn fit(name: impl Into<String>, f: impl Fn() + Send + Sync + 'static) -> Self {
        TestNode::It {
            name: name.into(),
            focused: true,
            pending: false,
            test_fn: Box::new(f),
        }
    }

    pub fn xit(name: impl Into<String>, f: impl Fn() + Send + Sync + 'static) -> Self {
        TestNode::It {
            name: name.into(),
            focused: false,
            pending: true,
            test_fn: Box::new(f),
        }
    }
}

// ============================================================================
// ANSI color helpers
// ============================================================================

fn use_color() -> bool {
    // Respect NO_COLOR env var (https://no-color.org/)
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    // Check if stdout is a terminal
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

        let mut i = 1; // skip binary name
        while i < args.len() {
            match args[i].as_str() {
                "--list" => list = true,
                "--include-ignored" | "--ignored" => include_ignored = true,
                arg if !arg.starts_with('-') => {
                    filter = Some(arg.to_string());
                }
                _ => {} // ignore unknown flags
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
    run_nodes(nodes, 0, &[], focus_mode, config, &mut result);
    print_summary(&result, start.elapsed());

    result
}

/// Run multiple named suites, printing a header per suite and a combined summary.
pub fn run_suites(suites: &[Suite], config: &RunConfig) -> RunResult {
    let focus_mode = suites
        .iter()
        .any(|s| tree_has_focus(&s.nodes));
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
        // Print suite header if it has a name or file
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

        run_nodes(&suite.nodes, 0, &[], focus_mode, config, &mut result);

        if suites.len() > 1 {
            println!();
        }
    }

    print_summary(&result, start.elapsed());

    result
}

fn run_nodes(
    nodes: &[TestNode],
    depth: usize,
    path: &[String],
    focus_mode: bool,
    config: &RunConfig,
    result: &mut RunResult,
) {
    let indent = "  ".repeat(depth);

    for node in nodes {
        match node {
            TestNode::Describe { name, children } => {
                println!("{indent}{}", bold(name));
                let mut child_path = path.to_vec();
                child_path.push(name.clone());
                run_nodes(children, depth + 1, &child_path, focus_mode, config, result);
            }
            TestNode::It {
                name,
                focused,
                pending,
                test_fn,
            } => {
                let full_path = {
                    let mut p = path.to_vec();
                    p.push(name.clone());
                    p.join(" > ")
                };

                // Filter check
                if let Some(ref f) = config.filter {
                    if !full_path.to_lowercase().contains(&f.to_lowercase()) {
                        continue;
                    }
                }

                // Pending
                if *pending {
                    println!("{indent}  {} {}", yellow("-"), dim(name));
                    result.pending += 1;
                    continue;
                }

                // Focus mode: skip non-focused
                if focus_mode && !focused && !config.include_ignored {
                    result.skipped += 1;
                    continue;
                }

                // Run the test
                let start = Instant::now();
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    test_fn();
                }));
                let elapsed = start.elapsed();
                let ms = elapsed.as_millis();
                let time_str = if ms > 100 {
                    format!(" {}", dim(&format!("({ms}ms)")))
                } else {
                    String::new()
                };

                match outcome {
                    Ok(()) => {
                        println!(
                            "{indent}  {} {}{}",
                            green("✓"),
                            name,
                            time_str
                        );
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
                        println!(
                            "{indent}  {} {}{}",
                            red("✗"),
                            red(name),
                            time_str
                        );
                        println!(
                            "{indent}    {}",
                            red(&format!("Error: {msg}"))
                        );
                        result.failed += 1;
                        result.failures.push(format!("{full_path}: {msg}"));
                    }
                }
            }
        }
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
            TestNode::Describe { name, children } => {
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
        }
    }
}

fn tree_has_focus(nodes: &[TestNode]) -> bool {
    nodes.iter().any(|node| match node {
        TestNode::It { focused, .. } => *focused,
        TestNode::Describe { children, .. } => tree_has_focus(children),
    })
}
