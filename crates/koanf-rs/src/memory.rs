//! In-memory configuration source (koanf's `confmap` equivalent).

use koanf_core::{Error, IntoPath, Map, Result, Source, Value};

use crate::util::insert_path;

/// A configuration source backed by an in-memory object.
///
/// This is the equivalent of koanf's `confmap` provider. It is always
/// available, regardless of features.
#[derive(Clone, Debug)]
pub struct Memory {
    map: Map,
}

impl Memory {
    /// Creates a source from an object.
    pub fn new(map: Map) -> Self {
        Self { map }
    }

    /// Creates an empty source.
    pub fn empty() -> Self {
        Self { map: Map::new() }
    }

    /// Creates a source from a [`Value`], which must be an object.
    pub fn from_value(value: Value) -> Result<Self> {
        let actual = value.type_name();
        match value {
            Value::Map(map) => Ok(Self { map }),
            _ => Err(Error::InvalidRoot {
                source: "memory".to_owned(),
                actual,
            }),
        }
    }

    /// Creates a source from path/value pairs.
    ///
    /// Keys use [`IntoPath`] semantics: `"server.port"` creates a nested
    /// object. Use [`koanf_core::Path::key`] for a literal dotted key.
    #[allow(clippy::should_implement_trait)]
    pub fn from_iter<I, P, V>(pairs: I) -> Result<Self>
    where
        I: IntoIterator<Item = (P, V)>,
        P: IntoPath,
        V: Into<Value>,
    {
        let mut map = Map::new();
        for (path, value) in pairs {
            insert_path(&mut map, &path.into_path(), value.into(), "memory")?;
        }
        Ok(Self { map })
    }
}

impl Source for Memory {
    fn name(&self) -> &str {
        "memory"
    }

    fn read(&self) -> Result<Map> {
        Ok(self.map.clone())
    }
}
