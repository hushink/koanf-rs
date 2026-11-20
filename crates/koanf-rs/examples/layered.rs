use koanf_rs::args::Args;
use koanf_rs::env::Env;
use koanf_rs::file::File;
use koanf_rs::memory::Memory;
use koanf_rs::prelude::*;
use koanf_rs::structs::Structs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Defaults {
    server: Server,
    debug: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Server {
    host: String,
    port: u16,
    tls: bool,
}

fn main() -> Result<()> {
    let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("config.toml");

    let defaults = Defaults {
        server: Server {
            host: "localhost".to_owned(),
            port: 8080,
            tls: false,
        },
        debug: false,
    };
    let _defaults_summary = format!(
        "{}:{} tls={} debug={}",
        defaults.server.host, defaults.server.port, defaults.server.tls, defaults.debug
    );

    let config = Config::builder()
        .source(Structs::new(defaults)?)
        .source(Memory::from_iter([("server.tls", Value::Bool(true))])?)
        .source(File::new(config_path).optional())
        .source(Env::prefixed("APP_"))
        .source(Args::new())
        .build()?;

    let snapshot = config.snapshot();
    println!("effective configuration:");
    for (key, value) in snapshot.as_map() {
        println!("  {key} = {value:?}");
    }
    println!("port = {}", config.get::<u16>("server.port")?);

    config.set("server.port", 9090)?;
    println!("after set: port = {}", config.get::<u16>("server.port")?);

    config.delete("server.port")?;
    println!("after delete: port = {}", config.get::<u16>("server.port")?);

    Ok(())
}
