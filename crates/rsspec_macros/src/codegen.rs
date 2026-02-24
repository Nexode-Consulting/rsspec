//! Code generation — transforms DSL AST into Rust test modules and functions.

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

use crate::dsl::*;

// ============================================================================
// Public entry point
// ============================================================================

pub fn generate(suite: Suite) -> TokenStream {
    let has_focus = suite_has_focus(&suite.items);
    let mut ctx = GenContext {
        before_each: Vec::new(),
        just_before_each: Vec::new(),
        after_each: Vec::new(),
        before_all: Vec::new(),
        after_all_guards: Vec::new(),
        subject: None,
        focus_mode: has_focus,
        force_focused: false,
    };
    let items = generate_items(&suite.items, &mut ctx);

    quote! {
        #items
    }
}

// ============================================================================
// Generation context — tracks inherited hooks
// ============================================================================

struct GenContext {
    /// Accumulated before_each blocks (outermost first).
    before_each: Vec<TokenStream>,
    /// Accumulated just_before_each blocks (outermost first, runs after all before_each).
    just_before_each: Vec<TokenStream>,
    /// Accumulated after_each blocks (outermost first, reversed at use site).
    after_each: Vec<TokenStream>,
    /// Accumulated before_all: (static_ident, body) pairs — statics live at module level.
    before_all: Vec<(Ident, TokenStream)>,
    /// after_all guards: each entry is (counter_ident, total_tests, body) to emit into each test.
    after_all_guards: Vec<(Ident, usize, TokenStream)>,
    /// Subject expression — inlined as `let subject = { expr };` after just_before_each.
    /// Nested subjects override parent (last one wins).
    subject: Option<TokenStream>,
    /// Whether any node in the entire suite is focused.
    focus_mode: bool,
    /// Whether all children should be treated as focused (inside fdescribe/fcontext).
    force_focused: bool,
}

impl GenContext {
    fn child(&self) -> Self {
        GenContext {
            before_each: self.before_each.clone(),
            just_before_each: self.just_before_each.clone(),
            after_each: self.after_each.clone(),
            before_all: self.before_all.clone(),
            after_all_guards: self.after_all_guards.clone(),
            subject: self.subject.clone(),
            focus_mode: self.focus_mode,
            force_focused: self.force_focused,
        }
    }
}

// ============================================================================
// Focus detection — recursive scan
// ============================================================================

/// Count the number of test functions that will actually run (not ignored).
/// Used for after_all counter tracking.
fn count_active_tests(items: &[DslItem], focus_mode: bool, force_focused: bool) -> usize {
    items
        .iter()
        .map(|item| match item {
            DslItem::It(it) => {
                if it.pending || (focus_mode && !it.focused && !force_focused) {
                    0
                } else {
                    1
                }
            }
            DslItem::Describe(d) => {
                if d.pending {
                    0
                } else {
                    let child_forced = force_focused || d.focused;
                    count_active_tests(&d.items, focus_mode, child_forced)
                }
            }
            DslItem::DescribeTable(dt) => {
                if dt.pending || (focus_mode && !dt.focused && !force_focused) {
                    0
                } else {
                    dt.entries.len()
                }
            }
            DslItem::Ordered(_) => {
                // ordered generates a single test fn — count 1 if any step would run
                if focus_mode && !force_focused {
                    0 // ordered blocks don't support individual focus
                } else {
                    1
                }
            }
            _ => 0,
        })
        .sum()
}

fn suite_has_focus(items: &[DslItem]) -> bool {
    items.iter().any(|item| match item {
        DslItem::Describe(d) => d.focused || suite_has_focus(&d.items),
        DslItem::It(it) => it.focused,
        DslItem::DescribeTable(dt) => dt.focused,
        DslItem::Ordered(o) => suite_has_focus(&o.items),
        _ => false,
    })
}

// ============================================================================
// Item generation
// ============================================================================

fn generate_items(items: &[DslItem], ctx: &mut GenContext) -> TokenStream {
    let mut output = TokenStream::new();
    let mut nameless_it_counter = 0u32;

    for item in items {
        match item {
            DslItem::BeforeEach(hook) => {
                ctx.before_each.push(hook.body.clone());
            }
            DslItem::JustBeforeEach(hook) => {
                ctx.just_before_each.push(hook.body.clone());
            }
            DslItem::AfterEach(hook) => {
                ctx.after_each.push(hook.body.clone());
            }
            DslItem::Subject(hook) => {
                ctx.subject = Some(hook.body.clone());
            }
            DslItem::BeforeAll(_) | DslItem::AfterAll(_) => {
                // Handled at describe level — see generate_describe
            }
            DslItem::Describe(block) => {
                output.extend(generate_describe(block, ctx));
            }
            DslItem::It(block) => {
                if block.name.value().is_empty() {
                    nameless_it_counter += 1;
                    let named = ItBlock {
                        name: syn::LitStr::new(
                            &format!("spec_{nameless_it_counter}"),
                            block.name.span(),
                        ),
                        focused: block.focused,
                        pending: block.pending,
                        labels: block.labels.clone(),
                        retries: block.retries,
                        must_pass_repeatedly: block.must_pass_repeatedly,
                        timeout_ms: block.timeout_ms,
                        body: block.body.clone(),
                    };
                    output.extend(generate_it(&named, ctx));
                } else {
                    output.extend(generate_it(block, ctx));
                }
            }
            DslItem::DescribeTable(block) => {
                output.extend(generate_describe_table(block, ctx));
            }
            DslItem::Ordered(block) => {
                output.extend(generate_ordered(block, ctx));
            }
        }
    }

    output
}

// ============================================================================
// describe / context / when
// ============================================================================

fn generate_describe(block: &DescribeBlock, ctx: &GenContext) -> TokenStream {
    let mod_name = sanitize_name(&block.name.value());
    let mod_ident = Ident::new(&mod_name, Span::call_site());

    let mut child_ctx = ctx.child();

    // Propagate focus from fdescribe/fcontext to children
    if block.focused {
        child_ctx.force_focused = true;
    }

    // Scan for before_all hooks at this describe level and set up module-level statics
    let before_all_bodies: Vec<_> = block
        .items
        .iter()
        .filter_map(|item| {
            if let DslItem::BeforeAll(hook) = item {
                Some(hook.body.clone())
            } else {
                None
            }
        })
        .collect();

    for (i, body) in before_all_bodies.into_iter().enumerate() {
        let static_ident = Ident::new(
            &format!("BEFORE_ALL_{}_{i}", mod_name.to_uppercase()),
            Span::call_site(),
        );
        child_ctx.before_all.push((static_ident, body));
    }

    // Generate static Once for before_all added at this scope
    let before_all_statics: Vec<TokenStream> = child_ctx
        .before_all
        .iter()
        .skip(ctx.before_all.len()) // only new ones from this scope
        .map(|(static_ident, _)| {
            quote! {
                static #static_ident: std::sync::Once = std::sync::Once::new();
            }
        })
        .collect();

    // Scan for after_all hooks at this describe level and set up counter-based guards
    let after_all_bodies: Vec<_> = block
        .items
        .iter()
        .filter_map(|item| {
            if let DslItem::AfterAll(hook) = item {
                Some(hook.body.clone())
            } else {
                None
            }
        })
        .collect();

    if !after_all_bodies.is_empty() {
        let total = count_active_tests(&block.items, child_ctx.focus_mode, child_ctx.force_focused);
        for (i, body) in after_all_bodies.into_iter().enumerate() {
            let counter_ident = Ident::new(
                &format!("AFTER_ALL_COUNTER_{}_{i}", mod_name.to_uppercase()),
                Span::call_site(),
            );
            child_ctx
                .after_all_guards
                .push((counter_ident, total, body));
        }
    }

    // Generate static counters for after_all guards added at this scope
    let after_all_statics: Vec<TokenStream> = child_ctx
        .after_all_guards
        .iter()
        .skip(ctx.after_all_guards.len()) // only new ones from this scope
        .map(|(counter_ident, _, _)| {
            quote! {
                static #counter_ident: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(0);
            }
        })
        .collect();

    if block.pending {
        let mut pending_ctx = ctx.child();
        let inner = generate_items_pending(&block.items, &mut pending_ctx);
        quote! {
            mod #mod_ident {
                use super::*;
                #(#before_all_statics)*
                #(#after_all_statics)*
                #inner
            }
        }
    } else {
        let inner = generate_items(&block.items, &mut child_ctx);
        quote! {
            mod #mod_ident {
                use super::*;
                #(#before_all_statics)*
                #(#after_all_statics)*
                #inner
            }
        }
    }
}

/// Generate items where all `it` blocks are forced to be pending (ignored).
fn generate_items_pending(items: &[DslItem], ctx: &mut GenContext) -> TokenStream {
    let mut output = TokenStream::new();
    let mut nameless_it_counter = 0u32;

    for item in items {
        match item {
            DslItem::BeforeEach(hook) => {
                ctx.before_each.push(hook.body.clone());
            }
            DslItem::JustBeforeEach(hook) => {
                ctx.just_before_each.push(hook.body.clone());
            }
            DslItem::AfterEach(hook) => {
                ctx.after_each.push(hook.body.clone());
            }
            DslItem::Subject(hook) => {
                ctx.subject = Some(hook.body.clone());
            }
            DslItem::BeforeAll(_) | DslItem::AfterAll(_) => {
                // In pending mode, these are irrelevant
            }
            DslItem::Describe(block) => {
                let mod_name = sanitize_name(&block.name.value());
                let mod_ident = Ident::new(&mod_name, Span::call_site());
                let mut child_ctx = ctx.child();
                let inner = generate_items_pending(&block.items, &mut child_ctx);
                output.extend(quote! {
                    mod #mod_ident {
                        use super::*;
                        #inner
                    }
                });
            }
            DslItem::It(block) => {
                // Force pending; handle nameless it
                let name = if block.name.value().is_empty() {
                    nameless_it_counter += 1;
                    syn::LitStr::new(
                        &format!("spec_{nameless_it_counter}"),
                        block.name.span(),
                    )
                } else {
                    block.name.clone()
                };
                let forced = ItBlock {
                    name,
                    focused: false,
                    pending: true,
                    labels: block.labels.clone(),
                    retries: block.retries,
                    must_pass_repeatedly: block.must_pass_repeatedly,
                    timeout_ms: block.timeout_ms,
                    body: block.body.clone(),
                };
                output.extend(generate_it(&forced, ctx));
            }
            DslItem::DescribeTable(block) => {
                let forced = DescribeTableBlock {
                    name: block.name.clone(),
                    focused: false,
                    pending: true,
                    params: block.params.iter().map(|p| TableParam { name: p.name.clone(), ty: p.ty.clone() }).collect(),
                    entries: block.entries.iter().map(|e| TableEntry { label: e.label.clone(), values: e.values.clone() }).collect(),
                    body: block.body.clone(),
                };
                output.extend(generate_describe_table(&forced, ctx));
            }
            DslItem::Ordered(block) => {
                let mut child_ctx = ctx.child();
                let inner = generate_items_pending(&block.items, &mut child_ctx);
                output.extend(inner);
            }
        }
    }

    output
}

// ============================================================================
// it / specify
// ============================================================================

fn generate_it(block: &ItBlock, ctx: &GenContext) -> TokenStream {
    let fn_name = sanitize_name(&block.name.value());
    let fn_ident = Ident::new(&fn_name, Span::call_site());
    let body = &block.body;

    // Determine test attribute (force_focused propagates from fdescribe/fcontext)
    let effectively_focused = block.focused || ctx.force_focused;
    let is_ignored = block.pending
        || (ctx.focus_mode && !effectively_focused);

    let test_attr = if is_ignored {
        quote! { #[test] #[ignore] }
    } else {
        quote! { #[test] }
    };

    // Inline before_each (outermost first)
    let before_each_code: Vec<_> = ctx.before_each.iter().collect();

    // Inline just_before_each (outermost first, runs after all before_each)
    let just_before_each_code: Vec<_> = ctx.just_before_each.iter().collect();

    // after_each bodies (innermost first for proper cleanup order)
    let after_each_bodies: Vec<_> = ctx.after_each.iter().rev().collect();

    // before_all via module-level Once statics
    let before_all_code: Vec<TokenStream> = ctx
        .before_all
        .iter()
        .map(|(static_ident, all_body)| {
            quote! {
                #static_ident.call_once(|| { #all_body });
            }
        })
        .collect();

    // after_all via counter-based guard (only for non-ignored tests)
    let after_all_guards_code: Vec<TokenStream> = if is_ignored {
        Vec::new()
    } else {
        ctx.after_all_guards
            .iter()
            .enumerate()
            .map(|(i, (counter_ident, total, all_body))| {
                let guard_name =
                    Ident::new(&format!("_after_all_guard_{i}"), Span::call_site());
                let total_u32 = *total as u32;
                quote! {
                    let #guard_name = rsspec::AfterAllGuard::new(
                        &#counter_ident,
                        #total_u32,
                        || { #all_body },
                    );
                }
            })
            .collect()
    };

    // Label filtering — always emit, even for unlabeled tests
    let label_strs: Vec<_> = block.labels.iter().collect();
    let label_check = quote! {
        if !rsspec::check_labels(&[#(#label_strs),*]) {
            return;
        }
    };

    // Fail-on-focus check (only for focused tests, prevents accidental commits)
    let focus_check = if effectively_focused && ctx.focus_mode {
        quote! { rsspec::check_fail_on_focus(); }
    } else {
        quote! {}
    };

    // Inline subject (if any) as `let subject = { expr };`
    let subject_code = ctx.subject.as_ref().map(|expr| {
        quote! { let subject = { #expr }; }
    });

    // Build the inner body: before_each + just_before_each + subject + body [+ after_each]
    // When after_each exists, use catch_unwind to guarantee after_each runs even on panic
    let inner_body = if ctx.after_each.is_empty() {
        quote! {
            #(#before_each_code)*
            #(#just_before_each_code)*
            #subject_code
            #body
        }
    } else {
        quote! {
            #(#before_each_code)*
            #(#just_before_each_code)*
            #subject_code
            let _rsspec_body_result = std::panic::catch_unwind(
                std::panic::AssertUnwindSafe(|| { #body })
            );
            // after_each runs unconditionally (innermost first)
            #(#after_each_bodies)*
            // resume body panic if any
            if let Err(_rsspec_panic) = _rsspec_body_result {
                std::panic::resume_unwind(_rsspec_panic);
            }
        }
    };

    // Wrap with retries if specified
    let with_retries = if let Some(n) = block.retries {
        quote! {
            rsspec::with_retries(#n, || {
                #inner_body
            });
        }
    } else {
        inner_body
    };

    // Wrap with must_pass_repeatedly if specified
    let with_mpr = if let Some(n) = block.must_pass_repeatedly {
        quote! {
            rsspec::must_pass_repeatedly(#n, || {
                #with_retries
            });
        }
    } else {
        with_retries
    };

    // Wrap with timeout if specified
    let test_body = if let Some(ms) = block.timeout_ms {
        quote! {
            rsspec::with_timeout(#ms, || {
                #with_mpr
            });
        }
    } else {
        with_mpr
    };

    quote! {
        #test_attr
        fn #fn_ident() {
            // Ensure deferred cleanups run even on panic
            let _rsspec_defer_guard = rsspec::Guard::new(|| {
                rsspec::run_deferred_cleanups();
            });
            #focus_check
            #(#before_all_code)*
            #(#after_all_guards_code)*
            #label_check
            #test_body
        }
    }
}

// ============================================================================
// describe_table
// ============================================================================

fn generate_describe_table(block: &DescribeTableBlock, ctx: &GenContext) -> TokenStream {
    let mod_name = sanitize_name(&block.name.value());
    let mod_ident = Ident::new(&mod_name, Span::call_site());

    let effectively_focused = block.focused || ctx.force_focused;
    let is_ignored = block.pending || (ctx.focus_mode && !effectively_focused);

    let mut tests = TokenStream::new();

    for (i, entry) in block.entries.iter().enumerate() {
        let entry_name = if let Some(ref label) = entry.label {
            sanitize_name(&label.value())
        } else {
            format!("case_{}", i + 1)
        };
        let fn_ident = Ident::new(&entry_name, Span::call_site());

        let test_attr = if is_ignored {
            quote! { #[test] #[ignore] }
        } else {
            quote! { #[test] }
        };

        // Generate parameter bindings
        let param_bindings: Vec<TokenStream> = block
            .params
            .iter()
            .enumerate()
            .map(|(j, param)| {
                let param_name = &param.name;
                let param_type = &param.ty;
                let idx = syn::Index::from(j);
                quote! {
                    let #param_name: #param_type = _rsspec_entry.#idx;
                }
            })
            .collect();

        let entry_values = &entry.values;
        let param_types: Vec<_> = block.params.iter().map(|p| &p.ty).collect();
        let body = &block.body;

        // Inline hooks
        let before_each_code: Vec<_> = ctx.before_each.iter().collect();
        let just_before_each_code: Vec<_> = ctx.just_before_each.iter().collect();
        let after_each_bodies: Vec<_> = ctx.after_each.iter().rev().collect();

        // before_all
        let before_all_code: Vec<TokenStream> = ctx
            .before_all
            .iter()
            .map(|(static_ident, all_body)| {
                quote! { #static_ident.call_once(|| { #all_body }); }
            })
            .collect();

        // after_all (only for non-ignored tests)
        let after_all_guards_code: Vec<TokenStream> = if is_ignored {
            Vec::new()
        } else {
            ctx.after_all_guards
                .iter()
                .enumerate()
                .map(|(j, (counter_ident, total, all_body))| {
                    let guard_name =
                        Ident::new(&format!("_after_all_guard_{j}"), Span::call_site());
                    let total_u32 = *total as u32;
                    quote! {
                        let #guard_name = rsspec::AfterAllGuard::new(
                            &#counter_ident, #total_u32, || { #all_body },
                        );
                    }
                })
                .collect()
        };

        // Label check
        let label_check = quote! {
            if !rsspec::check_labels(&[]) { return; }
        };

        // Inline subject
        let subject_code = ctx.subject.as_ref().map(|expr| {
            quote! { let subject = { #expr }; }
        });

        // Build inner body with catch_unwind for after_each
        let inner_body = if ctx.after_each.is_empty() {
            quote! {
                #(#before_each_code)*
                #(#just_before_each_code)*
                #subject_code
                #body
            }
        } else {
            quote! {
                #(#before_each_code)*
                #(#just_before_each_code)*
                #subject_code
                let _rsspec_body_result = std::panic::catch_unwind(
                    std::panic::AssertUnwindSafe(|| { #body })
                );
                #(#after_each_bodies)*
                if let Err(_rsspec_panic) = _rsspec_body_result {
                    std::panic::resume_unwind(_rsspec_panic);
                }
            }
        };

        tests.extend(quote! {
            #test_attr
            fn #fn_ident() {
                let _rsspec_defer_guard = rsspec::Guard::new(|| {
                    rsspec::run_deferred_cleanups();
                });
                #(#before_all_code)*
                #(#after_all_guards_code)*
                #label_check
                let _rsspec_entry: (#(#param_types),*,) = (#entry_values,);
                #(#param_bindings)*
                #inner_body
            }
        });
    }

    quote! {
        mod #mod_ident {
            use super::*;
            #tests
        }
    }
}

// ============================================================================
// ordered
// ============================================================================

fn generate_ordered(block: &OrderedBlock, ctx: &GenContext) -> TokenStream {
    let fn_name = sanitize_name(&block.name.value());
    let fn_ident = Ident::new(&fn_name, Span::call_site());

    // before_all
    let before_all_code: Vec<TokenStream> = ctx
        .before_all
        .iter()
        .map(|(static_ident, all_body)| {
            quote! { #static_ident.call_once(|| { #all_body }); }
        })
        .collect();

    // after_all
    let after_all_guards_code: Vec<TokenStream> = ctx
        .after_all_guards
        .iter()
        .enumerate()
        .map(|(i, (counter_ident, total, all_body))| {
            let guard_name = Ident::new(&format!("_after_all_guard_{i}"), Span::call_site());
            let total_u32 = *total as u32;
            quote! {
                let #guard_name = rsspec::AfterAllGuard::new(
                    &#counter_ident, #total_u32, || { #all_body },
                );
            }
        })
        .collect();

    let after_each_bodies: Vec<_> = ctx.after_each.iter().rev().collect();

    // Inline subject
    let subject_code = ctx.subject.as_ref().map(|expr| {
        quote! { let subject = { #expr }; }
    });

    // Collect all `it` blocks in order. Run them sequentially in one test function.
    let mut steps = Vec::new();
    for item in &block.items {
        if let DslItem::It(it_block) = item {
            let step_name = &it_block.name;
            let body = &it_block.body;
            let before_each_code: Vec<_> = ctx.before_each.iter().collect();
            let just_before_each_code: Vec<_> = ctx.just_before_each.iter().collect();
            steps.push(quote! {
                println!("  step: {}", #step_name);
                #(#before_each_code)*
                #(#just_before_each_code)*
                #subject_code
                #body
            });
        }
    }

    // Label check
    let label_check = quote! {
        if !rsspec::check_labels(&[]) { return; }
    };

    let ordered_body = if block.continue_on_failure {
        let total_steps = steps.len();
        let catch_steps: Vec<TokenStream> = steps
            .into_iter()
            .map(|step| {
                quote! {
                    if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        #step
                    })) {
                        _rsspec_failures.push(e);
                    }
                }
            })
            .collect();

        quote! {
            let mut _rsspec_failures: Vec<Box<dyn std::any::Any + Send>> = Vec::new();
            #(#catch_steps)*
            if !_rsspec_failures.is_empty() {
                panic!(
                    "{} of {} ordered steps failed",
                    _rsspec_failures.len(),
                    #total_steps,
                );
            }
        }
    } else {
        quote! { #(#steps)* }
    };

    // Wrap ordered body with after_each via catch_unwind
    let body_with_after = if ctx.after_each.is_empty() {
        ordered_body
    } else {
        quote! {
            let _rsspec_body_result = std::panic::catch_unwind(
                std::panic::AssertUnwindSafe(|| { #ordered_body })
            );
            #(#after_each_bodies)*
            if let Err(_rsspec_panic) = _rsspec_body_result {
                std::panic::resume_unwind(_rsspec_panic);
            }
        }
    };

    quote! {
        #[test]
        fn #fn_ident() {
            let _rsspec_defer_guard = rsspec::Guard::new(|| {
                rsspec::run_deferred_cleanups();
            });
            #(#before_all_code)*
            #(#after_all_guards_code)*
            #label_check
            #body_with_after
        }
    }
}

// ============================================================================
// BDD runner code generation (for `harness = false`)
// ============================================================================

/// Generate a `fn main()` that builds a test tree and runs it with the BDD runner.
pub fn generate_bdd(suite: Suite) -> TokenStream {
    let tree_nodes = generate_bdd_items(&suite.items, &BddGenContext::new());

    quote! {
        fn main() {
            let nodes: Vec<rsspec::runner::TestNode> = vec![#(#tree_nodes),*];
            let suite = rsspec::runner::Suite::new("", file!(), nodes);
            let config = rsspec::runner::RunConfig::from_args();
            let result = rsspec::runner::run_suites(&[suite], &config);
            if result.failed > 0 {
                std::process::exit(1);
            }
        }
    }
}

/// Generate just the test tree (no `fn main()`), for combining multiple suites.
pub fn generate_bdd_suite(suite: Suite) -> TokenStream {
    let tree_nodes = generate_bdd_items(&suite.items, &BddGenContext::new());

    quote! {
        {
            let nodes: Vec<rsspec::runner::TestNode> = vec![#(#tree_nodes),*];
            nodes
        }
    }
}

/// BDD generation context — tracks inherited hooks for the BDD runner path.
struct BddGenContext {
    before_each: Vec<TokenStream>,
    just_before_each: Vec<TokenStream>,
    after_each: Vec<TokenStream>,
    subject: Option<TokenStream>,
}

impl BddGenContext {
    fn new() -> Self {
        BddGenContext {
            before_each: Vec::new(),
            just_before_each: Vec::new(),
            after_each: Vec::new(),
            subject: None,
        }
    }

    fn child(&self) -> Self {
        BddGenContext {
            before_each: self.before_each.clone(),
            just_before_each: self.just_before_each.clone(),
            after_each: self.after_each.clone(),
            subject: self.subject.clone(),
        }
    }
}

/// Generate `TestNode` constructors for a list of DSL items.
fn generate_bdd_items(items: &[DslItem], ctx: &BddGenContext) -> Vec<TokenStream> {
    let mut local_ctx = ctx.child();
    let mut nodes = Vec::new();
    let mut nameless_it_counter = 0u32;

    for item in items {
        match item {
            DslItem::BeforeEach(hook) => {
                local_ctx.before_each.push(hook.body.clone());
            }
            DslItem::JustBeforeEach(hook) => {
                local_ctx.just_before_each.push(hook.body.clone());
            }
            DslItem::AfterEach(hook) => {
                local_ctx.after_each.push(hook.body.clone());
            }
            DslItem::Subject(hook) => {
                local_ctx.subject = Some(hook.body.clone());
            }
            DslItem::BeforeAll(_) | DslItem::AfterAll(_) => {
                // before_all/after_all not yet supported in BDD mode
                // (would need module-level statics, which don't fit closures well)
            }
            DslItem::Describe(block) => {
                let name = block.name.value();
                if block.pending {
                    let child_nodes = generate_bdd_items_pending(&block.items);
                    nodes.push(quote! {
                        rsspec::runner::TestNode::describe(#name, vec![#(#child_nodes),*])
                    });
                } else {
                    // Pass accumulated hooks to children
                    let child_nodes = generate_bdd_items(&block.items, &local_ctx);
                    nodes.push(quote! {
                        rsspec::runner::TestNode::describe(#name, vec![#(#child_nodes),*])
                    });
                }
            }
            DslItem::It(block) => {
                let name = if block.name.value().is_empty() {
                    nameless_it_counter += 1;
                    format!("spec {nameless_it_counter}")
                } else {
                    block.name.value()
                };
                let body = &block.body;
                let be = &local_ctx.before_each;
                let jbe = &local_ctx.just_before_each;
                let ae_bodies: Vec<_> = local_ctx.after_each.iter().rev().collect();
                let subject_code = local_ctx.subject.as_ref().map(|expr| {
                    quote! { let subject = { #expr }; }
                });

                // Use catch_unwind pattern for after_each (same as suite! path)
                let it_body = if local_ctx.after_each.is_empty() {
                    quote! {
                        #(#be)*
                        #(#jbe)*
                        #subject_code
                        #body
                    }
                } else {
                    quote! {
                        #(#be)*
                        #(#jbe)*
                        #subject_code
                        let _rsspec_body_result = std::panic::catch_unwind(
                            std::panic::AssertUnwindSafe(|| { #body })
                        );
                        #(#ae_bodies)*
                        if let Err(_rsspec_panic) = _rsspec_body_result {
                            std::panic::resume_unwind(_rsspec_panic);
                        }
                    }
                };

                let constructor = if block.pending {
                    quote! { rsspec::runner::TestNode::xit(#name, || {}) }
                } else if block.focused {
                    quote! { rsspec::runner::TestNode::fit(#name, || { #it_body }) }
                } else {
                    quote! { rsspec::runner::TestNode::it(#name, || { #it_body }) }
                };

                nodes.push(constructor);
            }
            DslItem::DescribeTable(block) => {
                let table_name = block.name.value();
                let mut entry_nodes = Vec::new();

                for (i, entry) in block.entries.iter().enumerate() {
                    let entry_name = if let Some(ref label) = entry.label {
                        label.value()
                    } else {
                        format!("case_{}", i + 1)
                    };

                    let entry_values = &entry.values;
                    let param_types: Vec<_> = block.params.iter().map(|p| &p.ty).collect();
                    let param_bindings: Vec<TokenStream> = block
                        .params
                        .iter()
                        .enumerate()
                        .map(|(j, param)| {
                            let param_name = &param.name;
                            let param_type = &param.ty;
                            let idx = syn::Index::from(j);
                            quote! {
                                let #param_name: #param_type = _rsspec_entry.#idx;
                            }
                        })
                        .collect();

                    let body = &block.body;
                    let be = &local_ctx.before_each;
                    let jbe = &local_ctx.just_before_each;
                    let ae_bodies: Vec<_> = local_ctx.after_each.iter().rev().collect();
                    let subject_code = local_ctx.subject.as_ref().map(|expr| {
                        quote! { let subject = { #expr }; }
                    });

                    let entry_body = if local_ctx.after_each.is_empty() {
                        quote! {
                            let _rsspec_entry: (#(#param_types),*,) = (#entry_values,);
                            #(#param_bindings)*
                            #(#be)*
                            #(#jbe)*
                            #subject_code
                            #body
                        }
                    } else {
                        quote! {
                            let _rsspec_entry: (#(#param_types),*,) = (#entry_values,);
                            #(#param_bindings)*
                            #(#be)*
                            #(#jbe)*
                            #subject_code
                            let _rsspec_body_result = std::panic::catch_unwind(
                                std::panic::AssertUnwindSafe(|| { #body })
                            );
                            #(#ae_bodies)*
                            if let Err(_rsspec_panic) = _rsspec_body_result {
                                std::panic::resume_unwind(_rsspec_panic);
                            }
                        }
                    };

                    let constructor = if block.pending {
                        quote! { rsspec::runner::TestNode::xit(#entry_name, || {}) }
                    } else {
                        quote! { rsspec::runner::TestNode::it(#entry_name, || { #entry_body }) }
                    };

                    entry_nodes.push(constructor);
                }

                nodes.push(quote! {
                    rsspec::runner::TestNode::describe(#table_name, vec![#(#entry_nodes),*])
                });
            }
            DslItem::Ordered(block) => {
                let name = block.name.value();
                let be = &local_ctx.before_each;
                let jbe = &local_ctx.just_before_each;
                let subject_code = local_ctx.subject.as_ref().map(|expr| {
                    quote! { let subject = { #expr }; }
                });
                let mut step_bodies = Vec::new();
                for item in &block.items {
                    if let DslItem::It(it_block) = item {
                        let step_name = &it_block.name;
                        let body = &it_block.body;
                        step_bodies.push(quote! {
                            rsspec::by(#step_name);
                            #(#be)*
                            #(#jbe)*
                            #subject_code
                            #body
                        });
                    }
                }
                nodes.push(quote! {
                    rsspec::runner::TestNode::it(#name, || {
                        #(#step_bodies)*
                    })
                });
            }
        }
    }

    nodes
}

/// Generate pending `TestNode` constructors for all items.
fn generate_bdd_items_pending(items: &[DslItem]) -> Vec<TokenStream> {
    let mut nodes = Vec::new();

    for item in items {
        match item {
            DslItem::Describe(block) => {
                let name = block.name.value();
                let child_nodes = generate_bdd_items_pending(&block.items);
                nodes.push(quote! {
                    rsspec::runner::TestNode::describe(#name, vec![#(#child_nodes),*])
                });
            }
            DslItem::It(block) => {
                let name = block.name.value();
                nodes.push(quote! {
                    rsspec::runner::TestNode::xit(#name, || {})
                });
            }
            DslItem::DescribeTable(block) => {
                let table_name = block.name.value();
                let mut entry_nodes = Vec::new();
                for (i, entry) in block.entries.iter().enumerate() {
                    let entry_name = if let Some(ref label) = entry.label {
                        label.value()
                    } else {
                        format!("case_{}", i + 1)
                    };
                    entry_nodes.push(quote! {
                        rsspec::runner::TestNode::xit(#entry_name, || {})
                    });
                }
                nodes.push(quote! {
                    rsspec::runner::TestNode::describe(#table_name, vec![#(#entry_nodes),*])
                });
            }
            _ => {}
        }
    }

    nodes
}

// ============================================================================
// Helpers
// ============================================================================

/// Convert a human-readable name to a valid Rust identifier.
fn sanitize_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();

    // Collapse consecutive underscores and trim
    let mut result = String::new();
    let mut prev_underscore = false;
    for c in sanitized.chars() {
        if c == '_' {
            if !prev_underscore && !result.is_empty() {
                result.push('_');
            }
            prev_underscore = true;
        } else {
            result.push(c);
            prev_underscore = false;
        }
    }

    // Trim trailing underscore
    if result.ends_with('_') {
        result.pop();
    }

    // Ensure non-empty and doesn't start with a digit
    if result.is_empty() {
        return "unnamed".to_string();
    }
    if result.chars().next().unwrap().is_ascii_digit() {
        result.insert(0, '_');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("adds two numbers"), "adds_two_numbers");
        assert_eq!(sanitize_name("handles -1 correctly"), "handles_1_correctly");
        assert_eq!(sanitize_name("when user is logged in"), "when_user_is_logged_in");
        assert_eq!(sanitize_name("step 1: create"), "step_1_create");
        assert_eq!(sanitize_name(""), "unnamed");
        assert_eq!(sanitize_name("123test"), "_123test");
    }
}
