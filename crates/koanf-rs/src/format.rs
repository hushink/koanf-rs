//! Built-in configuration formats.
//!
//! Each format is gated behind its feature. All formats parse bytes into the
//! ordered [`Value`] tree; the source is responsible for requiring an object
//! root. JSON, TOML, YAML, and RON also support rendering.

#[cfg(any(feature = "dotenv", feature = "ini"))]
use koanf_core::Map;
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

/// RON (Rusty Object Notation) format.
#[cfg(feature = "ron")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Ron;

#[cfg(feature = "ron")]
impl Format for Ron {
    fn name(&self) -> &str {
        "ron"
    }

    fn parse(&self, bytes: &[u8]) -> Result<Value> {
        let text =
            std::str::from_utf8(bytes).map_err(|error| Error::format_error("ron", None, error))?;
        ron::from_str(text).map_err(|error| Error::format_error("ron", None, error))
    }

    fn render(&self, value: &Value) -> Result<Vec<u8>> {
        ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
            .map(String::into_bytes)
            .map_err(|error| Error::render_error("ron", error))
    }
}

/// dotenv (`.env`) format.
///
/// Parsed as a flat object of string values. `${VAR}` substitution checks the
/// process environment first and then the values seen earlier in the file.
/// Duplicate keys use the last occurrence.
#[cfg(feature = "dotenv")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Dotenv;

#[cfg(feature = "dotenv")]
impl Format for Dotenv {
    fn name(&self) -> &str {
        "dotenv"
    }

    fn parse(&self, bytes: &[u8]) -> Result<Value> {
        let mut map = Map::new();
        for entry in dotenvy::from_read_iter(bytes) {
            let (key, value) = entry.map_err(|error| Error::format_error("dotenv", None, error))?;
            map.insert(key, Value::String(value));
        }
        Ok(Value::Map(map))
    }
}

/// INI format.
///
/// Keys in the global section live at the root; `[section]` values nest under
/// the section name. Values are strings and duplicate sections merge, with
/// later keys and sections taking precedence.
#[cfg(feature = "ini")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Ini;

#[cfg(feature = "ini")]
impl Format for Ini {
    fn name(&self) -> &str {
        "ini"
    }

    fn parse(&self, bytes: &[u8]) -> Result<Value> {
        let text =
            std::str::from_utf8(bytes).map_err(|error| Error::format_error("ini", None, error))?;
        let ini = ini::Ini::load_from_str(text)
            .map_err(|error| Error::format_error("ini", None, error))?;
        let mut root = Map::new();
        for (section, properties) in &ini {
            let values: Map = properties
                .iter()
                .map(|(key, value)| (key.to_owned(), Value::String(value.to_owned())))
                .collect();
            match section {
                Some(name) => match root.get_mut(name) {
                    Some(Value::Map(existing)) => existing.extend(values),
                    _ => {
                        root.insert(name.to_owned(), Value::Map(values));
                    }
                },
                None => root.extend(values),
            }
        }
        Ok(Value::Map(root))
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
