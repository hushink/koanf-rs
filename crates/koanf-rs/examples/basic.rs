use koanf_rs::memory::Memory;
use koanf_rs::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct App {
    server: Server,
}

#[derive(Debug, Deserialize)]
struct Server {
    host: String,
    port: u16,
}

fn main() -> Result<()> {
    let config = Config::builder()
        .source(Memory::from_iter([
            ("server.host", Value::from("localhost")),
            ("server.port", Value::from(8080)),
        ])?)
        .build()?;

    let app: App = config.deserialize()?;
    println!("server = {}:{}", app.server.host, app.server.port);
    Ok(())
}
