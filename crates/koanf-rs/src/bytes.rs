//! Inline byte source with an explicit format.

use std::sync::Arc;

use koanf_core::{Error, Format, Map, Result, Source, Value};

use crate::util::strip_bom;

/// A source backed by in-memory bytes and an explicit [`Format`].
///
/// This is useful for embedding configuration in tests or in code paths that
/// already have the raw bytes.
pub struct Bytes {
    data: Vec<u8>,
    format: Arc<dyn Format>,
    name: String,
}

impl Bytes {
    /// Creates a source from bytes and a format.
    pub fn new(data: impl AsRef<[u8]>, format: impl Format + 'static) -> Self {
        Self {
            data: data.as_ref().to_vec(),
            format: Arc::new(format),
            name: "bytes".to_owned(),
        }
    }

    /// Sets a custom source name used in error messages.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

impl Source for Bytes {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        let data = strip_bom(&self.data);
        let value = self.format.parse(data)?;
        let actual = value.type_name();
        match value {
            Value::Map(map) => Ok(map),
            _ => Err(Error::InvalidRoot {
                source: self.name.clone(),
                actual,
            }),
        }
    }
}
