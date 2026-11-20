//! HashiCorp Consul KV configuration source.
//!
//! The source reads a key prefix through Consul's HTTP KV API, decodes base64
//! values, and maps slash-separated keys into nested configuration objects.
//! Watching uses polling (default 30 seconds).

use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use koanf_core::{
    Error, Map, Path, Result, Source, Value, WatchCallback, WatchSignal, WatchSubscription,
    Watchable,
};
use reqwest::blocking::Client;
use reqwest::{StatusCode, Url};
use serde::Deserialize;

use crate::util::insert_path;

/// A Consul KV configuration source.
///
/// Keys below `prefix` are split on `/` to create nested paths. Values are
/// decoded as UTF-8 strings, so typed reads use the usual lenient coercion.
///
/// ```no_run
/// use koanf_rs::consul::Consul;
/// use koanf_rs::prelude::*;
/// use std::time::Duration;
///
/// let source = Consul::new("http://127.0.0.1:8500", "config/app")
///     .unwrap()
///     .token("secret")
///     .poll_interval(Duration::from_secs(15));
///
/// let config = Config::builder().watched(source).build().unwrap();
/// ```
#[derive(Clone)]
pub struct Consul {
    client: Client,
    base: Url,
    prefix: String,
    name: String,
    token: Option<String>,
    namespace: Option<String>,
    datacenter: Option<String>,
    timeout: Option<Duration>,
    poll_interval: Duration,
    cache: Arc<Mutex<Option<Map>>>,
}

impl Consul {
    /// Creates a source for the KV prefix under `base_url`.
    pub fn new(base_url: impl AsRef<str>, prefix: impl Into<String>) -> Result<Self> {
        let mut base =
            Url::parse(base_url.as_ref()).map_err(|error| Error::source_error("consul", error))?;
        base.set_query(None);
        base.set_fragment(None);
        let prefix = prefix.into().trim_matches('/').to_owned();
        let name = if prefix.is_empty() {
            format!("consul:{base}")
        } else {
            format!("consul:{base}:{prefix}")
        };
        let client = Client::builder()
            .user_agent(concat!("koanf-rs/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Error::source_error("consul", error))?;
        Ok(Self {
            client,
            base,
            prefix,
            name,
            token: None,
            namespace: None,
            datacenter: None,
            timeout: Some(Duration::from_secs(30)),
            poll_interval: Duration::from_secs(30),
            cache: Arc::new(Mutex::new(None)),
        })
    }

    /// Sets the Consul ACL token sent as `X-Consul-Token`.
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Sets the Consul Enterprise namespace sent as `X-Consul-Namespace`.
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Sets the Consul datacenter query parameter.
    pub fn datacenter(mut self, datacenter: impl Into<String>) -> Self {
        self.datacenter = Some(datacenter.into());
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

    fn kv_url(&self) -> Result<Url> {
        let mut url = self.base.clone();
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                Error::source_error(
                    self.name.clone(),
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Consul URL cannot be a base URL",
                    ),
                )
            })?;
            segments.pop_if_empty();
            segments.push("v1");
            segments.push("kv");
            for segment in self.prefix.split('/').filter(|segment| !segment.is_empty()) {
                segments.push(segment);
            }
        }
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("recurse", "");
            if let Some(datacenter) = &self.datacenter {
                query.append_pair("dc", datacenter);
            }
        }
        Ok(url)
    }

    fn fetch(&self) -> Result<Map> {
        let mut request = self.client.get(self.kv_url()?);
        if let Some(timeout) = self.timeout {
            request = request.timeout(timeout);
        }
        if let Some(token) = &self.token {
            request = request.header("X-Consul-Token", token);
        }
        if let Some(namespace) = &self.namespace {
            request = request.header("X-Consul-Namespace", namespace);
        }

        let response = request
            .send()
            .map_err(|error| Error::source_error(self.name.clone(), error))?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            let map = Map::new();
            self.store_cache(&map);
            return Ok(map);
        }
        if !status.is_success() {
            return Err(Error::source_error(
                self.name.clone(),
                io::Error::other(format!("Consul request failed with status {status}")),
            ));
        }

        let body = response
            .bytes()
            .map_err(|error| Error::source_error(self.name.clone(), error))?;
        let entries = serde_json::from_slice::<Option<Vec<ConsulEntry>>>(&body)
            .map_err(|error| Error::source_error(self.name.clone(), error))?
            .unwrap_or_default();
        let map = self.entries_to_map(entries)?;
        self.store_cache(&map);
        Ok(map)
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

    fn entries_to_map(&self, mut entries: Vec<ConsulEntry>) -> Result<Map> {
        entries.sort_by(|left, right| left.key.cmp(&right.key));
        let mut map = Map::new();
        for entry in entries {
            let Some(encoded) = entry.value else {
                continue;
            };
            let relative = if self.prefix.is_empty() {
                entry.key.trim_start_matches('/')
            } else {
                match entry
                    .key
                    .strip_prefix(&self.prefix)
                    .and_then(|rest| rest.strip_prefix('/'))
                {
                    Some(relative) => relative,
                    None => continue,
                }
            };
            if relative.is_empty() {
                continue;
            }
            let bytes = BASE64
                .decode(encoded)
                .map_err(|error| Error::source_error(self.name.clone(), error))?;
            let value = String::from_utf8(bytes)
                .map_err(|error| Error::source_error(self.name.clone(), error))?;
            let segments = relative
                .split('/')
                .filter(|segment| !segment.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            if segments.is_empty() {
                continue;
            }
            insert_path(
                &mut map,
                &Path::new(segments),
                Value::String(value),
                &self.name,
            )?;
        }
        Ok(map)
    }
}

impl Source for Consul {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        self.fetch()
    }
}

impl Watchable for Consul {
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
        Ok(Box::new(ConsulWatchSubscription {
            stop: Some(stop),
            handle: Mutex::new(Some(handle)),
        }))
    }
}

#[derive(Deserialize)]
struct ConsulEntry {
    #[serde(rename = "Key")]
    key: String,
    #[serde(rename = "Value")]
    value: Option<String>,
}

struct ConsulWatchSubscription {
    stop: Option<mpsc::Sender<()>>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl WatchSubscription for ConsulWatchSubscription {
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
