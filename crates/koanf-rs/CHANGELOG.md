# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/hushink/koanf-rs/compare/koanf-rs-v0.1.1...koanf-rs-v0.1.2) - 2026-11-20

### Other

- *(watch)* write files atomically in watch tests

## [0.1.1](https://github.com/hushink/koanf-rs/compare/koanf-rs-v0.1.0...koanf-rs-v0.1.1) - 2026-11-20

### Fixed

- *(watch)* directory and glob watchers reload on any change under the watched
  tree, so coalesced macOS FSEvents notifications are not missed
- *(watch)* directory watchers use the canonical path so FSEvents does not drop
  events for symlinked roots such as macOS temporary directories

### Other

- remove the compiler-sensitive trybuild UI check; the compile-fail restriction
  remains covered by a doctest

## [0.1.0](https://github.com/hushink/koanf-rs/releases/tag/koanf-rs-v0.1.0) - 2026-11-21

### Added

- layered configuration facade with `Memory`/`ConfMap`, `Structs`, `Bytes`,
  `File`, `dir::Dir`, `glob::Glob`, `Env`, `Args`, HTTP(S), Consul KV, and
  Vault KV sources
- JSON, TOML, and YAML formats, plus optional JSON5, HCL, dotenv, INI, and RON
  parsers; JSON5 and HCL also render
- `KoanfSchema` derive macro with secret-path discovery and schema validation
- deterministic directory layering with optional recursive traversal and watch
- glob-pattern matching under a root directory with recursive watch
- conditional HTTP(S) requests with compression and polling watch
- Consul KV loading with nested keys and polling watch
- Vault KV v1/v2 loading with token, namespace, and polling watch
- snapshot and section rendering back to JSON, TOML, and YAML
- rendered file saves through `Config`, `Snapshot`, and `Section`
- file and directory watching, async watch streams, and compile-time
  `Watchable` enforcement
- runtime source inspection and removal, with customizable source names
- snapshot validation hooks
- path-based debug redaction for configuration values
- optional `tracing` instrumentation
- async source loading via the re-exported core API
- optional atomic save helpers via the `atomic-save` feature
- WebAssembly builds for the default feature set on
  `wasm32-unknown-unknown`
