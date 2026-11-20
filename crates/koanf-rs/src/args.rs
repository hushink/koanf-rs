//! Command-line argument configuration source.

use std::ffi::OsString;
use std::io;

use koanf_core::{Error, Map, Path, Result, Source, Value};

use crate::util::insert_path;

/// A lightweight command-line flag source.
///
/// Supported forms:
///
/// - `--key=value`
/// - `--key value`
/// - `--flag` (stored as `true`)
/// - `--server.port=8080` (nested paths)
///
/// Only arguments starting with the configured prefix are read. Positional
/// arguments and single-dash flags are ignored. `--` terminates parsing.
/// Repeated keys use the last value. This source does not depend on clap.
pub struct Args {
    args: Vec<OsString>,
    prefix: String,
    key_separator: String,
    name: String,
}

impl Default for Args {
    fn default() -> Self {
        Self::new()
    }
}

impl Args {
    /// Reads the process arguments, skipping the program name.
    pub fn new() -> Self {
        Self {
            args: std::env::args_os().skip(1).collect(),
            prefix: "--".to_owned(),
            key_separator: ".".to_owned(),
            name: "args".to_owned(),
        }
    }

    /// Creates a source from an argument sequence that does **not** include
    /// the program name.
    #[allow(clippy::should_implement_trait)]
    pub fn from_iter<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            prefix: "--".to_owned(),
            key_separator: ".".to_owned(),
            name: "args".to_owned(),
        }
    }

    /// Sets a custom source name used by `Config::source_names` and
    /// `Config::remove_source`.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the flag prefix. The default is `--`.
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Sets the path segment separator. The default is `.`.
    pub fn key_separator(mut self, separator: impl Into<String>) -> Self {
        self.key_separator = separator.into();
        self
    }

    fn parse_key(&self, key: &str, original: &str) -> Result<Path> {
        let segments = key
            .split(self.key_separator.as_str())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if segments.iter().any(String::is_empty) {
            return Err(Error::InvalidPath {
                input: original.to_owned(),
                reason: "key contains an empty segment".to_owned(),
            });
        }
        Ok(Path::new(segments))
    }
}

impl Source for Args {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        if self.prefix.is_empty() {
            return Err(Error::source_error(
                self.name.clone(),
                io::Error::new(io::ErrorKind::InvalidInput, "flag prefix must not be empty"),
            ));
        }
        if self.key_separator.is_empty() {
            return Err(Error::source_error(
                self.name.clone(),
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "key separator must not be empty",
                ),
            ));
        }

        let args = self
            .args
            .iter()
            .filter_map(|arg| arg.to_str().map(ToOwned::to_owned))
            .collect::<Vec<_>>();

        let mut map = Map::new();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if arg == "--" {
                break;
            }
            if !arg.starts_with(&self.prefix) {
                index += 1;
                continue;
            }

            let body = &arg[self.prefix.len()..];
            if body.is_empty() {
                index += 1;
                continue;
            }

            let (key, value) = if let Some((key, value)) = body.split_once('=') {
                (key.to_owned(), Value::String(value.to_owned()))
            } else {
                match args.get(index + 1) {
                    Some(next) if next != "--" && !next.starts_with(&self.prefix) => {
                        index += 1;
                        (body.to_owned(), Value::String(next.clone()))
                    }
                    _ => (body.to_owned(), Value::Bool(true)),
                }
            };

            if key.is_empty() {
                return Err(Error::InvalidPath {
                    input: arg.clone(),
                    reason: "empty key".to_owned(),
                });
            }
            let path = self.parse_key(&key, arg)?;
            insert_path(&mut map, &path, value, &self.name)?;
            index += 1;
        }
        Ok(map)
    }
}
