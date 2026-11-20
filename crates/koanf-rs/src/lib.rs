//! Rust configuration library with layered sources, multiple formats, typed
//! serde access, and file watching.
//!
//! `koanf-rs` is an independent Rust implementation inspired by Go's
//! [koanf](https://github.com/knadh/koanf). It is not an official port.
//!
//! # Quick start
//!
//! ```no_run
//! use koanf_rs::Config;
//! use koanf_rs::Value;
//! use koanf_rs::memory::Memory;
//! use serde::Deserialize;
//!
//! #[derive(Debug, Deserialize)]
//! struct Server {
//!     host: String,
//!     port: u16,
//! }
//!
//! #[derive(Debug, Deserialize)]
//! struct App {
//!     server: Server,
//! }
//!
//! let config = Config::builder()
//!     .source(
//!         Memory::from_iter([
//!             ("server.host", Value::from("localhost")),
//!             ("server.port", Value::from(8080)),
//!         ])
//!         .unwrap(),
//!     )
//!     .build()
//!     .unwrap();
//!
//! let app: App = config.deserialize().unwrap();
//! assert!(!app.server.host.is_empty());
//! ```
//!
//! # Features
//!
//! The default features enable `json`, `toml`, `yaml`, `file`, `env`, `args`,
//! and `watch`. The optional `dotenv` and `ini` features add those parsers.
//! `memory`, `structs`, `confmap`, and `bytes` are always available. Use
//! `default-features = false` for a minimal core.
//!
//! The [`prelude`] re-exports the core types; built-in sources and formats
//! live in their own modules to avoid name collisions with `std`.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub use koanf_core::*;

#[cfg(feature = "args")]
pub mod args;
pub mod bytes;
pub mod confmap;
#[cfg(feature = "file")]
pub mod dir;
#[cfg(feature = "env")]
pub mod env;
#[cfg(feature = "file")]
pub mod file;
#[cfg(any(
    feature = "json",
    feature = "toml",
    feature = "yaml",
    feature = "dotenv",
    feature = "ini"
))]
pub mod format;
#[cfg(feature = "http")]
pub mod http;
pub mod memory;
pub mod prelude;
pub mod structs;
mod util;
