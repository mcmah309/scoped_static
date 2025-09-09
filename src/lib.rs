use std::{marker::PhantomData, mem, ops::Deref, sync::Arc};

/// A scope that holds a reference with lifetime `'a` that be converted to a reference with a
/// `'static` lifetime. Runtime checks are used to ensure that no `SArc` exists when this `Scope` is
/// dropped.
///
/// UNDEFINED BEHAVIOR: It may cause undefined behavior to forget this value - `std::mem::forget(scope)`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scope<'a, T: 'static> {
    data: Arc<&'static T>,
    _scope: PhantomData<&'a ()>,
}

impl<'a, T: 'static> Scope<'a, T> {
    pub fn new(value: &'a T) -> Self {
        let value = unsafe { mem::transmute::<&'a T, &'static T>(value) };
        let arc = Arc::new(value);
        Scope {
            data: arc,
            _scope: std::marker::PhantomData,
        }
    }

    /// Lifts reference with lifetime `'a` out of the scope into `'static` and relies on runtime
    /// Checks to ensure safety.
    pub fn lift(&self) -> SArc<T> {
        SArc(self.data.clone())
    }
}

impl<'a, T: 'static> Drop for Scope<'a, T> {
    fn drop(&mut self) {
        if std::sync::Arc::strong_count(&self.data) != 1 {
            const ROOT_MSG: &str = "Fatal error: Scope dropped while SArc references still exist. \
                This would cause undefined behavior. Aborting.\n";
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SArc<T: 'static>(Arc<&'static T>);

impl<T: 'static> Deref for SArc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use crate::Scope;

    struct NonCopy(f32);

    impl NonCopy {
        pub fn new() -> Self {
            NonCopy(1.0)
        }
        pub fn access_value(&self) {
            assert_eq!(self.0, 1.0);
        }
    }

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

        assert!(
            result.is_err(),
            "expected panic when dropping scope with live SArc"
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
        assert!(
            result.is_err(),
            "expected panic when dropping scope with live SArc in the task"
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
}
