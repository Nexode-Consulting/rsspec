//! DSL AST types and `syn::parse::Parse` implementations.
//!
//! Parses the Ginkgo-inspired DSL syntax into a structured AST.

use proc_macro2::TokenStream;
use syn::parse::{Parse, ParseStream};
use syn::{braced, bracketed, parenthesized, Ident, LitInt, LitStr, Result, Token, Type};

// ============================================================================
// AST types
// ============================================================================

/// Top-level suite — a list of DSL items.
#[derive(Debug)]
pub struct Suite {
    pub items: Vec<DslItem>,
}

/// A single DSL node.
#[derive(Debug)]
pub enum DslItem {
    Describe(DescribeBlock),
    It(ItBlock),
    BeforeEach(HookBlock),
    JustBeforeEach(HookBlock),
    AfterEach(HookBlock),
    BeforeAll(HookBlock),
    AfterAll(HookBlock),
    DescribeTable(DescribeTableBlock),
    Ordered(OrderedBlock),
}

/// `describe "name" { ... }` / `context "name" { ... }` / `when "name" { ... }`
/// Also handles focused (`fdescribe`, `fcontext`) and pending (`xdescribe`, `xcontext`, `pdescribe`, `pcontext`).
#[derive(Debug)]
pub struct DescribeBlock {
    pub name: LitStr,
    pub focused: bool,
    pub pending: bool,
    pub items: Vec<DslItem>,
}

/// `it "name" { ... }` / `specify "name" { ... }`
/// Also handles focused (`fit`) and pending (`xit`, `pit`).
#[derive(Debug)]
pub struct ItBlock {
    pub name: LitStr,
    pub focused: bool,
    pub pending: bool,
    pub labels: Vec<LitStr>,
    pub retries: Option<u32>,
    pub must_pass_repeatedly: Option<u32>,
    pub timeout_ms: Option<u64>,
    pub body: TokenStream,
}

/// `before_each { ... }` / `after_each { ... }` / `before_all { ... }` / `after_all { ... }`
#[derive(Debug)]
pub struct HookBlock {
    pub body: TokenStream,
}

/// `describe_table "name" (a: Type, b: Type) [ (v1, v2), ... ] { body }`
#[derive(Debug)]
pub struct DescribeTableBlock {
    pub name: LitStr,
    pub focused: bool,
    pub pending: bool,
    pub params: Vec<TableParam>,
    pub entries: Vec<TableEntry>,
    pub body: TokenStream,
}

/// A single parameter declaration in a describe_table.
#[derive(Debug)]
pub struct TableParam {
    pub name: Ident,
    pub ty: Type,
}

/// A single entry (row) in a describe_table.
#[derive(Debug)]
pub struct TableEntry {
    pub label: Option<LitStr>,
    pub values: TokenStream,
}

/// `ordered "name" { ... }` or `ordered "name" continue_on_failure { ... }`
#[derive(Debug)]
pub struct OrderedBlock {
    pub name: LitStr,
    pub continue_on_failure: bool,
    pub items: Vec<DslItem>,
}

// ============================================================================
// Parsing
// ============================================================================

impl Parse for Suite {
    fn parse(input: ParseStream) -> Result<Self> {
        let items = parse_items(input)?;
        Ok(Suite { items })
    }
}

/// Parse a sequence of DSL items until the stream is exhausted.
fn parse_items(input: ParseStream) -> Result<Vec<DslItem>> {
    let mut items = Vec::new();
    while !input.is_empty() {
        items.push(input.parse::<DslItem>()?);
    }
    Ok(items)
}

impl Parse for DslItem {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident: Ident = input.parse()?;
        let name = ident.to_string();

        match name.as_str() {
            // Container blocks
            "describe" | "context" | "when" => {
                Ok(DslItem::Describe(parse_describe_block(input, false, false)?))
            }
            "fdescribe" | "fcontext" | "fwhen" => {
                Ok(DslItem::Describe(parse_describe_block(input, true, false)?))
            }
            "xdescribe" | "xcontext" | "xwhen" | "pdescribe" | "pcontext" | "pwhen" => {
                Ok(DslItem::Describe(parse_describe_block(input, false, true)?))
            }

            // Spec blocks
            "it" | "specify" => Ok(DslItem::It(parse_it_block(input, false, false)?)),
            "fit" | "fspecify" => Ok(DslItem::It(parse_it_block(input, true, false)?)),
            "xit" | "xspecify" | "pit" | "pspecify" => {
                Ok(DslItem::It(parse_it_block(input, false, true)?))
            }

            // Hooks
            "before_each" => Ok(DslItem::BeforeEach(parse_hook_block(input)?)),
            "just_before_each" => Ok(DslItem::JustBeforeEach(parse_hook_block(input)?)),
            "after_each" => Ok(DslItem::AfterEach(parse_hook_block(input)?)),
            "before_all" => Ok(DslItem::BeforeAll(parse_hook_block(input)?)),
            "after_all" => Ok(DslItem::AfterAll(parse_hook_block(input)?)),

            // Table-driven
            "describe_table" => Ok(DslItem::DescribeTable(parse_describe_table(
                input, false, false,
            )?)),
            "fdescribe_table" => Ok(DslItem::DescribeTable(parse_describe_table(
                input, true, false,
            )?)),
            "xdescribe_table" | "pdescribe_table" => Ok(DslItem::DescribeTable(
                parse_describe_table(input, false, true)?,
            )),

            // Ordered
            "ordered" => Ok(DslItem::Ordered(parse_ordered_block(input)?)),

            _ => Err(syn::Error::new(
                ident.span(),
                format!(
                    "unknown DSL keyword `{name}`. Expected one of: \
                     describe, context, when, it, specify, before_each, after_each, \
                     before_all, after_all, describe_table, ordered \
                     (with optional f/x/p prefix for focus/pending)"
                ),
            )),
        }
    }
}

// ============================================================================
// Block parsers
// ============================================================================

/// Parse: `"name" { items... }`
fn parse_describe_block(
    input: ParseStream,
    focused: bool,
    pending: bool,
) -> Result<DescribeBlock> {
    let name: LitStr = input.parse()?;
    let content;
    braced!(content in input);
    let items = parse_items(&content)?;
    Ok(DescribeBlock {
        name,
        focused,
        pending,
        items,
    })
}

/// Parse: `"name" [labels(...)] [retries(N)] { body }`
fn parse_it_block(input: ParseStream, focused: bool, pending: bool) -> Result<ItBlock> {
    let name: LitStr = input.parse()?;

    let mut labels = Vec::new();
    let mut retries = None;
    let mut must_pass_repeatedly = None;
    let mut timeout_ms = None;

    // Parse optional decorators before the body block
    while !input.peek(syn::token::Brace) {
        let decorator: Ident = input.parse()?;
        match decorator.to_string().as_str() {
            "labels" => {
                let content;
                parenthesized!(content in input);
                while !content.is_empty() {
                    labels.push(content.parse::<LitStr>()?);
                    if !content.is_empty() {
                        content.parse::<Token![,]>()?;
                    }
                }
            }
            "retries" => {
                let content;
                parenthesized!(content in input);
                let n: LitInt = content.parse()?;
                retries = Some(n.base10_parse::<u32>()?);
            }
            "must_pass_repeatedly" => {
                let content;
                parenthesized!(content in input);
                let n: LitInt = content.parse()?;
                must_pass_repeatedly = Some(n.base10_parse::<u32>()?);
            }
            "timeout" => {
                let content;
                parenthesized!(content in input);
                let n: LitInt = content.parse()?;
                timeout_ms = Some(n.base10_parse::<u64>()?);
            }
            other => {
                return Err(syn::Error::new(
                    decorator.span(),
                    format!(
                        "unknown decorator `{other}`. Expected `labels`, `retries`, \
                         `must_pass_repeatedly`, or `timeout`"
                    ),
                ));
            }
        }
    }

    let body_content;
    braced!(body_content in input);
    let body: TokenStream = body_content.parse()?;

    Ok(ItBlock {
        name,
        focused,
        pending,
        labels,
        retries,
        must_pass_repeatedly,
        timeout_ms,
        body,
    })
}

/// Parse: `{ body }`
fn parse_hook_block(input: ParseStream) -> Result<HookBlock> {
    let content;
    braced!(content in input);
    let body: TokenStream = content.parse()?;
    Ok(HookBlock { body })
}

/// Parse: `"name" (param: Type, ...) [ (val, ...), ... ] { body }`
fn parse_describe_table(
    input: ParseStream,
    focused: bool,
    pending: bool,
) -> Result<DescribeTableBlock> {
    let name: LitStr = input.parse()?;

    // Parse parameter declarations: (a: Type, b: Type, ...)
    let params_content;
    parenthesized!(params_content in input);
    let mut params = Vec::new();
    while !params_content.is_empty() {
        let param_name: Ident = params_content.parse()?;
        params_content.parse::<Token![:]>()?;
        let param_type: Type = params_content.parse()?;
        params.push(TableParam {
            name: param_name,
            ty: param_type,
        });
        if !params_content.is_empty() {
            params_content.parse::<Token![,]>()?;
        }
    }

    // Parse entries: [ (v1, v2), (v3, v4), ... ] or [ "label" (v1, v2), ... ]
    let entries_content;
    bracketed!(entries_content in input);
    let mut entries = Vec::new();
    while !entries_content.is_empty() {
        let label = if entries_content.peek(LitStr) {
            Some(entries_content.parse::<LitStr>()?)
        } else {
            None
        };
        let values_content;
        parenthesized!(values_content in entries_content);
        let values: TokenStream = values_content.parse()?;
        entries.push(TableEntry { label, values });
        if !entries_content.is_empty() {
            entries_content.parse::<Token![,]>()?;
        }
    }

    // Parse body: { ... }
    let body_content;
    braced!(body_content in input);
    let body: TokenStream = body_content.parse()?;

    Ok(DescribeTableBlock {
        name,
        focused,
        pending,
        params,
        entries,
        body,
    })
}

/// Parse: `"name" [continue_on_failure] { items... }`
fn parse_ordered_block(input: ParseStream) -> Result<OrderedBlock> {
    let name: LitStr = input.parse()?;

    // Check for optional `continue_on_failure` keyword before the brace
    let continue_on_failure = if input.peek(Ident) {
        let lookahead = input.fork();
        let ident: Ident = lookahead.parse()?;
        if ident == "continue_on_failure" {
            // Consume the ident from the real stream
            let _: Ident = input.parse()?;
            true
        } else {
            false
        }
    } else {
        false
    };

    let content;
    braced!(content in input);
    let items = parse_items(&content)?;
    Ok(OrderedBlock {
        name,
        continue_on_failure,
        items,
    })
}
