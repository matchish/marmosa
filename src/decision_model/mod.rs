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

use alloc::vec::Vec;
use core::fmt::Debug;

use crate::domain::EventData;

/// A Decision encapsulates a business command: it declares what state it needs
/// (via a [`DecisionProjection`]) and produces events (or an error) from that state.
///
/// This trait composes with [`DecisionProjection`] — the projection handles events→state,
/// `Decision` handles state→events.
pub trait Decision: Send + Sync {
    /// The projection that derives the decision state from stored events.
    type State: DecisionProjection;

    /// The domain error type returned when business rules are violated.
    type Error: Debug + PartialEq + Send + Sync;

    /// Returns the projection instance used to build state from events.
    fn state(&self) -> Self::State;

    /// Core business logic: given the current state, produce new events or an error.
    fn process(
        &self,
        state: &<Self::State as DecisionProjection>::State,
    ) -> Result<Vec<EventData>, Self::Error>;
}
