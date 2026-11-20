//! Built-in configuration formats.
//!
//! Each format is gated behind its feature. All formats parse bytes into the
//! ordered [`Value`] tree; the source is responsible for requiring an object
//! root. JSON, JSON5, TOML, HCL, YAML, and RON also support rendering.

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

/// JSON5 format.
///
/// Accepts comments, unquoted keys, single-quoted strings, trailing commas,
/// hexadecimal numbers, and other JSON5 extensions.
#[cfg(feature = "json5")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Json5;

#[cfg(feature = "json5")]
impl Format for Json5 {
    fn name(&self) -> &str {
        "json5"
    }

    fn parse(&self, bytes: &[u8]) -> Result<Value> {
        let text = std::str::from_utf8(bytes)
            .map_err(|error| Error::format_error("json5", None, error))?;
        json5::from_str(text).map_err(|error| Error::format_error("json5", None, error))
    }

    fn render(&self, value: &Value) -> Result<Vec<u8>> {
        json5::to_string(value)
            .map(String::into_bytes)
            .map_err(|error| Error::render_error("json5", error))
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

/// HCL (HashiCorp Configuration Language) format.
///
/// Attributes become object keys. A single block becomes a nested object,
/// repeated unlabeled blocks become an array, and block labels become nested
/// object keys. HCL expressions and templates that cannot be represented as
/// data are preserved as interpolation strings such as `"${var.name}"`.
/// Numbers outside the `i64`/`u64` ranges are converted to `f64`.
///
/// Rendering emits attributes and object expressions; blocks are not
/// reconstructed. Interpolation-looking strings are escaped so they round-trip
/// as literal strings.
#[cfg(feature = "hcl")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Hcl;

#[cfg(feature = "hcl")]
impl Format for Hcl {
    fn name(&self) -> &str {
        "hcl"
    }

    fn parse(&self, bytes: &[u8]) -> Result<Value> {
        let text =
            std::str::from_utf8(bytes).map_err(|error| Error::format_error("hcl", None, error))?;
        let value: hcl::Value =
            hcl::from_str(text).map_err(|error| Error::format_error("hcl", None, error))?;
        Ok(hcl_value_to_value(value))
    }

    fn render(&self, value: &Value) -> Result<Vec<u8>> {
        hcl::to_string(value)
            .map(String::into_bytes)
            .map_err(|error| Error::render_error("hcl", error))
    }
}

#[cfg(feature = "hcl")]
fn hcl_value_to_value(value: hcl::Value) -> Value {
    match value {
        hcl::Value::Null => Value::Null,
        hcl::Value::Bool(value) => Value::Bool(value),
        hcl::Value::Number(value) => hcl_number_to_value(value),
        hcl::Value::String(value) => Value::String(value),
        hcl::Value::Array(values) => {
            Value::Array(values.into_iter().map(hcl_value_to_value).collect())
        }
        hcl::Value::Object(map) => Value::Map(
            map.into_iter()
                .map(|(key, value)| (key, hcl_value_to_value(value)))
                .collect(),
        ),
    }
}

#[cfg(feature = "hcl")]
fn hcl_number_to_value(value: hcl::Number) -> Value {
    if let Some(value) = value.as_u64() {
        Value::from(value)
    } else if let Some(value) = value.as_i64() {
        Value::from(value)
    } else if let Some(value) = value.as_f64() {
        Value::from(value)
    } else {
        Value::String(value.to_string())
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
