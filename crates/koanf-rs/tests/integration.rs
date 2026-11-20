use koanf_rs::prelude::*;
use koanf_rs::structs::Structs;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Defaults {
    server: ServerDefaults,
    debug: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct ServerDefaults {
    host: String,
    port: u16,
    tls: bool,
}

#[test]
fn structs_defaults_are_the_lowest_layer() {
    let defaults = Defaults {
        server: ServerDefaults {
            host: "localhost".to_owned(),
            port: 80,
            tls: false,
        },
        debug: false,
    };

    let config = Config::builder()
        .source(Structs::new(defaults).unwrap())
        .build()
        .unwrap();

    assert_eq!(config.get::<String>("server.host").unwrap(), "localhost");
    assert_eq!(config.get::<u16>("server.port").unwrap(), 80);
    assert!(!config.get::<bool>("debug").unwrap());
}

#[test]
fn structs_reject_non_object_roots() {
    assert!(matches!(
        Structs::new(vec![1_u8, 2]),
        Err(Error::InvalidRoot { .. })
    ));
}

#[test]
fn structs_support_nested_enums_and_renames() {
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    enum Mode {
        Development,
        Custom { level: u8 },
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    #[serde(rename_all = "kebab-case")]
    struct Settings {
        max_connections: u16,
        mode: Mode,
    }

    let config = Config::builder()
        .source(
            Structs::new(Settings {
                max_connections: 10,
                mode: Mode::Custom { level: 3 },
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    assert_eq!(config.get::<u16>("max-connections").unwrap(), 10);
    assert_eq!(
        config.get::<Mode>("mode").unwrap(),
        Mode::Custom { level: 3 }
    );
}

#[cfg(all(feature = "env", feature = "args"))]
#[test]
fn structs_memory_env_and_args_compose_in_order() {
    use std::ffi::OsString;

    use koanf_rs::args::Args;
    use koanf_rs::env::Env;
    use koanf_rs::memory::Memory;

    let defaults = Defaults {
        server: ServerDefaults {
            host: "default-host".to_owned(),
            port: 80,
            tls: false,
        },
        debug: false,
    };

    let env = Env::from_env_iter([
        (OsString::from("APP_SERVER__PORT"), OsString::from("9090")),
        (OsString::from("APP_DEBUG"), OsString::from("true")),
        (OsString::from("UNRELATED"), OsString::from("ignored")),
    ])
    .prefix("APP_");

    let args = Args::from_iter(["--server.host=cli-host"]);

    let config = Config::builder()
        .source(Structs::new(defaults).unwrap())
        .source(Memory::from_iter([("server.tls", true)]).unwrap())
        .source(env)
        .source(args)
        .build()
        .unwrap();

    assert_eq!(config.get::<String>("server.host").unwrap(), "cli-host");
    assert_eq!(config.get::<u16>("server.port").unwrap(), 9090);
    assert!(config.get::<bool>("server.tls").unwrap());
    assert!(config.get::<bool>("debug").unwrap());
    assert!(!config.contains("unrelated"));
}

#[cfg(feature = "env")]
mod env_tests {
    use std::ffi::OsString;

    use koanf_rs::env::{Env, KeyCase};
    use koanf_rs::prelude::*;

    #[test]
    fn maps_prefixed_variables_to_nested_lowercase_paths() {
        let env = Env::from_env_iter([
            (OsString::from("APP_SERVER__PORT"), OsString::from("8080")),
            (
                OsString::from("APP_SERVER__HOST"),
                OsString::from("localhost"),
            ),
            (
                OsString::from("APP_DATABASE__URL"),
                OsString::from("postgres://db"),
            ),
            (OsString::from("OTHER"), OsString::from("ignored")),
        ])
        .prefix("APP_");

        let config = Config::builder().source(env).build().unwrap();
        assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
        assert_eq!(config.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(
            config.get::<String>("database.url").unwrap(),
            "postgres://db"
        );
        assert!(!config.contains("other"));
    }

    #[test]
    fn supports_single_separator_and_case_preservation() {
        let env = Env::from_env_iter([
            (OsString::from("APP_SERVER_PORT"), OsString::from("8080")),
            (
                OsString::from("APP_SERVER_HOST"),
                OsString::from("localhost"),
            ),
        ])
        .prefix("APP_")
        .separator("_")
        .case(KeyCase::Preserve);

        let config = Config::builder().source(env).build().unwrap();
        assert_eq!(config.get::<u16>("SERVER.PORT").unwrap(), 8080);
        assert_eq!(config.get::<String>("SERVER.HOST").unwrap(), "localhost");
    }

    #[test]
    fn map_key_can_override_default_mapping() {
        let env = Env::from_env_iter([(OsString::from("APP_FOO_BAR"), OsString::from("1"))])
            .prefix("APP_")
            .map_key(|key| Path::new([key.to_ascii_lowercase()]));

        let config = Config::builder().source(env).build().unwrap();
        assert_eq!(config.get::<u16>("foo_bar").unwrap(), 1);
    }

    #[test]
    fn path_conflicts_are_reported() {
        let env = Env::from_env_iter([
            (OsString::from("APP_A"), OsString::from("1")),
            (OsString::from("APP_A__B"), OsString::from("2")),
        ])
        .prefix("APP_");

        assert!(matches!(
            Config::builder().source(env).build(),
            Err(Error::SourceConflict { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_values_are_skipped_or_rejected() {
        use std::os::unix::ffi::OsStringExt;

        let make_env = || {
            Env::from_env_iter([
                (OsString::from("APP_BAD"), OsString::from_vec(vec![0xFF])),
                (OsString::from("APP_GOOD"), OsString::from("ok")),
            ])
            .prefix("APP_")
        };

        let config = Config::builder().source(make_env()).build().unwrap();
        assert!(!config.contains("bad"));
        assert_eq!(config.get::<String>("good").unwrap(), "ok");

        assert!(
            Config::builder()
                .source(make_env().error_on_non_utf8())
                .build()
                .is_err()
        );
    }
}

#[cfg(feature = "args")]
mod args_tests {
    use koanf_rs::args::Args;
    use koanf_rs::prelude::*;

    #[test]
    fn parses_supported_flag_forms() {
        let args = Args::from_iter([
            "--server.host=localhost",
            "--server.port",
            "8080",
            "--debug",
            "--flag=false",
            "positional",
            "-x",
            "--",
            "--ignored=1",
        ]);

        let config = Config::builder().source(args).build().unwrap();
        assert_eq!(config.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
        assert!(config.get::<bool>("debug").unwrap());
        assert!(!config.get::<bool>("flag").unwrap());
        assert!(!config.contains("ignored"));
    }

    #[test]
    fn negative_numbers_are_values_and_duplicates_use_last() {
        let args = Args::from_iter(["--number", "-1", "--name=first", "--name=second"]);
        let config = Config::builder().source(args).build().unwrap();
        assert_eq!(config.get::<i32>("number").unwrap(), -1);
        assert_eq!(config.get::<String>("name").unwrap(), "second");
    }

    #[test]
    fn supports_custom_separator() {
        let args = Args::from_iter(["--server_port=8080"]).key_separator("_");
        let config = Config::builder().source(args).build().unwrap();
        assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
    }
}

#[cfg(feature = "file")]
mod file_tests {
    use std::fs;

    use koanf_rs::file::File;
    use koanf_rs::prelude::*;

    #[cfg(all(feature = "json", feature = "toml", feature = "yaml"))]
    #[test]
    fn detects_formats_by_extension() {
        let directory = tempfile::tempdir().unwrap();

        let json_path = directory.path().join("config.json");
        fs::write(&json_path, r#"{"server":{"port":8080}}"#).unwrap();
        let toml_path = directory.path().join("config.toml");
        fs::write(&toml_path, "[server]\nport = 8080\n").unwrap();
        let yaml_path = directory.path().join("config.yaml");
        fs::write(&yaml_path, "server:\n  port: 8080\n").unwrap();

        for path in [json_path, toml_path, yaml_path] {
            let config = Config::builder()
                .source(File::new(path.clone()))
                .build()
                .unwrap_or_else(|error| panic!("failed to load {}: {error}", path.display()));
            assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
        }
    }

    #[test]
    fn optional_missing_file_is_an_empty_layer() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing.toml");

        assert!(
            Config::builder()
                .source(File::new(missing.clone()))
                .build()
                .is_err()
        );

        let config = Config::builder()
            .source(File::new(missing).optional())
            .build()
            .unwrap();
        assert!(config.snapshot().as_map().is_empty());
    }

    #[cfg(feature = "json")]
    #[test]
    fn explicit_format_overrides_extension() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.data");
        fs::write(&path, r#"{"value":1}"#).unwrap();

        let config = Config::builder()
            .source(File::new(path).format(koanf_rs::format::Json))
            .build()
            .unwrap();
        assert_eq!(config.get::<u16>("value").unwrap(), 1);
    }

    #[cfg(feature = "toml")]
    #[test]
    fn toml_datetimes_become_strings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "stamp = 2024-01-02T03:04:05Z\n").unwrap();

        let config = Config::builder().source(File::new(path)).build().unwrap();
        let stamp = config.get::<String>("stamp").unwrap();
        assert!(stamp.contains("2024-01-02"), "{stamp}");
    }

    #[test]
    fn rejects_unknown_extensions() {
        let directory = tempfile::tempdir().unwrap();
        let unknown = directory.path().join("config.unknown");
        fs::write(&unknown, "value = 1").unwrap();
        assert!(matches!(
            Config::builder().source(File::new(unknown)).build(),
            Err(Error::UnknownFormat { .. })
        ));
    }

    #[cfg(feature = "json")]
    #[test]
    fn rejects_non_object_roots() {
        let directory = tempfile::tempdir().unwrap();
        let array = directory.path().join("array.json");
        fs::write(&array, "[1,2,3]").unwrap();
        assert!(matches!(
            Config::builder().source(File::new(array)).build(),
            Err(Error::InvalidRoot { .. })
        ));
    }

    #[cfg(feature = "json")]
    #[test]
    fn strips_utf8_bom() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        let mut data = vec![0xEF, 0xBB, 0xBF];
        data.extend_from_slice(br#"{"value":1}"#);
        fs::write(&path, data).unwrap();

        let config = Config::builder().source(File::new(path)).build().unwrap();
        assert_eq!(config.get::<u16>("value").unwrap(), 1);
    }
}

mod bytes_tests {
    #[cfg(feature = "json")]
    #[test]
    fn parses_inline_bytes() {
        use koanf_rs::bytes::Bytes;
        use koanf_rs::prelude::*;

        let source = Bytes::new(br#"{"value":1}"#, koanf_rs::format::Json).named("inline");
        let config = Config::builder().source(source).build().unwrap();
        assert_eq!(config.get::<u16>("value").unwrap(), 1);
    }
}

#[cfg(all(feature = "file", feature = "watch", feature = "toml"))]
mod watch_tests {
    use std::fs;
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    use koanf_rs::file::File;
    use koanf_rs::prelude::*;

    fn recv_matching<F>(receiver: &Receiver<WatchEvent>, predicate: F) -> WatchEvent
    where
        F: Fn(&WatchEvent) -> bool,
    {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "timed out waiting for watch event");
            match receiver.recv_timeout(remaining) {
                Ok(event) if predicate(&event) => return event,
                Ok(_) => {}
                Err(error) => panic!("failed waiting for watch event: {error}"),
            }
        }
    }

    #[test]
    fn reloads_when_file_changes_and_recovers_from_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "value = 1\n").unwrap();

        let config = Config::builder()
            .watched(File::new(path.clone()))
            .build()
            .unwrap();
        let (guard, receiver) = config.watch_channel().unwrap();

        fs::write(&path, "value = 2\n").unwrap();
        let event = recv_matching(&receiver, |event| {
            matches!(
                event,
                WatchEvent::Changed { snapshot, .. }
                    if snapshot.get::<u16>("value").ok() == Some(2)
            )
        });
        let WatchEvent::Changed { changes, .. } = event else {
            unreachable!()
        };
        assert!(!changes.is_empty());

        fs::write(&path, "value = [unterminated\n").unwrap();
        recv_matching(&receiver, |event| matches!(event, WatchEvent::Error { .. }));
        assert_eq!(config.get::<u16>("value").unwrap(), 2);

        fs::write(&path, "value = 3\n").unwrap();
        recv_matching(&receiver, |event| {
            matches!(
                event,
                WatchEvent::Changed { snapshot, .. }
                    if snapshot.get::<u16>("value").ok() == Some(3)
            )
        });

        drop(guard);
    }

    #[test]
    fn watches_optional_file_creation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");

        let config = Config::builder()
            .watched(File::new(path.clone()).optional())
            .build()
            .unwrap();
        let (guard, receiver) = config.watch_channel().unwrap();

        fs::write(&path, "value = 7\n").unwrap();
        recv_matching(&receiver, |event| {
            matches!(
                event,
                WatchEvent::Changed { snapshot, .. }
                    if snapshot.get::<u16>("value").ok() == Some(7)
            )
        });

        drop(guard);
    }
}

#[test]
fn confmap_alias_matches_memory() {
    use koanf_rs::confmap::ConfMap;

    let source = ConfMap::from_iter([("a.b", 1)]).unwrap();
    let config = Config::builder().source(source).build().unwrap();
    assert_eq!(config.get::<u16>("a.b").unwrap(), 1);
}

#[test]
fn memory_rejects_path_conflicts() {
    use koanf_rs::memory::Memory;

    assert!(matches!(
        Memory::from_iter([("a", Value::from(1)), ("a.b", Value::from(2)),]),
        Err(Error::SourceConflict { .. })
    ));
}
