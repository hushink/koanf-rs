# koanf-rs

[![CI](https://github.com/hushink/koanf-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/hushink/koanf-rs/actions/workflows/ci.yml)
[![docs.rs](https://docs.rs/koanf-rs/badge.svg)](https://docs.rs/koanf-rs)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

受 Go 语言 [koanf](https://github.com/knadh/koanf) 启发的 Rust 分层配置库。
这是**独立的 Rust 实现**，不是官方移植版本。

- 多配置源：内存、类型化默认值、文件、环境变量、命令行参数、内联字节。
- 多格式：JSON、TOML、YAML。
- 分层优先级、深度合并、可配置的数组/null 行为、持久的运行时覆盖。
- 基于 serde 的类型安全读取：`deserialize::<T>()`、`get::<T>(path)`、
  路径作用域的 `Section` 视图。
- 不可变、`Arc` 支撑的快照，保证多键读取的一致性。
- 文件监听：全量重建、变更集、失败时保留最后一个可用快照。
- 编译期约束：只有 `Watchable` 源才能注册到 watch 流程。
- 可扩展的 `Source` / `Format` / `Watchable` trait。
- 库自身不使用 `unsafe`。

## 安装

```bash
cargo add koanf-rs
```

最小构建（不含内置格式与本地源）：

```bash
cargo add koanf-rs --no-default-features
```

## 快速开始

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

## 分层默认值与配置源

后注册的源覆盖先注册的源。典型顺序：类型化默认值 → 可选文件 → 环境变量 →
命令行参数。

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

    // APP_SERVER__PORT=9090 覆盖文件与默认值。
    let port: u16 = config.get("server.port")?;
    println!("port = {port}");
    Ok(())
}
```

`Memory` 是内存 map 源，同时以 `confmap::ConfMap` 名称提供，方便熟悉 koanf
的用户迁移。

## 读取值

```rust
let app: App = config.deserialize()?;
let port: u16 = config.get("server.port")?;
let optional = config.get_opt::<String>("missing")?; // None
let fallback: u16 = config.get_or("server.port", 8080)?;
```

“缺失”与“显式 null”严格区分：

- `get`：路径缺失返回 `PathNotFound`。
- `get_opt`：仅路径缺失返回 `None`。
- `get_or`：仅路径缺失使用默认值。
- 显式 null 作为值正常反序列化；`Option<T>` 可以接收它。

`Section` 便于处理类型化子配置：

```rust
let database = config.section("database")?;
let url: String = database.get("url")?;
```

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

任一被监视源变化时，库会重新读取**全部**源、重建合并快照，并在结果确实变化时
推送变更集。解析或 I/O 失败时保留上一个可用快照，并推送 `WatchEvent::Error`。

### 异步 watch stream

启用 `async` feature 后，可以把事件作为运行时无关的 stream 接收：

```rust
let (guard, mut events) = config.watch_stream()?;
while let Some(event) = events.recv().await {
    match event {
        WatchEvent::Changed { snapshot, changes } => { /* ... */ }
        WatchEvent::Error { error } => eprintln!("watch error: {error}"),
    }
}
```

`WatchStream` 实现 `futures_core::Stream`；`recv` 是自带 async 方法，可配合任意
executor 或 `StreamExt` 使用。

## 配置源

| 源 | feature | 说明 |
|---|---|---|
| `memory::Memory` / `confmap::ConfMap` | 始终可用 | 内存对象 |
| `structs::Structs` | 始终可用 | 序列化任意 `serde::Serialize` 值；适合类型化默认值 |
| `bytes::Bytes` | 始终可用 | 内联字节 + 显式格式 |
| `file::File` | `file` | 单文件；默认按扩展名推断格式 |
| `env::Env` | `env` | 带前缀的环境变量，`__` 分隔嵌套路径 |
| `args::Args` | `args` | `--key=value`、`--key value`、`--flag` |

## 格式

| 格式 | feature | 说明 |
|---|---|---|
| `format::Json` | `json` | 严格 JSON；保留对象顺序 |
| `format::Toml` | `toml` | TOML datetime 转为字符串 |
| `format::Yaml` | `yaml` | 标准标量/序列/映射；自定义 tag 非一等支持 |

所有格式统一解析为有序 `Value` 树；每个源的根必须是对象。

## Feature flags

| Feature | 默认 | 内容 |
|---|---|---|
| `json` | 是 | `format::Json` |
| `toml` | 是 | `format::Toml` |
| `yaml` | 是 | `format::Yaml` |
| `file` | 是 | `file::File` |
| `env` | 是 | `env::Env` |
| `args` | 是 | `args::Args` |
| `watch` | 是 | 通过 `notify` 监听文件；隐含 `file` |
| `async` | 否 | `Config::watch_stream` / `WatchStream`，运行时无关 |

`memory`、`confmap`、`structs`、`bytes` 始终可用。核心类型与扩展 trait 位于
`koanf-core`；`koanf-rs` facade 负责 re-export。

## 语义

- **优先级**：按注册顺序合并，后者覆盖前者。运行时覆盖（`set` / `delete`）
  永远最后应用，并在 `reload` 后保留。
- **合并**：对象深度递归合并；标量覆盖；数组默认整体替换。`MergeOptions` 可
  配置数组追加/前插与 null 删除/忽略。
- **类型转换**：默认情况下，字符串值在目标类型要求数值/布尔时转换
  （`get::<u16>`、serde 字段）。`Coercion::Strict` 关闭该行为；字符串字段
  永远不会收到转换后的值。
- **路径**：`get("server.port")` 按 `.` 切分；含点键使用 `Path::key("a.b")`。
  数组作为整体读取；v1 不支持数字数组索引路径。
- **错误**：单一、`#[non_exhaustive]` 的 `Error` 枚举，携带源、格式、路径、
  转换、watch 与序列化上下文。库不会因用户输入 panic，也不提供 `expect_*`。
- **并发**：`Config`、`Snapshot`、`Section` 均为 `Send + Sync`。clone
  `Config` 共享同一状态；读取使用不可变 `Arc` 快照。

## 工作区

- `koanf-core`：Value、Path、合并/差异、serde 类型化提取、配置引擎、快照与
  watch 基础设施。
- `koanf-rs`：面向用户的 facade，包含内置源与格式。

扩展作者依赖 `koanf-core`，实现 `Source`、`Format` 或 `Watchable`。

## 状态

v0.1 聚焦本地源、三种格式、类型化读取与文件监听。

v1 不包含：远程源（HTTP/S3/etcd/consul/vault）、多文件/glob、async 适配、
derive schema 宏、验证框架、render/save API、`no_std`/WASM、数组索引路径、
动态层删除、敏感值脱敏。

发布流程使用 release-plz 自动化，见 [RELEASING.md](RELEASING.md)。

## MSRV

Rust 1.92（edition 2024）。

## 许可证

Apache-2.0，见 [LICENSE](LICENSE)。
