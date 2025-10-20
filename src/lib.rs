#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

mod scoped_pin;
mod scoped_ref;

pub use scoped_pin::{ScopedPin, ScopedPinGuard};
pub use scoped_ref::{ScopedRef, ScopedRefGuard};
