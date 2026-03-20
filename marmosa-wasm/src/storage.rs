use marmosa::ports::{Error, StorageBackend};
use send_wrapper::SendWrapper;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "node:fs/promises")]
extern "C" {
    #[wasm_bindgen(catch, js_name = readFile)]
    async fn node_read_file(path: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = writeFile)]
    async fn node_write_file(path: &str, data: &[u8]) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = unlink)]
    async fn node_unlink(path: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = mkdir)]
    async fn node_mkdir(path: &str, options: &JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = rm)]
    async fn node_rm(path: &str, options: &JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = readdir)]
    async fn node_readdir(path: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = open)]
    async fn node_open(path: &str, flags: &str) -> Result<JsValue, JsValue>;
}

#[wasm_bindgen(module = "node:fs/promises")]
extern "C" {
    type FileHandle;

    #[wasm_bindgen(method, catch)]
    async fn close(this: &FileHandle) -> Result<JsValue, JsValue>;
}

fn js_err_to_error(err: JsValue) -> Error {
    let msg = js_sys::Reflect::get(&err, &JsValue::from_str("code"))
        .ok()
        .and_then(|v| v.as_string());
    match msg.as_deref() {
        Some("ENOENT") => Error::NotFound,
        Some("EEXIST") => Error::AlreadyExists,
        _ => Error::IoError,
    }
}

fn sleep_ms(ms: i32) -> SendWrapper<impl core::future::Future<Output = ()>> {
    SendWrapper::new(async move {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            let global = js_sys::global();
            let set_timeout = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout"))
                .expect("setTimeout not found");
            let set_timeout: js_sys::Function = set_timeout.into();
            let _ = set_timeout.call2(&JsValue::NULL, &resolve, &JsValue::from(ms));
        });
        let _ = JsFuture::from(promise).await;
    })
}

pub struct NodeFileSystemStorage {
    base_path: String,
    stale_lock_threshold_ms: u64,
}

impl NodeFileSystemStorage {
    pub fn new(base_path: String) -> Self {
        Self {
            base_path,
            stale_lock_threshold_ms: 30_000,
        }
    }

    fn resolve(&self, path: &str) -> String {
        format!("{}/{}", self.base_path, path)
    }

    fn lock_path(&self, stream_id: &str) -> String {
        format!("{}/.locks/{}.lock", self.base_path, stream_id)
    }
}

impl StorageBackend for NodeFileSystemStorage {
    fn create_dir_all(&self, path: &str) -> impl core::future::Future<Output = Result<(), Error>> + Send {
        let full = self.resolve(path);
        SendWrapper::new(async move {
            let opts = js_sys::Object::new();
            js_sys::Reflect::set(&opts, &JsValue::from_str("recursive"), &JsValue::TRUE)
                .map_err(|_| Error::IoError)?;
            node_mkdir(&full, &opts.into())
                .await
                .map_err(js_err_to_error)?;
            Ok(())
        })
    }

    fn read_file(&self, path: &str) -> impl core::future::Future<Output = Result<Vec<u8>, Error>> + Send {
        let full = self.resolve(path);
        SendWrapper::new(async move {
            let result = node_read_file(&full).await.map_err(js_err_to_error)?;
            let buffer = js_sys::Uint8Array::new(&result);
            Ok(buffer.to_vec())
        })
    }

    fn write_file(
        &self,
        path: &str,
        data: &[u8],
    ) -> impl core::future::Future<Output = Result<(), Error>> + Send {
        let full = self.resolve(path);
        let data = data.to_vec();
        SendWrapper::new(async move {
            // Ensure parent directory exists
            if let Some(parent_end) = full.rfind('/') {
                let parent = &full[..parent_end];
                let opts = js_sys::Object::new();
                js_sys::Reflect::set(&opts, &JsValue::from_str("recursive"), &JsValue::TRUE)
                    .map_err(|_| Error::IoError)?;
                let _ = node_mkdir(parent, &opts.into()).await;
            }
            node_write_file(&full, &data)
                .await
                .map_err(js_err_to_error)?;
            Ok(())
        })
    }

    fn delete_file(&self, path: &str) -> impl core::future::Future<Output = Result<(), Error>> + Send {
        let full = self.resolve(path);
        SendWrapper::new(async move {
            node_unlink(&full).await.map_err(js_err_to_error)?;
            Ok(())
        })
    }

    fn delete_dir_all(&self, path: &str) -> impl core::future::Future<Output = Result<(), Error>> + Send {
        let full = self.resolve(path);
        SendWrapper::new(async move {
            let opts = js_sys::Object::new();
            js_sys::Reflect::set(&opts, &JsValue::from_str("recursive"), &JsValue::TRUE)
                .map_err(|_| Error::IoError)?;
            js_sys::Reflect::set(&opts, &JsValue::from_str("force"), &JsValue::TRUE)
                .map_err(|_| Error::IoError)?;
            node_rm(&full, &opts.into())
                .await
                .map_err(js_err_to_error)?;
            Ok(())
        })
    }

    fn read_dir(&self, path: &str) -> impl core::future::Future<Output = Result<Vec<String>, Error>> + Send {
        let full = self.resolve(path);
        let logical_prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{}/", path)
        };
        SendWrapper::new(async move {
            let result = node_readdir(&full).await.map_err(js_err_to_error)?;
            let arr = js_sys::Array::from(&result);
            let mut entries = Vec::new();
            for i in 0..arr.length() {
                if let Some(name) = arr.get(i).as_string() {
                    entries.push(format!("{}{}", logical_prefix, name));
                }
            }
            Ok(entries)
        })
    }

    fn acquire_stream_lock(
        &self,
        stream_id: &str,
    ) -> impl core::future::Future<Output = Result<(), Error>> + Send {
        let lock_file = self.lock_path(stream_id);
        let lock_dir = format!("{}/.locks", self.base_path);
        let threshold = self.stale_lock_threshold_ms;
        SendWrapper::new(async move {
            // Ensure locks directory exists
            let opts = js_sys::Object::new();
            js_sys::Reflect::set(&opts, &JsValue::from_str("recursive"), &JsValue::TRUE)
                .map_err(|_| Error::IoError)?;
            let _ = node_mkdir(&lock_dir, &opts.into()).await;

            loop {
                // Try exclusive create
                match node_open(&lock_file, "wx").await {
                    Ok(handle) => {
                        // Write timestamp
                        let now = js_sys::Date::now() as u64;
                        let timestamp = now.to_string();
                        let _ = node_write_file(&lock_file, timestamp.as_bytes()).await;
                        // Close the file handle
                        let fh: FileHandle = handle.unchecked_into();
                        let _ = fh.close().await;
                        return Ok(());
                    }
                    Err(err) => {
                        let code = js_sys::Reflect::get(&err, &JsValue::from_str("code"))
                            .ok()
                            .and_then(|v| v.as_string());
                        if code.as_deref() == Some("EEXIST") {
                            // Check for stale lock
                            if let Ok(data) = node_read_file(&lock_file).await {
                                let buf = js_sys::Uint8Array::new(&data);
                                let bytes = buf.to_vec();
                                if let Ok(ts_str) = String::from_utf8(bytes) {
                                    if let Ok(ts) = ts_str.trim().parse::<u64>() {
                                        let now = js_sys::Date::now() as u64;
                                        if now.saturating_sub(ts) > threshold {
                                            // Stale lock — force remove and retry
                                            let _ = node_unlink(&lock_file).await;
                                            continue;
                                        }
                                    }
                                }
                            }
                            // Not stale — wait and retry
                            sleep_ms(10).await;
                            continue;
                        }
                        return Err(Error::IoError);
                    }
                }
            }
        })
    }

    fn release_stream_lock(
        &self,
        stream_id: &str,
    ) -> impl core::future::Future<Output = Result<(), Error>> + Send {
        let lock_file = self.lock_path(stream_id);
        SendWrapper::new(async move {
            let _ = node_unlink(&lock_file).await;
            Ok(())
        })
    }
}
