# koanf-rs

[![CI](https://github.com/hushink/koanf-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/hushink/koanf-rs/actions/workflows/ci.yml)
[![docs.rs](https://docs.rs/koanf-rs/badge.svg)](https://docs.rs/koanf-rs)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A layered configuration library for Rust, inspired by Go's
[koanf](https://github.com/knadh/koanf). This is an **independent Rust
implementation**, not an official port.

- Multiple sources: memory, typed defaults, files and directories, glob
  patterns, environment variables, CLI arguments, inline bytes, HTTP(S),
  Consul KV, and Vault KV.
- Multiple formats: JSON, TOML, and YAML, with optional dotenv, INI, and RON.
- Layered precedence with deep merge, configurable array/null behavior, and
  persistent runtime overrides.
- Typed access through serde: `deserialize::<T>()`, `get::<T>(path)`, and
  path-scoped `Section` views.
- Immutable, `Arc`-backed snapshots for consistent multi-key reads.
- File and directory watching with full rebuilds, change sets, and
  last-known-good snapshots.
- Snapshot and section rendering back to JSON, TOML, or YAML.
- Configurable snapshot validation hooks.
- Path-based debug redaction for secrets and watch/change output.
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
lexicographic relative-path order, so `10-base.toml` / `20-overrides.toml`
style names provide deterministic layering within the directory. By default
only files with an enabled built-in extension are read; `.format(...)` applies
one explicit format to every regular file instead. Call `.recursive()` to
include subdirectories; directory symlinks are not followed.

`glob::Glob` selects files with a glob pattern relative to a root directory,
for example `Glob::new("config", "**/*.toml")`. `*` does not cross directory
separators; use `**` for recursion. Matches are merged in lexicographic
relative-path order.

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

### Removing layers

`source_names` lists registered layers and `remove_source` removes every layer
whose source name matches. Watchers for removed layers stop, runtime overrides
are retained, and the snapshot is rebuilt:

```rust
let names = config.source_names();
let removed = config.remove_source("memory")?;
```

Source names are not guaranteed to be unique, so all matching layers are
removed. Built-in sources accept `.named("...")` when the automatic name is not
convenient.

### Validation

Install a validator to check every new snapshot. Rejected changes are not
committed; watch-triggered failures keep the last good snapshot and emit
`WatchEvent::Error`:

```rust
use koanf_rs::{Config, Error};

let config = Config::builder()
    .source(File::new("config.toml"))
    .validate(|snapshot| {
        let port: u16 = snapshot.get("server.port")?;
        if port == 0 {
            return Err(Error::validation("server.port must be non-zero"));
        }
        Ok(())
    })
    .build()?;
```

Validators receive an immutable `Snapshot` and must not call mutating methods
on the same `Config`.

### Redacting secrets

Mark sensitive paths with `ConfigBuilder::redact`. Their values—and everything
below them—become `"***"` in `Debug` output for `Config`, `Snapshot`, `Section`,
`ChangeSet`, and `WatchEvent`:

```rust
let config = Config::builder()
    .source(File::new("config.toml"))
    .redact("database.password")
    .build()?;

println!("{config:?}"); // database.password is shown as "***"
```

Redaction is debug-only: reads, rendering, saving, and direct iteration over
`Change`/`ChangeKind` values still return the originals. Errors are not
redacted.

### Rendering

The JSON, TOML, YAML, and RON formats render complete configurations and
sections back to bytes:

```rust
use koanf_rs::format::{Json, Yaml};

let json = config.render(&Json)?;
let server_yaml = config.section("server")?.render(&Yaml)?;
```

Formats reject values they cannot represent. For example, TOML has no null
type, so rendering a snapshot containing `Value::Null` returns `Error::Render`.

`Snapshot::save`, `Section::save`, and `Config::save` render and write in one
step. Rendering completes before the file is touched, so format errors leave the
target unchanged; the write itself truncates or creates the target.

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

### Consul source

The optional `consul` feature reads a Consul KV prefix, decodes base64 values,
and maps slash-separated keys into nested objects:

```rust
use koanf_rs::consul::Consul;
use koanf_rs::prelude::*;

let source = Consul::new("http://127.0.0.1:8500", "config/app")?
    .token("secret")
    .datacenter("dc1");

let config = Config::builder().watched(source).build()?;
```

Watching polls the prefix at a configurable interval; values are strings, so
numeric and boolean reads use the usual coercion.

### Vault source

The optional `vault` feature reads a KV v1 or v2 secret. Its payload becomes the
root configuration object:

```rust
use koanf_rs::vault::{KvVersion, Vault};
use koanf_rs::prelude::*;

let source = Vault::new("http://127.0.0.1:8200", "secret", "app/config")?
    .kv_version(KvVersion::V2)
    .token("hvs.secret")
    .namespace("team-a");

let config = Config::builder().watched(source).build()?;
```

KV v2 is the default; use `.kv_v1()` for version 1. Missing secrets can be
treated as an empty layer with `.optional()`.

## Sources

| Source | Feature | Description |
|---|---|---|
| `memory::Memory` / `confmap::ConfMap` | always | In-memory object |
| `structs::Structs` | always | Serializes any `serde::Serialize` value; ideal for typed defaults |
| `bytes::Bytes` | always | Inline bytes with an explicit format |
| `file::File` | `file` | Single file; format inferred from extension or set explicitly |
| `dir::Dir` | `file` | Directory of files merged in relative-path order; optional recursive |
| `glob::Glob` | `glob` | Glob-pattern matches under a root directory; recursive watch |
| `env::Env` | `env` | Prefixed environment variables with `__`-separated paths |
| `args::Args` | `args` | `--key=value`, `--key value`, and `--flag` forms |
| `http::Http` | `http` | HTTP(S); conditional GETs, compression, polling watch |
| `consul::Consul` | `consul` | Consul KV prefix; nested keys, token/auth, polling watch |
| `vault::Vault` | `vault` | Vault KV v1/v2 secret; token/namespace, polling watch |

## Formats

| Format | Feature | Notes |
|---|---|---|
| `format::Json` | `json` | Strict JSON; object order preserved |
| `format::Toml` | `toml` | TOML datetimes become strings |
| `format::Yaml` | `yaml` | Standard scalars/sequences/mappings; custom tags are not first-class |
| `format::Dotenv` | `dotenv` | Flat string map; quoting and `${VAR}` substitution; parse-only |
| `format::Ini` | `ini` | Global keys at the root; sections nest under their names; parse-only |
| `format::Ron` | `ron` | Rusty Object Notation; render and parse |

All formats parse into the same ordered `Value` tree. The root of every source
must be an object. Dotenv and INI values are strings; the usual lenient
coercion applies when a numeric or boolean target is requested.

## Feature flags

| Feature | Default | Enables |
|---|---|---|
| `json` | yes | `format::Json` |
| `toml` | yes | `format::Toml` |
| `yaml` | yes | `format::Yaml` |
| `dotenv` | no | `format::Dotenv`; also detects `.env` file names |
| `ini` | no | `format::Ini` and `.ini` detection |
| `ron` | no | `format::Ron` and `.ron` detection |
| `file` | yes | `file::File` and `dir::Dir` |
| `glob` | no | `glob::Glob`; implies `file` |
| `env` | yes | `env::Env` |
| `args` | yes | `args::Args` |
| `watch` | yes | File and directory watching via `notify`; implies `file` |
| `async` | no | `Config::watch_stream` / `WatchStream`, runtime-agnostic |
| `http` | no | `http::Http`, reqwest + rustls; polling watch |
| `consul` | no | `consul::Consul`, reqwest + base64; polling watch |
| `vault` | no | `vault::Vault`, KV v1/v2; polling watch |

`memory`, `confmap`, `structs`, and `bytes` are always available. Core types and
extension traits live in `koanf-core`; the `koanf-rs` facade re-exports them.

## Semantics

- **Precedence**: sources are merged in registration order; later sources win.
  Runtime overrides (`set` / `delete`) always apply last and survive `reload`.
  Layers can be appended at runtime or removed by source name.
- **Merge**: objects merge deeply; scalars replace; arrays replace by default.
  `MergeOptions` can append/prepend arrays and delete/ignore nulls.
- **Coercion**: by default, string values convert to numeric or boolean target
  types when requested (`get::<u16>`, serde fields). `Coercion::Strict` disables
  this. String fields never receive converted values.
- **Paths**: `get("server.port")` splits on `.`. Use `Path::key("a.b")` for a
  literal dotted key. Numeric segments index arrays when traversal reaches one
  (`servers.0.host`); object keys such as `"0"` remain ordinary keys. Array
  overrides must replace the whole array.
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

v0.1 focuses on layered sources, multiple formats, typed access, and watching.

Not in v1: other remote sources (S3/etcd), async source adapters, a derive
schema macro, or `no_std`/WASM.

Releases are automated with release-plz; see [RELEASING.md](RELEASING.md).

## MSRV

Rust 1.92 (edition 2024).

## License

Apache-2.0. See [LICENSE](LICENSE).
