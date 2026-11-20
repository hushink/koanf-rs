//! Typed configuration schemas.

use alloc::string::String;
use alloc::vec::Vec;

use serde::de::DeserializeOwned;

/// A typed configuration schema.
///
/// Derive this trait with `koanf_rs::KoanfSchema` (enable the `derive` feature)
/// to collect secret field paths automatically. Implementations are otherwise
/// ordinary [`serde::Deserialize`] types.
pub trait Schema: DeserializeOwned {
    /// Human-readable schema name.
    const NAME: &'static str;

    /// Returns paths that should be redacted from debug output.
    ///
    /// Paths are dotted and relative to the schema root. The default is empty.
    fn secret_paths() -> Vec<String> {
        Vec::new()
    }
}
