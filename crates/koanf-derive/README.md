# koanf-derive

Derive macros for [`koanf-rs`](https://crates.io/crates/koanf-rs) configuration
schemas.

```rust
use serde::Deserialize;

#[derive(Deserialize, koanf_rs::KoanfSchema)]
struct App {
    port: u16,
    #[koanf(secret)]
    password: String,
    #[koanf(nested)]
    database: Database,
}

#[derive(Deserialize, koanf_rs::KoanfSchema)]
struct Database {
    host: String,
    #[koanf(secret)]
    password: String,
}
```

The generated `Schema` implementation reports `password` and
`database.password` as secret paths for `ConfigBuilder::redact_schema`.

The `KoanfSchema` macro is re-exported by `koanf-rs` when its `derive` feature is
enabled.
