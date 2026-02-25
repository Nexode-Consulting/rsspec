# rsspec

A Ginkgo/RSpec-inspired BDD testing framework for Rust with a closure-based API.

Write expressive, structured tests using `describe`, `context`, `it`, lifecycle hooks, table-driven tests, and more — all in idiomatic Rust.

## Quick Start

Add to your `Cargo.toml`:

```toml
[dev-dependencies]
rsspec = "0.1"

[[test]]
name = "my_tests"
harness = false
```

Write your first spec in `tests/my_tests.rs`:

```rust
fn main() {
    rsspec::run(|ctx| {
        ctx.describe("Calculator", |ctx| {
            ctx.it("adds two numbers", || {
                assert_eq!(2 + 3, 5);
            });

            ctx.context("with negative numbers", |ctx| {
                ctx.it("handles negatives", || {
                    assert_eq!(-1 + 1, 0);
                });
            });
        });
    });
}
```

Run with `cargo test`:

```
Calculator
  ✓ adds two numbers
  with negative numbers
    ✓ handles negatives

PASS
2 passed (0.001s)
```

## API Reference

### Containers

Nest your specs with `describe`, `context`, or `when` — they are aliases:

```rust
ctx.describe("outer", |ctx| {
    ctx.context("inner", |ctx| {
        ctx.when("something happens", |ctx| {
            ctx.it("works", || { assert!(true); });
        });
    });
});
```

**Focus** — only run focused containers and their children:

```rust
ctx.fdescribe("only this runs", |ctx| {
    ctx.it("focused by inheritance", || { /* runs */ });
});
```

Variants: `fdescribe`, `fcontext`, `fwhen`

**Pending** — skip entire containers:

```rust
ctx.xdescribe("not yet implemented", |ctx| {
    ctx.it("skipped", || { /* never runs */ });
});
```

Variants: `xdescribe`, `xcontext`, `xwhen`

### Specs

Individual test cases use `it` or `specify`:

```rust
ctx.it("does something", || {
    assert_eq!(1 + 1, 2);
});

ctx.specify("also works", || {
    assert!(true);
});
```

**Focus**: `fit`, `fspecify` — **Pending**: `xit`, `xspecify`

### Lifecycle Hooks

| Hook | Runs | Scope |
| --- | --- | --- |
| `before_each` | Before every `it` | Inherited by nested scopes |
| `just_before_each` | After all `before_each`, right before the body | Inherited |
| `after_each` | After every `it` (even on panic) | Inherited |
| `before_all` | Once before all tests in scope | Per describe |
| `after_all` | Once after all tests in scope | Per describe |

```rust
ctx.describe("database tests", |ctx| {
    ctx.before_all(|| {
        // expensive setup — runs once
    });

    ctx.before_each(|| {
        // runs before every test
    });

    ctx.after_each(|| {
        // runs after every test (even on panic)
    });

    ctx.after_all(|| {
        // cleanup — runs once after all tests
    });

    ctx.it("uses the database", || {
        assert!(true);
    });
});
```

Execution order per test:

```
before_all (once) -> before_each -> just_before_each -> body -> after_each -> after_all (once)
```

### Decorators

Attach metadata to `it` blocks using the fluent builder API:

```rust
ctx.it("tagged test", || { /* ... */ })
    .labels(&["integration", "slow"]);

ctx.it("flaky test", || { /* ... */ })
    .retries(3);

ctx.it("must be stable", || { /* ... */ })
    .must_pass_repeatedly(5);

ctx.it("fast test", || { /* ... */ })
    .timeout(1000);
```

Decorators can be combined:

```rust
ctx.it("everything", || { /* ... */ })
    .labels(&["smoke"])
    .retries(2)
    .timeout(5000);
```

### Describe-Level Labels

Set labels on a describe scope — they propagate to all child tests:

```rust
ctx.describe("integration tests", |ctx| {
    ctx.labels(&["integration"]);

    ctx.it("inherits labels", || { /* ... */ });
});
```

### Table-Driven Tests

Parameterized specs with `describe_table`:

```rust
ctx.describe_table("arithmetic")
    .case("addition", (2i32, 3i32, 5i32))
    .case("large numbers", (100, 200, 300))
    .case("negative", (-1, 1, 0))
    .run(|(a, b, expected): &(i32, i32, i32)| {
        assert_eq!(a + b, *expected);
    });
```

Each case becomes a separate test.

### Ordered Tests

Sequential, fail-fast test workflows:

```rust
ctx.ordered("user registration", |oct| {
    oct.step("create account", || {
        // ...
    });
    oct.step("verify email", || {
        // ...
    });
});
```

All steps run in sequence. If any step fails, subsequent steps are skipped.

Use `ordered_continue_on_failure` to run all steps regardless:

```rust
ctx.ordered_continue_on_failure("resilient workflow", |oct| {
    oct.step("step 1", || { /* ... */ });
    oct.step("step 2", || { /* runs even if step 1 fails */ });
});
```

## Runtime Helpers

### defer_cleanup

Register LIFO cleanup functions (like Go's `defer`):

```rust
ctx.it("creates temp resources", || {
    rsspec::defer_cleanup(|| {
        // cleanup runs after this test, even on panic
    });
});
```

### by

Document steps within a test:

```rust
ctx.it("complex workflow", || {
    rsspec::by("setting up prerequisites");
    // ...
    rsspec::by("performing the action");
    // ...
    rsspec::by("verifying the result");
    assert!(true);
});
```

### skip!

Skip a test at runtime:

```rust
ctx.it("requires a database", || {
    if !db_available() {
        rsspec::skip!("database not available");
    }
    // ... test body ...
});
```

## Environment Variables

| Variable | Description |
| --- | --- |
| `RSSPEC_LABEL_FILTER` | Filter tests by labels. `integration` = match label, `!slow` = exclude, `a,b` = OR, `a+b` = AND |
| `RSSPEC_FAIL_ON_FOCUS` | Set to `1` or `true` to fail when focused tests exist (CI safety) |
| `NO_COLOR` | Disable colored output |

## googletest Integration

Enable the `googletest` feature for composable matchers:

```toml
[dev-dependencies]
rsspec = { version = "0.1", features = ["googletest"] }
```

```rust
use rsspec::matchers::*;

fn main() {
    rsspec::run(|ctx| {
        ctx.describe("with matchers", |ctx| {
            ctx.it("has elements", || {
                let v = vec![1, 2, 3];
                assert_that!(v, not(empty()));
            });
        });
    });
}
```

## License

Licensed under either of [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
or [MIT license](http://opensource.org/licenses/MIT) at your option.
