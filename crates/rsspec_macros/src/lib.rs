//! Proc macros for the `rsspec` BDD testing framework.

mod codegen;
mod dsl;

/// A Ginkgo/RSpec-inspired BDD test suite macro.
///
/// # Example
///
/// ```rust,ignore
/// rsspec::suite! {
///     describe "Calculator" {
///         before_each {
///             let mut calc = Calculator::new();
///         }
///
///         it "adds two numbers" {
///             assert_eq!(calc.add(2, 3), 5);
///         }
///
///         context "with negative numbers" {
///             it "handles negatives" {
///                 assert_eq!(calc.add(-1, 3), 2);
///             }
///         }
///     }
/// }
/// ```
///
/// # Supported DSL keywords
///
/// ## Containers
/// - `describe "name" { ... }` / `context "name" { ... }` / `when "name" { ... }`
/// - `fdescribe` / `fcontext` / `fwhen` — focused (only these run)
/// - `xdescribe` / `xcontext` / `xwhen` / `pdescribe` / `pcontext` / `pwhen` — pending (skipped)
///
/// ## Specs
/// - `it "name" { ... }` / `specify "name" { ... }`
/// - `fit` / `fspecify` — focused
/// - `xit` / `xspecify` / `pit` / `pspecify` — pending
///
/// ## Lifecycle hooks
/// - `before_each { ... }` — runs before every `it` in this scope (and nested scopes)
/// - `after_each { ... }` — runs after every `it` (even on panic)
/// - `before_all { ... }` — runs once before all tests in scope
/// - `after_all { ... }` — runs once after all tests in scope
///
/// ## Decorators (on `it` blocks)
/// - `it "name" labels("integration", "slow") { ... }` — label filtering via `RSSPEC_LABEL_FILTER`
/// - `it "name" retries(3) { ... }` — retry flaky tests
///
/// ## Table-driven tests
/// ```rust,ignore
/// describe_table "arithmetic" (a: i32, b: i32, expected: i32) [
///     "addition" (2, 3, 5),
///     "subtraction" (5, 3, 2),
/// ] {
///     assert_eq!(a + b, expected);
/// }
/// ```
///
/// ## Ordered (sequential, fail-fast)
/// ```rust,ignore
/// ordered "workflow" {
///     it "step 1" { create_resource(); }
///     it "step 2" { verify_resource(); }
/// }
/// ```
#[proc_macro]
pub fn suite(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let suite = syn::parse_macro_input!(input as dsl::Suite);
    codegen::generate(suite).into()
}
