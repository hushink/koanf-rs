//! HTTP(S) configuration source with conditional requests and polling watch.

use std::path::{Path as StdPath, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use koanf_core::{
    Error, Format, Map, Result, Source, Value, WatchCallback, WatchSignal, WatchSubscription,
    Watchable,
};
use reqwest::blocking::Client;
use reqwest::header::{
    CONTENT_TYPE, ETAG, HeaderMap, HeaderName, HeaderValue, IF_MODIFIED_SINCE, IF_NONE_MATCH,
    LAST_MODIFIED,
};
use reqwest::{StatusCode, Url};
use sha2::{Digest, Sha256};

/// An HTTP(S) configuration source.
///
/// The source performs one request at construction/load time, then uses
/// `ETag` / `Last-Modified` conditional requests and a content hash on reloads.
/// Watching is implemented by polling at a configurable interval (default 30
/// seconds); no file watcher is involved.
///
/// ```no_run
/// use koanf_rs::http::Http;
/// use koanf_rs::prelude::*;
/// use std::time::Duration;
///
/// let source = Http::new("https://example.com/config.json")
///     .unwrap()
///     .poll_interval(Duration::from_secs(15));
///
/// let config = Config::builder().watched(source).build().unwrap();
/// ```
#[derive(Clone)]
pub struct Http {
    client: Client,
    url: Url,
    name: String,
    format: Option<Arc<dyn Format>>,
    headers: HeaderMap,
    bearer_token: Option<String>,
    timeout: Option<Duration>,
    poll_interval: Duration,
    cache: Arc<Mutex<Cache>>,
    fetch_lock: Arc<Mutex<()>>,
}

#[derive(Default)]
struct Cache {
    etag: Option<String>,
    last_modified: Option<String>,
    hash: Option<[u8; 32]>,
    map: Option<Map>,
}

impl Http {
    /// Creates a source for `url`.
    ///
    /// The format is inferred from `Content-Type`, then from the URL extension,
    /// unless [`Self::format`] is used.
    pub fn new(url: impl AsRef<str>) -> Result<Self> {
        let url = Url::parse(url.as_ref()).map_err(|error| Error::source_error("http", error))?;
        let client = Client::builder()
            .user_agent(concat!("koanf-rs/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|error| Error::source_error("http", error))?;
        let name = format!("http:{url}");
        Ok(Self {
            client,
            url,
            name,
            format: None,
            headers: HeaderMap::new(),
            bearer_token: None,
            timeout: Some(Duration::from_secs(30)),
            poll_interval: Duration::from_secs(30),
            cache: Arc::new(Mutex::new(Cache::default())),
            fetch_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Sets an explicit format, overriding content-type/extension inference.
    pub fn format<F>(mut self, format: F) -> Self
    where
        F: Format + 'static,
    {
        self.format = Some(Arc::new(format));
        self
    }

    /// Adds a request header.
    pub fn header(mut self, name: impl AsRef<str>, value: impl AsRef<str>) -> Result<Self> {
        let name = HeaderName::from_bytes(name.as_ref().as_bytes())
            .map_err(|error| Error::source_error("http", error))?;
        let value = HeaderValue::from_str(value.as_ref())
            .map_err(|error| Error::source_error("http", error))?;
        self.headers.append(name, value);
        Ok(self)
    }

    /// Adds a bearer token for the `Authorization` header.
    pub fn bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
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

    fn fetch(&self) -> Result<(Map, bool)> {
        let _fetch_guard = self
            .fetch_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (etag, last_modified, old_hash, cached_map) = {
            let cache = self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                cache.etag.clone(),
                cache.last_modified.clone(),
                cache.hash,
                cache.map.clone(),
            )
        };

        let mut request = self
            .client
            .get(self.url.clone())
            .headers(self.headers.clone());
        if let Some(token) = &self.bearer_token {
            request = request.bearer_auth(token.as_str());
        }
        if let Some(timeout) = self.timeout {
            request = request.timeout(timeout);
        }
        if let Some(value) = etag
            .as_deref()
            .and_then(|etag| HeaderValue::from_str(etag).ok())
        {
            request = request.header(IF_NONE_MATCH, value);
        }
        if let Some(value) = last_modified
            .as_deref()
            .and_then(|last_modified| HeaderValue::from_str(last_modified).ok())
        {
            request = request.header(IF_MODIFIED_SINCE, value);
        }

        let response = request
            .send()
            .map_err(|error| Error::source_error(self.name.clone(), error))?;
        let status = response.status();

        if status == StatusCode::NOT_MODIFIED {
            return cached_map.map(|map| (map, false)).ok_or_else(|| {
                Error::source_error(
                    self.name.clone(),
                    std::io::Error::other("HTTP 304 received without a cached response"),
                )
            });
        }
        if !status.is_success() {
            return Err(Error::source_error(
                self.name.clone(),
                std::io::Error::other(format!("HTTP request failed with status {status}")),
            ));
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let new_etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let new_last_modified = response
            .headers()
            .get(LAST_MODIFIED)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response
            .bytes()
            .map_err(|error| Error::source_error(self.name.clone(), error))?;
        let hash: [u8; 32] = Sha256::digest(&body).into();

        if old_hash == Some(hash)
            && let Some(map) = cached_map.as_ref()
        {
            self.update_metadata(new_etag, new_last_modified, hash);
            return Ok((map.clone(), false));
        }

        let changed = old_hash != Some(hash) || cached_map.is_none();
        let map = self.parse_body(&body, content_type.as_deref())?;
        {
            let mut cache = self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            cache.etag = new_etag;
            cache.last_modified = new_last_modified;
            cache.hash = Some(hash);
            cache.map = Some(map.clone());
        }
        Ok((map, changed))
    }

    fn update_metadata(&self, etag: Option<String>, last_modified: Option<String>, hash: [u8; 32]) {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.etag = etag;
        cache.last_modified = last_modified;
        cache.hash = Some(hash);
    }

    fn parse_body(&self, body: &[u8], content_type: Option<&str>) -> Result<Map> {
        let format = self.format_for(content_type)?;
        let value = match format.parse(body) {
            Ok(value) => value,
            Err(Error::Format {
                format,
                path: None,
                source,
            }) => {
                return Err(Error::Format {
                    format,
                    path: Some(PathBuf::from(self.url.as_str())),
                    source,
                });
            }
            Err(error) => return Err(error),
        };
        let actual = value.type_name();
        match value {
            Value::Map(map) => Ok(map),
            _ => Err(Error::InvalidRoot {
                source: self.name.clone(),
                actual,
            }),
        }
    }

    fn format_for(&self, content_type: Option<&str>) -> Result<Arc<dyn Format>> {
        if let Some(format) = &self.format {
            return Ok(Arc::clone(format));
        }

        if let Some(content_type) = content_type {
            let media_type = content_type
                .split(';')
                .next()
                .unwrap_or(content_type)
                .trim()
                .to_ascii_lowercase();
            if media_type == "application/json"
                || media_type == "text/json"
                || media_type.ends_with("+json")
            {
                return json_format();
            }
            if media_type == "application/toml"
                || media_type == "text/toml"
                || media_type.ends_with("+toml")
            {
                return toml_format();
            }
            if matches!(
                media_type.as_str(),
                "application/yaml" | "application/x-yaml" | "text/yaml" | "text/x-yaml"
            ) || media_type.ends_with("+yaml")
            {
                return yaml_format();
            }
        }

        let extension = StdPath::new(self.url.path())
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .unwrap_or_default();
        match extension.as_str() {
            "json" => json_format(),
            "toml" => toml_format(),
            "yaml" | "yml" => yaml_format(),
            _ => Err(Error::UnknownFormat {
                path: PathBuf::from(self.url.as_str()),
                extension,
            }),
        }
    }
}

#[cfg(feature = "json")]
fn json_format() -> Result<Arc<dyn Format>> {
    Ok(Arc::new(crate::format::Json))
}

#[cfg(not(feature = "json"))]
fn json_format() -> Result<Arc<dyn Format>> {
    Err(Error::Unsupported {
        format: "json".to_owned(),
        operation: "format feature not enabled",
    })
}

#[cfg(feature = "toml")]
fn toml_format() -> Result<Arc<dyn Format>> {
    Ok(Arc::new(crate::format::Toml))
}

#[cfg(not(feature = "toml"))]
fn toml_format() -> Result<Arc<dyn Format>> {
    Err(Error::Unsupported {
        format: "toml".to_owned(),
        operation: "format feature not enabled",
    })
}

#[cfg(feature = "yaml")]
fn yaml_format() -> Result<Arc<dyn Format>> {
    Ok(Arc::new(crate::format::Yaml))
}

#[cfg(not(feature = "yaml"))]
fn yaml_format() -> Result<Arc<dyn Format>> {
    Err(Error::Unsupported {
        format: "yaml".to_owned(),
        operation: "format feature not enabled",
    })
}

impl Source for Http {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        self.fetch().map(|(map, _)| map)
    }
}

impl Watchable for Http {
    fn watch(&self, callback: WatchCallback) -> Result<Box<dyn WatchSubscription>> {
        let this = self.clone();
        let (stop, stop_receiver) = mpsc::channel::<()>();
        let handle = thread::spawn(move || {
            loop {
                match stop_receiver.recv_timeout(this.poll_interval) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                match this.fetch() {
                    Ok((_map, true)) => callback(WatchSignal::Changed),
                    Ok((_map, false)) => {}
                    Err(error) => callback(WatchSignal::Error(error)),
                }
            }
        });
        Ok(Box::new(HttpWatchSubscription {
            stop: Some(stop),
            handle: Mutex::new(Some(handle)),
        }))
    }
}

struct HttpWatchSubscription {
    stop: Option<mpsc::Sender<()>>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl WatchSubscription for HttpWatchSubscription {
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
