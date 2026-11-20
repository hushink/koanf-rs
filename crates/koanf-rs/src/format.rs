//! Built-in configuration formats.
//!
//! Each format is gated behind its feature. All formats parse bytes into the
//! ordered [`Value`] tree; the source is responsible for requiring an object
//! root.

use koanf_core::{Error, Format, Result, Value};

/// JSON format.
#[cfg(feature = "json")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Json;

#[cfg(feature = "json")]
impl Format for Json {
    fn name(&self) -> &str {
        "json"
    }

    fn parse(&self, bytes: &[u8]) -> Result<Value> {
        serde_json::from_slice(bytes).map_err(|error| Error::format_error("json", None, error))
    }

    fn render(&self, value: &Value) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(value).map_err(|error| Error::render_error("json", error))
    }
}

/// TOML format.
#[cfg(feature = "toml")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Toml;

#[cfg(feature = "toml")]
impl Format for Toml {
    fn name(&self) -> &str {
        "toml"
    }

    fn parse(&self, bytes: &[u8]) -> Result<Value> {
        let text =
            std::str::from_utf8(bytes).map_err(|error| Error::format_error("toml", None, error))?;
        let table: toml::Table =
            toml::from_str(text).map_err(|error| Error::format_error("toml", None, error))?;
        Ok(toml_table_to_value(table))
    }

    fn render(&self, value: &Value) -> Result<Vec<u8>> {
        toml::to_string_pretty(value)
            .map(String::into_bytes)
            .map_err(|error| Error::render_error("toml", error))
    }
}

#[cfg(feature = "toml")]
fn toml_table_to_value(table: toml::Table) -> Value {
    Value::Map(
        table
            .into_iter()
            .map(|(key, value)| (key, toml_value_to_value(value)))
            .collect(),
    )
}

#[cfg(feature = "toml")]
fn toml_value_to_value(value: toml::Value) -> Value {
    match value {
        toml::Value::String(value) => Value::String(value),
        toml::Value::Integer(value) => Value::Int(value),
        toml::Value::Float(value) => Value::Float(value),
        toml::Value::Boolean(value) => Value::Bool(value),
        toml::Value::Datetime(value) => Value::String(value.to_string()),
        toml::Value::Array(values) => {
            Value::Array(values.into_iter().map(toml_value_to_value).collect())
        }
        toml::Value::Table(table) => toml_table_to_value(table),
    }
}

/// YAML format.
#[cfg(feature = "yaml")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Yaml;

#[cfg(feature = "yaml")]
impl Format for Yaml {
    fn name(&self) -> &str {
        "yaml"
    }

    fn parse(&self, bytes: &[u8]) -> Result<Value> {
        serde_yaml_ng::from_slice(bytes).map_err(|error| Error::format_error("yaml", None, error))
    }

    fn render(&self, value: &Value) -> Result<Vec<u8>> {
        serde_yaml_ng::to_string(value)
            .map(String::into_bytes)
            .map_err(|error| Error::render_error("yaml", error))
    }
}
