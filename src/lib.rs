#[cfg(any(feature = "min_safety", debug_assertions))]
use std::sync::Arc;
use std::{marker::PhantomData, mem, ops::Deref};

/// A scope that holds a reference with lifetime `'a` that be converted to a reference with a
/// `'static` lifetime. Runtime checks are used to ensure that no `SArc` exists when this `Scope` is
/// dropped. If a `Scope` is dropped while any derived `SArc` exist. Then it will abort the whole
/// program instead of panic. This is because `SArc` could exist on another thread and be unaffected
/// by the panic or the panic could be recovered from. This could lead to undefined behavior.
///
/// UNDEFINED BEHAVIOR: It may cause undefined behavior to forget this value (`std::mem::forget(scope)`),
/// the `Drop` code must run to prevent undefined behavior. If the default `min_safety` flag is not
/// enabled, this may cause undefined behavior in other scenarios during non-debug mode.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scope<'a, T: 'static> {
    #[cfg(any(feature = "min_safety", debug_assertions))]
    data: Arc<&'static T>,
    #[cfg(not(any(feature = "min_safety", debug_assertions)))]
    data: &'static T,
    _scope: PhantomData<&'a ()>,
}

impl<'a, T: 'static> Scope<'a, T> {
    pub fn new(value: &'a T) -> Self {
        let value = unsafe { mem::transmute::<&'a T, &'static T>(value) };
        #[cfg(any(feature = "min_safety", debug_assertions))]
        let value = Arc::new(value);
        Scope {
            data: value,
            _scope: std::marker::PhantomData,
        }
    }

    /// Lifts reference with lifetime `'a` out of the scope into `'static` and relies on runtime
    /// Checks to ensure safety.
    pub fn lift(&self) -> SArc<T> {
        #[cfg(any(feature = "min_safety", debug_assertions))]
        return SArc(self.data.clone());
        #[cfg(not(any(feature = "min_safety", debug_assertions)))]
        return SArc(self.data);
    }
}

impl<'a, T: 'static> Drop for Scope<'a, T> {
    fn drop(&mut self) {
        #[cfg(any(feature = "min_safety", debug_assertions))]
        {
            if std::sync::Arc::strong_count(&self.data) != 1 {
                const ROOT_MSG: &str = "Fatal error: Scope dropped while SArc references still exist. \
                This would cause undefined behavior. Aborting.\n";
                // We don't panic since panics can be recovered and panics also only effect a single thread
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

#[cfg(any(feature = "min_safety", debug_assertions))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SArc<T: 'static>(Arc<&'static T>);

#[cfg(not(any(feature = "min_safety", debug_assertions)))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SArc<T: 'static>(&'static T);

#[cfg(any(feature = "min_safety", debug_assertions))]
impl<T: 'static> Deref for SArc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

#[cfg(not(any(feature = "min_safety", debug_assertions)))]
impl<T: 'static> Deref for SArc<T> {
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
        use crate::{Scope, tests::NonCopy};

        #[test]
        fn dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let scope = Scope::new(ref_value);
            let sarc = scope.lift();
            sarc.access_value();
            let result = std::panic::catch_unwind(|| {
                std::mem::drop(scope);
            });
            #[cfg(any(feature = "min_safety", debug_assertions))]
            assert!(
                result.is_err(),
                "expected panic when dropping scope with live SArc"
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
            let scope = Scope::new(ref_value);
            let sarc = scope.lift();
            sarc.access_value();
            std::mem::drop(sarc);
            std::mem::drop(scope);
        }

        #[tokio::test]
        async fn async_dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let scope = Scope::new(ref_value);
            let sarc = scope.lift();
            sarc.access_value();
            tokio::spawn(async move {
                sarc.access_value();
            });
            let result = std::panic::catch_unwind(|| {
                std::mem::drop(scope);
            });
            #[cfg(any(feature = "min_safety", debug_assertions))]
            assert!(
                result.is_err(),
                "expected panic when dropping scope with live SArc in the task"
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
            let scope = Scope::new(ref_value);
            let sarc = scope.lift();
            sarc.access_value();
            tokio::spawn(async move {
                sarc.access_value();
            })
            .await
            .unwrap();
            std::mem::drop(scope);
        }

        #[cfg(miri)]
        #[test]
        #[should_panic]
        fn undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let scope = Scope::new(ref_value);
            let sarc = scope.lift();
            sarc.access_value();
            std::mem::forget(scope);
            std::mem::drop(concrete_value);
            let result = std::panic::catch_unwind(|| {
                // The assert here should fail (Showing UB) in a testable way
                sarc.access_value();
            });
            assert!(
                result.is_err(),
                "Forgetting the scope, dropping the underlying, then accessing an SArc value should be UB"
            );
        }
    }

    #[cfg(test)]
    mod ub_tests {
        use crate::{Scope, tests::NonCopy};

        #[test]
        fn undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let scope = Scope::new(ref_value);
            let sarc = scope.lift();
            sarc.access_value();
            std::mem::forget(scope);
            std::mem::drop(concrete_value);
            let result = std::panic::catch_unwind(|| {
                // The assert here should fail (Showing UB) in a testable way
                sarc.access_value();
            });
            assert!(
                result.is_err(),
                "Forgetting the scope, dropping the underlying, then accessing an SArc value should be UB"
            );
        }

        #[tokio::test]
        async fn async_undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let scope = Scope::new(ref_value);
            let sarc = scope.lift();
            sarc.access_value();
            let fut = tokio::spawn(async move {
                let result = std::panic::catch_unwind(|| {
                    // The assert here should fail (Showing UB) in a testable way
                    sarc.access_value();
                });
                result
            });
            std::mem::forget(scope);
            std::mem::drop(concrete_value);
            let result = fut.await.unwrap();
            assert!(
                result.is_err(),
                "Forgetting the scope, dropping the underlying, then accessing an SArc value should be UB"
            );
        }
    }
}
