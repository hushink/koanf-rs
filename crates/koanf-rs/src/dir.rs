//! Directory configuration source.
//!
//! [`Dir`] loads every regular file in a single directory (not recursively) and
//! merges them in lexicographic filename order. Later files override earlier
//! ones, subject to the configured [`MergeOptions`].

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use koanf_core::{Error, Format, Map, MergeOptions, Result, Source, merge_map};

use crate::file::{known_extension, read_file};

#[cfg(feature = "watch")]
use std::time::Duration;

#[cfg(feature = "watch")]
use koanf_core::{WatchCallback, WatchSignal, WatchSubscription, Watchable};

/// A directory-backed configuration source.
///
/// Files are read in lexicographic filename order, so names like
/// `10-base.toml` and `20-overrides.toml` provide explicit layering. By
/// default only files with an enabled built-in extension (`.json`, `.toml`,
/// `.yaml`, or `.yml`) are read; [`Dir::format`] applies one explicit format to
/// every regular file instead.
///
/// A missing directory is an error unless [`Dir::optional`] is used. Watching a
/// directory requires the directory to exist when the watcher starts.
#[derive(Clone)]
pub struct Dir {
    path: PathBuf,
    name: String,
    format: Option<Arc<dyn Format>>,
    optional: bool,
    merge: MergeOptions,
    #[cfg(feature = "watch")]
    debounce: Duration,
}

impl Dir {
    /// Creates a directory source.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            name: format!("dir:{}", path.display()),
            path,
            format: None,
            optional: false,
            merge: MergeOptions::default(),
            #[cfg(feature = "watch")]
            debounce: Duration::from_millis(100),
        }
    }

    /// Sets an explicit format for every regular file in the directory.
    ///
    /// With an explicit format, files without a known extension are read too.
    pub fn format<F>(mut self, format: F) -> Self
    where
        F: Format + 'static,
    {
        self.format = Some(Arc::new(format));
        self
    }

    /// Treats a missing directory as an empty layer.
    ///
    /// Other I/O errors still fail.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Sets the merge options used between files inside the directory.
    ///
    /// These options are independent of the options the [`Config`] builder uses
    /// to merge this source with other layers.
    ///
    /// [`Config`]: koanf_core::Config
    pub fn merge_options(mut self, options: MergeOptions) -> Self {
        self.merge = options;
        self
    }

    /// Sets the directory-watch debounce duration. The default is 100 ms.
    #[cfg(feature = "watch")]
    pub fn debounce(mut self, duration: Duration) -> Self {
        self.debounce = duration;
        self
    }
}

impl Source for Dir {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        let entries = match fs::read_dir(&self.path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound && self.optional => {
                return Ok(Map::new());
            }
            Err(error) => {
                return Err(Error::Io {
                    path: self.path.clone(),
                    source: error,
                });
            }
        };

        let mut files = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    return Err(Error::Io {
                        path: self.path.clone(),
                        source: error,
                    });
                }
            };
            let path = entry.path();
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(Error::Io {
                        path,
                        source: error,
                    });
                }
            };
            if !metadata.is_file() {
                continue;
            }
            if self.format.is_none() && !known_extension(&path) {
                continue;
            }
            files.push((entry.file_name(), path));
        }
        files.sort_by(|(left, _), (right, _)| left.cmp(right));

        let mut merged = Map::new();
        for (_, path) in files {
            let map = match read_file(&path, self.format.as_ref(), false) {
                Ok(map) => map,
                Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                    continue;
                }
                Err(error) => return Err(error),
            };
            merge_map(&mut merged, map, &self.merge);
        }
        Ok(merged)
    }
}

#[cfg(feature = "watch")]
impl Watchable for Dir {
    fn watch(&self, callback: WatchCallback) -> Result<Box<dyn WatchSubscription>> {
        let directory = self.path.clone();
        let has_explicit_format = self.format.is_some();
        let callback_for_events = Arc::clone(&callback);

        let mut debouncer = notify_debouncer_full::new_debouncer(
            self.debounce,
            None,
            move |result: notify_debouncer_full::DebounceEventResult| match result {
                Ok(events) => {
                    let relevant = events.iter().any(|event| {
                        event
                            .event
                            .paths
                            .iter()
                            .any(|path| has_explicit_format || known_extension(path))
                    });
                    if relevant {
                        callback_for_events(WatchSignal::Changed);
                    }
                }
                Err(errors) => {
                    if let Some(error) = errors.into_iter().next() {
                        callback_for_events(WatchSignal::Error(crate::file::watch_error(
                            format!("directory watcher for `{}` failed", directory.display()),
                            error,
                        )));
                    }
                }
            },
        )
        .map_err(|error| {
            crate::file::watch_error(
                format!("failed to start watcher for `{}`", self.path.display()),
                error,
            )
        })?;

        debouncer
            .watch(&self.path, notify::RecursiveMode::NonRecursive)
            .map_err(|error| {
                crate::file::watch_error(
                    format!("failed to watch `{}`", self.path.display()),
                    error,
                )
            })?;

        Ok(Box::new(crate::file::NotifyWatchSubscription::new(
            debouncer,
        )))
    }
}
