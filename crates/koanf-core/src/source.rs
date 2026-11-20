//! Extension traits for configuration sources, formats, and watchers.

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::value::{Map, Value};

/// A configuration source that reads a root object.
///
/// Sources are immutable after construction and must be safe to share across
/// threads. A source is re-read on every `reload` and watch-triggered rebuild.
pub trait Source: Send + Sync {
    /// A short name used in error messages, for example `file:app.toml`.
    fn name(&self) -> &str;

    /// Reads the source and returns its root object.
    fn read(&self) -> Result<Map>;
}

/// A configuration format that parses bytes into a [`Value`].
pub trait Format: Send + Sync {
    /// A short format name, for example `"json"`.
    fn name(&self) -> &str;

    /// Parses bytes into a value. The caller is responsible for requiring the
    /// root to be an object.
    fn parse(&self, bytes: &[u8]) -> Result<Value>;

    /// Renders a value back to bytes.
    ///
    /// The default returns [`Error::Unsupported`]. The built-in JSON, TOML,
    /// YAML, and RON formats implement rendering; other formats may opt in. Use
    /// [`crate::Snapshot::render`] to render a complete configuration.
    fn render(&self, _value: &Value) -> Result<Vec<u8>> {
        Err(Error::Unsupported {
            format: self.name().to_owned(),
            operation: "render",
        })
    }
}

/// A signal emitted by a watched source.
#[derive(Debug)]
pub enum WatchSignal {
    /// The source changed and should be re-read.
    Changed,
    /// The watch backend reported an error.
    Error(Error),
}

/// Callback type used by [`Watchable`] implementations.
///
/// Implementations should invoke the callback without blocking; the library's
/// callback only forwards the signal to its dispatcher thread.
pub type WatchCallback = Arc<dyn Fn(WatchSignal) + Send + Sync>;

/// A source that can notify the configuration when its underlying data changes.
///
/// Only sources registered through `ConfigBuilder::watched` or
/// `Config::load_watched` participate in watching.
pub trait Watchable: Source {
    /// Starts watching. The returned subscription must stop watching when
    /// dropped or when [`WatchSubscription::stop`] is called.
    fn watch(&self, callback: WatchCallback) -> Result<Box<dyn WatchSubscription>>;
}

/// A handle to an active source watcher.
pub trait WatchSubscription: Send + Sync {
    /// Stops the watcher and releases its resources.
    ///
    /// Implementations must not wait on the configuration dispatcher or on a
    /// user callback thread; this is called while subscription state is being
    /// updated.
    fn stop(self: Box<Self>);
}
