//! koanf-compatible name for the in-memory map source.

/// A map-backed configuration source.
///
/// This is an alias for [`crate::memory::Memory`], provided for familiarity
/// with koanf's `confmap` provider.
pub use crate::memory::Memory as ConfMap;
