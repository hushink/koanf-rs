//! Directory configuration source.
//!
//! [`Dir`] loads regular files from a directory and merges them in
//! lexicographic relative-path order. Later files override earlier ones,
//! subject to the configured [`MergeOptions`].

use std::fs;
use std::io;
use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;

use koanf_core::{Error, Format, Map, MergeOptions, Result, Source, merge_map};

use crate::file::{known_extension, read_file};

#[cfg(feature = "watch")]
use std::time::Duration;

#[cfg(feature = "watch")]
use koanf_core::{WatchCallback, WatchSignal, WatchSubscription, Watchable};
#[cfg(feature = "watch")]
use notify::event::{EventKind, ModifyKind};

/// A directory-backed configuration source.
///
/// Files are read in lexicographic relative-path order, so names like
/// `10-base.toml` and `20-overrides.toml` provide explicit layering. The source
/// is non-recursive unless [`Dir::recursive`] is used. By default only files
/// with an enabled built-in extension (`.json`, `.toml`, `.yaml`, `.yml`, or
/// `.ini`) or a `.env` filename are read; [`Dir::format`] applies one explicit
/// format to every regular file instead.
///
/// A missing directory is an error unless [`Dir::optional`] is used. Watching a
/// directory requires the directory to exist when the watcher starts.
#[derive(Clone)]
pub struct Dir {
    path: PathBuf,
    name: String,
    format: Option<Arc<dyn Format>>,
    optional: bool,
    recursive: bool,
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
            recursive: false,
            merge: MergeOptions::default(),
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

    /// Reads subdirectories recursively.
    ///
    /// Files are ordered by their relative path, so `nested/20.toml` sorts
    /// after `10.toml` and after `nested/10.toml`. Directory symlinks are not
    /// followed, which avoids cycles.
    pub fn recursive(mut self) -> Self {
        self.recursive = true;
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
        let mut files = Vec::new();
        collect_files(
            &self.path,
            &self.path,
            self.recursive,
            self.format.is_some(),
            self.optional,
            &mut files,
        )?;
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

pub(crate) fn collect_files(
    directory: &StdPath,
    root: &StdPath,
    recursive: bool,
    explicit_format: bool,
    ignore_missing: bool,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound && ignore_missing => {
            return Ok(());
        }
        Err(error) => {
            return Err(Error::Io {
                path: directory.to_path_buf(),
                source: error,
            });
        }
    };

    for entry in entries {
        let entry = entry.map_err(|error| Error::Io {
            path: directory.to_path_buf(),
            source: error,
        })?;
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(Error::Io {
                    path: path.clone(),
                    source: error,
                });
            }
        };

        if file_type.is_dir() {
            if recursive {
                collect_files(&path, root, true, explicit_format, true, files)?;
            }
            continue;
        }

        let is_file = if file_type.is_file() {
            true
        } else if file_type.is_symlink() {
            // Follow file symlinks, but never recurse through directory
            // symlinks, which can form cycles.
            match entry.metadata() {
                Ok(metadata) => metadata.is_file(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Err(error) => {
                    return Err(Error::Io {
                        path: path.clone(),
                        source: error,
                    });
                }
            }
        } else {
            false
        };
        if !is_file {
            continue;
        }
        if !explicit_format && !known_extension(&path) {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        files.push((relative, path));
    }
    Ok(())
}

#[cfg(feature = "watch")]
impl Watchable for Dir {
    fn watch(&self, callback: WatchCallback) -> Result<Box<dyn WatchSubscription>> {
        let directory = self.path.clone();
        let has_explicit_format = self.format.is_some();
        let recursive = self.recursive;
        let callback_for_events = Arc::clone(&callback);

        let mut debouncer = notify_debouncer_full::new_debouncer(
            self.debounce,
            None,
            move |result: notify_debouncer_full::DebounceEventResult| match result {
                Ok(events) => {
                    let relevant = events.iter().any(|event| {
                        // Recursive watchers may only report a directory removal
                        // or rename on the directory itself, which has no config
                        // extension. Reload on any tree-shape change.
                        let tree_change = recursive
                            && matches!(
                                event.event.kind,
                                EventKind::Create(_)
                                    | EventKind::Remove(_)
                                    | EventKind::Modify(ModifyKind::Name(_))
                            );
                        tree_change
                            || has_explicit_format
                            || event.event.paths.iter().any(|path| known_extension(path))
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

        let mode = if self.recursive {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };
        debouncer.watch(&self.path, mode).map_err(|error| {
            crate::file::watch_error(format!("failed to watch `{}`", self.path.display()), error)
        })?;

        Ok(Box::new(crate::file::NotifyWatchSubscription::new(
            debouncer,
        )))
    }
}
