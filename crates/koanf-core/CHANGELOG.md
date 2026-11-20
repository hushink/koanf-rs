# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/hushink/koanf-rs/releases/tag/koanf-core-v0.1.0) - 2026-11-21

### Added

- layered configuration engine with immutable snapshots and atomic rebuilds
- ordered `Value`/`Map`, dotted paths, merge options, and change sets
- typed serde access, sections, and type-directed scalar coercion
- numeric array-index reads and `Error::InvalidIndex`
- runtime overrides, watch callbacks/channels, and a runtime-agnostic async
  watch stream
- runtime source inspection and removal, stopping removed layers' watchers
- snapshot validation hooks with `Error::Validation`
- path-based debug redaction for config, snapshot, section, change set, and watch output
- optional `tracing` instrumentation
- async source loading with `AsyncSource`, `build_async`, and `load_async`
- optional atomic save helpers via the `atomic-save` feature
- snapshot and section rendering through the `Format` trait, including save
  helpers

### Fixed

- harden value equality and watch subscription races

### Other

- clarify override and watch contracts
