//! Dynamically typed configuration values.

use std::fmt;

use indexmap::IndexMap;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Result};

/// Ordered map used for configuration objects.
///
/// Insertion order is preserved. Later inserts of an existing key keep the
/// original position and replace the value.
pub type Map = IndexMap<String, Value>;

/// How string values are treated when deserializing into typed values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Coercion {
    /// Convert strings into the requested numeric or boolean type when the
    /// target type asks for one. This is the default.
    #[default]
    Lenient,
    /// Never convert strings into numeric or boolean values.
    Strict,
}

/// A configuration value.
///
/// Values form a tree whose objects use an ordered [`Map`]. Numeric values are
/// kept as [`Value::Int`], [`Value::Uint`], or [`Value::Float`]. Positive
/// integers that fit in `i64` are canonicalized to [`Value::Int`]; `Uint` is
/// only used for values greater than `i64::MAX`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// The absence of a value.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed 64-bit integer.
    Int(i64),
    /// An unsigned integer larger than `i64::MAX`.
    Uint(u64),
    /// A 64-bit floating point number.
    Float(f64),
    /// A UTF-8 string.
    String(String),
    /// An ordered array.
    Array(Vec<Value>),
    /// An ordered object.
    Map(Map),
}

impl Value {
    /// Returns a short name for the value's type, used in error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "integer",
            Self::Uint(_) => "unsigned integer",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Map(_) => "object",
        }
    }

    /// Returns `true` for [`Value::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns the boolean value, if any.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the signed integer value, if any.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the unsigned integer value if it is non-negative and fits.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Uint(value) => Some(*value),
            Self::Int(value) if *value >= 0 => Some(*value as u64),
            _ => None,
        }
    }

    /// Returns the numeric value as `f64` if it is numeric.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(*value),
            Self::Int(value) => Some(*value as f64),
            Self::Uint(value) => Some(*value as f64),
            _ => None,
        }
    }

    /// Returns the string value, if any.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the array value, if any.
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the object value, if any.
    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Self::Map(value) => Some(value),
            _ => None,
        }
    }

    /// Serializes any [`serde::Serialize`] value into a configuration value.
    ///
    /// Map keys must serialize as strings.
    pub fn from_serialize<T>(value: &T) -> Result<Value>
    where
        T: Serialize + ?Sized,
    {
        crate::ser::to_value(value)
    }

    pub(crate) fn canonical_uint(value: u64) -> Value {
        if value <= i64::MAX as u64 {
            Value::Int(value as i64)
        } else {
            Value::Uint(value)
        }
    }

    pub(crate) fn canonical_i128(value: i128) -> Result<Value> {
        if let Ok(value) = i64::try_from(value) {
            Ok(Value::Int(value))
        } else if let Ok(value) = u64::try_from(value) {
            Ok(Value::canonical_uint(value))
        } else {
            Err(Error::Serialize {
                message: format!("integer `{value}` does not fit in the configuration value model"),
            })
        }
    }

    pub(crate) fn canonical_u128(value: u128) -> Result<Value> {
        let value = u64::try_from(value).map_err(|_| Error::Serialize {
            message: format!("integer `{value}` does not fit in the configuration value model"),
        })?;
        Ok(Value::canonical_uint(value))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::Uint(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value}"),
            Self::String(value) => write!(f, "{value}"),
            Self::Array(_) => f.write_str("[array]"),
            Self::Map(_) => f.write_str("{object}"),
        }
    }
}

macro_rules! impl_from_signed {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for Value {
                fn from(value: $ty) -> Self {
                    Self::Int(value as i64)
                }
            }
        )*
    };
}

macro_rules! impl_from_unsigned {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for Value {
                fn from(value: $ty) -> Self {
                    Self::canonical_uint(value as u64)
                }
            }
        )*
    };
}

impl_from_signed!(i8, i16, i32, i64, isize);
impl_from_unsigned!(u8, u16, u32, u64, usize);

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<f32> for Value {
    fn from(value: f32) -> Self {
        Self::Float(value as f64)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<&String> for Value {
    fn from(value: &String) -> Self {
        Self::String(value.clone())
    }
}

impl From<char> for Value {
    fn from(value: char) -> Self {
        Self::String(value.to_string())
    }
}

impl From<()> for Value {
    fn from((): ()) -> Self {
        Self::Null
    }
}

impl From<Map> for Value {
    fn from(value: Map) -> Self {
        Self::Map(value)
    }
}

impl<T> From<Vec<T>> for Value
where
    T: Into<Value>,
{
    fn from(value: Vec<T>) -> Self {
        Self::Array(value.into_iter().map(Into::into).collect())
    }
}

impl<T> From<Option<T>> for Value
where
    T: Into<Value>,
{
    fn from(value: Option<T>) -> Self {
        match value {
            Some(value) => value.into(),
            None => Self::Null,
        }
    }
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Int(value) => serializer.serialize_i64(*value),
            Self::Uint(value) => serializer.serialize_u64(*value),
            Self::Float(value) => serializer.serialize_f64(*value),
            Self::String(value) => serializer.serialize_str(value),
            Self::Array(value) => value.serialize(serializer),
            Self::Map(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a configuration value")
    }

    fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
        Ok(Value::Int(value))
    }

    fn visit_i128<E>(self, value: i128) -> std::result::Result<Self::Value, E>
    where
        E: de::Error,
    {
        Value::canonical_i128(value).map_err(E::custom)
    }

    fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
        Ok(Value::canonical_uint(value))
    }

    fn visit_u128<E>(self, value: u128) -> std::result::Result<Self::Value, E>
    where
        E: de::Error,
    {
        Value::canonical_u128(value).map_err(E::custom)
    }

    fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E> {
        Ok(Value::Float(value))
    }

    fn visit_char<E>(self, value: char) -> std::result::Result<Self::Value, E> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_bytes<E>(self, value: &[u8]) -> std::result::Result<Self::Value, E> {
        Ok(Value::Array(
            value
                .iter()
                .map(|byte| Value::Uint(u64::from(*byte)))
                .collect(),
        ))
    }

    fn visit_byte_buf<E>(self, value: Vec<u8>) -> std::result::Result<Self::Value, E> {
        Ok(Value::Array(
            value
                .into_iter()
                .map(|byte| Value::Uint(u64::from(byte)))
                .collect(),
        ))
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer)
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0));
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::with_capacity(map.size_hint().unwrap_or(0));
        while let Some((key, value)) = map.next_entry::<String, Value>()? {
            values.insert(key, value);
        }
        Ok(Value::Map(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_small_uints_to_int() {
        assert_eq!(Value::from(1_u64), Value::Int(1));
        assert_eq!(Value::from(u64::MAX), Value::Uint(u64::MAX));
    }

    #[test]
    fn deserializes_from_json_preserving_order() {
        let value: Value = serde_json::from_str(r#"{"b":1,"a":2}"#).unwrap();
        let map = value.as_map().unwrap();
        assert_eq!(map.keys().collect::<Vec<_>>(), vec!["b", "a"]);
        assert_eq!(map["a"], Value::Int(2));
    }

    #[test]
    fn from_serialize_nested() {
        #[derive(serde::Serialize)]
        struct Inner {
            enabled: bool,
        }
        #[derive(serde::Serialize)]
        struct Outer {
            name: String,
            inner: Inner,
        }

        let value = Value::from_serialize(&Outer {
            name: "demo".into(),
            inner: Inner { enabled: true },
        })
        .unwrap();

        let map = value.as_map().unwrap();
        assert_eq!(map["name"], Value::String("demo".into()));
        assert_eq!(map["inner"].as_map().unwrap()["enabled"], Value::Bool(true));
    }
}
