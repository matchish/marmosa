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

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

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
    pub success: bool,
    pub total_rebuilt: usize,
    pub duration: core::time::Duration,
    pub details: Vec<String>,
    pub failed_projections: Vec<String>,
}

pub struct ProjectionRebuilder<'a, E: EventStore> {
    store: &'a E,
    runners: Vec<Box<dyn RebuildTask<E> + 'a>>,
}

impl<'a, E: EventStore + Send + Sync> ProjectionRebuilder<'a, E> {
    pub fn new(store: &'a E) -> Self {
        Self {
            store,
            runners: Vec::new(),
        }
    }
    
    pub fn register<R: RebuildTask<E> + 'a>(&mut self, runner: R) {
        self.runners.push(Box::new(runner));
    }
    
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
            // concurrent wait via tokio spawn or just awaiting in parallel. 
            // We use simple iteration as naive chunking for now since we don't have futures::future::join_all
            for t in tasks {
                let (name, res) = t.await;
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
