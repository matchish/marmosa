use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use marmosa::domain::{AppendCondition, DomainEvent, EventData, Query, QueryItem, Tag};
use marmosa::event_store::{EventStore, OpossumStore};
use marmosa::ports::{Clock, Error, StorageBackend};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// Storage and clock implementations for benchmarks
// (cannot access #[cfg(test)] modules from bench targets)

#[derive(Clone)]
struct InMemoryStorage {
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    dirs: Arc<Mutex<BTreeMap<String, ()>>>,
    locks: Arc<Mutex<BTreeSet<String>>>,
}

impl InMemoryStorage {
    fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(BTreeMap::new())),
            dirs: Arc::new(Mutex::new(BTreeMap::new())),
            locks: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }
}

impl StorageBackend for InMemoryStorage {
    async fn create_dir_all(&self, path: &str) -> Result<(), Error> {
        self.dirs.lock().unwrap().insert(path.to_string(), ());
        Ok(())
    }

    async fn read_file(&self, path: &str) -> Result<Vec<u8>, Error> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or(Error::NotFound)
    }

    async fn write_file(&self, path: &str, data: &[u8]) -> Result<(), Error> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), data.to_vec());
        Ok(())
    }

    async fn delete_file(&self, path: &str) -> Result<(), Error> {
        let mut files = self.files.lock().unwrap();
        if files.remove(path).is_some() {
            Ok(())
        } else {
            Err(Error::NotFound)
        }
    }

    async fn delete_dir_all(&self, path: &str) -> Result<(), Error> {
        let mut files = self.files.lock().unwrap();
        let mut dirs = self.dirs.lock().unwrap();
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{}/", path)
        };
        let file_keys: Vec<String> = files
            .keys()
            .filter(|k| k.starts_with(&prefix) || **k == *path)
            .cloned()
            .collect();
        for k in file_keys {
            files.remove(&k);
        }
        let dir_keys: Vec<String> = dirs
            .keys()
            .filter(|k| k.starts_with(&prefix) || **k == *path)
            .cloned()
            .collect();
        for k in dir_keys {
            dirs.remove(&k);
        }
        Ok(())
    }

    async fn read_dir(&self, path: &str) -> Result<Vec<String>, Error> {
        let files = self.files.lock().unwrap();
        let mut result = Vec::new();
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{}/", path)
        };
        for k in files.keys() {
            if let Some(file_name) = k.strip_prefix(&prefix) {
                if !file_name.contains('/') {
                    result.push(k.clone());
                }
            }
        }
        Ok(result)
    }

    async fn acquire_stream_lock(&self, stream_id: &str) -> Result<(), Error> {
        // Single-threaded benchmark context: just insert
        self.locks.lock().unwrap().insert(stream_id.to_string());
        Ok(())
    }

    async fn release_stream_lock(&self, stream_id: &str) -> Result<(), Error> {
        self.locks.lock().unwrap().remove(stream_id);
        Ok(())
    }
}

struct FakeClock {
    time: AtomicU64,
}

impl FakeClock {
    fn new(start: u64) -> Self {
        Self {
            time: AtomicU64::new(start),
        }
    }
}

impl Clock for FakeClock {
    fn now_millis(&self) -> u64 {
        self.time.load(Ordering::SeqCst)
    }
}

fn generate_events(count: usize, tag_count: usize) -> Vec<EventData> {
    let event_types = [
        "OrderCreated",
        "OrderShipped",
        "OrderDelivered",
        "OrderCancelled",
    ];
    let tag_keys = [
        "Region",
        "Environment",
        "Tenant",
        "UserId",
        "OrderId",
        "ProductId",
        "CategoryId",
        "Priority",
        "Status",
        "Version",
    ];
    let tag_values = [
        "US-West",
        "Production",
        "Tenant123",
        "User456",
        "Order789",
        "Product001",
        "Category-A",
        "High",
        "Active",
        "v1.0",
    ];

    (0..count)
        .map(|i| {
            let event_type = event_types[i % event_types.len()].to_string();
            let tags: Vec<Tag> = (0..tag_count.min(tag_keys.len()))
                .map(|j| Tag {
                    key: tag_keys[j].to_string(),
                    value: tag_values[j % tag_values.len()].to_string(),
                })
                .collect();

            EventData {
                event_id: format!("evt-{i}"),
                event: DomainEvent {
                    event_type,
                    data: format!("{{\"data\":\"Event data {i}\"}}"),
                    tags,
                },
                metadata: None,
            }
        })
        .collect()
}

fn make_store() -> OpossumStore<InMemoryStorage, FakeClock> {
    OpossumStore::new(InMemoryStorage::new(), FakeClock::new(1_696_000_000))
}

fn append_benchmarks(c: &mut Criterion) {
    // --- Single event append (baseline) ---
    c.bench_function("single_event_append", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter_batched(
                || (make_store(), generate_events(1, 2)),
                |(store, events)| async move {
                    store.append_async(events, None).await.unwrap();
                },
                BatchSize::SmallInput,
            );
    });

    // --- Batch appends of various sizes ---
    let mut group = c.benchmark_group("batch_append");
    for size in [2, 5, 10, 20, 50, 100] {
        group.bench_with_input(BenchmarkId::new("events", size), &size, |b, &size| {
            b.to_async(tokio::runtime::Runtime::new().unwrap())
                .iter_batched(
                    || (make_store(), generate_events(size, 2)),
                    |(store, events)| async move {
                        store.append_async(events, None).await.unwrap();
                    },
                    BatchSize::SmallInput,
                );
        });
    }
    group.finish();

    // --- Append with DCB validation (FailIfEventsMatch) ---
    c.bench_function("append_with_dcb_fail_if_events_match", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter_batched(
                || {
                    let store = make_store();
                    let email = format!("test-{i}@example.com", i = rand_id());
                    let events = vec![EventData {
                        event_id: format!("evt-{}", rand_id()),
                        event: DomainEvent {
                            event_type: "StudentRegistered".to_string(),
                            data: "{}".to_string(),
                            tags: vec![Tag {
                                key: "studentEmail".to_string(),
                                value: email.clone(),
                            }],
                        },
                        metadata: None,
                    }];
                    let condition = AppendCondition {
                        fail_if_events_match: Query {
                            items: vec![QueryItem {
                                event_types: vec![],
                                tags: vec![Tag {
                                    key: "studentEmail".to_string(),
                                    value: email,
                                }],
                            }],
                        },
                        after_sequence_position: None,
                    };
                    (store, events, condition)
                },
                |(store, events, condition)| async move {
                    store
                        .append_async(events, Some(condition))
                        .await
                        .unwrap();
                },
                BatchSize::SmallInput,
            );
    });
}

fn rand_id() -> u64 {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

criterion_group!(benches, append_benchmarks);
criterion_main!(benches);
