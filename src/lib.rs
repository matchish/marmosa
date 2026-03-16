#![cfg_attr(all(not(feature = "std"), not(test)), no_std)]

extern crate alloc;

pub mod domain;
pub mod ports;
pub mod event_store;
pub mod projections;

