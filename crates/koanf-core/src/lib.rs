//! Core engine for `koanf-rs`.
//!
//! This crate provides the value model, path model, layered merge semantics,
//! typed serde extraction, immutable snapshots, runtime overrides, and the
//! source/format/watch extension traits. The [`koanf_rs`] facade adds built-in
//! sources and formats.
//!
//! The central workflow is:
//!
//! 1. Build a [`Config`] with [`ConfigBuilder`], appending sources in priority
//!    order (later sources override earlier ones).
//! 2. Read values through a [`Snapshot`] or the convenience methods on
//!    [`Config`].
//! 3. Optionally register watch callbacks or channels and receive
//!    `WatchEvent` values when sources change (requires the `std` feature).
//!
//! # Features
//!
//! The default `std` feature enables filesystem saves, watch dispatch, and
//! `tracing`. Build with `default-features = false` for `no_std` targets with
//! `alloc`; the value model, paths, merging, diffing, snapshots, typed serde
//! access, runtime overrides, and the synchronous configuration engine remain
//! available. Without `std`, [`Map`] uses a deterministic FNV-1a hasher, so
//! construct empty maps with [`Map::default`].
//!
//! [`koanf_rs`]: https://crates.io/crates/koanf-rs

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod config;
mod de;
mod diff;
mod error;
mod merge;
mod path;
mod redact;
mod schema;
mod ser;
mod source;
mod value;
#[cfg(feature = "std")]
mod watch;

pub use config::{Config, ConfigBuilder, Section, Snapshot};
pub use diff::{Change, ChangeKind, ChangeSet};
pub use error::{Error, Result};
pub use merge::{ArrayMerge, MergeOptions, NullMerge, merge_map};
pub use path::{IntoPath, Path};
pub use schema::Schema;
pub use source::{
    AsyncSource, Format, Source, WatchCallback, WatchSignal, WatchSubscription, Watchable,
};
pub use value::{Coercion, Map, Value};
#[cfg(all(feature = "std", feature = "async"))]
pub use watch::WatchStream;
#[cfg(feature = "std")]
pub use watch::{WatchEvent, WatchGuard};

/// Re-export of the ordered map type used by [`Map`].
pub use indexmap::IndexMap;
