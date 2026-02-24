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
        after_each: Vec::new(),
        before_all: Vec::new(),
        after_all: Vec::new(),
        focus_mode: has_focus,
    };
    generate_items(&suite.items, &mut ctx)
}

// ============================================================================
// Generation context — tracks inherited hooks
// ============================================================================

struct GenContext {
    /// Accumulated before_each blocks (outermost first).
    before_each: Vec<TokenStream>,
    /// Accumulated after_each blocks (outermost first, reversed at use site).
    after_each: Vec<TokenStream>,
    /// Accumulated before_all blocks.
    before_all: Vec<TokenStream>,
    /// Accumulated after_all blocks.
    after_all: Vec<TokenStream>,
    /// Whether any node in the entire suite is focused.
    focus_mode: bool,
}

impl GenContext {
    fn child(&self) -> Self {
        GenContext {
            before_each: self.before_each.clone(),
            after_each: self.after_each.clone(),
            before_all: self.before_all.clone(),
            after_all: self.after_all.clone(),
            focus_mode: self.focus_mode,
        }
    }
}

// ============================================================================
// Focus detection — recursive scan
// ============================================================================

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

    for item in items {
        match item {
            DslItem::BeforeEach(hook) => {
                ctx.before_each.push(hook.body.clone());
            }
            DslItem::AfterEach(hook) => {
                ctx.after_each.push(hook.body.clone());
            }
            DslItem::BeforeAll(hook) => {
                ctx.before_all.push(hook.body.clone());
            }
            DslItem::AfterAll(hook) => {
                ctx.after_all.push(hook.body.clone());
            }
            DslItem::Describe(block) => {
                output.extend(generate_describe(block, ctx));
            }
            DslItem::It(block) => {
                output.extend(generate_it(block, ctx));
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
    let inner = generate_items(&block.items, &mut child_ctx);

    // If the entire describe is pending, mark all children as ignored.
    // If the describe is focused in focus_mode, its children inherit focus.
    // This is handled naturally: focused describes don't add #[ignore] to their children.
    if block.pending {
        // Wrap everything in a module where tests are ignored.
        // We achieve this by generating a module — the individual `it` blocks
        // inside already check pending status through the parent context.
        // Actually, we handle this by passing pending down. Let's just generate
        // the module; the child items will check describe-level pending in
        // generate_it via the focus_mode logic.
        let mut pending_ctx = ctx.child();
        // Override: in a pending describe, all `it` blocks should be ignored.
        // We signal this by generating the items with a special wrapper.
        let inner = generate_items_pending(&block.items, &mut pending_ctx);
        quote! {
            mod #mod_ident {
                use super::*;
                #inner
            }
        }
    } else {
        quote! {
            mod #mod_ident {
                use super::*;
                #inner
            }
        }
    }
}

/// Generate items where all `it` blocks are forced to be pending (ignored).
fn generate_items_pending(items: &[DslItem], ctx: &mut GenContext) -> TokenStream {
    let mut output = TokenStream::new();

    for item in items {
        match item {
            DslItem::BeforeEach(hook) => {
                ctx.before_each.push(hook.body.clone());
            }
            DslItem::AfterEach(hook) => {
                ctx.after_each.push(hook.body.clone());
            }
            DslItem::BeforeAll(hook) => {
                ctx.before_all.push(hook.body.clone());
            }
            DslItem::AfterAll(hook) => {
                ctx.after_all.push(hook.body.clone());
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
                // Force pending
                let forced = ItBlock {
                    name: block.name.clone(),
                    focused: false,
                    pending: true,
                    labels: block.labels.clone(),
                    retries: block.retries,
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

    // Determine test attribute
    let is_ignored = block.pending
        || (ctx.focus_mode && !block.focused);

    let test_attr = if is_ignored {
        quote! { #[test] #[ignore] }
    } else {
        quote! { #[test] }
    };

    // Inline before_each (outermost first)
    let before_each_code: Vec<_> = ctx.before_each.iter().collect();

    // Inline after_each via Guard (innermost first for proper cleanup order)
    let after_each_guards: Vec<TokenStream> = ctx
        .after_each
        .iter()
        .rev()
        .enumerate()
        .map(|(i, after_body)| {
            let guard_name = Ident::new(&format!("_after_each_guard_{i}"), Span::call_site());
            quote! {
                let #guard_name = rsspec::Guard::new(|| { #after_body });
            }
        })
        .collect();

    // before_all via OnceLock
    let before_all_code: Vec<TokenStream> = ctx
        .before_all
        .iter()
        .enumerate()
        .map(|(i, all_body)| {
            let lock_name = Ident::new(&format!("BEFORE_ALL_{i}"), Span::call_site());
            quote! {
                static #lock_name: std::sync::Once = std::sync::Once::new();
                #lock_name.call_once(|| { #all_body });
            }
        })
        .collect();

    // Label filtering
    let label_check = if block.labels.is_empty() {
        quote! {}
    } else {
        let label_strs: Vec<_> = block.labels.iter().collect();
        quote! {
            if !rsspec::check_labels(&[#(#label_strs),*]) {
                println!("  skipped (labels don't match filter)");
                return;
            }
        }
    };

    // Retry wrapping
    let test_body = if let Some(n) = block.retries {
        quote! {
            rsspec::with_retries(#n, || {
                #(#before_each_code)*
                #(#after_each_guards)*
                #body
            });
        }
    } else {
        quote! {
            #(#before_each_code)*
            #(#after_each_guards)*
            #body
        }
    };

    quote! {
        #test_attr
        fn #fn_ident() {
            #(#before_all_code)*
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

    let mut tests = TokenStream::new();

    for (i, entry) in block.entries.iter().enumerate() {
        let entry_name = if let Some(ref label) = entry.label {
            sanitize_name(&label.value())
        } else {
            format!("case_{}", i + 1)
        };
        let fn_ident = Ident::new(&entry_name, Span::call_site());

        let is_ignored = block.pending || (ctx.focus_mode && !block.focused);
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
        let after_each_guards: Vec<TokenStream> = ctx
            .after_each
            .iter()
            .rev()
            .enumerate()
            .map(|(i, after_body)| {
                let guard_name =
                    Ident::new(&format!("_after_each_guard_{i}"), Span::call_site());
                quote! {
                    let #guard_name = rsspec::Guard::new(|| { #after_body });
                }
            })
            .collect();

        tests.extend(quote! {
            #test_attr
            fn #fn_ident() {
                let _rsspec_entry: (#(#param_types),*,) = (#entry_values,);
                #(#param_bindings)*
                #(#before_each_code)*
                #(#after_each_guards)*
                #body
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

    // Collect all `it` blocks in order. Run them sequentially in one test function.
    let mut steps = Vec::new();
    for item in &block.items {
        if let DslItem::It(it_block) = item {
            let step_name = &it_block.name;
            let body = &it_block.body;
            let before_each_code: Vec<_> = ctx.before_each.iter().collect();
            steps.push(quote! {
                println!("  step: {}", #step_name);
                #(#before_each_code)*
                #body
            });
        }
    }

    quote! {
        #[test]
        fn #fn_ident() {
            #(#steps)*
        }
    }
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
