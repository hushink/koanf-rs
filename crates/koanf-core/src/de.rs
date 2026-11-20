//! Deserialization from a [`Value`] tree with optional scalar coercion.

use std::fmt;

use serde::de::value::BorrowedStrDeserializer;
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};

use crate::error::Error;
use crate::path::Path;
use crate::value::{Coercion, Value};

pub(crate) fn deserialize_value<T>(
    value: &Value,
    coercion: Coercion,
    path: Path,
) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    let deserializer = ValueDeserializer { value, coercion };
    let result: std::result::Result<T, serde_path_to_error::Error<DeError>> =
        serde_path_to_error::deserialize(deserializer);
    result.map_err(|error| map_error(error, path))
}

fn map_error(error: serde_path_to_error::Error<DeError>, path: Path) -> Error {
    let field = normalize_field(&error.path().to_string());
    let inner = error.into_inner();

    match inner {
        DeError::Type { expected, actual } if field.is_none() => Error::TypeMismatch {
            path,
            expected,
            actual,
        },
        DeError::Coercion { value, target } if field.is_none() => Error::Coercion {
            path,
            value,
            target,
        },
        other => Error::Deserialize {
            path,
            field,
            message: other.to_string(),
        },
    }
}

fn normalize_field(path: &str) -> Option<String> {
    let trimmed = path.trim_start_matches('.');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

#[derive(Debug)]
pub(crate) enum DeError {
    Type {
        expected: &'static str,
        actual: &'static str,
    },
    Coercion {
        value: String,
        target: &'static str,
    },
    Message(String),
}

impl fmt::Display for DeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Type { expected, actual } => {
                write!(formatter, "expected `{expected}`, found `{actual}`")
            }
            Self::Coercion { value, target } => {
                write!(formatter, "could not coerce string `{value}` to `{target}`")
            }
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for DeError {}

impl de::Error for DeError {
    fn custom<T>(message: T) -> Self
    where
        T: fmt::Display,
    {
        Self::Message(message.to_string())
    }
}

fn type_error<T>(actual: &Value, expected: &'static str) -> Result<T, DeError> {
    Err(DeError::Type {
        expected,
        actual: actual.type_name(),
    })
}

#[derive(Clone, Copy)]
struct ValueDeserializer<'de> {
    value: &'de Value,
    coercion: Coercion,
}

impl<'de> ValueDeserializer<'de> {
    fn is_lenient(self) -> bool {
        matches!(self.coercion, Coercion::Lenient)
    }

    fn signed<V>(self, visitor: V, target: &'static str) -> Result<V::Value, DeError>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Int(value) => visitor.visit_i64(*value),
            Value::Uint(value) if *value <= i64::MAX as u64 => visitor.visit_i64(*value as i64),
            Value::Uint(value) => Err(DeError::Message(format!(
                "value `{value}` is out of range for `{target}`"
            ))),
            Value::String(value) if self.is_lenient() => match value.parse::<i64>() {
                Ok(parsed) => visitor.visit_i64(parsed),
                Err(_) => Err(DeError::Coercion {
                    value: value.clone(),
                    target,
                }),
            },
            other => type_error(other, target),
        }
    }

    fn unsigned<V>(self, visitor: V, target: &'static str) -> Result<V::Value, DeError>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Int(value) if *value >= 0 => visitor.visit_u64(*value as u64),
            Value::Uint(value) => visitor.visit_u64(*value),
            Value::Int(value) => Err(DeError::Message(format!(
                "value `{value}` is out of range for `{target}`"
            ))),
            Value::String(value) if self.is_lenient() => match value.parse::<u64>() {
                Ok(parsed) => visitor.visit_u64(parsed),
                Err(_) => Err(DeError::Coercion {
                    value: value.clone(),
                    target,
                }),
            },
            other => type_error(other, target),
        }
    }

    fn float<V>(self, visitor: V, target: &'static str, single: bool) -> Result<V::Value, DeError>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Float(value) => {
                if single {
                    visitor.visit_f32(*value as f32)
                } else {
                    visitor.visit_f64(*value)
                }
            }
            Value::Int(value) => {
                if single {
                    visitor.visit_f32(*value as f32)
                } else {
                    visitor.visit_f64(*value as f64)
                }
            }
            Value::Uint(value) => {
                if single {
                    visitor.visit_f32(*value as f32)
                } else {
                    visitor.visit_f64(*value as f64)
                }
            }
            Value::String(value) if self.is_lenient() => {
                if single {
                    match value.parse::<f32>() {
                        Ok(parsed) => visitor.visit_f32(parsed),
                        Err(_) => Err(DeError::Coercion {
                            value: value.clone(),
                            target,
                        }),
                    }
                } else {
                    match value.parse::<f64>() {
                        Ok(parsed) => visitor.visit_f64(parsed),
                        Err(_) => Err(DeError::Coercion {
                            value: value.clone(),
                            target,
                        }),
                    }
                }
            }
            other => type_error(other, target),
        }
    }

    fn bytes<V>(self, visitor: V) -> Result<V::Value, DeError>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Array(values) => {
                let mut bytes = Vec::with_capacity(values.len());
                for value in values {
                    match value {
                        Value::Int(value) if (0..=u8::MAX as i64).contains(value) => {
                            bytes.push(*value as u8);
                        }
                        Value::Uint(value) if *value <= u64::from(u8::MAX) => {
                            bytes.push(*value as u8);
                        }
                        other => return type_error(other, "byte"),
                    }
                }
                visitor.visit_byte_buf(bytes)
            }
            other => type_error(other, "byte array"),
        }
    }
}

impl<'de> de::Deserializer<'de> for ValueDeserializer<'de> {
    type Error = DeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_unit(),
            Value::Bool(value) => visitor.visit_bool(*value),
            Value::Int(value) => visitor.visit_i64(*value),
            Value::Uint(value) => visitor.visit_u64(*value),
            Value::Float(value) => visitor.visit_f64(*value),
            Value::String(value) => visitor.visit_borrowed_str(value),
            Value::Array(values) => visitor.visit_seq(SeqAccessImpl {
                iter: values.iter(),
                coercion: self.coercion,
            }),
            Value::Map(values) => visitor.visit_map(MapAccessImpl {
                iter: values.iter(),
                value: None,
                coercion: self.coercion,
            }),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Bool(value) => visitor.visit_bool(*value),
            Value::String(value) if self.is_lenient() => {
                if value.eq_ignore_ascii_case("true") {
                    visitor.visit_bool(true)
                } else if value.eq_ignore_ascii_case("false") {
                    visitor.visit_bool(false)
                } else {
                    Err(DeError::Coercion {
                        value: value.clone(),
                        target: "bool",
                    })
                }
            }
            other => type_error(other, "bool"),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.signed(visitor, "i8")
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.signed(visitor, "i16")
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.signed(visitor, "i32")
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.signed(visitor, "i64")
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Int(value) => visitor.visit_i128(i128::from(*value)),
            Value::Uint(value) => visitor.visit_i128(i128::from(*value)),
            Value::String(value) if self.is_lenient() => match value.parse::<i128>() {
                Ok(parsed) => visitor.visit_i128(parsed),
                Err(_) => Err(DeError::Coercion {
                    value: value.clone(),
                    target: "i128",
                }),
            },
            other => type_error(other, "i128"),
        }
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.unsigned(visitor, "u8")
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.unsigned(visitor, "u16")
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.unsigned(visitor, "u32")
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.unsigned(visitor, "u64")
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Int(value) if *value >= 0 => visitor.visit_u128(*value as u128),
            Value::Uint(value) => visitor.visit_u128(u128::from(*value)),
            Value::Int(value) => Err(DeError::Message(format!(
                "value `{value}` is out of range for `u128`"
            ))),
            Value::String(value) if self.is_lenient() => match value.parse::<u128>() {
                Ok(parsed) => visitor.visit_u128(parsed),
                Err(_) => Err(DeError::Coercion {
                    value: value.clone(),
                    target: "u128",
                }),
            },
            other => type_error(other, "u128"),
        }
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.float(visitor, "f32", true)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.float(visitor, "f64", false)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) if value.chars().count() == 1 => {
                visitor.visit_char(value.chars().next().expect("length checked"))
            }
            other => type_error(other, "char"),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_borrowed_str(value),
            other => type_error(other, "string"),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.bytes(visitor)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_unit(),
            other => type_error(other, "null"),
        }
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Array(values) => visitor.visit_seq(SeqAccessImpl {
                iter: values.iter(),
                coercion: self.coercion,
            }),
            other => type_error(other, "array"),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Map(values) => visitor.visit_map(MapAccessImpl {
                iter: values.iter(),
                value: None,
                coercion: self.coercion,
            }),
            other => type_error(other, "object"),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(variant) => visitor.visit_enum(UnitEnumAccess { variant }),
            Value::Map(values) if values.len() == 1 => {
                let (variant, value) = values
                    .iter()
                    .next()
                    .expect("map length was checked to be one");
                visitor.visit_enum(MapEnumAccess {
                    variant,
                    value,
                    coercion: self.coercion,
                })
            }
            other => type_error(other, "string or single-entry object"),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn is_human_readable(&self) -> bool {
        true
    }
}

struct SeqAccessImpl<'de> {
    iter: std::slice::Iter<'de, Value>,
    coercion: Coercion,
}

impl<'de> SeqAccess<'de> for SeqAccessImpl<'de> {
    type Error = DeError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => seed
                .deserialize(ValueDeserializer {
                    value,
                    coercion: self.coercion,
                })
                .map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

struct MapAccessImpl<'de> {
    iter: indexmap::map::Iter<'de, String, Value>,
    value: Option<&'de Value>,
    coercion: Coercion,
}

impl<'de> MapAccess<'de> for MapAccessImpl<'de> {
    type Error = DeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                seed.deserialize(BorrowedStrDeserializer::<DeError>::new(key))
                    .map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let value = self
            .value
            .take()
            .ok_or_else(|| DeError::Message("value requested before key".to_owned()))?;
        seed.deserialize(ValueDeserializer {
            value,
            coercion: self.coercion,
        })
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

struct UnitEnumAccess<'de> {
    variant: &'de str,
}

impl<'de> EnumAccess<'de> for UnitEnumAccess<'de> {
    type Error = DeError;
    type Variant = UnitVariantAccess;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(BorrowedStrDeserializer::<DeError>::new(self.variant))?;
        Ok((variant, UnitVariantAccess))
    }
}

struct UnitVariantAccess;

impl<'de> VariantAccess<'de> for UnitVariantAccess {
    type Error = DeError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, _seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        Err(DeError::Message(
            "expected a unit enum variant, found a newtype variant".to_owned(),
        ))
    }

    fn tuple_variant<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DeError::Message(
            "expected a unit enum variant, found a tuple variant".to_owned(),
        ))
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DeError::Message(
            "expected a unit enum variant, found a struct variant".to_owned(),
        ))
    }
}

struct MapEnumAccess<'de> {
    variant: &'de str,
    value: &'de Value,
    coercion: Coercion,
}

impl<'de> EnumAccess<'de> for MapEnumAccess<'de> {
    type Error = DeError;
    type Variant = MapVariantAccess<'de>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = seed.deserialize(BorrowedStrDeserializer::<DeError>::new(self.variant))?;
        Ok((
            variant,
            MapVariantAccess {
                value: self.value,
                coercion: self.coercion,
            },
        ))
    }
}

struct MapVariantAccess<'de> {
    value: &'de Value,
    coercion: Coercion,
}

impl<'de> VariantAccess<'de> for MapVariantAccess<'de> {
    type Error = DeError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        match self.value {
            Value::Null => Ok(()),
            other => type_error(other, "null"),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(ValueDeserializer {
            value: self.value,
            coercion: self.coercion,
        })
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Array(values) => visitor.visit_seq(SeqAccessImpl {
                iter: values.iter(),
                coercion: self.coercion,
            }),
            other => type_error(other, "array"),
        }
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Map(values) => visitor.visit_map(MapAccessImpl {
                iter: values.iter(),
                value: None,
                coercion: self.coercion,
            }),
            other => type_error(other, "object"),
        }
    }
}
