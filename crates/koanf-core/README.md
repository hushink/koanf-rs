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

See the [repository README](https://github.com/hushink/koanf-rs) for the full
documentation and examples.
