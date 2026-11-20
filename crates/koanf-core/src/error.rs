//! Error types for `koanf-core`.

use std::fmt;
use std::path::PathBuf;

use crate::path::Path;

/// Result type used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while loading, merging, watching, or reading configuration.
///
/// The enum is `#[non_exhaustive]`; match on the variants you care about and
/// provide a fallback arm.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A third-party source failed to read its value.
    Source {
        /// Source name.
        name: String,
        /// Underlying error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// A format failed to parse bytes.
    Format {
        /// Format name, for example `"json"`.
        format: String,
        /// File path when the bytes came from a file.
        path: Option<PathBuf>,
        /// Underlying error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// An I/O operation failed.
    Io {
        /// Path involved in the failure.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// A source produced a root value that is not a map.
    InvalidRoot {
        /// Source name.
        source: String,
        /// Type name of the produced value.
        actual: &'static str,
    },
    /// A path was not present in the configuration.
    PathNotFound {
        /// Missing path.
        path: Path,
    },
    /// A path segment was traversed through a non-map, non-array value.
    NotAMap {
        /// Path prefix that was expected to be a map.
        path: Path,
        /// Type name of the value that was found.
        actual: &'static str,
    },
    /// An array was traversed with a segment that is not a valid index.
    InvalidIndex {
        /// Path containing the invalid segment.
        path: Path,
        /// Segment that could not be parsed as an index.
        segment: String,
    },
    /// A value did not have the type requested by the caller.
    TypeMismatch {
        /// Configuration path.
        path: Path,
        /// Expected type name.
        expected: &'static str,
        /// Actual type name.
        actual: &'static str,
    },
    /// A string value could not be coerced into the requested scalar type.
    Coercion {
        /// Configuration path.
        path: Path,
        /// String value that failed to parse.
        value: String,
        /// Target type name.
        target: &'static str,
    },
    /// Generic deserialization failure, including missing or unknown fields.
    Deserialize {
        /// Configuration path requested by the caller.
        path: Path,
        /// Relative field path reported by serde, when available.
        field: Option<String>,
        /// Error message.
        message: String,
    },
    /// Serialization of a Rust value into a configuration tree failed.
    Serialize {
        /// Error message.
        message: String,
    },
    /// A user-supplied configuration validator rejected a snapshot.
    Validation {
        /// Validation failure message.
        message: String,
    },
    /// A format failed to render a configuration value back to bytes.
    Render {
        /// Format name, for example `"json"`.
        format: String,
        /// Underlying error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// A runtime override path was invalid.
    InvalidPath {
        /// Path as supplied by the caller.
        input: String,
        /// Reason the path was rejected.
        reason: String,
    },
    /// A file extension did not map to a known format.
    UnknownFormat {
        /// File path.
        path: PathBuf,
        /// Extension that was found, possibly empty.
        extension: String,
    },
    /// A source produced conflicting keys while building a nested map.
    SourceConflict {
        /// Source name.
        source: String,
        /// Description of the conflict.
        message: String,
    },
    /// `watch` was requested but no watchable sources are registered.
    NoWatchableSources,
    /// A watch backend failed.
    Watch {
        /// Error message.
        message: String,
        /// Optional underlying error.
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    /// An operation is not supported for a format or feature combination.
    Unsupported {
        /// Format name.
        format: String,
        /// Operation name.
        operation: &'static str,
    },
}

impl Error {
    /// Creates an [`Error::Source`] with a boxed underlying error.
    pub fn source_error(
        name: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Source {
            name: name.into(),
            source: Box::new(source),
        }
    }

    /// Creates an [`Error::Format`] with a boxed underlying error.
    pub fn format_error(
        format: impl Into<String>,
        path: Option<PathBuf>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Format {
            format: format.into(),
            path,
            source: Box::new(source),
        }
    }

    /// Creates an [`Error::Render`] with a boxed underlying error.
    pub fn render_error(
        format: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Render {
            format: format.into(),
            source: Box::new(source),
        }
    }

    /// Creates an [`Error::Validation`] with a message.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source { name, source } => {
                write!(f, "source `{name}` failed: {source}")
            }
            Self::Format {
                format,
                path,
                source,
            } => {
                write!(f, "failed to parse format `{format}`")?;
                if let Some(path) = path {
                    write!(f, " at `{}`", path.display())?;
                }
                write!(f, ": {source}")
            }
            Self::Io { path, source } => {
                write!(f, "I/O error at `{}`: {source}", path.display())
            }
            Self::InvalidRoot { source, actual } => {
                write!(
                    f,
                    "source `{source}` produced a non-object root of type `{actual}`"
                )
            }
            Self::PathNotFound { path } => {
                write!(f, "configuration path `{path}` not found")
            }
            Self::NotAMap { path, actual } => {
                write!(f, "path `{path}` is not an object (found `{actual}`)")
            }
            Self::InvalidIndex { path, segment } => {
                write!(f, "path `{path}` uses invalid array index `{segment}`")
            }
            Self::TypeMismatch {
                path,
                expected,
                actual,
            } => {
                write!(f, "expected `{expected}` at `{path}`, found `{actual}`")
            }
            Self::Coercion {
                path,
                value,
                target,
            } => {
                write!(f, "could not coerce `{value}` at `{path}` to `{target}`")
            }
            Self::Deserialize {
                path,
                field,
                message,
            } => {
                write!(f, "failed to deserialize at `{path}`")?;
                if let Some(field) = field {
                    write!(f, " (field `{field}`)")?;
                }
                write!(f, ": {message}")
            }
            Self::Serialize { message } => {
                write!(f, "failed to serialize configuration value: {message}")
            }
            Self::Render { format, source } => {
                write!(f, "failed to render format `{format}`: {source}")
            }
            Self::Validation { message } => {
                write!(f, "configuration validation failed: {message}")
            }
            Self::InvalidPath { input, reason } => {
                write!(f, "invalid path `{input}`: {reason}")
            }
            Self::UnknownFormat { path, extension } => {
                if extension.is_empty() {
                    write!(f, "unknown configuration format for `{}`", path.display())
                } else {
                    write!(
                        f,
                        "unknown configuration format `.{extension}` for `{}`",
                        path.display()
                    )
                }
            }
            Self::SourceConflict { source, message } => {
                write!(f, "source `{source}` has conflicting keys: {message}")
            }
            Self::NoWatchableSources => f.write_str(
                "no watchable sources are registered; use `ConfigBuilder::watched(...)` \
                 to add one",
            ),
            Self::Watch { message, source } => {
                write!(f, "watch error: {message}")?;
                if let Some(source) = source {
                    write!(f, ": {source}")?;
                }
                Ok(())
            }
            Self::Unsupported { format, operation } => {
                write!(
                    f,
                    "operation `{operation}` is not supported for format `{format}`"
                )
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source { source, .. } => Some(source.as_ref()),
            Self::Format { source, .. } => Some(source.as_ref()),
            Self::Render { source, .. } => Some(source.as_ref()),
            Self::Io { source, .. } => Some(source),
            Self::Watch {
                source: Some(source),
                ..
            } => Some(source.as_ref()),
            _ => None,
        }
    }
}
