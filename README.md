# scoped_static

[<img alt="github" src="https://img.shields.io/badge/github-mcmah309/scoped_static-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20">](https://github.com/mcmah309/scoped_static)
[<img alt="crates.io" src="https://img.shields.io/crates/v/scoped_static.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/scoped_static)
[<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-scoped_static-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" height="20">](https://docs.rs/scoped_static)
[<img alt="test status" src="https://img.shields.io/github/actions/workflow/status/mcmah309/scoped_static/test.yml?branch=master&style=for-the-badge" height="20">](https://github.com/mcmah309/scoped_static/actions?query=branch%3Amaster)

> **Lift references into `'static` safely — at runtime.**

`scoped_static` allows temporarily extending a reference’s lifetime to `'static` using runtime safety checks.
This enables you to safely spawn asynchronous tasks, threads, or other `'static` contexts without running into borrow checker limitations — while still avoiding undefined behavior.

---

## Motivation

Rust’s lifetime system ensures safety at compile time, but sometimes you need to move a non-`'static` reference into an async task or thread:

```rust,ignore
#[tokio::main]
async fn main() {
    let concrete_value = Box::new(1.0);
    let ref_value = &concrete_value; // This is does not live long enough (not 'static)
    tokio::spawn(async move {
        let value = **ref_value + 1.0;
        assert_eq!(value, 2.0);
    })
    .await
    .unwrap();
}
```

This fails because the reference to `ref_value` isn’t `'static`.

`scoped_static` solves this by allowing you to **lift** a reference to `'static` under the protection of a **scope guard** that enforces correct drop order at runtime.

---

## Example

```rust
use scoped_static::{ScopeGuard, Scoped};

#[tokio::main]
async fn main() {
    let value = Box::new(1.0);
    let ref_value = &value;
    // `guard` ensures no derived "lifted" values exist when dropped
    let guard = ScopeGuard::new(ref_value);
    // `lifted` holds a `'static` reference to `'ref_value`
    let lifted: Scoped<Box<f64>> = guard.lift();
    tokio::spawn(async move {
        // lifted moved here
        let value = **lifted + 1.0;
        assert_eq!(value, 2.0);
        // lifted dropped
    })
    .await
    .unwrap();
   // guard dropped
}
```

See [ScopeGuard](https://docs.rs/scoped_static/latest/scoped_static/struct.ScopeGuard.html) and 
[UncheckedScopeGuard](https://docs.rs/scoped_static/latest/scoped_static/struct.UncheckedScopeGuard.html) for more info.