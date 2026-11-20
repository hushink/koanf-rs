# koanf-rs

[![CI](https://github.com/hushink/koanf-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/hushink/koanf-rs/actions/workflows/ci.yml)
[![docs.rs](https://docs.rs/koanf-rs/badge.svg)](https://docs.rs/koanf-rs)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A layered configuration library for Rust, inspired by Go's
[koanf](https://github.com/knadh/koanf). This is an **independent Rust
implementation**, not an official port.

- Multiple sources: memory, typed defaults, files and directories, environment
  variables, CLI arguments, and inline bytes.
- Multiple formats: JSON, TOML, and YAML.
- Layered precedence with deep merge, configurable array/null behavior, and
  persistent runtime overrides.
- Typed access through serde: `deserialize::<T>()`, `get::<T>(path)`, and
  path-scoped `Section` views.
- Immutable, `Arc`-backed snapshots for consistent multi-key reads.
- File watching with full rebuilds, change sets, and last-known-good snapshots.
- Compile-time enforcement that only `Watchable` sources can be watched.
- Extension traits for custom sources, formats, and watch backends.
- The library crates forbid `unsafe` code.

## Install

```bash
cargo add koanf-rs
```

For a minimal build without built-in formats or local sources:

```bash
cargo add koanf-rs --no-default-features
```

## Quick start

```rust
use koanf_rs::prelude::*;
use koanf_rs::structs::Structs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
struct Defaults {
    server: Server,
}

fn main() -> Result<()> {
    let defaults = Defaults {
        server: Server {
            host: "localhost".to_owned(),
            port: 8080,
        },
    };

    let config = Config::builder()
        .source(Structs::new(defaults)?)
        .build()?;

    let port: u16 = config.get("server.port")?;
    assert_eq!(port, 8080);

    let server = config.section("server")?;
    assert_eq!(server.get::<String>("host")?, "localhost");

    Ok(())
}
```

## Layered defaults and sources

Later sources override earlier ones. A typical setup puts typed defaults first,
then optional files, environment variables, and CLI arguments:

```rust
use koanf_rs::args::Args;
use koanf_rs::env::Env;
use koanf_rs::file::File;
use koanf_rs::prelude::*;
use koanf_rs::structs::Structs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Defaults {
    server: Server,
}

#[derive(Debug, Serialize, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

fn main() -> Result<()> {
    let config = Config::builder()
        .source(Structs::new(Defaults {
            server: Server {
                host: "localhost".to_owned(),
                port: 8080,
            },
        })?)
        .source(File::new("config.toml").optional())
        .source(Env::prefixed("APP_"))
        .source(Args::new())
        .build()?;

    // APP_SERVER__PORT=9090 wins over the file and the defaults.
    let port: u16 = config.get("server.port")?;
    println!("port = {port}");
    Ok(())
}
```

`Memory` is the in-memory map source and is also available as
`confmap::ConfMap` for koanf familiarity.

`dir::Dir` loads one directory as a source. Regular files are merged in
lexicographic filename order, so `10-base.toml` / `20-overrides.toml` style
names provide deterministic layering within the directory. By default only
files with an enabled built-in extension are read; `.format(...)` applies one
explicit format to every regular file instead.

## Reading values

```rust
let app: App = config.deserialize()?;
let port: u16 = config.get("server.port")?;
let optional = config.get_opt::<String>("missing")?; // None
let fallback: u16 = config.get_or("server.port", 8080)?;
```

Missing paths and explicit `null` values are distinct:

- `get` fails with `PathNotFound` for a missing path.
- `get_opt` returns `None` only for a missing path.
- `get_or` uses the default only for a missing path.
- An explicit null is deserialized normally; `Option<T>` accepts it.

`Section` views make typed sub-configs convenient:

```rust
let database = config.section("database")?;
let url: String = database.get("url")?;
```

### Rendering

Built-in formats render complete configurations and sections back to bytes:

```rust
use koanf_rs::format::{Json, Yaml};

let json = config.render(&Json)?;
let server_yaml = config.section("server")?.render(&Yaml)?;
```

Formats reject values they cannot represent. For example, TOML has no null
type, so rendering a snapshot containing `Value::Null` returns `Error::Render`.
Use `std::fs` to persist the returned bytes if needed.

## Watch

```rust
use koanf_rs::file::File;
use koanf_rs::prelude::*;

fn main() -> Result<()> {
    let config = Config::builder()
        .watched(File::new("config.toml"))
        .build()?;

    let (guard, events) = config.watch_channel()?;
    for event in events {
        match event {
            WatchEvent::Changed { snapshot, changes } => {
                println!(
                    "value = {:?}, {} change(s)",
                    snapshot.get::<String>("server.host"),
                    changes.len()
                );
            }
            WatchEvent::Error { error } => {
                eprintln!("watch error (keeping last good snapshot): {error}");
            }
        }
    }

    drop(guard);
    Ok(())
}
```

Watching re-reads **all** sources, rebuilds the merged snapshot, and emits a
change set only when the result differs. Parse or I/O failures keep the previous
snapshot and emit `WatchEvent::Error`.

### Async watch stream

Enable the `async` feature to receive events as a runtime-agnostic stream:

```rust
let (guard, mut events) = config.watch_stream()?;
while let Some(event) = events.recv().await {
    match event {
        WatchEvent::Changed { snapshot, changes } => { /* ... */ }
        WatchEvent::Error { error } => eprintln!("watch error: {error}"),
    }
}
```

`WatchStream` implements `futures_core::Stream`; `recv` is an inherent async
method, so you can use it with any executor or with `StreamExt`.

### HTTP source

The optional `http` feature adds a source with conditional requests, gzip /
brotli / deflate / zstd decompression, and polling-based watch:

```rust
use koanf_rs::http::Http;
use koanf_rs::prelude::*;
use std::time::Duration;

let source = Http::new("https://example.com/config.json")?
    .header("X-Api-Key", "secret")?
    .timeout(Duration::from_secs(10))
    .poll_interval(Duration::from_secs(30));

let config = Config::builder().watched(source).build()?;
```

`Http` sends `If-None-Match` / `If-Modified-Since` on reloads and falls back to
a SHA-256 content hash. The watch loop polls at `poll_interval` (default 30s)
and emits an event only when the content actually changes.

## Sources

| Source | Feature | Description |
|---|---|---|
| `memory::Memory` / `confmap::ConfMap` | always | In-memory object |
| `structs::Structs` | always | Serializes any `serde::Serialize` value; ideal for typed defaults |
| `bytes::Bytes` | always | Inline bytes with an explicit format |
| `file::File` | `file` | Single file; format inferred from extension or set explicitly |
| `dir::Dir` | `file` | Directory of files merged in filename order; non-recursive |
| `env::Env` | `env` | Prefixed environment variables with `__`-separated paths |
| `args::Args` | `args` | `--key=value`, `--key value`, and `--flag` forms |
| `http::Http` | `http` | HTTP(S); conditional GETs, compression, polling watch |

## Formats

| Format | Feature | Notes |
|---|---|---|
| `format::Json` | `json` | Strict JSON; object order preserved |
| `format::Toml` | `toml` | TOML datetimes become strings |
| `format::Yaml` | `yaml` | Standard scalars/sequences/mappings; custom tags are not first-class |

All formats parse into the same ordered `Value` tree. The root of every source
must be an object.

## Feature flags

| Feature | Default | Enables |
|---|---|---|
| `json` | yes | `format::Json` |
| `toml` | yes | `format::Toml` |
| `yaml` | yes | `format::Yaml` |
| `file` | yes | `file::File` and `dir::Dir` |
| `env` | yes | `env::Env` |
| `args` | yes | `args::Args` |
| `watch` | yes | File and directory watching via `notify`; implies `file` |
| `async` | no | `Config::watch_stream` / `WatchStream`, runtime-agnostic |
| `http` | no | `http::Http`, reqwest + rustls; polling watch |

`memory`, `confmap`, `structs`, and `bytes` are always available. Core types and
extension traits live in `koanf-core`; the `koanf-rs` facade re-exports them.

## Semantics

- **Precedence**: sources are merged in registration order; later sources win.
  Runtime overrides (`set` / `delete`) always apply last and survive `reload`.
- **Merge**: objects merge deeply; scalars replace; arrays replace by default.
  `MergeOptions` can append/prepend arrays and delete/ignore nulls.
- **Coercion**: by default, string values convert to numeric or boolean target
  types when requested (`get::<u16>`, serde fields). `Coercion::Strict` disables
  this. String fields never receive converted values.
- **Paths**: `get("server.port")` splits on `.`. Use `Path::key("a.b")` for a
  literal dotted key. Arrays are read as whole values; numeric array indexing is
  not part of v1.
- **Errors**: a single non-exhaustive `Error` enum with source, format, path,
  coercion, watch, and serialization context. The library does not panic on user
  input and does not provide `expect_*` helpers.
- **Concurrency**: `Config`, `Snapshot`, and `Section` are `Send + Sync`.
  Cloning `Config` shares the same state. Reads use immutable `Arc` snapshots.

## Workspace

- `koanf-core`: values, paths, merge/diff, typed serde extraction, config
  engine, snapshots, and watch primitives.
- `koanf-rs`: user-facing facade with built-in sources and formats.

Extension authors should depend on `koanf-core` and implement `Source`,
`Format`, or `Watchable`.

## Status

v0.1 focuses on local sources, three formats, typed access, and file watching.

Not in v1: remote sources (S3/etcd/consul/vault), glob-pattern sources, async
adapters, a derive schema macro, validation hooks, save-to-file helpers,
`no_std`/WASM, array-index paths, dynamic layer removal, or secret redaction.

Releases are automated with release-plz; see [RELEASING.md](RELEASING.md).

## MSRV

Rust 1.92 (edition 2024).

## License

Apache-2.0. See [LICENSE](LICENSE).
