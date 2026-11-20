# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/hushink/koanf-rs/releases/tag/koanf-rs-v0.1.0) - 2026-11-21

### Added

- layered configuration facade with `Memory`/`ConfMap`, `Structs`, `Bytes`,
  `File`, `dir::Dir`, `glob::Glob`, `Env`, `Args`, and HTTP(S) sources
- JSON, TOML, and YAML formats, plus optional dotenv and INI parsers
- deterministic directory layering with optional recursive traversal and watch
- glob-pattern matching under a root directory with recursive watch
- conditional HTTP(S) requests with compression and polling watch
- snapshot and section rendering back to JSON, TOML, and YAML
- rendered file saves through `Config`, `Snapshot`, and `Section`
- file and directory watching, async watch streams, and compile-time
  `Watchable` enforcement
- runtime source inspection and removal
- snapshot validation hooks
