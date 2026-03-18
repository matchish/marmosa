use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;

#[derive(Debug, PartialEq)]
pub enum Error {
    IoError,
    NotFound,
    AlreadyExists,
    AppendConditionFailed,
}

pub trait StorageBackend {
    fn create_dir_all(&self, path: &str) -> impl Future<Output = Result<(), Error>> + Send;
    fn read_file(&self, path: &str) -> impl Future<Output = Result<Vec<u8>, Error>> + Send;
    fn write_file(&self, path: &str, data: &[u8])
    -> impl Future<Output = Result<(), Error>> + Send;
    fn delete_file(&self, path: &str) -> impl Future<Output = Result<(), Error>> + Send;
    fn delete_dir_all(&self, path: &str) -> impl Future<Output = Result<(), Error>> + Send;
    fn read_dir(&self, path: &str) -> impl Future<Output = Result<Vec<String>, Error>> + Send;

    /// Acquires an exclusive lock for the given stream.
    fn acquire_stream_lock(
        &self,
        stream_id: &str,
    ) -> impl Future<Output = Result<(), Error>> + Send;
    /// Releases the exclusive lock for the given stream.
    fn release_stream_lock(
        &self,
        stream_id: &str,
    ) -> impl Future<Output = Result<(), Error>> + Send;
}

impl<T: ?Sized + StorageBackend + Send + Sync> StorageBackend for alloc::sync::Arc<T> {
    fn create_dir_all(&self, path: &str) -> impl Future<Output = Result<(), Error>> + Send {
        (**self).create_dir_all(path)
    }

    fn read_file(&self, path: &str) -> impl Future<Output = Result<Vec<u8>, Error>> + Send {
        (**self).read_file(path)
    }

    fn write_file(
        &self,
        path: &str,
        data: &[u8],
    ) -> impl Future<Output = Result<(), Error>> + Send {
        (**self).write_file(path, data)
    }

    fn delete_file(&self, path: &str) -> impl Future<Output = Result<(), Error>> + Send {
        (**self).delete_file(path)
    }

    fn delete_dir_all(&self, path: &str) -> impl Future<Output = Result<(), Error>> + Send {
        (**self).delete_dir_all(path)
    }

    fn read_dir(&self, path: &str) -> impl Future<Output = Result<Vec<String>, Error>> + Send {
        (**self).read_dir(path)
    }

    fn acquire_stream_lock(
        &self,
        stream_id: &str,
    ) -> impl Future<Output = Result<(), Error>> + Send {
        (**self).acquire_stream_lock(stream_id)
    }

    fn release_stream_lock(
        &self,
        stream_id: &str,
    ) -> impl Future<Output = Result<(), Error>> + Send {
        (**self).release_stream_lock(stream_id)
    }
}

pub trait Clock {
    fn now_millis(&self) -> u64;
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use alloc::collections::{BTreeMap, BTreeSet};
    use alloc::string::ToString;
    use core::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    pub struct InMemoryStorage {
        files: Mutex<BTreeMap<String, Vec<u8>>>,
        dirs: Mutex<BTreeMap<String, ()>>,
        locks: Mutex<BTreeSet<String>>,
    }

    impl Default for InMemoryStorage {
        fn default() -> Self {
            Self::new()
        }
    }

    impl InMemoryStorage {
        pub fn new() -> Self {
            Self {
                files: Mutex::new(BTreeMap::new()),
                dirs: Mutex::new(BTreeMap::new()),
                locks: Mutex::new(BTreeSet::new()),
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

            // Collect keys to remove
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
                if k.starts_with(&prefix) {
                    let file_name = k.strip_prefix(&prefix).unwrap();
                    // We only want direct children (no slashes in file_name)
                    if !file_name.contains('/') {
                        result.push(k.clone());
                    }
                }
            }
            Ok(result)
        }

        async fn acquire_stream_lock(&self, stream_id: &str) -> Result<(), Error> {
            loop {
                {
                    let mut locks = self.locks.lock().unwrap();
                    if locks.insert(stream_id.to_string()) {
                        return Ok(());
                    }
                }
                tokio::task::yield_now().await;
            }
        }

        async fn release_stream_lock(&self, stream_id: &str) -> Result<(), Error> {
            let mut locks = self.locks.lock().unwrap();
            locks.remove(stream_id);
            Ok(())
        }
    }

    pub struct FakeClock {
        time: AtomicU64,
    }

    impl FakeClock {
        pub fn new(start: u64) -> Self {
            Self {
                time: AtomicU64::new(start),
            }
        }
        pub fn advance(&self, millis: u64) {
            self.time.fetch_add(millis, Ordering::SeqCst);
        }
    }

    impl Clock for FakeClock {
        fn now_millis(&self) -> u64 {
            self.time.load(Ordering::SeqCst)
        }
    }

    #[tokio::test]
    async fn test_in_memory_backend() {
        let storage = InMemoryStorage::new();
        assert!(storage.write_file("test.txt", b"hello").await.is_ok());
        let content = storage.read_file("test.txt").await.unwrap();
        assert_eq!(content, b"hello");
    }
}
