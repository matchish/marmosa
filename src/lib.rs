#![cfg_attr(all(not(feature = "std"), not(test)), no_std)]

extern crate alloc;

pub mod decision_model;
pub mod domain;
pub mod error;
pub mod event_store;
pub mod extensions;
#[cfg(feature = "in-memory")]
pub mod in_memory;
pub mod ports;
pub mod projections;
