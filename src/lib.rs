#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

use std::sync::Arc;
use std::{marker::PhantomData, mem};

/// A reference with lifetime `'a` that can be lifted to a reference with a `'static` lifetime ([`ScopedRef`]).
/// Runtime checks are used to ensure that no derived [`ScopedRef`] exists when this [`ScopeGuard`] is
/// dropped.
///
/// ```rust
/// use scoped_static::ScopeGuard;
///
/// #[tokio::main]
/// async fn main() {
///     let concrete_value = Box::new(1.0);
///     let ref_value = &concrete_value;
///     let guard = ScopeGuard::new(ref_value);
///     let lifted = guard.lift();
///     tokio::spawn(async move {
///         // Lifted is 'static so it can be moved into this closure that needs 'static
///         let value = **unsafe { lifted.deref() } + 1.0;
///         assert_eq!(value, 2.0);
///         // `lifted` is dropped here
///     })
///     .await
///     .unwrap();
///    // `guard` is dropped here
/// }
/// ```
///
/// If a [`ScopeGuard`] is dropped while any derived [`ScopedRef`] exist, then it will abort the whole
/// program (instead of panic). This is because [`ScopedRef`] could exist on another thread and be unaffected
/// by the panic or the panic could be recovered from. This could lead to undefined behavior.
///
/// UNDEFINED BEHAVIOR: It may cause undefined behavior to forget this value (`std::mem::forget(guard)`) -
/// the `Drop` code must run to prevent undefined behavior.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeGuard<'a, T: 'static> {
    data: Arc<&'static T>,
    _scope: PhantomData<&'a ()>,
}

impl<'a, T: 'static> ScopeGuard<'a, T> {
    pub fn new(value: &'a T) -> Self {
        let value = unsafe { mem::transmute::<&'a T, &'static T>(value) };
        let value = Arc::new(value);
        ScopeGuard {
            data: value,
            _scope: std::marker::PhantomData,
        }
    }

    /// Lifts this reference with lifetime `'a` into `'static` and relies on runtime
    /// checks to ensure safety.
    pub fn lift(&self) -> ScopedRef<T> {
        return ScopedRef(self.data.clone());
    }
}

impl<'a, T: 'static> Drop for ScopeGuard<'a, T> {
    fn drop(&mut self) {
        if std::sync::Arc::strong_count(&self.data) != 1 {
            const ROOT_MSG: &str = "Fatal error: Scope dropped while Lifted references still exist. \
                This would cause undefined behavior. Aborting.\n";
            // We don't panic since panics can be recovered and panics also only effect a single thread.
            // While the value could have been sent to a different thread.
            #[cfg(not(test))]
            {
                let bt = std::backtrace::Backtrace::capture();
                let msg = match bt.status() {
                    std::backtrace::BacktraceStatus::Unsupported => ROOT_MSG.to_owned(),
                    std::backtrace::BacktraceStatus::Disabled => format!(
                        "{ROOT_MSG}\n(Hint: re-run with `RUST_BACKTRACE=1` to see a backtrace.)\n"
                    ),
                    std::backtrace::BacktraceStatus::Captured => {
                        format!("{ROOT_MSG}\nBacktrace:\n{bt}\n")
                    }
                    _ => ROOT_MSG.to_owned(),
                };
                use std::io::Write;
                let _ = std::io::stderr().write_all(msg.as_bytes());
                let _ = std::io::stderr().flush();
                std::process::abort();
            }
            #[cfg(test)]
            {
                panic!("{}", ROOT_MSG);
            }
        }
    }
}

/// A reference derived from a [`ScopeGuard`]. The lifetime of the underlying
/// value has been lifted to `'static`. See [`ScopeGuard`] for more info.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopedRef<T: 'static>(Arc<&'static T>);

impl<T: 'static> ScopedRef<T> {
    /// Gets the scoped reference
    ///
    /// UNDEFINED BEHAVIOR: This is marked as "unsafe" since it is undefined behavior to leak the derived `ScopeGuard` and
    /// this can be accomplished in safe Rust - e.g. `mem::forget` or `Rc` cycles.
    /// For all cases except this edge case, one case safely call this function.
    pub unsafe fn deref(&self) -> &T {
        self.0.as_ref()
    }
}

#[cfg(test)]
mod checked_tests {
    struct NonCopy(f32);

    impl NonCopy {
        pub fn new() -> Self {
            NonCopy(1.0)
        }
        pub fn access_value(&self) {
            assert_eq!(self.0, 1.0, "If these values are not equal it signals UB");
        }
    }

    #[cfg(test)]
    mod normal_tests {
        use crate::{ScopeGuard, checked_tests::NonCopy};

        #[test]
        fn dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            let result = std::panic::catch_unwind(|| {
                std::mem::drop(guard);
            });
            assert!(
                result.is_err(),
                "expected panic when dropping ScopeGuard with an alive Scoped"
            );
        }

        #[test]
        fn valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            std::mem::drop(lifted);
            std::mem::drop(guard);
        }

        #[tokio::test]
        async fn async_dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            tokio::spawn(async move {
                unsafe { lifted.deref() }.access_value();
            });
            let result = std::panic::catch_unwind(|| {
                std::mem::drop(guard);
            });
            assert!(
                result.is_err(),
                "expected panic when dropping ScopeGuard with live Scoped in the task"
            );
        }

        #[tokio::test]
        async fn async_valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            tokio::spawn(async move {
                unsafe { lifted.deref() }.access_value();
            })
            .await
            .unwrap();
            std::mem::drop(guard);
        }
    }

    #[cfg(test)]
    mod ub_tests {
        use crate::{ScopeGuard, checked_tests::NonCopy};

        #[test]
        fn undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            std::mem::forget(guard);
            std::mem::drop(concrete_value);
            let result = std::panic::catch_unwind(|| {
                // The assert here should fail (Showing UB) in a testable way
                unsafe { lifted.deref() }.access_value();
            });
            assert!(
                result.is_err(),
                "Forgetting the ScopeGuard, dropping the underlying, then accessing an Scoped value should be UB"
            );
        }

        #[tokio::test]
        async fn async_undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            let fut = tokio::spawn(async move {
                let result = std::panic::catch_unwind(|| {
                    // The assert here should fail (Showing UB) in a testable way
                    unsafe { lifted.deref() }.access_value();
                });
                result
            });
            std::mem::forget(guard);
            std::mem::drop(concrete_value);
            let result = fut.await.unwrap();
            assert!(
                result.is_err(),
                "Forgetting the ScopeGuard, dropping the underlying, then accessing an Scoped value should be UB"
            );
        }
    }
}

//************************************************************************//

/// Works like a [`ScopeGuard`], except more performant in release-like modes, since no checks are used.
/// But at the cost of risking additional UB if not used correctly. See UB notes below.
/// Only consider using over [`ScopeGuard`] if one is certain this is dropped after all derived
/// scoped values.
///
/// A reference with lifetime `'a` that can be lifted to a reference with a `'static` lifetime ([`UncheckedScopedRef`]).
/// Runtime checks are used to ensure that no derived [`UncheckedScopedRef`] exists when this [`UncheckedScopeGuard`] is
/// dropped.
///
/// ```rust
/// use scoped_static::UncheckedScopeGuard;
///
/// #[tokio::main]
/// async fn main() {
///     let concrete_value = Box::new(1.0);
///     let ref_value = &concrete_value;
///     let guard = UncheckedScopeGuard::new(ref_value);
///     let lifted = guard.lift();
///     tokio::spawn(async move {
///         // Lifted is 'static so it can be moved into this closure that needs 'static
///         let value = **unsafe { lifted.deref() } + 1.0;
///         assert_eq!(value, 2.0);
///         // `lifted` is dropped here
///     })
///     .await
///     .unwrap();
///    // `guard` is dropped here
/// }
/// ```
///
/// If a [`UncheckedScopeGuard`] is dropped while any derived [`UncheckedScopedRef`] exist, then it will abort the whole
/// program (instead of panic). This is because [`UncheckedScopedRef`] could exist on another thread and be unaffected
/// by the panic or the panic could be recovered from. This could lead to undefined behavior.
///
/// UNDEFINED BEHAVIOR: It may cause undefined behavior to forget this value (`std::mem::forget(guard)`) -
/// the `Drop` code must run to prevent undefined behavior.
///
/// UNDEFINED BEHAVIOR: If the`checked` feature flag is not enabled, in non-debug mode,
/// this may cause undefined behavior if [`UncheckedScopeGuard`] is drop before all derived [`UncheckedScopedRef`] are dropped.
/// This is because there are no runtime safety checks in this scenario and the program will not abort.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UncheckedScopeGuard<'a, T: 'static> {
    #[cfg(any(feature = "checked", debug_assertions))]
    data: Arc<&'static T>,
    #[cfg(not(any(feature = "checked", debug_assertions)))]
    data: &'static T,
    _scope: PhantomData<&'a ()>,
}

impl<'a, T: 'static> UncheckedScopeGuard<'a, T> {
    pub fn new(value: &'a T) -> Self {
        let value = unsafe { mem::transmute::<&'a T, &'static T>(value) };
        #[cfg(any(feature = "checked", debug_assertions))]
        let value = Arc::new(value);
        UncheckedScopeGuard {
            data: value,
            _scope: std::marker::PhantomData,
        }
    }

    /// Lifts this reference with lifetime `'a` into `'static` and relies on runtime
    /// checks to ensure safety.
    pub fn lift(&self) -> UncheckedScopedRef<T> {
        #[cfg(any(feature = "checked", debug_assertions))]
        return UncheckedScopedRef(self.data.clone());
        #[cfg(not(any(feature = "checked", debug_assertions)))]
        return UncheckedScopedRef(self.data);
    }
}

impl<'a, T: 'static> Drop for UncheckedScopeGuard<'a, T> {
    fn drop(&mut self) {
        #[cfg(any(feature = "checked", debug_assertions))]
        {
            if std::sync::Arc::strong_count(&self.data) != 1 {
                const ROOT_MSG: &str = "Fatal error: Scope dropped while Lifted references still exist. \
                This would cause undefined behavior. Aborting.\n";
                // We don't panic since panics can be recovered and panics also only effect a single thread.
                // While the value could have been sent to a different thread.
                #[cfg(not(test))]
                {
                    let bt = std::backtrace::Backtrace::capture();
                    let msg = match bt.status() {
                        std::backtrace::BacktraceStatus::Unsupported => ROOT_MSG.to_owned(),
                        std::backtrace::BacktraceStatus::Disabled => format!(
                            "{ROOT_MSG}\n(Hint: re-run with `RUST_BACKTRACE=1` to see a backtrace.)\n"
                        ),
                        std::backtrace::BacktraceStatus::Captured => {
                            format!("{ROOT_MSG}\nBacktrace:\n{bt}\n")
                        }
                        _ => ROOT_MSG.to_owned(),
                    };
                    use std::io::Write;
                    let _ = std::io::stderr().write_all(msg.as_bytes());
                    let _ = std::io::stderr().flush();
                    std::process::abort();
                }
                #[cfg(test)]
                {
                    panic!("{}", ROOT_MSG);
                }
            }
        }
    }
}

/// A reference derived from a [`UncheckedScopeGuard`]. The lifetime of the underlying
/// value has been lifted to `'static`. See [`UncheckedScopeGuard`] for more info.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UncheckedScopedRef<T: 'static>(
    #[cfg(any(feature = "checked", debug_assertions))] Arc<&'static T>,
    #[cfg(not(any(feature = "checked", debug_assertions)))] &'static T,
);

impl<T: 'static> UncheckedScopedRef<T>  {
    #[cfg(any(feature = "checked", debug_assertions))]
    pub unsafe fn deref(&self) -> &T {
        self.0.as_ref()
    }

    #[cfg(not(any(feature = "checked", debug_assertions)))]
    pub unsafe fn deref(&self) -> &T {
        self.0
    }
}

#[cfg(test)]
mod unchecked_tests {
    struct NonCopy(f32);

    impl NonCopy {
        pub fn new() -> Self {
            NonCopy(1.0)
        }
        pub fn access_value(&self) {
            assert_eq!(self.0, 1.0, "If these values are not equal it signals UB");
        }
    }

    #[cfg(test)]
    mod normal_tests {
        use crate::{UncheckedScopeGuard, unchecked_tests::NonCopy};

        #[test]
        fn dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = UncheckedScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            let result = std::panic::catch_unwind(|| {
                std::mem::drop(guard);
            });
            #[cfg(any(feature = "checked", debug_assertions))]
            assert!(
                result.is_err(),
                "expected panic when dropping ScopeGuard with an alive Scoped"
            );
            #[cfg(not(any(feature = "checked", debug_assertions)))]
            assert!(
                result.is_ok(),
                "This should show UB of disabling `checked` in release mode"
            );
        }

        #[test]
        fn valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = UncheckedScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            std::mem::drop(lifted);
            std::mem::drop(guard);
        }

        #[tokio::test]
        async fn async_dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = UncheckedScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            tokio::spawn(async move {
                unsafe { lifted.deref() }.access_value();
            });
            let result = std::panic::catch_unwind(|| {
                std::mem::drop(guard);
            });
            #[cfg(any(feature = "checked", debug_assertions))]
            assert!(
                result.is_err(),
                "expected panic when dropping ScopeGuard with live Scoped in the task"
            );
            #[cfg(not(any(feature = "checked", debug_assertions)))]
            assert!(
                result.is_ok(),
                "This should show UB of disabling `checked` in release mode"
            );
        }

        #[tokio::test]
        async fn async_valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = UncheckedScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            tokio::spawn(async move {
                unsafe { lifted.deref() }.access_value();
            })
            .await
            .unwrap();
            std::mem::drop(guard);
        }
    }

    #[cfg(test)]
    mod ub_tests {
        use crate::{UncheckedScopeGuard, unchecked_tests::NonCopy};

        #[test]
        fn undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = UncheckedScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            std::mem::forget(guard);
            std::mem::drop(concrete_value);
            let result = std::panic::catch_unwind(|| {
                // The assert here should fail (Showing UB) in a testable way
                unsafe { lifted.deref() }.access_value();
            });
            assert!(
                result.is_err(),
                "Forgetting the ScopeGuard, dropping the underlying, then accessing an Scoped value should be UB"
            );
        }

        #[tokio::test]
        async fn async_undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = UncheckedScopeGuard::new(ref_value);
            let lifted = guard.lift();
            unsafe { lifted.deref() }.access_value();
            let fut = tokio::spawn(async move {
                let result = std::panic::catch_unwind(|| {
                    // The assert here should fail (Showing UB) in a testable way
                    unsafe { lifted.deref() }.access_value();
                });
                result
            });
            std::mem::forget(guard);
            std::mem::drop(concrete_value);
            let result = fut.await.unwrap();
            assert!(
                result.is_err(),
                "Forgetting the ScopeGuard, dropping the underlying, then accessing an Scoped value should be UB"
            );
        }
    }
}
