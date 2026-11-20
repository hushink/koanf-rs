use koanf_rs::args::Args;
use koanf_rs::env::Env;
use koanf_rs::file::File;
use koanf_rs::prelude::*;
use koanf_rs::structs::Structs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Defaults {
    server: Server,
    database: Database,
}

#[derive(Debug, Serialize, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
struct Database {
    url: String,
}

fn main() -> Result<()> {
    let defaults = Defaults {
        server: Server {
            host: "localhost".to_owned(),
            port: 8080,
        },
        database: Database {
            url: "sqlite://app.db".to_owned(),
        },
    };

    let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("config.toml");

    let config = Config::builder()
        .source(Structs::new(defaults)?)
        .source(File::new(config_path).optional())
        .source(Env::prefixed("APP_"))
        .source(Args::new())
        .build()?;

    let app: Defaults = config.deserialize()?;
    println!("server = {}:{}", app.server.host, app.server.port);
    println!("database = {}", app.database.url);

    let server = config.section("server")?;
    println!("server.port = {}", server.get::<u16>("port")?);
    Ok(())
}
