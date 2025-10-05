#[cfg(any(feature = "min_safety", debug_assertions))]
use std::sync::Arc;
use std::{marker::PhantomData, mem, ops::Deref};

/// A reference with lifetime `'a` that can be lifted to a reference with a `'static` lifetime ([`Scoped`]).
/// Runtime checks are used to ensure that no derived [`Scoped`] exists when this [`ScopeGuard`] is
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
///         let value = **lifted + 1.0;
///         assert_eq!(value, 2.0);
///         // `lifted` is dropped here
///     })
///     .await
///     .unwrap();
///    // `guard` is dropped here
/// }
/// ```
/// 
/// If a [`ScopeGuard`] is dropped while any derived [`Scoped`] exist, then it will abort the whole
/// program (instead of panic). This is because [`Scoped`] could exist on another thread and be unaffected
/// by the panic or the panic could be recovered from. This could lead to undefined behavior.
///
/// UNDEFINED BEHAVIOR: It may cause undefined behavior to forget this value (`std::mem::forget(guard)`) -
/// the `Drop` code must run to prevent undefined behavior. 
/// 
/// UNDEFINED BEHAVIOR: If the *default* `min_safety` flag is not enabled, in non-debug mode, 
/// this may cause undefined behavior if [`ScopedGuard`] is drop before all derived [`Scoped`] are dropped.
/// This is because there are no runtime safety checks in this scenario and the program will not abort.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeGuard<'a, T: 'static> {
    #[cfg(any(feature = "min_safety", debug_assertions))]
    data: Arc<&'static T>,
    #[cfg(not(any(feature = "min_safety", debug_assertions)))]
    data: &'static T,
    _scope: PhantomData<&'a ()>,
}

impl<'a, T: 'static> ScopeGuard<'a, T> {
    pub fn new(value: &'a T) -> Self {
        let value = unsafe { mem::transmute::<&'a T, &'static T>(value) };
        #[cfg(any(feature = "min_safety", debug_assertions))]
        let value = Arc::new(value);
        ScopeGuard {
            data: value,
            _scope: std::marker::PhantomData,
        }
    }

    /// Lifts this reference with lifetime `'a` into `'static` and relies on runtime
    /// checks to ensure safety.
    pub fn lift(&self) -> Scoped<T> {
        #[cfg(any(feature = "min_safety", debug_assertions))]
        return Scoped(self.data.clone());
        #[cfg(not(any(feature = "min_safety", debug_assertions)))]
        return Scoped(self.data);
    }
}

#[cfg(any(feature = "min_safety", debug_assertions))]
impl<'a, T> Deref for ScopeGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.data.as_ref()
    }
}

#[cfg(not(any(feature = "min_safety", debug_assertions)))]
impl<'a, T> Deref for ScopeGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a, T: 'static> Drop for ScopeGuard<'a, T> {
    fn drop(&mut self) {
        #[cfg(any(feature = "min_safety", debug_assertions))]
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

/// A reference derived from a [`ScopeGuard`]. The lifetime of the underlying
/// value has been lifted to `'static`. See `Scoped` for more info.
#[cfg(any(feature = "min_safety", debug_assertions))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scoped<T: 'static>(Arc<&'static T>);

/// A reference derived from a `Scoped`. The lifetime of the underlying
/// value has been lifted to `'static`. See `Scoped` for more info.
#[cfg(not(any(feature = "min_safety", debug_assertions)))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scoped<T: 'static>(&'static T);

#[cfg(any(feature = "min_safety", debug_assertions))]
impl<T: 'static> Deref for Scoped<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

#[cfg(not(any(feature = "min_safety", debug_assertions)))]
impl<T: 'static> Deref for Scoped<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

#[cfg(test)]
mod tests {
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
        use crate::{ScopeGuard, tests::NonCopy};

        #[test]
        fn dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            let result = std::panic::catch_unwind(|| {
                std::mem::drop(guard);
            });
            #[cfg(any(feature = "min_safety", debug_assertions))]
            assert!(
                result.is_err(),
                "expected panic when dropping ScopeGuard with an alive Scoped"
            );
            #[cfg(not(any(feature = "min_safety", debug_assertions)))]
            assert!(
                result.is_ok(),
                "This should show UB of disabling `min_safety` in release mode"
            );
        }

        #[test]
        fn valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            std::mem::drop(lifted);
            std::mem::drop(guard);
        }

        #[tokio::test]
        async fn async_dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            tokio::spawn(async move {
                lifted.access_value();
            });
            let result = std::panic::catch_unwind(|| {
                std::mem::drop(guard);
            });
            #[cfg(any(feature = "min_safety", debug_assertions))]
            assert!(
                result.is_err(),
                "expected panic when dropping ScopeGuard with live Scoped in the task"
            );
            #[cfg(not(any(feature = "min_safety", debug_assertions)))]
            assert!(
                result.is_ok(),
                "This should show UB of disabling `min_safety` in release mode"
            );
        }

        #[tokio::test]
        async fn async_valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            tokio::spawn(async move {
                lifted.access_value();
            })
            .await
            .unwrap();
            std::mem::drop(guard);
        }
    }

    #[cfg(test)]
    mod ub_tests {
        use crate::{ScopeGuard, tests::NonCopy};

        #[test]
        fn undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = ScopeGuard::new(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            std::mem::forget(guard);
            std::mem::drop(concrete_value);
            let result = std::panic::catch_unwind(|| {
                // The assert here should fail (Showing UB) in a testable way
                lifted.access_value();
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
            lifted.access_value();
            let fut = tokio::spawn(async move {
                let result = std::panic::catch_unwind(|| {
                    // The assert here should fail (Showing UB) in a testable way
                    lifted.access_value();
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
