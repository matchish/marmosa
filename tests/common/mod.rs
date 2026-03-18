#![allow(dead_code)]
use core::sync::atomic::{AtomicU64, Ordering};
use marmosa::ports::{Clock, Error, StorageBackend};
use std::collections::{BTreeMap, BTreeSet};
use std::format;
use std::string::{String, ToString};
use std::sync::{Arc, Mutex};
use std::vec::Vec;

#[derive(Clone)]
pub struct InMemoryStorage {
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    dirs: Arc<Mutex<BTreeMap<String, ()>>>,
    locks: Arc<Mutex<BTreeSet<String>>>,
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStorage {
    pub fn new() -> Self {
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
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
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
}

impl Clock for FakeClock {
    fn now_millis(&self) -> u64 {
        self.time.load(Ordering::SeqCst)
    }
}
