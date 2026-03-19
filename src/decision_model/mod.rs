//! Decision model primitives and orchestration utilities.
//!
//! # Overview
//!
//! This module provides projection-based building blocks used during command handling:
//! - [`DecisionProjection`] for deterministic folds over relevant events,
//! - `build_*` helpers for deriving state plus append conditions,
//! - [`DecisionModelExt`] for retry-aware execution against an event store.
//!
//! Use these APIs to keep decision logic explicit and testable while preserving optimistic
//! concurrency boundaries.

pub mod decision_projection;
pub use decision_projection::*;
pub mod build;
pub use build::*;
pub mod execute;
pub use execute::*;
