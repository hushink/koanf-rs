//! Configuration key paths.
//!
//! Paths are stored as a sequence of string segments. The string convenience
//! form (`"server.port"`) is split on `.`; a literal dot inside a single key
//! must be expressed with [`Path::key`].

use std::fmt;

/// A configuration key path made of zero or more string segments.
///
/// An empty path ([`Path::root`]) refers to the root object of a snapshot.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Path(Vec<String>);

impl Path {
    /// Creates the root path.
    pub fn root() -> Self {
        Self(Vec::new())
    }

    /// Creates a path from string segments.
    pub fn new<I, S>(segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self(segments.into_iter().map(Into::into).collect())
    }

    /// Creates a single-segment path. Dots in `segment` are literal.
    pub fn key(segment: impl Into<String>) -> Self {
        Self(vec![segment.into()])
    }

    /// Parses a dotted path, splitting on `.`.
    ///
    /// An empty string produces the root path.
    pub fn parse(dotted: &str) -> Self {
        Self::parse_with(dotted, '.')
    }

    /// Parses a path using a custom delimiter.
    pub fn parse_with(dotted: &str, delimiter: char) -> Self {
        if dotted.is_empty() {
            Self::root()
        } else {
            Self(dotted.split(delimiter).map(ToOwned::to_owned).collect())
        }
    }

    /// Returns the path segments.
    pub fn segments(&self) -> &[String] {
        &self.0
    }

    /// Returns `true` if this is the root path.
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// Appends a segment.
    pub fn push(&mut self, segment: impl Into<String>) {
        self.0.push(segment.into());
    }

    /// Returns a new path with `segment` appended.
    pub fn child(&self, segment: impl Into<String>) -> Self {
        let mut path = self.clone();
        path.push(segment);
        path
    }

    /// Concatenates two paths.
    pub fn join(&self, other: &Path) -> Self {
        let mut path = self.clone();
        path.0.extend(other.0.iter().cloned());
        path
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str("<root>")
        } else {
            f.write_str(&self.0.join("."))
        }
    }
}

impl fmt::Debug for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Path").field(&self.0).finish()
    }
}

impl From<Vec<String>> for Path {
    fn from(segments: Vec<String>) -> Self {
        Self(segments)
    }
}

/// Conversion into a [`Path`].
///
/// Implemented for `&str` (split on `.`), [`String`], [`Path`], string slices,
/// vectors, and fixed-size string arrays.
pub trait IntoPath {
    /// Converts `self` into a path.
    fn into_path(self) -> Path;
}

impl IntoPath for Path {
    fn into_path(self) -> Path {
        self
    }
}

impl IntoPath for &Path {
    fn into_path(self) -> Path {
        self.clone()
    }
}

impl IntoPath for String {
    fn into_path(self) -> Path {
        Path::parse(&self)
    }
}

impl IntoPath for &String {
    fn into_path(self) -> Path {
        Path::parse(self)
    }
}

impl IntoPath for &str {
    fn into_path(self) -> Path {
        Path::parse(self)
    }
}

impl IntoPath for Vec<String> {
    fn into_path(self) -> Path {
        Path::new(self)
    }
}

impl IntoPath for &[String] {
    fn into_path(self) -> Path {
        Path::new(self.iter().cloned())
    }
}

impl IntoPath for &[&str] {
    fn into_path(self) -> Path {
        Path::new(self.iter().copied())
    }
}

impl<const N: usize> IntoPath for [&str; N] {
    fn into_path(self) -> Path {
        Path::new(self)
    }
}

impl<const N: usize> IntoPath for [String; N] {
    fn into_path(self) -> Path {
        Path::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_displays() {
        let path = Path::parse("server.port");
        assert_eq!(path.segments(), &["server".to_owned(), "port".to_owned()]);
        assert_eq!(path.to_string(), "server.port");
        assert_eq!(Path::root().to_string(), "<root>");
    }

    #[test]
    fn key_keeps_dots_literal() {
        let path = Path::key("a.b");
        assert_eq!(path.segments(), &["a.b".to_owned()]);
    }

    #[test]
    fn joins_paths() {
        let path = Path::new(["a"]).join(&Path::new(["b", "c"]));
        assert_eq!(path.to_string(), "a.b.c");
    }
}
