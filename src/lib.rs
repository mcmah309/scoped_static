#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

mod scoped_pin;
mod scoped;
mod utils;

pub use scoped_pin::{ScopedPin, ScopedPinGuard};
pub use scoped::{Scoped, ScopedGuard};
