# rsspec

A Ginkgo/RSpec-inspired BDD testing framework for Rust.

Write expressive, structured tests using a familiar BDD syntax with `describe`, `context`, `it`, lifecycle hooks, table-driven tests, and more.

## Quick Start

Add to your `Cargo.toml`:

```toml
[dev-dependencies]
rsspec = "0.1"
```

Write your first spec:

```rust
rsspec::suite! {
    describe "Calculator" {
        before_each {
            let a = 2;
            let b = 3;
        }

        it "adds two numbers" {
            assert_eq!(a + b, 5);
        }

        context "with negative numbers" {
            it "handles negatives" {
                assert_eq!(-1 + b, 2);
            }
        }
    }
}
```

Run with `cargo test`.

## DSL Reference

### Containers

Nest your specs with `describe`, `context`, or `when` — they are aliases:

```rust
describe "outer" {
    context "inner" {
        when "something happens" {
            it "works" { assert!(true); }
        }
    }
}
```

**Focus** — only run focused containers and their children:

```rust
fdescribe "only this runs" {
    it "focused by inheritance" { /* runs */ }
}
```

Variants: `fdescribe`, `fcontext`, `fwhen`

**Pending** — skip entire containers:

```rust
xdescribe "not yet implemented" {
    it "skipped" { /* never runs */ }
}
```

Variants: `xdescribe`, `xcontext`, `xwhen`, `pdescribe`, `pcontext`, `pwhen`

### Specs

Individual test cases use `it` or `specify`:

```rust
it "does something" {
    assert_eq!(1 + 1, 2);
}

specify "also works" {
    assert!(true);
}
```

**Nameless specs** — auto-named `spec_1`, `spec_2`, etc.:

```rust
subject { 2 + 3 }

it { assert_eq!(subject, 5); }
it { assert!(subject > 0); }
```

**Focus**: `fit`, `fspecify` — **Pending**: `xit`, `xspecify`, `pit`, `pspecify`

### Lifecycle Hooks

| Hook | Runs | Scope |
| --- | --- | --- |
| `before_each` | Before every `it` | Inherited by nested scopes |
| `just_before_each` | After all `before_each`, right before the body | Inherited |
| `after_each` | After every `it` (even on panic) | Inherited |
| `before_all` | Once before all tests in scope | Module-level |
| `after_all` | Once after all tests in scope | Module-level |

```rust
describe "hooks" {
    before_all {
        // expensive setup — runs once
        INIT.call_once(|| setup_db());
    }

    before_each {
        let conn = get_connection();
    }

    just_before_each {
        conn.begin_transaction();
    }

    after_each {
        conn.rollback();
    }

    after_all {
        teardown_db();
    }

    it "uses the connection" {
        assert!(conn.is_active());
    }
}
```

Execution order:

```
before_all (once) -> before_each -> just_before_each -> subject -> body -> after_each -> after_all (once)
```

### Subject

Define the "act" step once, then write concise assertions (RSpec-style):

```rust
describe "Calculator" {
    before_each {
        let a = 2;
        let b = 3;
    }

    subject { a + b }

    it "returns the sum" {
        assert_eq!(subject, 5);
    }

    it { assert!(subject > 0); }

    context "with multiplication" {
        subject { a * b }  // overrides parent

        it "returns the product" {
            assert_eq!(subject, 6);
        }
    }
}
```

Nested `subject` blocks override the parent (last one wins).

### Decorators

Attach metadata to `it` blocks:

```rust
it "tagged test" labels("integration", "slow") {
    // filtered via RSSPEC_LABEL_FILTER env var
}

it "flaky test" retries(3) {
    // retries up to 3 additional times on failure
}

it "must be stable" must_pass_repeatedly(5) {
    // must pass 5 consecutive times
}

it "fast test" timeout(1000) {
    // fails if not complete within 1000ms
}
```

Decorators can be combined:

```rust
it "everything" labels("smoke") retries(2) timeout(5000) {
    // ...
}
```

### Table-Driven Tests

Parameterized specs with `describe_table`:

```rust
describe_table "arithmetic" (a: i32, b: i32, expected: i32) [
    "addition"      (2, 3, 5),
    "large numbers" (100, 200, 300),
    "negative"      (-1, 1, 0),
] {
    assert_eq!(a + b, expected);
}
```

Each entry becomes a separate test. Optional labels for entry names; without a label, entries are named `case_1`, `case_2`, etc.

Focus/pending variants: `fdescribe_table`, `xdescribe_table`, `pdescribe_table`

### Ordered Tests

Sequential, fail-fast test workflows:

```rust
ordered "user registration" {
    it "step 1: create account" {
        create_user("alice");
    }

    it "step 2: verify email" {
        verify_email("alice");
    }
}
```

All steps run in a single test function. If any step fails, subsequent steps are skipped.

Use `continue_on_failure` to run all steps regardless:

```rust
ordered "resilient workflow" continue_on_failure {
    it "step 1" { /* ... */ }
    it "step 2" { /* runs even if step 1 fails */ }
}
```

## BDD Runner

For Ginkgo-style colored tree output, use `bdd!` with a custom test harness:

In `Cargo.toml`:

```toml
[[test]]
name = "bdd_tests"
harness = false
```

In `tests/bdd_tests.rs`:

```rust
rsspec::bdd! {
    describe "Calculator" {
        it "adds" { assert_eq!(2 + 3, 5); }
        it "multiplies" { assert_eq!(3 * 4, 12); }
        xit "divides by zero" { /* pending */ }
    }
}
```

Output:

```
Calculator
    ✓ adds
    ✓ multiplies
    - divides by zero

PASS
2 passed, 1 pending (0.001s)
```

### Multi-Suite

Combine multiple suites with `bdd_suite!`:

```rust
fn main() {
    let auth = rsspec::bdd_suite! {
        describe "Auth" { it "logs in" { assert!(true); } }
    };
    let api = rsspec::bdd_suite! {
        describe "API" { it "responds" { assert!(true); } }
    };

    let suites = vec![
        rsspec::runner::Suite::new("auth", file!(), auth),
        rsspec::runner::Suite::new("api", file!(), api),
    ];

    let config = rsspec::runner::RunConfig::from_args();
    let result = rsspec::runner::run_suites(&suites, &config);
    if result.failed > 0 {
        std::process::exit(1);
    }
}
```

The BDD runner supports `--list`, `--include-ignored`, and name-based filtering via command-line args.

## Runtime Helpers

### defer_cleanup

Register LIFO cleanup functions (like Go's `defer`):

```rust
it "creates temp resources" {
    let file = create_temp_file();
    rsspec::defer_cleanup(move || {
        std::fs::remove_file(file).ok();
    });
    // cleanup runs after this test, even on panic
}
```

### by

Document steps within a test:

```rust
it "complex workflow" {
    rsspec::by("setting up prerequisites");
    let user = create_user();

    rsspec::by("performing the action");
    user.activate();

    rsspec::by("verifying the result");
    assert!(user.is_active());
}
```

### skip!

Skip a test at runtime:

```rust
it "requires a database" {
    if !db_available() {
        rsspec::skip!("database not available");
    }
    // ... test body ...
}
```

## Environment Variables

| Variable | Description |
| --- | --- |
| `RSSPEC_LABEL_FILTER` | Filter tests by labels. `integration` = match label, `!slow` = exclude, `a,b` = OR, `a+b` = AND |
| `RSSPEC_FAIL_ON_FOCUS` | Set to `1` or `true` to fail when focused tests exist (CI safety) |
| `NO_COLOR` | Disable colored output in the BDD runner |

## googletest Integration

Enable the `googletest` feature for composable matchers:

```toml
[dev-dependencies]
rsspec = { version = "0.1", features = ["googletest"] }
```

```rust
use rsspec::matchers::*;

rsspec::suite! {
    describe "with matchers" {
        subject { vec![1, 2, 3] }

        it "has elements" {
            assert_that!(subject, not(empty()));
        }

        it "contains expected values" {
            assert_that!(subject, contains(eq(2)));
        }
    }
}
```

The `rsspec::matchers` module re-exports `googletest::prelude::*`.

## License

Licensed under either of [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
or [MIT license](http://opensource.org/licenses/MIT) at your option.
