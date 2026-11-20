# koanf-core

Core engine for [koanf-rs](https://github.com/hushink/koanf-rs).

This crate contains the value/path model, layered merge and diff logic, typed
serde extraction, immutable snapshots, runtime overrides, and the `Source`,
`Format`, and `Watchable` extension traits.

Most users should depend on `koanf-rs`, which re-exports this crate and adds
built-in sources and formats.

```toml
[dependencies]
koanf-core = "0.1"
```

## `no_std`

The crate supports `no_std` targets with `alloc`:

```toml
[dependencies]
koanf-core = { version = "0.1", default-features = false }
```

The value model, paths, merging, diffing, snapshots, typed serde access, runtime
overrides, and the synchronous configuration engine work without `std`.
Watching, saving, and `tracing` require the default `std` feature.

Without `std`, `Map` uses a deterministic FNV-1a hasher because `IndexMap`
requires an explicit hasher; use `Map::default()` rather than `Map::new()`.

See the [repository README](https://github.com/hushink/koanf-rs) for the full
documentation and examples.
