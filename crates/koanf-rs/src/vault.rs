//! HashiCorp Vault KV configuration source.
//!
//! The source reads a secret through Vault's KV v1 or v2 HTTP API. The secret
//! payload is used directly as the root configuration object. Watching uses
//! polling (default 30 seconds).

use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use koanf_core::{
    Error, Map, Result, Source, Value, WatchCallback, WatchSignal, WatchSubscription, Watchable,
};
use reqwest::blocking::Client;
use reqwest::{StatusCode, Url};

/// Vault KV API version.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KvVersion {
    /// KV version 1.
    V1,
    /// KV version 2. This is the default.
    #[default]
    V2,
}

/// A HashiCorp Vault KV configuration source.
///
/// ```no_run
/// use koanf_rs::vault::{KvVersion, Vault};
/// use koanf_rs::prelude::*;
///
/// let source = Vault::new("http://127.0.0.1:8200", "secret", "app/config")
///     .unwrap()
///     .kv_version(KvVersion::V2)
///     .token("hvs.secret");
///
/// let config = Config::builder().watched(source).build().unwrap();
/// ```
#[derive(Clone)]
pub struct Vault {
    client: Client,
    base: Url,
    mount: String,
    path: String,
    name: String,
    version: KvVersion,
    token: Option<String>,
    namespace: Option<String>,
    optional: bool,
    timeout: Option<Duration>,
    poll_interval: Duration,
    cache: Arc<Mutex<Option<Map>>>,
}

impl Vault {
    /// Creates a source for `mount/path` under `address`.
    ///
    /// Returns an error when the address, mount, or path is invalid. Mount and
    /// path are normalized by trimming surrounding `/`.
    pub fn new(
        address: impl AsRef<str>,
        mount: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<Self> {
        let mut base =
            Url::parse(address.as_ref()).map_err(|error| Error::source_error("vault", error))?;
        base.set_query(None);
        base.set_fragment(None);
        let mount = normalize_part("mount", mount.into())?;
        let path = normalize_part("path", path.into())?;
        let name = format!("vault:{base}:{mount}/{path}");
        let client = Client::builder()
            .user_agent(concat!("koanf-rs/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Error::source_error("vault", error))?;
        Ok(Self {
            client,
            base,
            mount,
            path,
            name,
            version: KvVersion::V2,
            token: None,
            namespace: None,
            optional: false,
            timeout: Some(Duration::from_secs(30)),
            poll_interval: Duration::from_secs(30),
            cache: Arc::new(Mutex::new(None)),
        })
    }

    /// Selects the KV API version.
    pub fn kv_version(mut self, version: KvVersion) -> Self {
        self.version = version;
        self
    }

    /// Selects the KV version 1 API.
    pub fn kv_v1(self) -> Self {
        self.kv_version(KvVersion::V1)
    }

    /// Sets the Vault token sent as `X-Vault-Token`.
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Sets the Vault namespace sent as `X-Vault-Namespace`.
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Treats a missing secret (HTTP 404) as an empty layer.
    ///
    /// Other HTTP errors still fail.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Sets the per-request timeout. The default is 30 seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Sets the polling interval used by watch subscriptions.
    ///
    /// The default is 30 seconds; values below 10 ms are raised to 10 ms.
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval.max(Duration::from_millis(10));
        self
    }

    /// Sets a custom source name used in error messages.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    fn request_url(&self) -> Result<Url> {
        let mut url = self.base.clone();
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                Error::source_error(
                    self.name.clone(),
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Vault URL cannot be a base URL",
                    ),
                )
            })?;
            segments.pop_if_empty();
            segments.push("v1");
            for segment in self.mount.split('/').filter(|segment| !segment.is_empty()) {
                segments.push(segment);
            }
            if self.version == KvVersion::V2 {
                segments.push("data");
            }
            for segment in self.path.split('/').filter(|segment| !segment.is_empty()) {
                segments.push(segment);
            }
        }
        Ok(url)
    }

    fn fetch(&self) -> Result<Map> {
        let mut request = self.client.get(self.request_url()?);
        if let Some(timeout) = self.timeout {
            request = request.timeout(timeout);
        }
        if let Some(token) = &self.token {
            request = request.header("X-Vault-Token", token);
        }
        if let Some(namespace) = &self.namespace {
            request = request.header("X-Vault-Namespace", namespace);
        }

        let response = request
            .send()
            .map_err(|error| Error::source_error(self.name.clone(), error))?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            if self.optional {
                let map = Map::new();
                self.store_cache(&map);
                return Ok(map);
            }
            return Err(Error::source_error(
                self.name.clone(),
                io::Error::other("Vault secret not found"),
            ));
        }
        if !status.is_success() {
            return Err(Error::source_error(
                self.name.clone(),
                io::Error::other(format!("Vault request failed with status {status}")),
            ));
        }

        let body = response
            .bytes()
            .map_err(|error| Error::source_error(self.name.clone(), error))?;
        let root: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|error| Error::source_error(self.name.clone(), error))?;
        let secret = match self.version {
            KvVersion::V1 => root.get("data"),
            KvVersion::V2 => root.get("data").and_then(|data| data.get("data")),
        }
        .ok_or_else(|| {
            Error::source_error(
                self.name.clone(),
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Vault response has no secret data",
                ),
            )
        })?;

        let value: Value = serde_json::from_value(secret.clone())
            .map_err(|error| Error::source_error(self.name.clone(), error))?;
        let actual = value.type_name();
        match value {
            Value::Map(map) => {
                self.store_cache(&map);
                Ok(map)
            }
            _ => Err(Error::InvalidRoot {
                source: self.name.clone(),
                actual,
            }),
        }
    }

    fn store_cache(&self, map: &Map) {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *cache = Some(map.clone());
    }

    fn cached(&self) -> Option<Map> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

fn normalize_part(part: &str, value: String) -> Result<String> {
    let value = value.trim_matches('/').to_owned();
    if value.is_empty() {
        return Err(Error::source_error(
            "vault",
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Vault {part} must not be empty"),
            ),
        ));
    }
    Ok(value)
}

impl Source for Vault {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        self.fetch()
    }
}

impl Watchable for Vault {
    fn watch(&self, callback: WatchCallback) -> Result<Box<dyn WatchSubscription>> {
        let source = self.clone();
        let interval = self.poll_interval;
        let (stop, stop_receiver) = mpsc::channel::<()>();
        let handle = thread::spawn(move || {
            let mut last = source.cached();
            if last.is_none() {
                last = source.read().ok();
            }
            loop {
                match stop_receiver.recv_timeout(interval) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                match source.read() {
                    Ok(map) => {
                        if last.as_ref() != Some(&map) {
                            last = Some(map);
                            callback(WatchSignal::Changed);
                        }
                    }
                    Err(error) => callback(WatchSignal::Error(error)),
                }
            }
        });
        Ok(Box::new(VaultWatchSubscription {
            stop: Some(stop),
            handle: Mutex::new(Some(handle)),
        }))
    }
}

struct VaultWatchSubscription {
    stop: Option<mpsc::Sender<()>>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl WatchSubscription for VaultWatchSubscription {
    fn stop(self: Box<Self>) {
        if let Some(stop) = &self.stop {
            let _ = stop.send(());
        }
        if let Some(handle) = self
            .handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = handle.join();
        }
    }
}
