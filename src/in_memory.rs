use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::ports::{Clock, Error, StorageBackend};

pub struct InMemoryStorage {
    files: std::sync::Mutex<BTreeMap<String, Vec<u8>>>,
    dirs: std::sync::Mutex<BTreeMap<String, ()>>,
    locks: std::sync::Mutex<BTreeSet<String>>,
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            files: std::sync::Mutex::new(BTreeMap::new()),
            dirs: std::sync::Mutex::new(BTreeMap::new()),
            locks: std::sync::Mutex::new(BTreeSet::new()),
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
            alloc::format!("{}/", path)
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
            alloc::format!("{}/", path)
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
            std::thread::yield_now();
        }
    }

    async fn release_stream_lock(&self, stream_id: &str) -> Result<(), Error> {
        let mut locks = self.locks.lock().unwrap();
        locks.remove(stream_id);
        Ok(())
    }
}

pub struct ManualClock {
    time: AtomicU64,
}

impl ManualClock {
    pub fn new(start: u64) -> Self {
        Self {
            time: AtomicU64::new(start),
        }
    }

    pub fn advance(&self, millis: u64) {
        self.time.fetch_add(millis, Ordering::SeqCst);
    }

    pub fn set(&self, millis: u64) {
        self.time.store(millis, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_millis(&self) -> u64 {
        self.time.load(Ordering::SeqCst)
    }
}
