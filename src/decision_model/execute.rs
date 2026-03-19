//! Decision model orchestration helpers for command-style workflows.
//!
//! # Overview
//!
//! Instead of a reflection-based mediator, Rust code typically composes explicit functions and
//! traits. [`DecisionModelExt`] provides a thin orchestration layer on top of
//! [`crate::event_store::EventStore`] for common decision flow patterns:
//! - build state from relevant events,
//! - execute a command operation,
//! - retry when optimistic concurrency fails.
//!
//! # Examples
//!
//! ```rust,no_run
//! use marmosa::decision_model::{DecisionModelExt, DelegateDecisionProjection};
//! use marmosa::domain::{Query, QueryItem};
//! use marmosa::event_store::EventStore;
//! use marmosa::ports::Error;
//!
//! async fn ensure_course_exists<S>(store: &S) -> Result<bool, Error>
//! where
//!     S: EventStore + DecisionModelExt + Send + Sync,
//! {
//!     let projection = DelegateDecisionProjection::new(
//!         false,
//!         Query {
//!             items: vec![QueryItem {
//!                 event_types: vec!["CourseCreatedEvent".to_string()],
//!                 tags: vec![],
//!             }],
//!         },
//!         |state, evt| {
//!             if evt.event.event_type == "CourseCreatedEvent" {
//!                 true
//!             } else {
//!                 state
//!             }
//!         },
//!     );
//!
//!     let model = store.build_decision_model_async(projection).await?;
//!     Ok(model.state)
//! }
//! ```
//!
//! ```rust,no_run
//! use marmosa::decision_model::DecisionModelExt;
//! use marmosa::event_store::EventStore;
//! use marmosa::ports::Error;
//!
//! async fn run_with_retry<S>(store: &S) -> Result<(), Error>
//! where
//!     S: EventStore + DecisionModelExt + Send + Sync,
//! {
//!     store
//!         .execute_decision_async(3, |_s| async move {
//!             // append / business decision logic goes here
//!             Ok(())
//!         })
//!         .await
//! }
//! ```

use crate::event_store::EventStore;
use crate::ports::Error;

/// Provides execution orchestration for decision models with automatic retry.
pub trait DecisionModelExt {
    /// Executes an operation that modifies the event store, retrying on concurrency conflicts.
    /// In the event of an `AppendConditionFailed` error, the operation is retried up to
    /// `max_retries` times.
    ///
    /// # Errors
    ///
    /// Returns the operation error when retries are exhausted or when a non-retriable error
    /// occurs.
    fn execute_decision_async<R, F, Fut>(
        &self,
        max_retries: usize,
        operation: F,
    ) -> impl core::future::Future<Output = Result<R, Error>> + Send
    where
        F: Fn(&Self) -> Fut + Send + Sync,
        Fut: core::future::Future<Output = Result<R, Error>> + Send;

    /// Builds a decision model by querying relevant events and folding them with one projection.
    fn build_decision_model_async<P, S>(
        &self,
        projection: P,
    ) -> impl core::future::Future<Output = Result<crate::decision_model::DecisionModel<S>, Error>> + Send
    where
        P: crate::decision_model::DecisionProjection<State = S> + Send + Sync,
        S: Send + Sync;

    /// Builds decision state for multiple homogeneous projections and returns one append condition.
    fn build_decision_model_nary_async<P, S>(
        &self,
        projections: &[&P],
    ) -> impl core::future::Future<
        Output = Result<(alloc::vec::Vec<S>, crate::domain::AppendCondition), Error>,
    > + Send
    where
        P: crate::decision_model::DecisionProjection<State = S> + Send + Sync + ?Sized,
        S: Send + Sync;
}

impl<T: EventStore + Send + Sync> DecisionModelExt for T {
    async fn execute_decision_async<R, F, Fut>(
        &self,
        max_retries: usize,
        operation: F,
    ) -> Result<R, Error>
    where
        F: Fn(&Self) -> Fut + Send + Sync,
        Fut: core::future::Future<Output = Result<R, Error>> + Send,
    {
        let mut attempt = 0;
        loop {
            match operation(self).await {
                Ok(result) => return Ok(result),
                Err(Error::AppendConditionFailed) if attempt < max_retries - 1 => {
                    // Retry backoff logic can be implemented here in the future
                    // Currently we immediately retry (similar to C# initialDelayMs: 0)
                }
                // Handle other errors or when max_retries is reached
                Err(err) => return Err(err),
            }
            attempt += 1;
        }
    }

    async fn build_decision_model_async<P, S>(
        &self,
        projection: P,
    ) -> Result<crate::decision_model::DecisionModel<S>, Error>
    where
        P: crate::decision_model::DecisionProjection<State = S> + Send + Sync,
        S: Send + Sync,
    {
        let query = projection.query().clone();
        let events = self.read_async(query, None, None, None).await?;
        let model = crate::decision_model::build_decision_model_from_events(&projection, &events);
        Ok(model)
    }

    async fn build_decision_model_nary_async<P, S>(
        &self,
        projections: &[&P],
    ) -> Result<(alloc::vec::Vec<S>, crate::domain::AppendCondition), Error>
    where
        P: crate::decision_model::DecisionProjection<State = S> + Send + Sync + ?Sized,
        S: Send + Sync,
    {
        if projections.is_empty() {
            return Err(Error::ArgumentError(alloc::string::String::from(
                "projections cannot be empty",
            )));
        }

        let mut composite_query_items = alloc::vec::Vec::new();
        for p in projections.iter() {
            composite_query_items.extend(p.query().items.iter().cloned());
        }
        let composite_query = crate::domain::Query {
            items: composite_query_items,
        };
        let events = self.read_async(composite_query, None, None, None).await?;

        let model =
            crate::decision_model::build_decision_model_nary_from_events(projections, &events);
        Ok(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::{String, ToString};
    use alloc::sync::Arc;
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicUsize, Ordering};

    use crate::domain::{AppendCondition, EventData, Query};

    // Minimal stub - the store instance is only passed through to the operation delegate
    struct StubEventStore;

    impl EventStore for StubEventStore {
        async fn append_async(
            &self,
            _events: Vec<EventData>,
            _condition: Option<AppendCondition>,
        ) -> Result<(), Error> {
            Ok(())
        }

        async fn read_async(
            &self,
            _query: Query,
            _from_position: Option<u64>,
            _max_count: Option<usize>,
            _read_options: Option<Vec<crate::domain::ReadOption>>,
        ) -> Result<Vec<crate::domain::EventRecord>, Error> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn execute_decision_async_succeeds_on_first_attempt_returns_result() {
        let store = StubEventStore;
        let call_count = Arc::new(AtomicUsize::new(0));

        let count_clone = call_count.clone();
        let result = store
            .execute_decision_async(3, |_store| {
                let counts = count_clone.clone();
                async move {
                    counts.fetch_add(1, Ordering::SeqCst);
                    Ok::<i32, Error>(42)
                }
            })
            .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn execute_decision_async_retries_on_append_condition_failed_succeeds_on_second() {
        let store = StubEventStore;
        let call_count = Arc::new(AtomicUsize::new(0));

        let count_clone = call_count.clone();
        let result = store
            .execute_decision_async(3, |_store| {
                let counts = count_clone.clone();
                async move {
                    let current = counts.fetch_add(1, Ordering::SeqCst);
                    if current < 1 {
                        Err(Error::AppendConditionFailed)
                    } else {
                        Ok::<String, Error>("ok".to_string())
                    }
                }
            })
            .await;

        assert_eq!(result.unwrap(), "ok");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn execute_decision_async_rethrows_append_condition_failed_after_max_retries() {
        let store = StubEventStore;

        let result = store
            .execute_decision_async(3, |_store| async move {
                Err::<String, Error>(Error::AppendConditionFailed)
            })
            .await;

        assert!(matches!(result, Err(Error::AppendConditionFailed)));
    }

    #[tokio::test]
    async fn execute_decision_async_invokes_operation_exactly_max_retries_before_rethrowing() {
        let store = StubEventStore;
        let call_count = Arc::new(AtomicUsize::new(0));

        let count_clone = call_count.clone();
        let _ = store
            .execute_decision_async(3, |_store| {
                let counts = count_clone.clone();
                async move {
                    counts.fetch_add(1, Ordering::SeqCst);
                    Err::<String, Error>(Error::AppendConditionFailed)
                }
            })
            .await;

        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn execute_decision_async_non_retriable_error_propagates_immediately() {
        let store = StubEventStore;
        let call_count = Arc::new(AtomicUsize::new(0));

        let count_clone = call_count.clone();
        let result = store
            .execute_decision_async(3, |_store| {
                let counts = count_clone.clone();
                async move {
                    counts.fetch_add(1, Ordering::SeqCst);
                    Err::<String, Error>(Error::NotFound)
                }
            })
            .await;

        assert!(matches!(result, Err(Error::NotFound)));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }
}
