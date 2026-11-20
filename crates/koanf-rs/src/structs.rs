//! Typed defaults source backed by any `serde::Serialize` value.

use std::any::type_name;

use koanf_core::{Error, Map, Result, Source, Value};
use serde::Serialize;

/// A configuration source that serializes a Rust value into an object.
///
/// This is useful for expressing typed defaults as the lowest-priority layer:
///
/// ```no_run
/// use koanf_rs::Config;
/// use koanf_rs::structs::Structs;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Serialize, Deserialize)]
/// struct Defaults {
///     host: String,
///     port: u16,
/// }
///
/// impl Default for Defaults {
///     fn default() -> Self {
///         Self {
///             host: "localhost".into(),
///             port: 8080,
///         }
///     }
/// }
///
/// let config = Config::builder()
///     .source(Structs::new(Defaults::default()).unwrap())
///     .build()
///     .unwrap();
///
/// assert_eq!(config.get::<String>("host").unwrap(), "localhost");
/// ```
#[derive(Clone, Debug)]
pub struct Structs {
    map: Map,
    name: String,
}

impl Structs {
    /// Serializes `value` eagerly and creates a source named `structs:<type>`.
    ///
    /// The serialized root must be an object. Serialization is eager, so
    /// later interior mutations of `value` are not reflected.
    pub fn new<T>(value: T) -> Result<Self>
    where
        T: Serialize,
    {
        Self::named(format!("structs:{}", type_name::<T>()), value)
    }

    /// Like [`Self::new`], with a custom source name.
    pub fn named<T>(name: impl Into<String>, value: T) -> Result<Self>
    where
        T: Serialize,
    {
        let name = name.into();
        let value = Value::from_serialize(&value)?;
        let actual = value.type_name();
        match value {
            Value::Map(map) => Ok(Self { map, name }),
            _ => Err(Error::InvalidRoot {
                source: name,
                actual,
            }),
        }
    }
}

impl Source for Structs {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<Map> {
        Ok(self.map.clone())
    }
}
