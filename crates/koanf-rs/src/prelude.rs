//! Core types for convenient importing.
//!
//! Built-in sources and formats are intentionally not re-exported here; import
//! them from their modules, for example `koanf_rs::file::File`.

pub use koanf_core::{
    ArrayMerge, Change, ChangeKind, ChangeSet, Coercion, Config, ConfigBuilder, Error, Format,
    IntoPath, Map, MergeOptions, NullMerge, Path, Result, Section, Snapshot, Source, Value,
    WatchEvent, WatchGuard, WatchSignal, Watchable,
};
