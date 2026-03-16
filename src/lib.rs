#![cfg_attr(all(not(feature = "std"), not(test)), no_std)]

extern crate alloc;

pub mod domain;
pub mod error;
pub mod event_store;
pub mod ports;
pub mod projections;
pub mod extensions;
