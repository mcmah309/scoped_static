# scoped_static

> **Lift references into `'static` safely — at runtime.**

`scoped_static` allows temporarily extending a reference’s lifetime to `'static` using runtime safety checks.
This enables you to safely spawn asynchronous tasks, threads, or other `'static` contexts without running into borrow checker limitations — while still avoiding undefined behavior.

---

## ✨ Motivation

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

## 🚀 Example

```rust
use scoped_static::ScopeGuard;

#[tokio::main]
async fn main() {
    let value = Box::new(1.0);
    let ref_value = &value;
    let guard = ScopeGuard::new(ref_value);
    let lifted = guard.lift();
    tokio::spawn(async move {
        // `lifted` has `'static` lifetime, so it's valid here
        let value = **lifted + 1.0;
        assert_eq!(value, 2.0);
    })
    .await
    .unwrap();
}
```
