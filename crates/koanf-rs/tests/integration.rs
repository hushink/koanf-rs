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

#[cfg(feature = "glob")]
mod glob_tests {
    use std::fs;

    use koanf_rs::glob::Glob;
    use koanf_rs::prelude::*;

    #[cfg(feature = "toml")]
    #[test]
    fn reads_matching_files_in_relative_path_order() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(
            directory.path().join("10-base.toml"),
            "server.port = 80\nserver.host = \"localhost\"\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("20-override.toml"),
            "[server]\nport = 8080\n",
        )
        .unwrap();
        fs::write(directory.path().join("notes.json"), r#"{"ignored":true}"#).unwrap();
        fs::write(nested.join("30-nested.toml"), "nested = true\n").unwrap();

        let only_override = Config::builder()
            .source(Glob::new(directory.path(), "20-*.toml").unwrap())
            .build()
            .unwrap();
        assert_eq!(only_override.get::<u16>("server.port").unwrap(), 8080);
        assert!(!only_override.contains("server.host"));
        assert!(!only_override.contains("nested"));

        let root_only = Config::builder()
            .source(Glob::new(directory.path(), "*.toml").unwrap())
            .build()
            .unwrap();
        assert!(!root_only.contains("nested"));

        let all = Config::builder()
            .source(Glob::new(directory.path(), "**/*.toml").unwrap())
            .build()
            .unwrap();
        assert_eq!(all.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(all.get::<u16>("server.port").unwrap(), 8080);
        assert!(all.get::<bool>("nested").unwrap());
    }

    #[cfg(feature = "json")]
    #[test]
    fn explicit_format_reads_nonstandard_extensions() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("a.conf"), r#"{"value":1}"#).unwrap();
        fs::write(directory.path().join("b.conf"), r#"{"value":2}"#).unwrap();

        let config = Config::builder()
            .source(
                Glob::new(directory.path(), "*.conf")
                    .unwrap()
                    .format(koanf_rs::format::Json),
            )
            .build()
            .unwrap();
        assert_eq!(config.get::<u16>("value").unwrap(), 2);
    }

    #[test]
    fn missing_root_and_invalid_patterns_are_errors() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing");

        assert!(
            Config::builder()
                .source(Glob::new(missing.clone(), "*.toml").unwrap())
                .build()
                .is_err()
        );
        let optional = Config::builder()
            .source(Glob::new(missing, "*.toml").unwrap().optional())
            .build()
            .unwrap();
        assert!(optional.snapshot().as_map().is_empty());

        assert!(matches!(
            Glob::new(directory.path(), "["),
            Err(Error::InvalidGlob { .. })
        ));
    }

    #[cfg(all(feature = "watch", feature = "toml"))]
    #[test]
    fn watches_new_matching_files() {
        use std::sync::mpsc::Receiver;
        use std::time::{Duration, Instant};

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

        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("10-base.toml"), "value = 1\n").unwrap();

        let config = Config::builder()
            .watched(
                Glob::new(directory.path(), "*.toml")
                    .unwrap()
                    .debounce(Duration::from_millis(50)),
            )
            .build()
            .unwrap();
        let (guard, receiver) = config.watch_channel().unwrap();

        fs::write(directory.path().join("20-extra.toml"), "value = 2\n").unwrap();
        recv_matching(&receiver, |event| {
            matches!(
                event,
                WatchEvent::Changed { snapshot, .. }
                    if snapshot.get::<u16>("value").ok() == Some(2)
            )
        });

        drop(guard);
    }
}

#[cfg(feature = "file")]
mod dir_tests {
    use std::fs;

    use koanf_rs::dir::Dir;
    use koanf_rs::prelude::*;

    #[cfg(all(feature = "watch", feature = "toml"))]
    use std::sync::mpsc::Receiver;
    #[cfg(all(feature = "watch", feature = "toml"))]
    use std::time::{Duration, Instant};

    #[cfg(all(feature = "watch", feature = "toml"))]
    fn recv_matching<F>(receiver: &Receiver<WatchEvent>, predicate: F) -> WatchEvent
    where
        F: Fn(&WatchEvent) -> bool,
    {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "timed out waiting for watch event");
            match receiver.recv_timeout(remaining) {
                Ok(event) if predicate(&event) => return event,
                Ok(event) => seen.push(format!("{event:?}")),
                Err(error) => {
                    panic!("failed waiting for watch event: {error}; observed: {seen:#?}")
                }
            }
        }
    }

    #[cfg(feature = "toml")]
    #[test]
    fn merges_files_in_filename_order() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("10-base.toml"),
            "server.host = \"localhost\"\nserver.port = 80\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("20-override.toml"),
            "[server]\nport = 8080\n",
        )
        .unwrap();
        fs::write(directory.path().join("notes.txt"), "not configuration").unwrap();

        let config = Config::builder()
            .source(Dir::new(directory.path()))
            .build()
            .unwrap();

        assert_eq!(config.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
        assert!(!config.contains("notes"));
    }

    #[test]
    fn optional_missing_directory_is_an_empty_layer() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing");

        assert!(
            Config::builder()
                .source(Dir::new(missing.clone()))
                .build()
                .is_err()
        );

        let config = Config::builder()
            .source(Dir::new(missing).optional())
            .build()
            .unwrap();
        assert!(config.snapshot().as_map().is_empty());
    }

    #[cfg(feature = "json")]
    #[test]
    fn explicit_format_reads_all_regular_files() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("a.conf"),
            r#"{"value":1,"untouched":{"enabled":true}}"#,
        )
        .unwrap();
        fs::write(directory.path().join("b.conf"), r#"{"value":2}"#).unwrap();

        let config = Config::builder()
            .source(Dir::new(directory.path()).format(koanf_rs::format::Json))
            .build()
            .unwrap();

        assert_eq!(config.get::<u16>("value").unwrap(), 2);
        assert!(config.get::<bool>("untouched.enabled").unwrap());
    }

    #[cfg(feature = "json")]
    #[test]
    fn merge_options_control_cross_file_merging() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("10-items.json"), r#"{"items":[1]}"#).unwrap();
        fs::write(directory.path().join("20-items.json"), r#"{"items":[2]}"#).unwrap();

        let config = Config::builder()
            .source(Dir::new(directory.path()).merge_options(MergeOptions {
                arrays: ArrayMerge::Append,
                ..MergeOptions::default()
            }))
            .build()
            .unwrap();

        assert_eq!(config.get::<Vec<u16>>("items").unwrap(), vec![1, 2]);
    }

    #[cfg(all(feature = "dotenv", feature = "ini"))]
    #[test]
    fn detects_dotenv_and_ini_files() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(".env"), "DOTENV_VALUE=1\n").unwrap();
        fs::write(directory.path().join("config.ini"), "[server]\nport = 2\n").unwrap();

        let config = Config::builder()
            .source(Dir::new(directory.path()))
            .build()
            .unwrap();
        assert_eq!(config.get::<u16>("DOTENV_VALUE").unwrap(), 1);
        assert_eq!(config.get::<u16>("server.port").unwrap(), 2);
    }

    #[cfg(feature = "toml")]
    #[test]
    fn recursive_reads_nested_files_in_relative_path_order() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(
            directory.path().join("10-root.toml"),
            "server.host = \"localhost\"\nserver.port = 80\n",
        )
        .unwrap();
        fs::write(
            nested.join("20-override.toml"),
            "[server]\nport = 8080\ntls = true\n",
        )
        .unwrap();

        let flat = Config::builder()
            .source(Dir::new(directory.path()))
            .build()
            .unwrap();
        assert_eq!(flat.get::<u16>("server.port").unwrap(), 80);
        assert!(!flat.contains("server.tls"));

        let recursive = Config::builder()
            .source(Dir::new(directory.path()).recursive())
            .build()
            .unwrap();
        assert_eq!(recursive.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(recursive.get::<u16>("server.port").unwrap(), 8080);
        assert!(recursive.get::<bool>("server.tls").unwrap());
    }

    #[cfg(all(feature = "watch", feature = "toml"))]
    #[test]
    fn watches_files_added_and_removed() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("10-base.toml"), "value = 1\n").unwrap();

        let config = Config::builder()
            .watched(Dir::new(directory.path()).debounce(Duration::from_millis(50)))
            .build()
            .unwrap();
        let (guard, receiver) = config.watch_channel().unwrap();

        let extra = directory.path().join("20-extra.toml");
        fs::write(&extra, "value = 2\n").unwrap();
        recv_matching(&receiver, |event| {
            matches!(
                event,
                WatchEvent::Changed { snapshot, .. }
                    if snapshot.get::<u16>("value").ok() == Some(2)
            )
        });

        fs::remove_file(&extra).unwrap();
        recv_matching(&receiver, |event| {
            matches!(
                event,
                WatchEvent::Changed { snapshot, .. }
                    if snapshot.get::<u16>("value").ok() == Some(1)
            )
        });

        drop(guard);
    }

    #[cfg(all(feature = "watch", feature = "toml"))]
    #[test]
    fn recursive_watch_sees_nested_files() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(directory.path().join("10-base.toml"), "value = 1\n").unwrap();

        let config = Config::builder()
            .watched(
                Dir::new(directory.path())
                    .recursive()
                    .debounce(Duration::from_millis(50)),
            )
            .build()
            .unwrap();
        let (guard, receiver) = config.watch_channel().unwrap();

        // Give backends such as FSEvents time to start delivering events.
        std::thread::sleep(Duration::from_millis(250));

        // Write atomically so a watcher reload can never observe a partially
        // written file.
        let staged = nested.join("20-extra.toml.tmp");
        fs::write(&staged, "value = 2\n").unwrap();
        fs::rename(staged, nested.join("20-extra.toml")).unwrap();
        recv_matching(&receiver, |event| {
            matches!(
                event,
                WatchEvent::Changed { snapshot, .. }
                    if snapshot.get::<u16>("value").ok() == Some(2)
            )
        });

        fs::remove_file(nested.join("20-extra.toml")).unwrap();
        recv_matching(&receiver, |event| {
            matches!(
                event,
                WatchEvent::Changed { snapshot, .. }
                    if snapshot.get::<u16>("value").ok() == Some(1)
            )
        });

        drop(guard);
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

    fn write_atomic(path: &std::path::Path, contents: &str) {
        let staged = path.with_extension("tmp");
        fs::write(&staged, contents).unwrap();
        fs::rename(&staged, path).unwrap();
    }

    fn recv_matching<F>(receiver: &Receiver<WatchEvent>, predicate: F) -> WatchEvent
    where
        F: Fn(&WatchEvent) -> bool,
    {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "timed out waiting for watch event");
            match receiver.recv_timeout(remaining) {
                Ok(event) if predicate(&event) => return event,
                Ok(event) => seen.push(format!("{event:?}")),
                Err(error) => {
                    panic!("failed waiting for watch event: {error}; observed: {seen:#?}")
                }
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

        write_atomic(&path, "value = 2\n");
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

        write_atomic(&path, "value = [unterminated\n");
        recv_matching(&receiver, |event| matches!(event, WatchEvent::Error { .. }));
        assert_eq!(config.get::<u16>("value").unwrap(), 2);

        write_atomic(&path, "value = 3\n");
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

        write_atomic(&path, "value = 7\n");
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

#[cfg(any(feature = "json", feature = "toml", feature = "yaml"))]
mod render_tests {
    use koanf_rs::bytes::Bytes;
    use koanf_rs::memory::Memory;
    use koanf_rs::prelude::*;

    fn sample_config() -> Config {
        Config::builder()
            .source(
                Memory::from_iter([
                    ("server.host", Value::from("localhost")),
                    ("server.port", Value::from(8080)),
                    ("server.tls", Value::from(true)),
                ])
                .unwrap(),
            )
            .build()
            .unwrap()
    }

    #[cfg(feature = "json")]
    #[test]
    fn renders_json_and_round_trips() {
        let format = koanf_rs::format::Json;
        let rendered = sample_config().render(&format).unwrap();
        let roundtrip = Config::builder()
            .source(Bytes::new(rendered, format))
            .build()
            .unwrap();
        assert_eq!(roundtrip.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(roundtrip.get::<u16>("server.port").unwrap(), 8080);
        assert!(roundtrip.get::<bool>("server.tls").unwrap());
    }

    #[cfg(feature = "toml")]
    #[test]
    fn renders_toml_and_round_trips() {
        let format = koanf_rs::format::Toml;
        let rendered = sample_config().render(&format).unwrap();
        let roundtrip = Config::builder()
            .source(Bytes::new(rendered, format))
            .build()
            .unwrap();
        assert_eq!(roundtrip.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(roundtrip.get::<u16>("server.port").unwrap(), 8080);
        assert!(roundtrip.get::<bool>("server.tls").unwrap());
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn renders_yaml_and_round_trips() {
        let format = koanf_rs::format::Yaml;
        let rendered = sample_config().render(&format).unwrap();
        let roundtrip = Config::builder()
            .source(Bytes::new(rendered, format))
            .build()
            .unwrap();
        assert_eq!(roundtrip.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(roundtrip.get::<u16>("server.port").unwrap(), 8080);
        assert!(roundtrip.get::<bool>("server.tls").unwrap());
    }

    #[cfg(feature = "toml")]
    #[test]
    fn renders_toml_arrays_of_tables() {
        let servers = Value::Array(vec![
            Value::Map(
                [("host".to_owned(), Value::from("a"))]
                    .into_iter()
                    .collect(),
            ),
            Value::Map(
                [("host".to_owned(), Value::from("b"))]
                    .into_iter()
                    .collect(),
            ),
        ]);
        let config = Config::builder()
            .source(Memory::from_iter([("servers", servers)]).unwrap())
            .build()
            .unwrap();

        let format = koanf_rs::format::Toml;
        let rendered = config.render(&format).unwrap();
        let roundtrip = Config::builder()
            .source(Bytes::new(rendered, format))
            .build()
            .unwrap();
        assert_eq!(roundtrip.get::<String>("servers.1.host").unwrap(), "b");
    }

    #[cfg(all(feature = "json", feature = "file"))]
    #[test]
    fn saves_snapshots_and_sections() {
        let directory = tempfile::tempdir().unwrap();
        let config = sample_config();
        let format = koanf_rs::format::Json;

        let snapshot_path = directory.path().join("snapshot.json");
        config.snapshot().save(&snapshot_path, &format).unwrap();
        let loaded = Config::builder()
            .source(koanf_rs::file::File::new(&snapshot_path))
            .build()
            .unwrap();
        assert_eq!(loaded.get::<u16>("server.port").unwrap(), 8080);

        let config_path = directory.path().join("config.json");
        config.save(&config_path, &format).unwrap();
        let loaded = Config::builder()
            .source(koanf_rs::file::File::new(&config_path))
            .build()
            .unwrap();
        assert_eq!(loaded.get::<String>("server.host").unwrap(), "localhost");

        let section_path = directory.path().join("section.json");
        config
            .section("server")
            .unwrap()
            .save(&section_path, &format)
            .unwrap();
        let loaded = Config::builder()
            .source(koanf_rs::file::File::new(&section_path))
            .build()
            .unwrap();
        assert_eq!(loaded.get::<u16>("port").unwrap(), 8080);
        assert!(!loaded.contains("server"));
    }

    #[cfg(all(feature = "atomic-save", feature = "json", feature = "toml"))]
    #[test]
    fn atomic_save_replaces_files_and_preserves_on_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        let config = sample_config();

        config.save_atomic(&path, &koanf_rs::format::Json).unwrap();
        let content = std::fs::read(&path).unwrap();
        let loaded = Config::builder()
            .source(Bytes::new(&content, koanf_rs::format::Json))
            .build()
            .unwrap();
        assert_eq!(loaded.get::<u16>("server.port").unwrap(), 8080);

        config.set("server.optional", Value::Null).unwrap();
        assert!(matches!(
            config.save_atomic(&path, &koanf_rs::format::Toml),
            Err(Error::Render { .. })
        ));
        assert_eq!(std::fs::read(&path).unwrap(), content);
    }

    #[cfg(feature = "toml")]
    #[test]
    fn save_failures_leave_the_target_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "sentinel").unwrap();

        let config = sample_config();
        config.set("server.optional", Value::Null).unwrap();
        assert!(matches!(
            config.save(&path, &koanf_rs::format::Toml),
            Err(Error::Render { .. })
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "sentinel");
    }

    #[cfg(feature = "json")]
    #[test]
    fn renders_snapshots_and_sections() {
        let format = koanf_rs::format::Json;
        let config = sample_config();
        let snapshot = config.snapshot();
        let rendered = snapshot.render(&format).unwrap();
        let parsed = format.parse(&rendered).unwrap();
        assert_eq!(snapshot.as_map(), parsed.as_map().unwrap());

        let section = config.section("server").unwrap();
        let rendered = section.render(&format).unwrap();
        let parsed = format.parse(&rendered).unwrap();
        assert_eq!(
            parsed.as_map().unwrap().get("port"),
            Some(&Value::from(8080))
        );
    }

    #[cfg(feature = "toml")]
    #[test]
    fn toml_render_reports_unrepresentable_values() {
        let config = sample_config();
        config.set("server.optional", Value::Null).unwrap();
        assert!(matches!(
            config.render(&koanf_rs::format::Toml),
            Err(Error::Render { .. })
        ));
    }
}

#[cfg(feature = "dotenv")]
mod dotenv_tests {
    use koanf_rs::bytes::Bytes;
    use koanf_rs::prelude::*;

    #[test]
    fn parses_flat_string_values() {
        let source = Bytes::new(
            b"PORT=8080\nNAME=\"demo app\"\nDEBUG=true\n# comment\nexport ENABLED=yes\n",
            koanf_rs::format::Dotenv,
        );
        let config = Config::builder().source(source).build().unwrap();

        assert_eq!(config.get::<u16>("PORT").unwrap(), 8080);
        assert_eq!(config.get::<String>("NAME").unwrap(), "demo app");
        assert!(config.get::<bool>("DEBUG").unwrap());
        assert_eq!(config.get::<String>("ENABLED").unwrap(), "yes");
    }

    #[cfg(feature = "file")]
    #[test]
    fn detects_the_dotenv_filename() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".env");
        std::fs::write(&path, "PORT=9090\n").unwrap();

        let config = Config::builder()
            .source(koanf_rs::file::File::new(path))
            .build()
            .unwrap();
        assert_eq!(config.get::<u16>("PORT").unwrap(), 9090);
    }
}

#[cfg(feature = "ron")]
mod ron_tests {
    use koanf_rs::bytes::Bytes;
    use koanf_rs::prelude::*;

    const CONFIG: &[u8] = br#"
        {
            "server": {
                "host": "localhost",
                "port": 8080,
            },
        }
    "#;

    #[test]
    fn parses_and_renders_ron() {
        let source = Bytes::new(CONFIG, koanf_rs::format::Ron);
        let config = Config::builder().source(source).build().unwrap();
        assert_eq!(config.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);

        let rendered = config.render(&koanf_rs::format::Ron).unwrap();
        let roundtrip = Config::builder()
            .source(Bytes::new(rendered, koanf_rs::format::Ron))
            .build()
            .unwrap();
        assert_eq!(roundtrip.get::<u16>("server.port").unwrap(), 8080);
    }

    #[cfg(feature = "file")]
    #[test]
    fn detects_ron_by_extension() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.ron");
        std::fs::write(&path, CONFIG).unwrap();

        let config = Config::builder()
            .source(koanf_rs::file::File::new(path))
            .build()
            .unwrap();
        assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
    }
}

#[cfg(feature = "json5")]
mod json5_tests {
    use koanf_rs::bytes::Bytes;
    use koanf_rs::prelude::*;

    const CONFIG: &[u8] = br#"
        {
            // JSON5 extension: comments, unquoted keys, single quotes,
            // trailing commas, and hexadecimal numbers.
            server: {
                host: 'localhost',
                port: 0x1F90,
            },
            enabled: true,
            items: [1, 2, 3,],
        }
    "#;

    #[test]
    fn parses_and_renders_json5() {
        let config = Config::builder()
            .source(Bytes::new(CONFIG, koanf_rs::format::Json5))
            .build()
            .unwrap();

        assert_eq!(config.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
        assert!(config.get::<bool>("enabled").unwrap());
        assert_eq!(config.get::<u8>("items.2").unwrap(), 3);

        let rendered = config.render(&koanf_rs::format::Json5).unwrap();
        let roundtrip = Config::builder()
            .source(Bytes::new(rendered, koanf_rs::format::Json5))
            .build()
            .unwrap();
        assert_eq!(roundtrip.get::<u16>("server.port").unwrap(), 8080);
        assert_eq!(roundtrip.get::<String>("server.host").unwrap(), "localhost");
    }

    #[test]
    fn invalid_json5_is_a_format_error() {
        let error = koanf_rs::format::Json5.parse(b"{ server: ").unwrap_err();
        assert!(matches!(error, Error::Format { .. }));
    }

    #[cfg(feature = "file")]
    #[test]
    fn detects_json5_by_extension() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json5");
        std::fs::write(&path, CONFIG).unwrap();

        let config = Config::builder()
            .source(koanf_rs::file::File::new(path))
            .build()
            .unwrap();
        assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
    }
}

#[cfg(feature = "hcl")]
mod hcl_tests {
    use koanf_rs::bytes::Bytes;
    use koanf_rs::prelude::*;

    const CONFIG: &[u8] = br#"
        name = "demo"
        port = 8080
        ratio = 1.5
        enabled = true
        nothing = null

        server {
            host = "localhost"
            tags = ["api", "public"]
        }

        item {
            name = "first"
        }

        item {
            name = "second"
        }

        resource "aws_s3_bucket" "logs" {
            acl = "private"
        }

        path = var.root
    "#;

    #[test]
    fn parses_attributes_blocks_and_expressions() {
        let config = Config::builder()
            .source(Bytes::new(CONFIG, koanf_rs::format::Hcl))
            .build()
            .unwrap();

        assert_eq!(config.get::<String>("name").unwrap(), "demo");
        assert_eq!(config.get::<u16>("port").unwrap(), 8080);
        assert_eq!(config.get::<f64>("ratio").unwrap(), 1.5);
        assert!(config.get::<bool>("enabled").unwrap());
        assert!(config.get::<Option<String>>("nothing").unwrap().is_none());
        assert_eq!(config.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(config.get::<String>("server.tags.1").unwrap(), "public");
        assert_eq!(config.get::<String>("item.0.name").unwrap(), "first");
        assert_eq!(config.get::<String>("item.1.name").unwrap(), "second");
        assert_eq!(
            config
                .get::<String>("resource.aws_s3_bucket.logs.acl")
                .unwrap(),
            "private"
        );
        assert_eq!(config.get::<String>("path").unwrap(), "${var.root}");
    }

    #[test]
    fn renders_a_round_trippable_value_tree() {
        let config = Config::builder()
            .source(Bytes::new(CONFIG, koanf_rs::format::Hcl))
            .build()
            .unwrap();

        let rendered = config.render(&koanf_rs::format::Hcl).unwrap();
        let roundtrip = Config::builder()
            .source(Bytes::new(rendered, koanf_rs::format::Hcl))
            .build()
            .unwrap();
        assert_eq!(roundtrip.get::<u16>("port").unwrap(), 8080);
        assert!(
            roundtrip
                .get::<Option<String>>("nothing")
                .unwrap()
                .is_none()
        );
        assert_eq!(roundtrip.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(roundtrip.get::<String>("item.1.name").unwrap(), "second");
        assert_eq!(
            roundtrip
                .get::<String>("resource.aws_s3_bucket.logs.acl")
                .unwrap(),
            "private"
        );
    }

    #[test]
    fn invalid_hcl_is_a_format_error() {
        let error = koanf_rs::format::Hcl.parse(b"server {").unwrap_err();
        assert!(matches!(error, Error::Format { .. }));
    }

    #[cfg(feature = "file")]
    #[test]
    fn detects_hcl_by_extension() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.hcl");
        std::fs::write(&path, CONFIG).unwrap();

        let config = Config::builder()
            .source(koanf_rs::file::File::new(path))
            .build()
            .unwrap();
        assert_eq!(config.get::<u16>("port").unwrap(), 8080);
    }
}

#[cfg(feature = "ini")]
mod ini_tests {
    use koanf_rs::bytes::Bytes;
    use koanf_rs::prelude::*;

    #[test]
    fn parses_global_keys_and_sections() {
        let source = Bytes::new(
            b"root = global\n[server]\nhost = localhost\nport = 8080\n[server]\ntls = true\n",
            koanf_rs::format::Ini,
        );
        let config = Config::builder().source(source).build().unwrap();

        assert_eq!(config.get::<String>("root").unwrap(), "global");
        assert_eq!(config.get::<String>("server.host").unwrap(), "localhost");
        assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
        assert!(config.get::<bool>("server.tls").unwrap());
    }

    #[cfg(feature = "file")]
    #[test]
    fn detects_ini_by_extension() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.ini");
        std::fs::write(&path, "[server]\nport = 7070\n").unwrap();

        let config = Config::builder()
            .source(koanf_rs::file::File::new(path))
            .build()
            .unwrap();
        assert_eq!(config.get::<u16>("server.port").unwrap(), 7070);
    }
}

#[cfg(feature = "derive")]
mod schema_tests {
    use koanf_rs::memory::Memory;
    use koanf_rs::prelude::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, koanf_rs::KoanfSchema)]
    struct Database {
        host: String,
        #[koanf(secret)]
        password: String,
    }

    #[derive(Debug, Deserialize, koanf_rs::KoanfSchema)]
    struct App {
        port: u16,
        #[koanf(secret)]
        token: String,
        #[serde(rename = "db")]
        #[koanf(nested, rename = "db")]
        database: Database,
    }

    #[test]
    fn derives_secret_paths_and_typed_access() {
        assert_eq!(
            App::secret_paths(),
            vec!["token".to_owned(), "db.password".to_owned()]
        );
        assert_eq!(App::NAME, "App");

        let config = Config::builder()
            .source(
                Memory::from_iter([
                    ("port", Value::from(8080)),
                    ("token", Value::from("token-secret")),
                    ("db.host", Value::from("localhost")),
                    ("db.password", Value::from("db-secret")),
                ])
                .unwrap(),
            )
            .redact_schema::<App>()
            .validate_schema::<App>()
            .build()
            .unwrap();

        let app = config.schema::<App>().unwrap();
        assert_eq!(app.port, 8080);
        assert_eq!(app.token, "token-secret");
        assert_eq!(app.database.host, "localhost");
        assert_eq!(app.database.password, "db-secret");

        let debug = format!("{config:?}");
        assert!(!debug.contains("token-secret"));
        assert!(!debug.contains("db-secret"));
        assert!(debug.contains("***"));
    }

    #[test]
    fn schema_validation_rejects_incomplete_configs() {
        assert!(matches!(
            Config::builder()
                .source(Memory::from_iter([("port", Value::from(8080))]).unwrap())
                .validate_schema::<App>()
                .build(),
            Err(Error::Deserialize { .. })
        ));
    }
}

#[test]
fn named_sources_can_be_inspected_and_removed() {
    use koanf_rs::memory::Memory;

    let config = Config::builder()
        .source(
            Memory::from_iter([("value", Value::from(1))])
                .unwrap()
                .named("defaults"),
        )
        .source(
            Memory::from_iter([("value", Value::from(2)), ("extra", Value::from(true))])
                .unwrap()
                .named("overrides"),
        )
        .build()
        .unwrap();

    assert_eq!(config.source_names(), vec!["defaults", "overrides"]);
    assert_eq!(config.remove_source("overrides").unwrap(), 1);
    assert_eq!(config.get::<u16>("value").unwrap(), 1);
    assert!(!config.contains("extra"));
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
