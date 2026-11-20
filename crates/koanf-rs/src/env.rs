//! Environment-variable configuration source.

use std::ffi::OsString;
use std::io;
use std::sync::Arc;

use koanf_core::{Error, Map, Path, Result, Source, Value};

use crate::util::insert_path;

type KeyMapper = Arc<dyn Fn(&str) -> Path + Send + Sync>;

/// Case transformation applied to each environment-variable path segment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyCase {
    /// Convert each segment to lowercase. This is the default.
    #[default]
    Lower,
    /// Keep each segment unchanged.
    Preserve,
    /// Convert each segment to uppercase.
    Upper,
}

/// A configuration source backed by environment variables.
///
/// By default, keys are split on `__`, each segment is lowercased, and values
/// remain strings (type-directed coercion happens at read time). Use
/// [`Env::map_key`] for full control.
pub struct Env {
    prefix: Option<String>,
    separator: String,
    case: KeyCase,
    mapper: Option<KeyMapper>,
    error_on_non_utf8: bool,
    vars: Option<Vec<(OsString, OsString)>>,
    name: String,
}

impl Env {
    /// Reads environment variables with the given prefix.
    pub fn prefixed(prefix: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
            separator: "__".to_owned(),
            case: KeyCase::Lower,
            mapper: None,
            error_on_non_utf8: false,
            vars: None,
            name: "env".to_owned(),
        }
    }

    /// Reads all environment variables. Use deliberately: the process
    /// environment typically contains many unrelated variables.
    pub fn all() -> Self {
        Self {
            prefix: None,
            separator: "__".to_owned(),
            case: KeyCase::Lower,
            mapper: None,
            error_on_non_utf8: false,
            vars: None,
            name: "env".to_owned(),
        }
    }

    /// Creates a source over a supplied set of variables.
    ///
    /// This is useful for tests and for embedding configuration. The source
    /// does not read the process environment.
    pub fn from_env_iter<I>(vars: I) -> Self
    where
        I: IntoIterator<Item = (OsString, OsString)>,
    {
        Self {
            prefix: None,
            separator: "__".to_owned(),
            case: KeyCase::Lower,
            mapper: None,
            error_on_non_utf8: false,
            vars: Some(vars.into_iter().collect()),
            name: "env".to_owned(),
        }
    }

    /// Sets a custom source name used by `Config::source_names` and
    /// `Config::remove_source`.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the prefix to strip.
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Sets the path separator. The default is `__`.
    pub fn separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Sets the case transformation. The default is [`KeyCase::Lower`].
    pub fn case(mut self, case: KeyCase) -> Self {
        self.case = case;
        self
    }

    /// Replaces the default key mapping with a custom function.
    ///
    /// The function receives the key after prefix stripping. When set, the
    /// separator and case options are ignored.
    pub fn map_key<F>(mut self, mapper: F) -> Self
    where
        F: Fn(&str) -> Path + Send + Sync + 'static,
    {
        self.mapper = Some(Arc::new(mapper));
        self
    }

    /// Returns an error for matching variables whose values are not UTF-8.
    ///
    /// By default such variables are skipped.
    pub fn error_on_non_utf8(mut self) -> Self {
        self.error_on_non_utf8 = true;
        self
    }
}

impl Source for Env {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        if self.separator.is_empty() {
            return Err(Error::source_error(
                self.name.clone(),
                io::Error::new(io::ErrorKind::InvalidInput, "separator must not be empty"),
            ));
        }

        let vars = match &self.vars {
            Some(vars) => vars.clone(),
            None => std::env::vars_os().collect(),
        };

        let mut entries = vars
            .into_iter()
            .filter_map(|(key, value)| key.to_str().map(|key| (key.to_owned(), value)))
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));

        let mut map = Map::new();
        for (key, value) in entries {
            let stripped = match &self.prefix {
                Some(prefix) => match key.strip_prefix(prefix) {
                    Some(stripped) => stripped,
                    None => continue,
                },
                None => key.as_str(),
            };
            if stripped.is_empty() {
                continue;
            }

            let path = match &self.mapper {
                Some(mapper) => mapper(stripped),
                None => {
                    let segments = stripped
                        .split(self.separator.as_str())
                        .map(|segment| match self.case {
                            KeyCase::Lower => segment.to_lowercase(),
                            KeyCase::Preserve => segment.to_owned(),
                            KeyCase::Upper => segment.to_uppercase(),
                        })
                        .collect::<Vec<_>>();
                    Path::new(segments)
                }
            };
            if path.is_root() {
                continue;
            }

            let value = match value.to_str() {
                Some(value) => value.to_owned(),
                None => {
                    if self.error_on_non_utf8 {
                        return Err(Error::source_error(
                            self.name.clone(),
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("environment variable `{key}` has a non-UTF-8 value"),
                            ),
                        ));
                    }
                    continue;
                }
            };

            insert_path(&mut map, &path, Value::String(value), &self.name)?;
        }
        Ok(map)
    }
}
