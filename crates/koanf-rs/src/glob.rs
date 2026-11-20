//! Glob-pattern configuration source.
//!
//! [`Glob`] matches regular files under a root directory with a glob pattern
//! and merges matches in lexicographic relative-path order.

use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;

use glob_pattern::{MatchOptions, Pattern};
use koanf_core::{Error, Format, Map, MergeOptions, Result, Source, merge_map};

use crate::dir::collect_files;
use crate::file::{known_extension, read_file};

#[cfg(feature = "watch")]
use std::time::Duration;

#[cfg(feature = "watch")]
use koanf_core::{WatchCallback, WatchSignal, WatchSubscription, Watchable};
#[cfg(feature = "watch")]
use notify::event::{EventKind, ModifyKind};

/// A glob-pattern-backed configuration source.
///
/// The pattern is matched against paths relative to `root`; use `/` as the
/// separator in patterns. `*` does not cross a separator; use `**` for
/// recursive matches. Matching files are merged in lexicographic relative-path
/// order, so later matches override earlier ones. By default only files with an
/// enabled built-in extension are read; [`Glob::format`] applies one explicit
/// format to every matching regular file instead.
///
/// A missing root is an error unless [`Glob::optional`] is used. Watching
/// requires the root directory to exist when the watcher starts.
#[derive(Clone)]
pub struct Glob {
    root: PathBuf,
    pattern: Pattern,
    name: String,
    format: Option<Arc<dyn Format>>,
    optional: bool,
    merge: MergeOptions,
    #[cfg(feature = "watch")]
    debounce: Duration,
}

impl Glob {
    /// Creates a glob source rooted at `root`.
    ///
    /// Returns [`Error::InvalidGlob`] if the pattern cannot be compiled.
    pub fn new(root: impl Into<PathBuf>, pattern: impl AsRef<str>) -> Result<Self> {
        let root = root.into();
        let pattern_text = pattern.as_ref().to_owned();
        let pattern = Pattern::new(&pattern_text).map_err(|error| Error::InvalidGlob {
            pattern: pattern_text.clone(),
            message: error.to_string(),
        })?;
        let name = format!("glob:{}:{}", root.display(), pattern_text);
        Ok(Self {
            root,
            pattern,
            name,
            format: None,
            optional: false,
            merge: MergeOptions::default(),
            #[cfg(feature = "watch")]
            debounce: Duration::from_millis(100),
        })
    }

    /// Sets an explicit format for every matching regular file.
    ///
    /// With an explicit format, matches without a known extension are read too.
    pub fn format<F>(mut self, format: F) -> Self
    where
        F: Format + 'static,
    {
        self.format = Some(Arc::new(format));
        self
    }

    /// Treats a missing root directory as an empty layer.
    ///
    /// Other I/O errors still fail.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Sets the merge options used between matching files.
    ///
    /// These options are independent of the options the [`Config`] builder uses
    /// to merge this source with other layers.
    ///
    /// [`Config`]: koanf_core::Config
    pub fn merge_options(mut self, options: MergeOptions) -> Self {
        self.merge = options;
        self
    }

    /// Sets the glob-watch debounce duration. The default is 100 ms.
    #[cfg(feature = "watch")]
    pub fn debounce(mut self, duration: Duration) -> Self {
        self.debounce = duration;
        self
    }

    fn matches(&self, relative: &StdPath) -> bool {
        matches_relative(&self.pattern, relative)
    }
}

impl Source for Glob {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        let mut files = Vec::new();
        collect_files(
            &self.root,
            &self.root,
            true,
            true,
            self.optional,
            &mut files,
        )?;
        files.retain(|(relative, path)| {
            self.matches(relative) && (self.format.is_some() || known_extension(path))
        });
        files.sort_by(|(left, _), (right, _)| left.cmp(right));

        let mut merged = Map::new();
        for (_, path) in files {
            let map = match read_file(&path, self.format.as_ref(), false) {
                Ok(map) => map,
                Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
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
impl Watchable for Glob {
    fn watch(&self, callback: WatchCallback) -> Result<Box<dyn WatchSubscription>> {
        let watch_root = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        let root_for_events = watch_root.clone();
        let pattern = self.pattern.clone();
        let has_explicit_format = self.format.is_some();
        let callback_for_events = Arc::clone(&callback);
        let description = self.name.clone();

        let mut debouncer = notify_debouncer_full::new_debouncer(
            self.debounce,
            None,
            move |result: notify_debouncer_full::DebounceEventResult| match result {
                Ok(events) => {
                    let relevant = events.iter().any(|event| {
                        let tree_change = matches!(
                            event.event.kind,
                            EventKind::Create(_)
                                | EventKind::Remove(_)
                                | EventKind::Modify(ModifyKind::Name(_))
                        );
                        event.event.paths.iter().any(|path| {
                            let Ok(relative) = path.strip_prefix(&root_for_events) else {
                                return false;
                            };
                            tree_change
                                || ((has_explicit_format || known_extension(path))
                                    && matches_relative(&pattern, relative))
                        })
                    });
                    if relevant {
                        callback_for_events(WatchSignal::Changed);
                    }
                }
                Err(errors) => {
                    if let Some(error) = errors.into_iter().next() {
                        callback_for_events(WatchSignal::Error(crate::file::watch_error(
                            format!("glob watcher for `{description}` failed"),
                            error,
                        )));
                    }
                }
            },
        )
        .map_err(|error| {
            crate::file::watch_error(
                format!("failed to start watcher for `{}`", self.root.display()),
                error,
            )
        })?;

        debouncer
            .watch(&watch_root, notify::RecursiveMode::Recursive)
            .map_err(|error| {
                crate::file::watch_error(
                    format!("failed to watch `{}`", watch_root.display()),
                    error,
                )
            })?;

        Ok(Box::new(crate::file::NotifyWatchSubscription::new(
            debouncer,
        )))
    }
}

const MATCH_OPTIONS: MatchOptions = MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: false,
};

fn matches_relative(pattern: &Pattern, relative: &StdPath) -> bool {
    #[cfg(windows)]
    {
        relative.to_str().is_some_and(|path| {
            pattern.matches_with(path, MATCH_OPTIONS)
                || pattern.matches_with(&path.replace('\\', "/"), MATCH_OPTIONS)
        })
    }
    #[cfg(not(windows))]
    {
        relative
            .to_str()
            .is_some_and(|path| pattern.matches_with(path, MATCH_OPTIONS))
    }
}
