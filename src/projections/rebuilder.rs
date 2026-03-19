//! Utilities for rebuilding multiple projections from the event store.
//!
//! # Overview
//!
//! [`ProjectionRebuilder`] coordinates batch rebuilds across registered projection runners.
//! Each registered task is executed asynchronously and summarized in
//! [`ProjectionRebuildResult`].
//!
//! This module is useful for startup catch-up, explicit maintenance operations, and projection
//! recovery scenarios.

use core::future::Future;
use core::pin::Pin;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

use crate::event_store::EventStore;
use crate::ports::Error;
use crate::projections::{ProjectionCheckpoint, ProjectionDefinition, ProjectionRunner, ProjectionStore};
use crate::ports::StorageBackend;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Rebuild contract used by [`ProjectionRebuilder`].
///
/// Implementors expose a projection name, checkpoint lookup, and rebuild operation for one
/// projection pipeline.
pub trait RebuildTask<E: EventStore>: Send + Sync {
    fn name(&self) -> String;
    fn rebuild<'a>(&'a self, store: &'a E) -> BoxFuture<'a, Result<u64, Error>>;
    fn get_checkpoint<'a>(&'a self) -> BoxFuture<'a, Result<Option<ProjectionCheckpoint>, Error>>;
}

impl<S, TState, P, Store, E> RebuildTask<E> for ProjectionRunner<S, TState, P, Store>
where
    S: StorageBackend + Send + Sync,
    TState: serde::Serialize + for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
    P: ProjectionDefinition<State = TState> + Send + Sync,
    Store: ProjectionStore<TState> + Send + Sync,
    E: EventStore + Send + Sync,
{
    fn name(&self) -> String {
        self.projection.projection_name().to_string()
    }

    fn rebuild<'a>(&'a self, store: &'a E) -> BoxFuture<'a, Result<u64, Error>> {
        Box::pin(async {
            self.rebuild(store).await
        })
    }

    fn get_checkpoint<'a>(&'a self) -> BoxFuture<'a, Result<Option<ProjectionCheckpoint>, Error>> {
        Box::pin(async {
            self.get_checkpoint().await
        })
    }
}

pub struct ProjectionRebuildResult {
    /// `true` when every scheduled projection rebuild succeeded.
    pub success: bool,
    /// Number of projections that completed rebuild successfully.
    pub total_rebuilt: usize,
    /// Total elapsed time for the rebuild operation.
    pub duration: core::time::Duration,
    /// Human-readable status lines for individual projections.
    pub details: Vec<String>,
    /// Projection names that failed to rebuild.
    pub failed_projections: Vec<String>,
}

/// Runs rebuilds for a set of registered projection tasks.
///
/// # Examples
///
/// ```rust,no_run
/// use marmosa::event_store::EventStore;
/// use marmosa::projections::ProjectionRebuilder;
///
/// async fn rebuild_all_registered<E: EventStore + Send + Sync>(store: &E) {
///     let rebuilder = ProjectionRebuilder::new(store);
///     let result = rebuilder.rebuild_all(false).await;
///     assert!(result.failed_projections.len() <= result.details.len());
/// }
/// ```
pub struct ProjectionRebuilder<'a, E: EventStore> {
    store: &'a E,
    runners: Vec<Box<dyn RebuildTask<E> + 'a>>,
}

impl<'a, E: EventStore + Send + Sync> ProjectionRebuilder<'a, E> {
    /// Creates a new rebuilder bound to an event store.
    pub fn new(store: &'a E) -> Self {
        Self {
            store,
            runners: Vec::new(),
        }
    }
    
    /// Registers one rebuild task.
    pub fn register<R: RebuildTask<E> + 'a>(&mut self, runner: R) {
        self.runners.push(Box::new(runner));
    }
    
    /// Rebuilds eligible projections and returns a summary.
    ///
    /// # Notes
    ///
    /// - If `force_rebuild` is `false`, tasks with an existing checkpoint are skipped.
    /// - Scheduled tasks are executed concurrently via `futures::future::join_all`.
    /// - Errors are collected per projection and returned in [`ProjectionRebuildResult`].
    pub async fn rebuild_all(&self, force_rebuild: bool) -> ProjectionRebuildResult {
        let mut total_rebuilt = 0;
        let mut details = Vec::new();
        let mut failed = Vec::new();
        
        let mut tasks = Vec::new();
        for runner in &self.runners {
            let name = runner.name();
            let cp = if force_rebuild {
                None
            } else {
                runner.get_checkpoint().await.unwrap_or(None)
            };
            if cp.is_some() && !force_rebuild {
                continue;
            }
            
            let fut = runner.rebuild(self.store);
            tasks.push(async move {
                let res = fut.await;
                (name, res)
            });
        }
        
        if !tasks.is_empty() {
            let results = futures::future::join_all(tasks).await;
            for (name, res) in results {
                match res {
                    Ok(_) => {
                        total_rebuilt += 1;
                        details.push(format!("{} rebuilt successfully", name));
                    }
                    Err(e) => {
                        failed.push(name.clone());
                        details.push(format!("{} failed: {:?}", name, e));
                    }
                }
            }
        }
        
        ProjectionRebuildResult {
            success: failed.is_empty(),
            total_rebuilt,
            duration: core::time::Duration::from_secs(0),
            details,
            failed_projections: failed,
        }
    }
}
