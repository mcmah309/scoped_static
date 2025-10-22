use std::marker::PhantomPinned;
use std::pin::Pin;
use std::ptr::NonNull;
use std::sync::atomic::AtomicUsize;
use std::{marker::PhantomData, mem, ops::Deref};

use crate::utils;

/// A safe way to create a [`ScopedPinGuard`].
/// ```rust
/// use scoped_static::scoped_pin;
///
/// #[tokio::main]
/// async fn main() {
///     let concrete_value = Box::new(1.0);
///     let ref_value = &concrete_value;
///     scoped_pin!(guard, ref_value);
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
macro_rules! scoped_pin {
    ($guard_ident:ident, $ref_value:expr) => {
        let mut $guard_ident = unsafe { $crate::ScopedPinGuard::new($ref_value) };
        let $guard_ident = &mut unsafe { std::pin::Pin::new_unchecked(&mut $guard_ident) };
    };
}

/// A reference with lifetime `'a` that can be lifted to a reference with a `'static` lifetime ([`ScopedPin`]).
/// Runtime checks are used to ensure that no derived [`ScopedPin`] exists when this [`ScopedPinGuard`] is
/// dropped.
///
/// ```rust
/// use scoped_static::ScopedPinGuard;
///
/// #[tokio::main]
/// async fn main() {
///     let concrete_value = Box::new(1.0);
///     let ref_value = &concrete_value;
///     let mut guard_unpinned = unsafe { ScopedPinGuard::new(ref_value) };
///     let guard = unsafe { std::pin::Pin::new_unchecked(&mut guard_unpinned) };
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
/// If a [`ScopedPinGuard`] is dropped while any derived [`ScopedPin`] exist, then it will abort the whole
/// program (instead of panic). This is because [`ScopedPin`] could exist on another thread and be unaffected
/// by the panic or the panic could be recovered from. This could lead to undefined behavior.
///
/// Unlike [`crate::ScopedRefGuard`] this pins the guard to the current stack without boxing. Thus it is more
/// efficient, but it cannot be moved.
///
/// UNDEFINED BEHAVIOR: It may cause undefined behavior to leak/forget this value. Since
/// the `Drop` code must run to prevent undefined behavior.
/// e.g. [`std::mem::forget`], [`std::mem::ManuallyDrop`], or Rc cycles, etc.
///
/// See [`scoped_pin`] macro for a safe way to create.
#[derive(Debug)]
pub struct ScopedPinGuard<'a, T: 'static> {
    value: &'static T,
    counter: AtomicUsize,
    _scope: PhantomData<&'a ()>,
    _unpinnable: PhantomPinned,
}

impl<'a, T: 'static> ScopedPinGuard<'a, T> {
    /// Creates a new [`ScopedPinGuard`]. See [`scoped_static`] for a safe way to create.
    pub unsafe fn new(value: &'a T) -> Self {
        let value = unsafe { mem::transmute::<&'a T, &'static T>(value) };
        let counter = AtomicUsize::new(0);
        ScopedPinGuard {
            value,
            counter,
            _scope: std::marker::PhantomData,
            _unpinnable: std::marker::PhantomPinned,
        }
    }

    /// Lifts this reference with lifetime `'a` into `'static` and relies on runtime
    /// checks to ensure safety.
    pub fn lift(self: &Pin<&mut Self>) -> ScopedPin<T> {
        self.counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        ScopedPin {
            value: self.value,
            counter: NonNull::from_ref(&self.counter),
            _counter: PhantomData,
        }
    }
}

impl<'a, T> Deref for ScopedPinGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'a, T: 'static> Drop for ScopedPinGuard<'a, T> {
    fn drop(&mut self) {
        let count = self.counter.load(std::sync::atomic::Ordering::SeqCst);
        if count != 0 {
            utils::abort();
        }
    }
}

/// A reference derived from a [`ScopedPinGuard`]. The lifetime of the underlying
/// value has been lifted to `'static`. See [`ScopedPinGuard`] for more info.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopedPin<T: 'static> {
    value: &'static T,
    counter: NonNull<AtomicUsize>,
    _counter: PhantomData<AtomicUsize>,
}

unsafe impl<T: 'static + Send> Send for ScopedPin<T> {}
unsafe impl<T: 'static + Sync> Sync for ScopedPin<T> {}

impl<T: 'static> Deref for ScopedPin<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T: 'static> Clone for ScopedPin<T> {
    fn clone(&self) -> Self {
        unsafe {
            let counter = self.counter.as_ref();
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        ScopedPin {
            value: self.value,
            counter: self.counter,
            _counter: PhantomData,
        }
    }
}

impl<T: 'static> Drop for ScopedPin<T> {
    fn drop(&mut self) {
        unsafe {
            let counter = self.counter.as_ref();
            counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
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
        use super::super::ScopedPinGuard;
        use super::NonCopy;

        #[test]
        #[should_panic]
        fn dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let mut guard_unpinned = unsafe { ScopedPinGuard::new(ref_value) };
            let guard = unsafe { std::pin::Pin::new_unchecked(&mut guard_unpinned) };
            let lifted = guard.lift();
            lifted.access_value();
            std::mem::drop(guard_unpinned);
        }

        #[test]
        fn valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let mut guard_unpinned = unsafe { ScopedPinGuard::new(ref_value) };
            let guard = unsafe { std::pin::Pin::new_unchecked(&mut guard_unpinned) };
            let lifted = guard.lift();
            lifted.access_value();
            std::mem::drop(lifted);
            std::mem::drop(guard_unpinned);
        }

        #[tokio::test]
        #[should_panic]
        async fn async_dangling() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let mut guard_unpinned = unsafe { ScopedPinGuard::new(ref_value) };
            let guard = unsafe { std::pin::Pin::new_unchecked(&mut guard_unpinned) };
            let lifted = guard.lift();
            lifted.access_value();
            std::mem::drop(guard_unpinned);
            tokio::spawn(async move {
                lifted.access_value();
            });
        }

        #[tokio::test]
        async fn async_valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let mut guard_unpinned = unsafe { ScopedPinGuard::new(ref_value) };
            let guard = unsafe { std::pin::Pin::new_unchecked(&mut guard_unpinned) };
            let lifted = guard.lift();
            lifted.access_value();
            tokio::spawn(async move {
                lifted.access_value();
            })
            .await
            .unwrap();
            std::mem::drop(guard_unpinned);
        }
    }

    #[cfg(test)]
    mod ub_tests {
        use super::super::ScopedPinGuard;
        use super::NonCopy;

        #[test]
        fn undefined_behavior() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            let mut guard_unpinned = unsafe { ScopedPinGuard::new(ref_value) };
            let guard = unsafe { std::pin::Pin::new_unchecked(&mut guard_unpinned) };
            let lifted = guard.lift();
            lifted.access_value();
            std::mem::forget(guard);
            std::mem::forget(guard_unpinned);
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
            let mut guard_unpinned = unsafe { ScopedPinGuard::new(ref_value) };
            let guard = unsafe { std::pin::Pin::new_unchecked(&mut guard_unpinned) };
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
            std::mem::forget(guard_unpinned);
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
            scoped_pin!(guard, ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            #[allow(dropping_references)]
            std::mem::drop(guard);
        }

        #[test]
        fn valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            scoped_pin!(guard, ref_value);
            let lifted = guard.lift();
            lifted.access_value();
            std::mem::drop(lifted);
            #[allow(dropping_references)]
            std::mem::drop(guard);
        }

        #[tokio::test]
        async fn async_dangling() {
            let result = std::panic::catch_unwind(|| {
                let lifted;
                {
                    let concrete_value = Box::new(NonCopy::new());
                    let ref_value = &concrete_value;
                    scoped_pin!(guard, ref_value);
                    lifted = guard.lift();
                    lifted.access_value();
                    #[allow(dropping_references)]
                    std::mem::drop(guard);
                }
                lifted
            });
            match result {
                Ok(lifted) => {
                    tokio::spawn(async move {
                        lifted.access_value();
                    });
                }
                Err(_) => {
                    // Expected
                }
            }
        }

        #[tokio::test]
        async fn async_valid() {
            let concrete_value = Box::new(NonCopy::new());
            let ref_value = &concrete_value;
            scoped_pin!(guard, ref_value);
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
            scoped_pin!(guard, ref_value);
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
            scoped_pin!(guard, ref_value);
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
