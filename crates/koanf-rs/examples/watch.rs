use std::fs;
use std::time::{Duration, Instant};

use koanf_rs::file::File;
use koanf_rs::prelude::*;

fn main() -> Result<()> {
    let path = std::env::temp_dir().join(format!("koanf-rs-watch-{}.toml", std::process::id()));
    fs::write(&path, "value = 1\n").expect("write initial config");

    let config = Config::builder().watched(File::new(path.clone())).build()?;
    let (guard, events) = config.watch_channel()?;
    println!("watching {}", path.display());

    let writer_path = path.clone();
    std::thread::spawn(move || {
        for value in 2..=4 {
            std::thread::sleep(Duration::from_millis(400));
            let _ = fs::write(&writer_path, format!("value = {value}\n"));
        }
    });

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match events.recv_timeout(Duration::from_millis(200)) {
            Ok(WatchEvent::Changed { snapshot, changes }) => {
                let value = snapshot.get::<u16>("value")?;
                println!("value={value}, changes={}", changes.len());
            }
            Ok(WatchEvent::Error { error }) => eprintln!("watch error: {error}"),
            Err(_) => {}
        }
    }

    drop(guard);
    let _ = fs::remove_file(path);
    Ok(())
}
