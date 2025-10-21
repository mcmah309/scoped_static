use std::sync::Arc;
use std::{marker::PhantomData, mem, ops::Deref};

/// A safe way to create a [`ScopedGuard`].
/// ```rust
/// use scoped_static::scoped;
///
/// #[tokio::main]
/// async fn main() {
///     let concrete_value = Box::new(1.0);
///     let ref_value = &concrete_value;
///     let guard = scoped!(ref_value);
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
#[macro_export]
macro_rules! scoped {
    ($ref_value:expr) => {
        &mut unsafe { $crate::ScopedGuard::new($ref_value) }
    };
}

/// A reference with lifetime `'a` that can be lifted to a reference with a `'static` lifetime ([`Scoped`]).
/// Runtime checks are used to ensure that no derived [`Scoped`] exists when this [`ScopedGuard`] is
/// dropped.
///
/// ```rust
/// use scoped_static::ScopedGuard;
///
/// #[tokio::main]
/// async fn main() {
///     let concrete_value = Box::new(1.0);
///     let ref_value = &concrete_value;
///     let guard = unsafe { ScopedGuard::new(ref_value) };
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
/// If a [`ScopedGuard`] is dropped while any derived [`Scoped`] exist, then it will abort the whole
/// program (instead of panic). This is because [`Scoped`] could exist on another thread and be unaffected
/// by the panic or the panic could be recovered from. This could lead to undefined behavior.
///
/// Unlike [`crate::ScopedPinGuard`] this uses boxing internally. Thus it is slightly less efficient, but it can be moved.
///
/// UNDEFINED BEHAVIOR: It may cause undefined behavior to leak/forget this value. Since
/// the `Drop` code must run to prevent undefined behavior. 
/// e.g. [`std::mem::forget`], [`std::mem::ManuallyDrop`], or Rc cycles, etc.
///
/// See [`scoped`] macro for a safe way to create.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopedGuard<'a, T: 'static> {
    data: Arc<&'static T>,
    _scope: PhantomData<&'a ()>,
}

impl<'a, T: 'static> ScopedGuard<'a, T> {
    /// Creates a new [`ScopedGuard`]. See [`scoped`] for a safe way to create.
    pub unsafe fn new(value: &'a T) -> Self {
        let value = unsafe { mem::transmute::<&'a T, &'static T>(value) };
        let value = Arc::new(value);
        ScopedGuard {
            data: value,
            _scope: std::marker::PhantomData,
        }
    }

    /// Lifts this reference with lifetime `'a` into `'static` and relies on runtime
    /// checks to ensure safety.
    pub fn lift(&self) -> Scoped<T> {
        return Scoped(self.data.clone());
    }
}

impl<'a, T> Deref for ScopedGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.data.as_ref()
    }
}

impl<'a, T: 'static> Drop for ScopedGuard<'a, T> {
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

/// A reference derived from a [`ScopedGuard`]. The lifetime of the underlying
/// value has been lifted to `'static`. See [`ScopedGuard`] for more info.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scoped<T: 'static>(Arc<&'static T>);

impl<T: 'static> Deref for Scoped<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
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
        use super::super::ScopedGuard;
        use super::NonCopy;

        #[test]
        fn dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = unsafe { ScopedGuard::new(ref_value) };
            let lifted = guard.lift();
            lifted.access_value();
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
            let guard = unsafe { ScopedGuard::new(ref_value) };
            let lifted = guard.lift();
            lifted.access_value();
            std::mem::drop(lifted);
            std::mem::drop(guard);
        }

        #[tokio::test]
        async fn async_dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = unsafe { ScopedGuard::new(ref_value) };
            let lifted = guard.lift();
            lifted.access_value();
            tokio::spawn(async move {
                lifted.access_value();
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
            let guard = unsafe { ScopedGuard::new(ref_value) };
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
        use super::super::ScopedGuard;
        use super::NonCopy;

        #[test]
        fn undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = unsafe { ScopedGuard::new(ref_value) };
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
            let guard = unsafe { ScopedGuard::new(ref_value) };
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

    #[cfg(test)]
    mod macro_tests {
        #![deny(dropping_references)]
        #![deny(forgetting_references)]
        use super::NonCopy;

        #[test]
        fn dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = scoped!(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            #[allow(dropping_references)]
            std::mem::drop(guard);
        }

        #[test]
        fn valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = scoped!(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            std::mem::drop(lifted);
            #[allow(dropping_references)]
            std::mem::drop(guard);
        }

        #[tokio::test]
        #[should_panic]
        async fn async_dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = scoped!(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            tokio::spawn(async move {
                lifted.access_value();
            });
            #[allow(dropping_references)]
            std::mem::drop(guard);
        }

        #[tokio::test]
        async fn async_valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = scoped!(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            tokio::spawn(async move {
                lifted.access_value();
            })
            .await
            .unwrap();
            #[allow(dropping_references)]
            std::mem::drop(guard);
        }

        #[test]
        fn undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = scoped!(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            #[allow(forgetting_references)]
            std::mem::forget(guard);
            lifted.access_value();
        }

        #[tokio::test]
        async fn async_undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let guard = scoped!(ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            let fut = tokio::spawn(async move {
                let result = std::panic::catch_unwind(|| {
                    // The assert here should fail (Showing UB) in a testable way
                    lifted.access_value();
                });
                result
            });
            #[allow(forgetting_references)]
            std::mem::forget(guard);
            // std::mem::drop(concrete_value);
            let result = fut.await.unwrap();
            assert!(result.is_ok(), "Forgetting a reference has no effect");
        }
    }
}
