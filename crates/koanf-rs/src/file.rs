//! File configuration source.

use std::fs;
use std::io;
use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;

use koanf_core::{Error, Format, Map, Result, Source, Value};

use crate::util::strip_bom;

#[cfg(feature = "watch")]
use std::sync::Mutex;
#[cfg(feature = "watch")]
use std::time::Duration;

#[cfg(feature = "watch")]
use koanf_core::{WatchCallback, WatchSignal, WatchSubscription, Watchable};

/// A single-file configuration source.
///
/// The format is inferred from the extension (`.json`, `.toml`, `.yaml`,
/// `.yml`, `.ini`, or `.ron`) or from a `.env` filename, when the corresponding
/// format feature is enabled. Use [`File::format`] to override inference. A missing
/// file is an error unless [`File::optional`] is used.
#[derive(Clone)]
pub struct File {
    path: PathBuf,
    name: String,
    format: Option<Arc<dyn Format>>,
    optional: bool,
    #[cfg(feature = "watch")]
    debounce: Duration,
}

impl File {
    /// Creates a file source.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            name: format!("file:{}", path.display()),
            path,
            format: None,
            optional: false,
            #[cfg(feature = "watch")]
            debounce: Duration::from_millis(100),
        }
    }

    /// Sets a custom source name used by `Config::source_names` and
    /// `Config::remove_source`.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets an explicit format, overriding extension inference.
    pub fn format<F>(mut self, format: F) -> Self
    where
        F: Format + 'static,
    {
        self.format = Some(Arc::new(format));
        self
    }

    /// Treats a missing file as an empty layer.
    ///
    /// Other I/O errors still fail.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Sets the file-watch debounce duration. The default is 100 ms.
    #[cfg(feature = "watch")]
    pub fn debounce(mut self, duration: Duration) -> Self {
        self.debounce = duration;
        self
    }
}

pub(crate) fn known_extension(path: &StdPath) -> bool {
    if is_dotenv_path(path) {
        return cfg!(feature = "dotenv");
    }
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("json") if cfg!(feature = "json") => true,
        Some("toml") if cfg!(feature = "toml") => true,
        Some("yaml" | "yml") if cfg!(feature = "yaml") => true,
        Some("ini") if cfg!(feature = "ini") => true,
        Some("ron") if cfg!(feature = "ron") => true,
        _ => false,
    }
}

fn is_dotenv_path(path: &StdPath) -> bool {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(".env"))
    {
        return true;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("env"))
}

pub(crate) fn format_for_path(
    path: &StdPath,
    explicit: Option<&Arc<dyn Format>>,
) -> Result<Arc<dyn Format>> {
    if let Some(format) = explicit {
        return Ok(Arc::clone(format));
    }
    if is_dotenv_path(path) {
        return dotenv_format();
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "json" => json_format(),
        "toml" => toml_format(),
        "yaml" | "yml" => yaml_format(),
        "ini" => ini_format(),
        "ron" => ron_format(),
        _ => Err(Error::UnknownFormat {
            path: path.to_path_buf(),
            extension,
        }),
    }
}

pub(crate) fn read_file(
    path: &StdPath,
    explicit_format: Option<&Arc<dyn Format>>,
    optional: bool,
) -> Result<Map> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == io::ErrorKind::NotFound && optional => {
            return Ok(Map::new());
        }
        Err(error) => {
            return Err(Error::Io {
                path: path.to_path_buf(),
                source: error,
            });
        }
    };
    let data = strip_bom(&data);
    let format = format_for_path(path, explicit_format)?;
    let value = match format.parse(data) {
        Ok(value) => value,
        Err(Error::Format {
            format,
            path: None,
            source,
        }) => {
            return Err(Error::Format {
                format,
                path: Some(path.to_path_buf()),
                source,
            });
        }
        Err(error) => return Err(error),
    };
    let actual = value.type_name();
    match value {
        Value::Map(map) => Ok(map),
        _ => Err(Error::InvalidRoot {
            source: format!("file:{}", path.display()),
            actual,
        }),
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

#[cfg(feature = "dotenv")]
fn dotenv_format() -> Result<Arc<dyn Format>> {
    Ok(Arc::new(crate::format::Dotenv))
}

#[cfg(not(feature = "dotenv"))]
fn dotenv_format() -> Result<Arc<dyn Format>> {
    Err(Error::Unsupported {
        format: "dotenv".to_owned(),
        operation: "format feature not enabled",
    })
}

#[cfg(feature = "ini")]
fn ini_format() -> Result<Arc<dyn Format>> {
    Ok(Arc::new(crate::format::Ini))
}

#[cfg(not(feature = "ini"))]
fn ini_format() -> Result<Arc<dyn Format>> {
    Err(Error::Unsupported {
        format: "ini".to_owned(),
        operation: "format feature not enabled",
    })
}

#[cfg(feature = "ron")]
fn ron_format() -> Result<Arc<dyn Format>> {
    Ok(Arc::new(crate::format::Ron))
}

#[cfg(not(feature = "ron"))]
fn ron_format() -> Result<Arc<dyn Format>> {
    Err(Error::Unsupported {
        format: "ron".to_owned(),
        operation: "format feature not enabled",
    })
}

impl Source for File {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        read_file(&self.path, self.format.as_ref(), self.optional)
    }
}

#[cfg(feature = "watch")]
pub(crate) fn watch_error(message: impl Into<String>, error: notify::Error) -> Error {
    Error::Watch {
        message: message.into(),
        source: Some(Box::new(error)),
    }
}

#[cfg(feature = "watch")]
pub(crate) type NotifyDebouncer = notify_debouncer_full::Debouncer<
    notify::RecommendedWatcher,
    notify_debouncer_full::RecommendedCache,
>;

#[cfg(feature = "watch")]
pub(crate) struct NotifyWatchSubscription {
    debouncer: Mutex<Option<NotifyDebouncer>>,
}

#[cfg(feature = "watch")]
impl NotifyWatchSubscription {
    pub(crate) fn new(debouncer: NotifyDebouncer) -> Self {
        Self {
            debouncer: Mutex::new(Some(debouncer)),
        }
    }
}

#[cfg(feature = "watch")]
impl WatchSubscription for NotifyWatchSubscription {
    fn stop(self: Box<Self>) {
        let mut guard = self
            .debouncer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(debouncer) = guard.take() {
            debouncer.stop();
        }
    }
}

#[cfg(feature = "watch")]
impl Watchable for File {
    fn watch(&self, callback: WatchCallback) -> Result<Box<dyn WatchSubscription>> {
        let watch_root = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(|parent| parent.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let target = self.path.clone();
        let target_name = target.file_name().map(|name| name.to_os_string());
        let callback_for_events = Arc::clone(&callback);

        let mut debouncer = notify_debouncer_full::new_debouncer(
            self.debounce,
            None,
            move |result: notify_debouncer_full::DebounceEventResult| match result {
                Ok(events) => {
                    let relevant = match &target_name {
                        Some(name) => events.iter().any(|event| {
                            event.event.paths.iter().any(|path| {
                                path == &target || path.file_name() == Some(name.as_os_str())
                            })
                        }),
                        None => !events.is_empty(),
                    };
                    if relevant {
                        callback_for_events(WatchSignal::Changed);
                    }
                }
                Err(errors) => {
                    if let Some(error) = errors.into_iter().next() {
                        callback_for_events(WatchSignal::Error(watch_error(
                            format!("file watcher for `{}` failed", target.display()),
                            error,
                        )));
                    }
                }
            },
        )
        .map_err(|error| {
            watch_error(
                format!("failed to start watcher for `{}`", self.path.display()),
                error,
            )
        })?;

        debouncer
            .watch(&watch_root, notify::RecursiveMode::NonRecursive)
            .map_err(|error| {
                watch_error(format!("failed to watch `{}`", watch_root.display()), error)
            })?;

        Ok(Box::new(NotifyWatchSubscription::new(debouncer)))
    }
}
