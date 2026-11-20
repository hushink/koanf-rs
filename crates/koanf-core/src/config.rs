//! Layered configuration, immutable snapshots, sections, and watch dispatch.

use std::fmt;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path as StdPath;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};
use std::thread;

use serde::de::DeserializeOwned;

use crate::de;
use crate::diff::ChangeSet;
use crate::error::{Error, Result};
use crate::merge::{MergeOptions, merge_map};
use crate::path::{IntoPath, Path};
use crate::source::{
    AsyncSource, Format, Source, WatchCallback, WatchSignal, WatchSubscription, Watchable,
};
use crate::value::{Coercion, Map, Value};
use crate::watch::{WatchEvent, WatchGuard};

#[cfg(feature = "async")]
use crate::watch::WatchStream;

#[cfg(feature = "tracing")]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        tracing::debug!($($arg)*);
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! log_debug {
    ($($arg:tt)*) => {};
}

#[cfg(feature = "tracing")]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        tracing::warn!($($arg)*);
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! log_warn {
    ($($arg:tt)*) => {};
}

#[derive(Clone)]
struct Layer {
    source: Arc<dyn Source>,
    watchable: Option<Arc<dyn Watchable>>,
}

impl Layer {
    fn new<S>(source: S) -> Self
    where
        S: Source + 'static,
    {
        Self {
            source: Arc::new(source),
            watchable: None,
        }
    }

    fn watched<W>(source: W) -> Self
    where
        W: Watchable + 'static,
    {
        let source = Arc::new(source);
        let source_dyn: Arc<dyn Source> = source.clone();
        let watchable: Arc<dyn Watchable> = source;
        Self {
            source: source_dyn,
            watchable: Some(watchable),
        }
    }
}

struct PreloadedSource {
    name: String,
    map: Map,
}

impl Source for PreloadedSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        Ok(self.map.clone())
    }
}

pub(crate) struct SnapshotInner {
    value: Value,
    coercion: Coercion,
    redactions: Arc<[Path]>,
}

/// An immutable view of a configuration.
///
/// Cloning a snapshot is cheap (an `Arc` clone). Use a snapshot when multiple
/// values must be read consistently.
///
/// `Debug` output redacts paths configured through
/// [`ConfigBuilder::redact`]. Reads, rendering, and saving still return the
/// original values.
#[derive(Clone)]
pub struct Snapshot {
    inner: Arc<SnapshotInner>,
}

impl Snapshot {
    pub(crate) fn from_parts(value: Value, coercion: Coercion, redactions: Arc<[Path]>) -> Self {
        Self {
            inner: Arc::new(SnapshotInner {
                value,
                coercion,
                redactions,
            }),
        }
    }

    pub(crate) fn redactions(&self) -> Arc<[Path]> {
        Arc::clone(&self.inner.redactions)
    }

    fn resolve<'a>(&'a self, path: &Path) -> Result<&'a Value> {
        let mut current = &self.inner.value;
        let mut traversed = Path::root();
        for segment in path.segments() {
            match current {
                Value::Map(map) => match map.get(segment) {
                    Some(value) => {
                        current = value;
                        traversed.push(segment.clone());
                    }
                    None => {
                        return Err(Error::PathNotFound { path: path.clone() });
                    }
                },
                Value::Array(items) => {
                    let index = segment.parse::<usize>().map_err(|_| Error::InvalidIndex {
                        path: traversed.child(segment.clone()),
                        segment: segment.clone(),
                    })?;
                    match items.get(index) {
                        Some(value) => {
                            current = value;
                            traversed.push(segment.clone());
                        }
                        None => {
                            return Err(Error::PathNotFound { path: path.clone() });
                        }
                    }
                }
                other => {
                    return Err(Error::NotAMap {
                        path: traversed,
                        actual: other.type_name(),
                    });
                }
            }
        }
        Ok(current)
    }

    /// Reads and deserializes the value at `path`.
    ///
    /// A missing path is an error. An explicit `null` is a value and is
    /// deserialized normally; use `Option<T>` to accept it. Numeric segments
    /// index arrays when traversal reaches one; out-of-bounds indices count as
    /// missing.
    pub fn get<T>(&self, path: impl IntoPath) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let path = path.into_path();
        let value = self.resolve(&path)?;
        de::deserialize_value(value, self.inner.coercion, path)
    }

    /// Reads the value at `path`, returning `None` only when the path is
    /// missing.
    ///
    /// An explicit `null` is still deserialized; `get_opt::<Option<T>>` yields
    /// `Some(None)` for a present null.
    pub fn get_opt<T>(&self, path: impl IntoPath) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let path = path.into_path();
        match self.resolve(&path) {
            Ok(value) => de::deserialize_value(value, self.inner.coercion, path).map(Some),
            Err(Error::PathNotFound { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Reads the value at `path`, returning `default` only when the path is
    /// missing.
    pub fn get_or<T>(&self, path: impl IntoPath, default: T) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let path = path.into_path();
        match self.resolve(&path) {
            Ok(value) => de::deserialize_value(value, self.inner.coercion, path),
            Err(Error::PathNotFound { .. }) => Ok(default),
            Err(error) => Err(error),
        }
    }

    /// Deserializes the whole configuration object.
    pub fn deserialize<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        de::deserialize_value(&self.inner.value, self.inner.coercion, Path::root())
    }

    /// Returns a view of the object at `path`.
    ///
    /// The path must resolve to an object.
    pub fn section(&self, path: impl IntoPath) -> Result<Section> {
        let path = path.into_path();
        let value = self.resolve(&path)?;
        match value {
            Value::Map(_) => Ok(Section {
                snapshot: self.clone(),
                prefix: path,
            }),
            other => Err(Error::NotAMap {
                path,
                actual: other.type_name(),
            }),
        }
    }

    /// Returns the root value.
    pub fn as_value(&self) -> &Value {
        &self.inner.value
    }

    /// Returns the root object.
    pub fn as_map(&self) -> &Map {
        match &self.inner.value {
            Value::Map(map) => map,
            other => unreachable!(
                "configuration snapshot root is always an object, found `{}`",
                other.type_name()
            ),
        }
    }

    /// Returns `true` if `path` resolves to a value.
    pub fn contains(&self, path: impl IntoPath) -> bool {
        self.resolve(&path.into_path()).is_ok()
    }

    /// Renders the root object with `format`.
    ///
    /// Formats may reject values they cannot represent, such as TOML nulls.
    pub fn render(&self, format: &dyn Format) -> Result<Vec<u8>> {
        format.render(&self.inner.value)
    }

    /// Renders the root object and writes it to `path`.
    ///
    /// Rendering completes before the file is touched, so format errors leave
    /// the target unchanged. The write itself uses [`std::fs::write`], which
    /// creates or truncates the target.
    pub fn save(&self, path: impl AsRef<StdPath>, format: &dyn Format) -> Result<()> {
        save_value(path.as_ref(), &self.inner.value, format)
    }

    /// Computes the changes between two snapshots.
    pub fn diff(&self, other: &Snapshot) -> ChangeSet {
        ChangeSet::diff_redacted(
            &self.inner.value,
            &other.inner.value,
            &self.inner.redactions,
        )
    }
}

impl fmt::Debug for Snapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = crate::redact::redact(&self.inner.value, &self.inner.redactions);
        formatter
            .debug_struct("Snapshot")
            .field("value", &value)
            .finish()
    }
}

/// A relative view of an object inside a [`Snapshot`].
///
/// Sections are cheap to create and clone. Paths passed to a section are
/// relative to its prefix.
#[derive(Clone)]
pub struct Section {
    snapshot: Snapshot,
    prefix: Path,
}

impl Section {
    fn full_path(&self, relative: &Path) -> Path {
        self.prefix.join(relative)
    }

    fn resolve<'a>(&'a self, relative: &Path) -> Result<&'a Value> {
        self.snapshot.resolve(&self.full_path(relative))
    }

    /// Returns the section's path prefix.
    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    /// Reads and deserializes a value relative to this section.
    pub fn get<T>(&self, path: impl IntoPath) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let relative = path.into_path();
        let full = self.full_path(&relative);
        let value = self.snapshot.resolve(&full)?;
        de::deserialize_value(value, self.snapshot.inner.coercion, full)
    }

    /// Reads a value relative to this section, returning `None` only when the
    /// path is missing.
    pub fn get_opt<T>(&self, path: impl IntoPath) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let relative = path.into_path();
        let full = self.full_path(&relative);
        match self.snapshot.resolve(&full) {
            Ok(value) => de::deserialize_value(value, self.snapshot.inner.coercion, full).map(Some),
            Err(Error::PathNotFound { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Reads a value relative to this section, returning `default` only when
    /// the path is missing.
    pub fn get_or<T>(&self, path: impl IntoPath, default: T) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let relative = path.into_path();
        let full = self.full_path(&relative);
        match self.snapshot.resolve(&full) {
            Ok(value) => de::deserialize_value(value, self.snapshot.inner.coercion, full),
            Err(Error::PathNotFound { .. }) => Ok(default),
            Err(error) => Err(error),
        }
    }

    /// Deserializes the object at this section's prefix.
    pub fn deserialize<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let value = self.snapshot.resolve(&self.prefix)?;
        de::deserialize_value(value, self.snapshot.inner.coercion, self.prefix.clone())
    }

    /// Returns a nested section.
    pub fn section(&self, path: impl IntoPath) -> Result<Section> {
        let relative = path.into_path();
        let full = self.full_path(&relative);
        let value = self.snapshot.resolve(&full)?;
        match value {
            Value::Map(_) => Ok(Section {
                snapshot: self.snapshot.clone(),
                prefix: full,
            }),
            other => Err(Error::NotAMap {
                path: full,
                actual: other.type_name(),
            }),
        }
    }

    /// Returns the value at this section's prefix.
    pub fn as_value(&self) -> &Value {
        self.snapshot
            .resolve(&self.prefix)
            .expect("section prefix is valid for its immutable snapshot")
    }

    /// Returns the object at this section's prefix.
    pub fn as_map(&self) -> &Map {
        match self.as_value() {
            Value::Map(map) => map,
            other => unreachable!(
                "section prefix always points at an object, found `{}`",
                other.type_name()
            ),
        }
    }

    /// Returns `true` if `path` resolves relative to this section.
    pub fn contains(&self, path: impl IntoPath) -> bool {
        self.resolve(&path.into_path()).is_ok()
    }

    /// Renders this section's object as the root value with `format`.
    pub fn render(&self, format: &dyn Format) -> Result<Vec<u8>> {
        format.render(self.as_value())
    }

    /// Renders this section's object and writes it to `path`.
    ///
    /// Rendering completes before the file is touched, so format errors leave
    /// the target unchanged. The write itself uses [`std::fs::write`], which
    /// creates or truncates the target.
    pub fn save(&self, path: impl AsRef<StdPath>, format: &dyn Format) -> Result<()> {
        save_value(path.as_ref(), self.as_value(), format)
    }
}

impl fmt::Debug for Section {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Section")
            .field("prefix", &self.prefix)
            .field("snapshot", &self.snapshot)
            .finish()
    }
}

type Validator = Arc<dyn Fn(&Snapshot) -> Result<()> + Send + Sync>;

enum BuilderLayer {
    Sync(Layer),
    Async(Arc<dyn AsyncSource>),
}

/// Builder for a [`Config`].
pub struct ConfigBuilder {
    layers: Vec<BuilderLayer>,
    merge: MergeOptions,
    coercion: Coercion,
    validation: Option<Validator>,
    redactions: Vec<Path>,
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            merge: MergeOptions::default(),
            coercion: Coercion::default(),
            validation: None,
            redactions: Vec::new(),
        }
    }

    /// Appends a plain source layer.
    ///
    /// Later layers have higher priority. Use [`Self::watched`] for sources
    /// that should participate in file watching.
    pub fn source<S>(mut self, source: S) -> Self
    where
        S: Source + 'static,
    {
        self.layers.push(BuilderLayer::Sync(Layer::new(source)));
        self
    }

    /// Appends an asynchronous source layer.
    ///
    /// Async layers keep their position in the layer order but are read once
    /// during [`Self::build_async`]. They become static layers and do not
    /// participate in `reload` or watch rebuilding.
    ///
    /// A synchronous [`Self::build`] fails with [`Error::AsyncBuildRequired`]
    /// when async layers are present.
    pub fn source_async<S>(mut self, source: S) -> Self
    where
        S: AsyncSource + 'static,
    {
        self.layers.push(BuilderLayer::Async(Arc::new(source)));
        self
    }

    /// Appends a watchable source layer.
    ///
    /// The source is also a normal load layer; it is watched once
    /// `Config::watch` or `Config::watch_channel` is called.
    ///
    /// Sources that are not [`Watchable`] are rejected at compile time:
    ///
    /// ```compile_fail
    /// use koanf_core::{Config, Map, Result, Source};
    ///
    /// struct NotWatchable;
    ///
    /// impl Source for NotWatchable {
    ///     fn name(&self) -> &str {
    ///         "not-watchable"
    ///     }
    ///
    ///     fn read(&self) -> Result<Map> {
    ///         Ok(Map::new())
    ///     }
    /// }
    ///
    /// let _ = Config::builder().watched(NotWatchable);
    /// ```
    pub fn watched<W>(mut self, source: W) -> Self
    where
        W: Watchable + 'static,
    {
        self.layers.push(BuilderLayer::Sync(Layer::watched(source)));
        self
    }

    /// Sets merge behavior.
    pub fn merge_options(mut self, options: MergeOptions) -> Self {
        self.merge = options;
        self
    }

    /// Sets the scalar coercion policy.
    pub fn coercion(mut self, coercion: Coercion) -> Self {
        self.coercion = coercion;
        self
    }

    /// Marks `path` as sensitive for debug output.
    ///
    /// Values at the path, and values below it, are replaced with `"***"` in
    /// `Debug` output for [`Config`], [`Snapshot`], [`Section`], and
    /// [`ChangeSet`]/[`WatchEvent`] values. Reads, rendering, saving, and
    /// iteration over the original [`Change`](crate::Change) values are
    /// unaffected.
    ///
    /// Errors are not redacted.
    pub fn redact(mut self, path: impl IntoPath) -> Self {
        self.redactions.push(path.into_path());
        self
    }

    /// Installs a validator that runs against every new snapshot.
    ///
    /// Validation runs during construction, `reload`, source loading/removal,
    /// runtime overrides, and watch-triggered rebuilds. A rejected snapshot is
    /// never committed; watch-triggered failures are reported as
    /// [`WatchEvent::Error`] while the last good snapshot remains visible.
    ///
    /// The validator receives an immutable [`Snapshot`] and must not call
    /// mutating methods on the same `Config` (doing so would deadlock).
    ///
    /// ```no_run
    /// use koanf_core::{Config, Error};
    ///
    /// let config = Config::builder()
    ///     .validate(|snapshot| {
    ///         let port: u16 = snapshot.get_or("port", 8080)?;
    ///         if port == 0 {
    ///             return Err(Error::validation("port must be non-zero"));
    ///         }
    ///         Ok(())
    ///     })
    ///     .build()
    ///     .unwrap();
    /// ```
    pub fn validate<F>(mut self, validator: F) -> Self
    where
        F: Fn(&Snapshot) -> Result<()> + Send + Sync + 'static,
    {
        self.validation = Some(Arc::new(validator));
        self
    }

    /// Loads all sources and builds the configuration.
    ///
    /// This is eager and fails if any source cannot be read or parsed, if the
    /// configured validator rejects the result, or if async source layers are
    /// present. Use [`Self::build_async`] for async layers.
    pub fn build(self) -> Result<Config> {
        let ConfigBuilder {
            layers,
            merge,
            coercion,
            validation,
            redactions,
        } = self;
        let mut sync_layers = Vec::with_capacity(layers.len());
        for layer in layers {
            match layer {
                BuilderLayer::Sync(layer) => sync_layers.push(layer),
                BuilderLayer::Async(_) => return Err(Error::AsyncBuildRequired),
            }
        }
        build_from_layers(sync_layers, merge, coercion, validation, redactions)
    }

    /// Loads all sources, awaiting async layers, and builds the configuration.
    ///
    /// Async layers are read in registration order and then become static
    /// layers. Later `reload` calls and watch rebuilds do not re-run their
    /// async reads.
    pub async fn build_async(self) -> Result<Config> {
        let ConfigBuilder {
            layers,
            merge,
            coercion,
            validation,
            redactions,
        } = self;
        let mut sync_layers = Vec::with_capacity(layers.len());
        for layer in layers {
            match layer {
                BuilderLayer::Sync(layer) => sync_layers.push(layer),
                BuilderLayer::Async(source) => {
                    let name = source.name().to_owned();
                    let map = source.read().await?;
                    sync_layers.push(Layer::new(PreloadedSource { name, map }));
                }
            }
        }
        build_from_layers(sync_layers, merge, coercion, validation, redactions)
    }
}

fn build_from_layers(
    layers: Vec<Layer>,
    merge: MergeOptions,
    coercion: Coercion,
    validation: Option<Validator>,
    redactions: Vec<Path>,
) -> Result<Config> {
    log_debug!(layers = layers.len(), "building configuration");
    let sources_value = load_sources(&layers, &merge)?;
    let overlay = Map::new();
    let merged = apply_overlay(&sources_value, &overlay, &merge)?;
    let snapshot = Snapshot::from_parts(merged, coercion, Arc::from(redactions));
    if let Some(validation) = &validation
        && let Err(error) = validation(&snapshot)
    {
        log_warn!(error = %error, "configuration validation failed");
        return Err(error);
    }
    let inner = ConfigInner::new(
        layers,
        overlay,
        sources_value,
        snapshot,
        merge,
        coercion,
        validation,
    );
    Ok(Config {
        inner: Arc::new(inner),
    })
}

/// A layered configuration handle.
///
/// Cloning a `Config` shares the same underlying state: updates performed
/// through any clone are visible to all clones, and watch subscriptions are
/// shared.
#[derive(Clone)]
pub struct Config {
    inner: Arc<ConfigInner>,
}

impl Config {
    /// Creates a builder.
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::new()
    }

    /// Returns the current immutable snapshot.
    pub fn snapshot(&self) -> Snapshot {
        self.inner.snapshot()
    }

    /// Reads and deserializes the value at `path` from the current snapshot.
    pub fn get<T>(&self, path: impl IntoPath) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.snapshot().get(path)
    }

    /// Reads the value at `path`, returning `None` only when it is missing.
    pub fn get_opt<T>(&self, path: impl IntoPath) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.snapshot().get_opt(path)
    }

    /// Reads the value at `path`, returning `default` only when it is missing.
    pub fn get_or<T>(&self, path: impl IntoPath, default: T) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.snapshot().get_or(path, default)
    }

    /// Deserializes the whole configuration from the current snapshot.
    pub fn deserialize<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.snapshot().deserialize()
    }

    /// Returns a view of the object at `path` from the current snapshot.
    pub fn section(&self, path: impl IntoPath) -> Result<Section> {
        self.snapshot().section(path)
    }

    /// Returns `true` if `path` resolves in the current snapshot.
    pub fn contains(&self, path: impl IntoPath) -> bool {
        self.snapshot().contains(path)
    }

    /// Runs the configured validator against the current snapshot.
    ///
    /// This is useful when the validator has side effects or when a caller
    /// wants to re-check the current state after external changes.
    pub fn validate(&self) -> Result<()> {
        self.inner.validate(&self.snapshot())
    }

    /// Renders the root object from the current snapshot with `format`.
    pub fn render(&self, format: &dyn Format) -> Result<Vec<u8>> {
        self.snapshot().render(format)
    }

    /// Renders the current snapshot and writes it to `path`.
    ///
    /// See [`Snapshot::save`] for the write semantics.
    pub fn save(&self, path: impl AsRef<StdPath>, format: &dyn Format) -> Result<()> {
        self.snapshot().save(path, format)
    }

    /// Appends a plain source layer and rebuilds.
    pub fn load<S>(&self, source: S) -> Result<()>
    where
        S: Source + 'static,
    {
        self.inner.load_layer(Layer::new(source))
    }

    /// Appends a watchable source layer and rebuilds.
    ///
    /// If watch subscriptions are active, the new source's watcher is started
    /// before the layer is committed.
    pub fn load_watched<W>(&self, source: W) -> Result<()>
    where
        W: Watchable + 'static,
    {
        self.inner.load_layer(Layer::watched(source))
    }

    /// Reads an async source and appends it as a static layer.
    ///
    /// The source is read before the layer is committed, so a read failure
    /// leaves the configuration unchanged. The async source is not watched.
    pub async fn load_async<S>(&self, source: S) -> Result<()>
    where
        S: AsyncSource + 'static,
    {
        let name = source.name().to_owned();
        let map = source.read().await?;
        self.load(PreloadedSource { name, map })
    }

    /// Returns the names of the registered source layers, lowest priority
    /// first.
    ///
    /// Names come from [`Source::name`] and are not guaranteed to be unique.
    pub fn source_names(&self) -> Vec<String> {
        self.inner
            .read_state()
            .layers
            .iter()
            .map(|layer| layer.source.name().to_owned())
            .collect()
    }

    /// Removes every source layer whose [`Source::name`] equals `name` and
    /// rebuilds.
    ///
    /// Watchers belonging to removed layers are stopped. Runtime overrides are
    /// retained. Returns the number of removed layers.
    pub fn remove_source(&self, name: &str) -> Result<usize> {
        self.inner.remove_source(name)
    }

    /// Re-reads all source layers and rebuilds.
    pub fn reload(&self) -> Result<()> {
        self.inner.reload()
    }

    /// Sets a runtime override at `path`.
    ///
    /// Overrides form the highest-priority layer and survive `reload`. The
    /// override is merged into the most recently loaded source values; call
    /// [`Self::reload`] first to observe external file/environment changes.
    ///
    /// Array index paths are read-only. Override the whole array instead.
    pub fn set<P, V>(&self, path: P, value: V) -> Result<()>
    where
        P: IntoPath,
        V: Into<Value>,
    {
        self.inner.set(path.into_path(), value.into())
    }

    /// Removes a runtime override at `path`.
    ///
    /// This removes the override from the overlay; values from source layers
    /// become visible again. Use [`Self::set`] with `Value::Null` to mask a
    /// lower-layer value. Like [`Self::set`], this uses the most recently
    /// loaded source values; call [`Self::reload`] first to observe external
    /// changes. Array index paths are rejected; override the whole array
    /// instead.
    pub fn delete(&self, path: impl IntoPath) -> Result<()> {
        self.inner.delete(path.into_path())
    }

    /// Registers a watch callback.
    ///
    /// The callback runs on a dedicated dispatcher thread. It should be fast;
    /// offload heavy work to another thread or channel. A panic in a callback
    /// is caught so it cannot kill the watcher. Keep the returned guard alive
    /// for as long as the subscription is needed; dropping it unregisters the
    /// callback.
    pub fn watch<F>(&self, callback: F) -> Result<WatchGuard>
    where
        F: Fn(WatchEvent) + Send + Sync + 'static,
    {
        let id = self
            .inner
            .add_callback(Arc::downgrade(&self.inner), Arc::new(callback))?;
        Ok(WatchGuard::new(Arc::clone(&self.inner), id))
    }

    /// Registers a watch channel.
    ///
    /// The returned receiver is a standard single-consumer channel. Multiple
    /// consumers can register multiple channels or callbacks. Keep the guard
    /// alive for as long as the subscription is needed; dropping it stops the
    /// subscription. Dropping the receiver stops delivery (the sender is
    /// removed on the next event).
    pub fn watch_channel(&self) -> Result<(WatchGuard, Receiver<WatchEvent>)> {
        let (sender, receiver) = mpsc::channel();
        let id = self
            .inner
            .add_channel(Arc::downgrade(&self.inner), sender)?;
        Ok((WatchGuard::new(Arc::clone(&self.inner), id), receiver))
    }

    /// Registers an asynchronous watch stream.
    ///
    /// This is available with the `async` feature. The stream is
    /// runtime-agnostic; drive it with any executor, or call
    /// [`WatchStream::recv`] directly. Keep the guard alive for as long as the
    /// subscription is needed.
    #[cfg(feature = "async")]
    pub fn watch_stream(&self) -> Result<(WatchGuard, WatchStream)> {
        let (sender, receiver) = futures_channel::mpsc::unbounded();
        let id = self
            .inner
            .add_async_channel(Arc::downgrade(&self.inner), sender)?;
        Ok((
            WatchGuard::new(Arc::clone(&self.inner), id),
            WatchStream::new(receiver),
        ))
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

struct State {
    layers: Vec<Layer>,
    overlay: Map,
    sources_value: Value,
    snapshot: Snapshot,
    merge: MergeOptions,
    coercion: Coercion,
}

enum DispatchCommand {
    SourceChanged,
    SourceError(Error),
    Emit(WatchEvent),
    Shutdown,
}

type WatchCallbackEntry = (u64, Arc<dyn Fn(WatchEvent) + Send + Sync>);
type WatchChannelEntry = (u64, Sender<WatchEvent>);
#[cfg(feature = "async")]
type AsyncWatchChannelEntry = (u64, futures_channel::mpsc::UnboundedSender<WatchEvent>);
type WatchedSubscription = (Arc<dyn Watchable>, Box<dyn WatchSubscription>);

struct WatchState {
    next_id: u64,
    callbacks: Vec<WatchCallbackEntry>,
    channels: Vec<WatchChannelEntry>,
    #[cfg(feature = "async")]
    async_channels: Vec<AsyncWatchChannelEntry>,
    subscriptions: Vec<WatchedSubscription>,
    dispatcher: Option<Sender<DispatchCommand>>,
}

impl Default for WatchState {
    fn default() -> Self {
        Self {
            next_id: 1,
            callbacks: Vec::new(),
            channels: Vec::new(),
            #[cfg(feature = "async")]
            async_channels: Vec::new(),
            subscriptions: Vec::new(),
            dispatcher: None,
        }
    }
}

impl WatchState {
    fn is_empty(&self) -> bool {
        self.callbacks.is_empty() && self.channels.is_empty() && {
            #[cfg(feature = "async")]
            {
                self.async_channels.is_empty()
            }
            #[cfg(not(feature = "async"))]
            {
                true
            }
        }
    }
}

pub(crate) struct ConfigInner {
    state: RwLock<State>,
    write_lock: Mutex<()>,
    watch: Mutex<WatchState>,
    validation: Option<Validator>,
    redactions: Arc<[Path]>,
}

impl ConfigInner {
    fn new(
        layers: Vec<Layer>,
        overlay: Map,
        sources_value: Value,
        snapshot: Snapshot,
        merge: MergeOptions,
        coercion: Coercion,
        validation: Option<Validator>,
    ) -> Self {
        let redactions = snapshot.redactions();
        Self {
            state: RwLock::new(State {
                layers,
                overlay,
                sources_value,
                snapshot,
                merge,
                coercion,
            }),
            write_lock: Mutex::new(()),
            watch: Mutex::new(WatchState::default()),
            validation,
            redactions,
        }
    }

    fn validate(&self, snapshot: &Snapshot) -> Result<()> {
        if let Some(validation) = &self.validation
            && let Err(error) = validation(snapshot)
        {
            log_warn!(error = %error, "configuration validation failed");
            return Err(error);
        }
        Ok(())
    }

    fn read_state(&self) -> RwLockReadGuard<'_, State> {
        self.state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_state(&self) -> RwLockWriteGuard<'_, State> {
        self.state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_write(&self) -> MutexGuard<'_, ()> {
        self.write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_watch(&self) -> MutexGuard<'_, WatchState> {
        self.watch
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn snapshot(&self) -> Snapshot {
        self.read_state().snapshot.clone()
    }

    fn load_layer(&self, layer: Layer) -> Result<()> {
        let _write_guard = self.lock_write();
        let (mut layers, overlay, merge, coercion, old) = {
            let state = self.read_state();
            (
                state.layers.clone(),
                state.overlay.clone(),
                state.merge,
                state.coercion,
                state.snapshot.clone(),
            )
        };
        log_debug!(source = %layer.source.name(), "loading source layer");
        layers.push(layer);
        let sources_value = load_sources(&layers, &merge)?;
        let merged = apply_overlay(&sources_value, &overlay, &merge)?;
        let new_snapshot = Snapshot::from_parts(merged, coercion, Arc::clone(&self.redactions));
        self.validate(&new_snapshot)?;

        let mut watch = self.lock_watch();
        let mut new_subscription = None;
        if let Some(watchable) = layers
            .last()
            .and_then(|layer| layer.watchable.as_ref())
            .cloned()
            && let Some(sender) = watch.dispatcher.clone()
        {
            let callback = watch_callback(sender);
            let subscription = watchable.watch(callback)?;
            new_subscription = Some((watchable, subscription));
        }
        if let Some(subscription) = new_subscription {
            watch.subscriptions.push(subscription);
        }
        log_debug!(
            source = %layers.last().map(|layer| layer.source.name()).unwrap_or(""),
            "loaded source layer"
        );

        {
            let mut state = self.write_state();
            state.layers = layers;
            state.overlay = overlay;
            state.sources_value = sources_value;
            state.snapshot = new_snapshot.clone();
        }

        let changes = old.diff(&new_snapshot);
        if !changes.is_empty()
            && let Some(sender) = &watch.dispatcher
        {
            let _ = sender.send(DispatchCommand::Emit(WatchEvent::Changed {
                snapshot: new_snapshot,
                changes,
            }));
        }
        Ok(())
    }

    fn remove_source(&self, name: &str) -> Result<usize> {
        let _write_guard = self.lock_write();
        let (layers, overlay, merge, coercion, old) = {
            let state = self.read_state();
            (
                state.layers.clone(),
                state.overlay.clone(),
                state.merge,
                state.coercion,
                state.snapshot.clone(),
            )
        };

        let mut kept = Vec::with_capacity(layers.len());
        let mut removed_watchables = Vec::new();
        let mut removed = 0;
        for layer in layers {
            if layer.source.name() == name {
                removed += 1;
                if let Some(watchable) = &layer.watchable {
                    removed_watchables.push(Arc::clone(watchable));
                }
            } else {
                kept.push(layer);
            }
        }
        if removed == 0 {
            return Ok(0);
        }

        let sources_value = load_sources(&kept, &merge)?;
        let merged = apply_overlay(&sources_value, &overlay, &merge)?;
        let new_snapshot = Snapshot::from_parts(merged, coercion, Arc::clone(&self.redactions));
        self.validate(&new_snapshot)?;

        let mut watch = self.lock_watch();
        stop_watch_subscriptions(&mut watch, &removed_watchables);
        {
            let mut state = self.write_state();
            state.layers = kept;
            state.sources_value = sources_value;
            state.snapshot = new_snapshot.clone();
        }

        let changes = old.diff(&new_snapshot);
        if !changes.is_empty()
            && let Some(sender) = &watch.dispatcher
        {
            let _ = sender.send(DispatchCommand::Emit(WatchEvent::Changed {
                snapshot: new_snapshot,
                changes,
            }));
        }
        log_debug!(source = %name, removed, "removed source layers");
        Ok(removed)
    }

    fn reload(&self) -> Result<()> {
        log_debug!("reloading configuration");
        let _write_guard = self.lock_write();
        let (layers, overlay, merge, coercion, old) = {
            let state = self.read_state();
            (
                state.layers.clone(),
                state.overlay.clone(),
                state.merge,
                state.coercion,
                state.snapshot.clone(),
            )
        };
        let sources_value = load_sources(&layers, &merge)?;
        let merged = apply_overlay(&sources_value, &overlay, &merge)?;
        let new_snapshot = Snapshot::from_parts(merged, coercion, Arc::clone(&self.redactions));
        self.validate(&new_snapshot)?;
        {
            let mut state = self.write_state();
            state.sources_value = sources_value;
            state.snapshot = new_snapshot.clone();
        }
        self.emit_changed(old, new_snapshot);
        Ok(())
    }

    fn set(&self, path: Path, value: Value) -> Result<()> {
        let _write_guard = self.lock_write();
        let (sources_value, mut overlay, merge, coercion, old) = {
            let state = self.read_state();
            (
                state.sources_value.clone(),
                state.overlay.clone(),
                state.merge,
                state.coercion,
                state.snapshot.clone(),
            )
        };
        reject_array_index_path(&sources_value, &path)?;
        log_debug!(path = %path, "setting runtime override");
        insert_overlay(&mut overlay, &path, value)?;
        let merged = apply_overlay(&sources_value, &overlay, &merge)?;
        let new_snapshot = Snapshot::from_parts(merged, coercion, Arc::clone(&self.redactions));
        self.validate(&new_snapshot)?;
        {
            let mut state = self.write_state();
            state.overlay = overlay;
            state.snapshot = new_snapshot.clone();
        }
        self.emit_changed(old, new_snapshot);
        Ok(())
    }

    fn delete(&self, path: Path) -> Result<()> {
        let _write_guard = self.lock_write();
        let (sources_value, mut overlay, merge, coercion, old) = {
            let state = self.read_state();
            (
                state.sources_value.clone(),
                state.overlay.clone(),
                state.merge,
                state.coercion,
                state.snapshot.clone(),
            )
        };
        reject_array_index_path(&sources_value, &path)?;
        log_debug!(path = %path, "deleting runtime override");
        delete_overlay(&mut overlay, path.segments());
        let merged = apply_overlay(&sources_value, &overlay, &merge)?;
        let new_snapshot = Snapshot::from_parts(merged, coercion, Arc::clone(&self.redactions));
        self.validate(&new_snapshot)?;
        {
            let mut state = self.write_state();
            state.overlay = overlay;
            state.snapshot = new_snapshot.clone();
        }
        self.emit_changed(old, new_snapshot);
        Ok(())
    }

    fn emit_changed(&self, old: Snapshot, new: Snapshot) {
        let changes = old.diff(&new);
        if changes.is_empty() {
            return;
        }
        log_debug!(changes = changes.len(), "emitting change event");
        let watch = self.lock_watch();
        if let Some(sender) = &watch.dispatcher {
            let _ = sender.send(DispatchCommand::Emit(WatchEvent::Changed {
                snapshot: new,
                changes,
            }));
        }
    }

    pub(crate) fn reload_for_watch(&self) -> Result<Option<(Snapshot, ChangeSet)>> {
        let _write_guard = self.lock_write();
        let (layers, overlay, merge, coercion, old) = {
            let state = self.read_state();
            (
                state.layers.clone(),
                state.overlay.clone(),
                state.merge,
                state.coercion,
                state.snapshot.clone(),
            )
        };
        let sources_value = load_sources(&layers, &merge)?;
        let merged = apply_overlay(&sources_value, &overlay, &merge)?;
        let new_snapshot = Snapshot::from_parts(merged, coercion, Arc::clone(&self.redactions));
        self.validate(&new_snapshot)?;
        {
            let mut state = self.write_state();
            state.sources_value = sources_value;
            state.snapshot = new_snapshot.clone();
        }
        let changes = old.diff(&new_snapshot);
        if changes.is_empty() {
            Ok(None)
        } else {
            log_debug!(changes = changes.len(), "watch reload produced changes");
            Ok(Some((new_snapshot, changes)))
        }
    }

    fn add_callback(
        &self,
        inner: Weak<Self>,
        callback: Arc<dyn Fn(WatchEvent) + Send + Sync>,
    ) -> Result<u64> {
        let _write_guard = self.lock_write();
        let mut watch = self.lock_watch();
        let id = watch.next_id;
        watch.next_id += 1;
        // Register before starting watchers so an event arriving during
        // watcher setup cannot be missed by this subscription.
        watch.callbacks.push((id, callback));
        if let Err(error) = self.ensure_watching(&mut watch, inner) {
            watch
                .callbacks
                .retain(|(callback_id, _)| *callback_id != id);
            return Err(error);
        }
        log_debug!(subscription = id, "registered watch callback");
        Ok(id)
    }

    fn add_channel(&self, inner: Weak<Self>, sender: Sender<WatchEvent>) -> Result<u64> {
        let _write_guard = self.lock_write();
        let mut watch = self.lock_watch();
        let id = watch.next_id;
        watch.next_id += 1;
        watch.channels.push((id, sender));
        if let Err(error) = self.ensure_watching(&mut watch, inner) {
            watch.channels.retain(|(channel_id, _)| *channel_id != id);
            return Err(error);
        }
        log_debug!(subscription = id, "registered watch channel");
        Ok(id)
    }

    #[cfg(feature = "async")]
    fn add_async_channel(
        &self,
        inner: Weak<Self>,
        sender: futures_channel::mpsc::UnboundedSender<WatchEvent>,
    ) -> Result<u64> {
        let _write_guard = self.lock_write();
        let mut watch = self.lock_watch();
        let id = watch.next_id;
        watch.next_id += 1;
        watch.async_channels.push((id, sender));
        if let Err(error) = self.ensure_watching(&mut watch, inner) {
            watch
                .async_channels
                .retain(|(channel_id, _)| *channel_id != id);
            return Err(error);
        }
        log_debug!(subscription = id, "registered async watch channel");
        Ok(id)
    }

    fn ensure_watching(&self, watch: &mut WatchState, inner: Weak<Self>) -> Result<()> {
        if watch.dispatcher.is_some() {
            return Ok(());
        }
        let layers = self.read_state().layers.clone();
        if layers.iter().all(|layer| layer.watchable.is_none()) {
            return Err(Error::NoWatchableSources);
        }

        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || dispatcher_loop(inner, receiver));
        let callback = watch_callback(sender.clone());

        let mut subscriptions = Vec::new();
        for layer in &layers {
            if let Some(watchable) = &layer.watchable {
                match watchable.watch(callback.clone()) {
                    Ok(subscription) => {
                        subscriptions.push((Arc::clone(watchable), subscription));
                    }
                    Err(error) => {
                        for (_, subscription) in subscriptions {
                            subscription.stop();
                        }
                        return Err(error);
                    }
                }
            }
        }

        watch.dispatcher = Some(sender);
        watch.subscriptions = subscriptions;
        log_debug!(
            subscriptions = watch.subscriptions.len(),
            "started watch subscriptions"
        );
        Ok(())
    }

    pub(crate) fn remove_subscription(&self, id: u64) {
        let mut watch = self.lock_watch();
        watch
            .callbacks
            .retain(|(callback_id, _)| *callback_id != id);
        watch.channels.retain(|(channel_id, _)| *channel_id != id);
        #[cfg(feature = "async")]
        watch
            .async_channels
            .retain(|(channel_id, _)| *channel_id != id);
        if watch.is_empty() {
            log_debug!(subscription = id, "shutting down watch subscriptions");
            if let Some(sender) = watch.dispatcher.take() {
                let _ = sender.send(DispatchCommand::Shutdown);
            }
            for (_, subscription) in watch.subscriptions.drain(..) {
                subscription.stop();
            }
        }
    }

    fn broadcast(&self, event: WatchEvent) {
        let (callbacks, channels) = {
            let watch = self.lock_watch();
            (
                watch
                    .callbacks
                    .iter()
                    .map(|(id, callback)| (*id, Arc::clone(callback)))
                    .collect::<Vec<_>>(),
                watch
                    .channels
                    .iter()
                    .map(|(id, sender)| (*id, sender.clone()))
                    .collect::<Vec<_>>(),
            )
        };
        #[cfg(feature = "async")]
        let async_channels = {
            let watch = self.lock_watch();
            watch
                .async_channels
                .iter()
                .map(|(id, sender)| (*id, sender.clone()))
                .collect::<Vec<_>>()
        };

        for (_, callback) in callbacks {
            let event = event.clone();
            let _ = catch_unwind(AssertUnwindSafe(|| callback(event)));
        }

        let mut dead = Vec::new();
        for (id, sender) in channels {
            if sender.send(event.clone()).is_err() {
                dead.push(id);
            }
        }
        #[cfg(feature = "async")]
        let mut async_dead = Vec::new();
        #[cfg(feature = "async")]
        for (id, sender) in async_channels {
            if sender.unbounded_send(event.clone()).is_err() {
                async_dead.push(id);
            }
        }

        #[cfg(feature = "async")]
        let has_dead = !dead.is_empty() || !async_dead.is_empty();
        #[cfg(not(feature = "async"))]
        let has_dead = !dead.is_empty();

        if has_dead {
            let mut watch = self.lock_watch();
            watch.channels.retain(|(id, _)| !dead.contains(id));
            #[cfg(feature = "async")]
            watch
                .async_channels
                .retain(|(id, _)| !async_dead.contains(id));
            if watch.is_empty() {
                if let Some(sender) = watch.dispatcher.take() {
                    let _ = sender.send(DispatchCommand::Shutdown);
                }
                for (_, subscription) in watch.subscriptions.drain(..) {
                    subscription.stop();
                }
            }
        }
    }
}

fn dispatcher_loop(inner: Weak<ConfigInner>, receiver: Receiver<DispatchCommand>) {
    while let Ok(command) = receiver.recv() {
        let Some(inner) = inner.upgrade() else {
            break;
        };
        match command {
            DispatchCommand::SourceChanged => match inner.reload_for_watch() {
                Ok(Some((snapshot, changes))) => {
                    inner.broadcast(WatchEvent::Changed { snapshot, changes })
                }
                Ok(None) => {}
                Err(error) => {
                    log_warn!(error = %error, "watch reload failed");
                    inner.broadcast(WatchEvent::Error {
                        error: Arc::new(error),
                    });
                }
            },
            DispatchCommand::SourceError(error) => {
                log_warn!(error = %error, "source watch error");
                inner.broadcast(WatchEvent::Error {
                    error: Arc::new(error),
                });
            }
            DispatchCommand::Emit(event) => inner.broadcast(event),
            DispatchCommand::Shutdown => break,
        }
    }
}

fn watch_callback(sender: Sender<DispatchCommand>) -> WatchCallback {
    Arc::new(move |signal| {
        let command = match signal {
            WatchSignal::Changed => DispatchCommand::SourceChanged,
            WatchSignal::Error(error) => DispatchCommand::SourceError(error),
        };
        let _ = sender.send(command);
    })
}

fn save_value(path: &StdPath, value: &Value, format: &dyn Format) -> Result<()> {
    let bytes = format.render(value)?;
    fs::write(path, bytes).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn stop_watch_subscriptions(watch: &mut WatchState, removed: &[Arc<dyn Watchable>]) {
    let mut kept = Vec::with_capacity(watch.subscriptions.len());
    for (watchable, subscription) in watch.subscriptions.drain(..) {
        if removed
            .iter()
            .any(|removed| Arc::ptr_eq(removed, &watchable))
        {
            subscription.stop();
        } else {
            kept.push((watchable, subscription));
        }
    }
    watch.subscriptions = kept;
}

fn load_sources(layers: &[Layer], merge: &MergeOptions) -> Result<Value> {
    let mut map = Map::new();
    for layer in layers {
        let source_map = layer.source.read()?;
        merge_map(&mut map, source_map, merge);
    }
    Ok(Value::Map(map))
}

fn apply_overlay(sources_value: &Value, overlay: &Map, merge: &MergeOptions) -> Result<Value> {
    let mut merged = sources_value.clone();
    match &mut merged {
        Value::Map(map) => merge_map(map, overlay.clone(), merge),
        other => {
            return Err(Error::InvalidRoot {
                source: "overlay".to_owned(),
                actual: other.type_name(),
            });
        }
    }
    Ok(merged)
}

fn reject_array_index_path(sources_value: &Value, path: &Path) -> Result<()> {
    let mut current = sources_value;
    for segment in path.segments() {
        match current {
            Value::Map(map) => match map.get(segment) {
                Some(value) => current = value,
                None => return Ok(()),
            },
            Value::Array(_) => {
                return Err(Error::InvalidPath {
                    input: path.to_string(),
                    reason: "array index paths are read-only; override the whole array".to_owned(),
                });
            }
            _ => return Ok(()),
        }
    }
    Ok(())
}

fn insert_overlay(overlay: &mut Map, path: &Path, value: Value) -> Result<()> {
    if path.is_root() {
        return match value {
            Value::Map(map) => {
                *overlay = map;
                Ok(())
            }
            other => Err(Error::InvalidPath {
                input: "<root>".to_owned(),
                reason: format!(
                    "root override must be an object, found `{}`",
                    other.type_name()
                ),
            }),
        };
    }
    insert_path(overlay, path.segments(), value, &path.to_string())
}

fn insert_path(map: &mut Map, segments: &[String], value: Value, full: &str) -> Result<()> {
    let (first, rest) = segments
        .split_first()
        .expect("non-root paths contain at least one segment");
    if rest.is_empty() {
        map.insert(first.clone(), value);
        return Ok(());
    }
    let entry = map
        .entry(first.clone())
        .or_insert_with(|| Value::Map(Map::new()));
    match entry {
        Value::Map(child) => insert_path(child, rest, value, full),
        other => Err(Error::InvalidPath {
            input: full.to_owned(),
            reason: format!(
                "`{first}` is a {} and cannot be traversed",
                other.type_name()
            ),
        }),
    }
}

fn delete_overlay(overlay: &mut Map, segments: &[String]) {
    if segments.is_empty() {
        overlay.clear();
        return;
    }
    delete_path(overlay, segments);
}

fn delete_path(map: &mut Map, segments: &[String]) {
    let Some((first, rest)) = segments.split_first() else {
        return;
    };
    if rest.is_empty() {
        map.shift_remove(first);
        return;
    }
    let prune = if let Some(Value::Map(child)) = map.get_mut(first) {
        delete_path(child, rest);
        child.is_empty()
    } else {
        false
    };
    if prune {
        map.shift_remove(first);
    }
}
