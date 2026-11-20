use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
#[cfg(feature = "std")]
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
#[cfg(feature = "std")]
use std::thread;
#[cfg(feature = "std")]
use std::time::Duration;

use koanf_core::{
    ArrayMerge, AsyncSource, ChangeKind, Config, Error, MergeOptions, NullMerge, Path, Result,
    Source, Value,
};
#[cfg(feature = "std")]
use koanf_core::{WatchCallback, WatchEvent, WatchSignal, WatchSubscription, Watchable};
use serde::Deserialize;

fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> koanf_core::Map {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

#[derive(Clone)]
struct StaticSource {
    name: String,
    map: koanf_core::Map,
}

impl StaticSource {
    fn new(map: koanf_core::Map) -> Self {
        Self::named("static", map)
    }

    fn named(name: &str, map: koanf_core::Map) -> Self {
        Self {
            name: name.to_owned(),
            map,
        }
    }
}

impl Source for StaticSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> Result<koanf_core::Map> {
        Ok(self.map.clone())
    }
}

struct AsyncStaticSource {
    name: String,
    map: koanf_core::Map,
}

impl AsyncStaticSource {
    fn new(name: &str, map: koanf_core::Map) -> Self {
        Self {
            name: name.to_owned(),
            map,
        }
    }
}

impl AsyncSource for AsyncStaticSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn read<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<koanf_core::Map>> + Send + 'a>> {
        let map = self.map.clone();
        Box::pin(async move { Ok(map) })
    }
}

#[derive(Deserialize, Debug, PartialEq)]
struct Server {
    host: String,
    port: u16,
    #[serde(default)]
    tls: bool,
}

#[derive(Deserialize, Debug, PartialEq)]
struct App {
    server: Server,
    #[serde(default)]
    debug: bool,
}

#[test]
fn layered_typed_access_and_sections() {
    let defaults = StaticSource::new(map([
        (
            "server",
            Value::Map(map([
                ("host", Value::from("localhost")),
                ("port", Value::from(80)),
                ("tls", Value::Bool(false)),
            ])),
        ),
        ("debug", Value::Bool(true)),
    ]));
    let overrides = StaticSource::new(map([(
        "server",
        Value::Map(map([("port", Value::from(8080))])),
    )]));

    let config = Config::builder()
        .source(defaults)
        .source(overrides)
        .build()
        .unwrap();

    assert_eq!(config.get::<String>("server.host").unwrap(), "localhost");
    assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
    assert!(config.get::<bool>("debug").unwrap());

    let section = config.section("server").unwrap();
    assert_eq!(section.get::<u16>("port").unwrap(), 8080);
    assert_eq!(section.prefix().to_string(), "server");

    let app: App = config.deserialize().unwrap();
    assert_eq!(
        app,
        App {
            server: Server {
                host: "localhost".to_owned(),
                port: 8080,
                tls: false,
            },
            debug: true,
        }
    );
}

#[test]
fn reads_array_indices_and_rejects_index_overrides() {
    let servers = Value::Array(vec![
        Value::Map(map([("host", Value::from("a")), ("port", Value::from(80))])),
        Value::Map(map([
            ("host", Value::from("b")),
            ("port", Value::from(8080)),
        ])),
    ]);
    let metrics = Value::Map(map([("0", Value::from(5))]));
    let config = Config::builder()
        .source(StaticSource::new(map([
            ("servers", servers),
            ("metrics", metrics),
        ])))
        .build()
        .unwrap();

    assert_eq!(config.get::<String>("servers.0.host").unwrap(), "a");
    assert_eq!(config.get::<u16>("servers.1.port").unwrap(), 8080);
    assert_eq!(config.get::<u16>("metrics.0").unwrap(), 5);
    assert!(config.contains("servers.1"));
    assert_eq!(config.get_opt::<String>("servers.2.host").unwrap(), None);
    assert_eq!(
        config
            .get_or::<String>("servers.2.host", "fallback".to_owned())
            .unwrap(),
        "fallback"
    );
    assert!(matches!(
        config.get::<String>("servers.two"),
        Err(Error::InvalidIndex { .. })
    ));
    assert!(matches!(
        config.get::<String>("servers.2.host"),
        Err(Error::PathNotFound { .. })
    ));

    let section = config.section("servers.1").unwrap();
    assert_eq!(section.get::<u16>("port").unwrap(), 8080);
    assert_eq!(section.prefix().to_string(), "servers.1");

    assert!(matches!(
        config.set("servers.0.host", "x"),
        Err(Error::InvalidPath { .. })
    ));
    assert!(matches!(
        config.delete("servers.1"),
        Err(Error::InvalidPath { .. })
    ));
    config
        .set(
            "servers",
            Value::Array(vec![Value::Map(map([("host", Value::from("c"))]))]),
        )
        .unwrap();
    assert_eq!(config.get::<String>("servers.0.host").unwrap(), "c");
}

#[test]
fn missing_and_null_are_distinct() {
    let source = StaticSource::new(map([
        ("null_value", Value::Null),
        ("value", Value::from(1)),
    ]));
    let config = Config::builder().source(source).build().unwrap();

    assert_eq!(config.get::<Option<String>>("null_value").unwrap(), None);
    assert!(matches!(
        config.get::<u16>("missing"),
        Err(Error::PathNotFound { .. })
    ));
    assert_eq!(config.get_opt::<u16>("missing").unwrap(), None);
    assert_eq!(
        config.get_opt::<Option<String>>("null_value").unwrap(),
        Some(None)
    );
    assert_eq!(config.get_or::<u16>("missing", 42).unwrap(), 42);
    assert_eq!(config.get_or::<u16>("value", 42).unwrap(), 1);
    assert!(matches!(
        config.get_or::<u16>("null_value", 42),
        Err(Error::TypeMismatch { .. } | Error::Deserialize { .. })
    ));
}

#[test]
fn lenient_and_strict_coercion() {
    let source = StaticSource::new(map([
        ("port", Value::from("8080")),
        ("enabled", Value::from("TRUE")),
        ("name", Value::from("demo")),
    ]));
    let config = Config::builder().source(source.clone()).build().unwrap();

    assert_eq!(config.get::<u16>("port").unwrap(), 8080);
    assert!(config.get::<bool>("enabled").unwrap());
    assert_eq!(config.get::<String>("name").unwrap(), "demo");

    let strict = Config::builder()
        .source(source)
        .coercion(koanf_core::Coercion::Strict)
        .build()
        .unwrap();
    assert!(matches!(
        strict.get::<u16>("port"),
        Err(Error::TypeMismatch { .. } | Error::Deserialize { .. })
    ));
    assert_eq!(strict.get::<String>("port").unwrap(), "8080");
}

#[test]
fn coercion_error_reports_path() {
    let source = StaticSource::new(map([(
        "server",
        Value::Map(map([("port", Value::from("not-a-number"))])),
    )]));
    let config = Config::builder().source(source).build().unwrap();

    match config.get::<u16>("server.port") {
        Err(Error::Coercion {
            path,
            value,
            target,
        }) => {
            assert_eq!(path.to_string(), "server.port");
            assert_eq!(value, "not-a-number");
            assert_eq!(target, "u16");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn merge_options_are_configurable() {
    let base = StaticSource::new(map([
        ("items", Value::Array(vec![Value::from(1)])),
        ("keep", Value::from(1)),
        ("remove", Value::from(2)),
    ]));
    let overlay = StaticSource::new(map([
        ("items", Value::Array(vec![Value::from(2)])),
        ("remove", Value::Null),
    ]));
    let config = Config::builder()
        .source(base)
        .source(overlay)
        .merge_options(MergeOptions {
            arrays: ArrayMerge::Append,
            nulls: NullMerge::Delete,
        })
        .build()
        .unwrap();

    assert_eq!(config.get::<Vec<u16>>("items").unwrap(), vec![1, 2]);
    assert!(!config.contains("remove"));
    assert_eq!(config.get::<u16>("keep").unwrap(), 1);
}

#[test]
fn overlay_set_delete_and_reload() {
    let source = StaticSource::new(map([("a", Value::from(1)), ("b", Value::from(2))]));
    let config = Config::builder().source(source).build().unwrap();

    config.set("a", 10).unwrap();
    assert_eq!(config.get::<u16>("a").unwrap(), 10);

    config.set("nested.value", 5).unwrap();
    assert_eq!(config.get::<u16>("nested.value").unwrap(), 5);

    config.delete("a").unwrap();
    assert_eq!(config.get::<u16>("a").unwrap(), 1);

    config.set("b", Value::Null).unwrap();
    assert_eq!(config.get::<Option<u16>>("b").unwrap(), None);

    config.reload().unwrap();
    assert_eq!(config.get::<u16>("a").unwrap(), 1);
    assert_eq!(config.get::<u16>("nested.value").unwrap(), 5);
}

#[test]
fn async_sources_load_in_layer_order() {
    let builder = Config::builder()
        .source(StaticSource::named(
            "base",
            map([("a", Value::from(1)), ("b", Value::from(1))]),
        ))
        .source_async(AsyncStaticSource::new(
            "remote",
            map([("a", Value::from(2)), ("c", Value::from(3))]),
        ))
        .source(StaticSource::named("top", map([("a", Value::from(4))])));

    let config = futures_executor::block_on(builder.build_async()).unwrap();
    assert_eq!(config.get::<u16>("a").unwrap(), 4);
    assert_eq!(config.get::<u16>("b").unwrap(), 1);
    assert_eq!(config.get::<u16>("c").unwrap(), 3);
    assert_eq!(config.source_names(), vec!["base", "remote", "top"]);
}

#[test]
fn synchronous_build_rejects_async_layers() {
    let builder = Config::builder().source_async(AsyncStaticSource::new(
        "remote",
        map([("value", Value::from(1))]),
    ));
    assert!(matches!(builder.build(), Err(Error::AsyncBuildRequired)));
}

#[test]
fn async_layers_can_be_loaded_at_runtime() {
    let config = Config::builder()
        .source(StaticSource::named(
            "base",
            map([("value", Value::from(1))]),
        ))
        .build()
        .unwrap();
    futures_executor::block_on(config.load_async(AsyncStaticSource::new(
        "remote",
        map([("value", Value::from(2))]),
    )))
    .unwrap();

    assert_eq!(config.get::<u16>("value").unwrap(), 2);
    assert_eq!(config.source_names(), vec!["base", "remote"]);
}

#[test]
fn removes_named_sources_and_retains_overrides() {
    let base = StaticSource::named("base", map([("a", Value::from(1)), ("b", Value::from(1))]));
    let overrides = StaticSource::named("overrides", map([("a", Value::from(2))]));
    let config = Config::builder()
        .source(base)
        .source(overrides)
        .build()
        .unwrap();
    config.set("c", 3).unwrap();

    assert_eq!(config.source_names(), vec!["base", "overrides"]);
    assert_eq!(config.remove_source("overrides").unwrap(), 1);
    assert_eq!(config.get::<u16>("a").unwrap(), 1);
    assert_eq!(config.get::<u16>("b").unwrap(), 1);
    assert_eq!(config.get::<u16>("c").unwrap(), 3);
    assert_eq!(config.source_names(), vec!["base"]);
    assert_eq!(config.remove_source("missing").unwrap(), 0);
}

#[cfg(feature = "std")]
#[test]
fn removing_a_watched_source_stops_its_watcher() {
    let (source, trigger) = TriggerSource::new(map([("value", Value::from(1))]));
    let config = Config::builder().watched(source).build().unwrap();
    let (guard, receiver) = config.watch_channel().unwrap();

    assert_eq!(config.remove_source("trigger").unwrap(), 1);
    match recv_event(&receiver) {
        WatchEvent::Changed { snapshot, .. } => assert!(snapshot.as_map().is_empty()),
        other => panic!("unexpected event: {other:?}"),
    }

    trigger.set_value(Value::from(2));
    trigger.ping();
    assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());

    drop(guard);
}

fn validate_positive_port(snapshot: &koanf_core::Snapshot) -> Result<()> {
    let port: u16 = snapshot.get("port")?;
    if port == 0 {
        return Err(Error::validation("port must be positive"));
    }
    Ok(())
}

#[test]
fn debug_output_redacts_configured_paths() {
    let source = StaticSource::new(map([
        ("public", Value::from("visible")),
        (
            "database",
            Value::Map(map([
                ("host", Value::from("db.example")),
                ("password", Value::from("super-secret")),
            ])),
        ),
        (
            "items",
            Value::Array(vec![Value::from("first"), Value::from("second-secret")]),
        ),
    ]));
    let config = Config::builder()
        .source(source)
        .redact("database.password")
        .redact("items.1")
        .build()
        .unwrap();

    let debug = format!("{config:?}");
    assert!(debug.contains("visible"));
    assert!(debug.contains("db.example"));
    assert!(debug.contains("***"));
    assert!(!debug.contains("super-secret"));
    assert!(!debug.contains("second-secret"));

    let snapshot = config.snapshot();
    assert!(!format!("{snapshot:?}").contains("super-secret"));

    config.set("database.password", "new-secret").unwrap();
    let after = config.snapshot();
    let changes = snapshot.diff(&after);
    let changes_debug = format!("{changes:?}");
    assert!(!changes_debug.contains("new-secret"));
    assert!(!changes_debug.contains("super-secret"));

    #[cfg(feature = "std")]
    {
        let event = WatchEvent::Changed {
            snapshot: after,
            changes,
        };
        assert!(!format!("{event:?}").contains("new-secret"));
    }
    #[cfg(not(feature = "std"))]
    let _ = (after, changes);

    let prefix = Config::builder()
        .source(StaticSource::new(map([(
            "database",
            Value::Map(map([
                ("host", Value::from("db")),
                ("password", Value::from("s3cr3t")),
            ])),
        )])))
        .redact("database")
        .build()
        .unwrap();
    let debug = format!("{prefix:?}");
    assert!(!debug.contains("s3cr3t"));
    assert!(!debug.contains("\"db\""));

    let literal = Config::builder()
        .source(StaticSource::new(map([(
            "a.b",
            Value::from("literal-secret"),
        )])))
        .redact(Path::key("a.b"))
        .build()
        .unwrap();
    assert!(!format!("{literal:?}").contains("literal-secret"));
}

#[test]
fn validation_rejects_build_and_keeps_last_good_snapshot() {
    let invalid = Config::builder()
        .source(StaticSource::new(map([("port", Value::from(0))])))
        .validate(validate_positive_port)
        .build();
    assert!(matches!(invalid, Err(Error::Validation { .. })));

    let (source, trigger) = TriggerSource::new(map([("port", Value::from(1))]));
    let config = Config::builder()
        .source(source)
        .validate(validate_positive_port)
        .build()
        .unwrap();

    trigger.set("port", Value::from(0));
    assert!(matches!(config.reload(), Err(Error::Validation { .. })));
    assert_eq!(config.get::<u16>("port").unwrap(), 1);

    assert!(matches!(
        config.set("port", 0),
        Err(Error::Validation { .. })
    ));
    assert_eq!(config.get::<u16>("port").unwrap(), 1);
}

#[cfg(feature = "std")]
#[test]
fn validation_errors_are_reported_by_watch() {
    let (source, trigger) = TriggerSource::new(map([("port", Value::from(1))]));
    let config = Config::builder()
        .watched(source)
        .validate(validate_positive_port)
        .build()
        .unwrap();
    let (guard, receiver) = config.watch_channel().unwrap();

    trigger.set("port", Value::from(0));
    trigger.ping();
    assert!(matches!(
        recv_event(&receiver),
        WatchEvent::Error { error } if matches!(error.as_ref(), Error::Validation { .. })
    ));
    assert_eq!(config.get::<u16>("port").unwrap(), 1);

    trigger.set("port", Value::from(2));
    trigger.ping();
    match recv_event(&receiver) {
        WatchEvent::Changed { snapshot, .. } => {
            assert_eq!(snapshot.get::<u16>("port").unwrap(), 2);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    drop(guard);
}

#[cfg(feature = "std")]
#[test]
fn failed_reload_and_load_keep_the_previous_snapshot() {
    let (source, trigger) = TriggerSource::new(map([("value", Value::from(1))]));
    let config = Config::builder().watched(source).build().unwrap();

    trigger.set_fail(true);
    assert!(config.reload().is_err());
    assert_eq!(config.get::<u16>("value").unwrap(), 1);

    let extra = StaticSource::new(map([("extra", Value::from(2))]));
    assert!(config.load(extra).is_err());
    assert!(!config.contains("extra"));

    trigger.set_fail(false);
    config.reload().unwrap();
    assert_eq!(config.get::<u16>("value").unwrap(), 1);
}

#[test]
fn set_rejects_traversal_through_scalar_overrides() {
    let source = StaticSource::new(map([("a", Value::from(1))]));
    let config = Config::builder().source(source).build().unwrap();

    config.set("a", 2).unwrap();
    assert!(matches!(
        config.set("a.b", 3),
        Err(Error::InvalidPath { .. })
    ));
    assert_eq!(config.get::<u16>("a").unwrap(), 2);
}

#[test]
fn config_clones_share_updates() {
    let source = StaticSource::new(map([("a", Value::from(1))]));
    let config = Config::builder().source(source).build().unwrap();
    let clone = config.clone();

    clone.set("a", 2).unwrap();
    assert_eq!(config.get::<u16>("a").unwrap(), 2);

    config.set("a", 3).unwrap();
    assert_eq!(clone.get::<u16>("a").unwrap(), 3);
}

#[test]
fn snapshot_diff_reports_changes() {
    let config = Config::builder()
        .source(StaticSource::new(map([("a", Value::from(1))])))
        .build()
        .unwrap();
    let before = config.snapshot();
    config.set("a", 2).unwrap();
    config.set("b", 3).unwrap();
    let after = config.snapshot();

    let changes = before.diff(&after);
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().any(|change| {
        change.path.to_string() == "a" && matches!(change.kind, ChangeKind::Modified { .. })
    }));
    assert!(changes.iter().any(|change| {
        change.path.to_string() == "b" && matches!(change.kind, ChangeKind::Added(_))
    }));
}

#[cfg_attr(not(feature = "std"), allow(dead_code))]
#[derive(Clone)]
struct TriggerSource {
    map: Arc<Mutex<koanf_core::Map>>,
    senders: Arc<Mutex<Vec<Sender<()>>>>,
    fail: Arc<AtomicBool>,
}

#[cfg_attr(not(feature = "std"), allow(dead_code))]
#[derive(Clone)]
struct TriggerHandle {
    map: Arc<Mutex<koanf_core::Map>>,
    senders: Arc<Mutex<Vec<Sender<()>>>>,
    fail: Arc<AtomicBool>,
}

impl TriggerSource {
    fn new(map: koanf_core::Map) -> (Self, TriggerHandle) {
        let map = Arc::new(Mutex::new(map));
        let senders = Arc::new(Mutex::new(Vec::new()));
        let fail = Arc::new(AtomicBool::new(false));
        (
            Self {
                map: Arc::clone(&map),
                senders: Arc::clone(&senders),
                fail: Arc::clone(&fail),
            },
            TriggerHandle { map, senders, fail },
        )
    }
}

#[cfg_attr(not(feature = "std"), allow(dead_code))]
impl TriggerHandle {
    fn set(&self, key: &str, value: Value) {
        self.map.lock().unwrap().insert(key.to_owned(), value);
    }

    fn set_value(&self, value: Value) {
        self.set("value", value);
    }

    fn set_fail(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }

    fn ping(&self) {
        let mut senders = self.senders.lock().unwrap();
        senders.retain(|sender| sender.send(()).is_ok());
    }
}

impl Source for TriggerSource {
    fn name(&self) -> &str {
        "trigger"
    }

    fn read(&self) -> Result<koanf_core::Map> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(Error::source_error(
                "trigger",
                io::Error::other("simulated failure"),
            ));
        }
        Ok(self.map.lock().unwrap().clone())
    }
}

#[cfg(feature = "std")]
impl Watchable for TriggerSource {
    fn watch(&self, callback: WatchCallback) -> Result<Box<dyn WatchSubscription>> {
        let (sender, receiver) = mpsc::channel::<()>();
        self.senders.lock().unwrap().push(sender);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !stop_thread.load(Ordering::SeqCst) {
                match receiver.recv_timeout(Duration::from_millis(20)) {
                    Ok(()) => callback(WatchSignal::Changed),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Ok(Box::new(TriggerSubscription {
            stop,
            handle: Mutex::new(Some(handle)),
        }))
    }
}

#[cfg(feature = "std")]
struct TriggerSubscription {
    stop: Arc<AtomicBool>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

#[cfg(feature = "std")]
impl WatchSubscription for TriggerSubscription {
    fn stop(self: Box<Self>) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self
            .handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = handle.join();
        }
    }
}

#[cfg(feature = "std")]
fn recv_event(receiver: &Receiver<WatchEvent>) -> WatchEvent {
    receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("timed out waiting for watch event")
}

#[cfg(feature = "std")]
#[test]
fn watch_delivers_changes_and_errors() {
    let (source, trigger) = TriggerSource::new(map([("value", Value::from(1))]));
    let config = Config::builder().watched(source).build().unwrap();
    let (guard, receiver) = config.watch_channel().unwrap();

    trigger.set_value(Value::from(2));
    trigger.ping();
    match recv_event(&receiver) {
        WatchEvent::Changed { snapshot, changes } => {
            assert_eq!(snapshot.get::<u16>("value").unwrap(), 2);
            assert_eq!(changes.len(), 1);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    trigger.set_fail(true);
    trigger.ping();
    match recv_event(&receiver) {
        WatchEvent::Error { error } => {
            assert!(error.to_string().contains("simulated failure"));
            assert_eq!(config.get::<u16>("value").unwrap(), 2);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    trigger.set_fail(false);
    trigger.set_value(Value::from(3));
    trigger.ping();
    match recv_event(&receiver) {
        WatchEvent::Changed { snapshot, .. } => {
            assert_eq!(snapshot.get::<u16>("value").unwrap(), 3);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    drop(guard);
}

#[cfg(feature = "std")]
#[test]
fn watch_requires_watchable_sources() {
    let config = Config::builder()
        .source(StaticSource::new(map([("a", Value::from(1))])))
        .build()
        .unwrap();
    assert!(matches!(
        config.watch_channel(),
        Err(Error::NoWatchableSources)
    ));
}

#[cfg(feature = "std")]
#[test]
fn manual_updates_emit_watch_events() {
    let (source, trigger) = TriggerSource::new(map([("value", Value::from(1))]));
    let config = Config::builder().watched(source).build().unwrap();
    let (guard, receiver) = config.watch_channel().unwrap();

    config.set("value", 5).unwrap();
    match recv_event(&receiver) {
        WatchEvent::Changed { snapshot, .. } => {
            assert_eq!(snapshot.get::<u16>("value").unwrap(), 5);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    config.delete("value").unwrap();
    match recv_event(&receiver) {
        WatchEvent::Changed { snapshot, .. } => {
            assert_eq!(snapshot.get::<u16>("value").unwrap(), 1);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    trigger.set_value(Value::from(9));
    config.reload().unwrap();
    match recv_event(&receiver) {
        WatchEvent::Changed { snapshot, .. } => {
            assert_eq!(snapshot.get::<u16>("value").unwrap(), 9);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    drop(guard);
}

#[cfg(feature = "std")]
#[test]
fn load_watched_starts_a_watcher_for_the_new_layer() {
    let (first, _first_trigger) = TriggerSource::new(map([("first", Value::from(1))]));
    let config = Config::builder().watched(first).build().unwrap();
    let (guard, receiver) = config.watch_channel().unwrap();

    let (second, second_trigger) = TriggerSource::new(map([("second", Value::from(2))]));
    config.load_watched(second).unwrap();
    assert_eq!(config.get::<u16>("second").unwrap(), 2);

    // Adding a layer while watching emits a change event for the new key.
    match recv_event(&receiver) {
        WatchEvent::Changed { snapshot, .. } => {
            assert_eq!(snapshot.get::<u16>("second").unwrap(), 2);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    second_trigger.set("second", Value::from(3));
    second_trigger.ping();
    match recv_event(&receiver) {
        WatchEvent::Changed { snapshot, .. } => {
            assert_eq!(snapshot.get::<u16>("second").unwrap(), 3);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    drop(guard);
}

#[cfg(feature = "std")]
#[test]
fn watch_restarts_after_last_guard_is_dropped() {
    let (source, trigger) = TriggerSource::new(map([("value", Value::from(1))]));
    let config = Config::builder().watched(source).build().unwrap();

    let (first_guard, first_receiver) = config.watch_channel().unwrap();
    drop(first_guard);
    assert!(
        first_receiver
            .recv_timeout(Duration::from_millis(50))
            .is_err()
    );

    let (second_guard, second_receiver) = config.watch_channel().unwrap();
    trigger.set_value(Value::from(2));
    trigger.ping();
    match recv_event(&second_receiver) {
        WatchEvent::Changed { snapshot, .. } => {
            assert_eq!(snapshot.get::<u16>("value").unwrap(), 2);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    drop(second_guard);
}

#[cfg(feature = "async")]
#[cfg(feature = "std")]
#[test]
fn watch_stream_delivers_events() {
    use futures_executor::block_on;

    let (source, trigger) = TriggerSource::new(map([("value", Value::from(1))]));
    let config = Config::builder().watched(source).build().unwrap();
    let (guard, mut stream) = config.watch_stream().unwrap();

    trigger.set_value(Value::from(2));
    trigger.ping();

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let event = block_on(stream.recv());
        let _ = sender.send(event);
    });

    let event = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("timed out waiting for async watch event")
        .expect("watch stream ended unexpectedly");
    match event {
        WatchEvent::Changed { snapshot, .. } => {
            assert_eq!(snapshot.get::<u16>("value").unwrap(), 2);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    drop(guard);
}

#[cfg(feature = "std")]
#[test]
fn watch_callback_sees_events() {
    let (source, trigger) = TriggerSource::new(map([("value", Value::from(1))]));
    let config = Config::builder().watched(source).build().unwrap();
    let (sender, receiver) = mpsc::channel();
    let _guard = config
        .watch(move |event| {
            let _ = sender.send(event);
        })
        .unwrap();

    trigger.set_value(Value::from(2));
    trigger.ping();
    let event = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    match event {
        WatchEvent::Changed { snapshot, .. } => {
            assert_eq!(snapshot.get::<u16>("value").unwrap(), 2);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn deserialize_reports_nested_field_paths() {
    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    struct Wrapper {
        server: ServerWithPort,
    }
    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    struct ServerWithPort {
        port: u16,
    }

    let source = StaticSource::new(map([(
        "server",
        Value::Map(map([("port", Value::from("not-a-number"))])),
    )]));
    let config = Config::builder().source(source).build().unwrap();

    match config.deserialize::<Wrapper>() {
        Err(Error::Deserialize { field, message, .. }) => {
            let field = field.unwrap_or_default();
            assert!(
                field.contains("server.port"),
                "field={field}, message={message}"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn deserialize_reports_missing_and_unknown_fields() {
    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    struct Required {
        a: u16,
        b: u16,
    }

    let source = StaticSource::new(map([("a", Value::from(1))]));
    let config = Config::builder().source(source).build().unwrap();
    match config.deserialize::<Required>() {
        Err(Error::Deserialize { message, .. }) => {
            assert!(message.contains("missing field `b`"), "{message}");
        }
        other => panic!("unexpected result: {other:?}"),
    }

    #[allow(dead_code)]
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Strict {
        a: u16,
    }

    let source = StaticSource::new(map([("a", Value::from(1)), ("extra", Value::from(2))]));
    let config = Config::builder().source(source).build().unwrap();
    assert!(matches!(
        config.deserialize::<Strict>(),
        Err(Error::Deserialize { .. })
    ));
}

#[test]
fn deserialize_supports_flatten_and_untagged() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Flat {
        a: u16,
        #[serde(flatten)]
        nested: Nested,
    }
    #[derive(Deserialize, Debug, PartialEq)]
    struct Nested {
        b: u16,
    }

    let source = StaticSource::new(map([("a", Value::from(1)), ("b", Value::from(2))]));
    let config = Config::builder().source(source).build().unwrap();
    assert_eq!(
        config.deserialize::<Flat>().unwrap(),
        Flat {
            a: 1,
            nested: Nested { b: 2 }
        }
    );

    #[derive(Deserialize, Debug, PartialEq)]
    #[serde(untagged)]
    enum BoolOrText {
        Bool(bool),
        Text(String),
    }

    let source = StaticSource::new(map([
        ("flag", Value::Bool(true)),
        ("text", Value::from("hello")),
    ]));
    let config = Config::builder().source(source).build().unwrap();
    assert_eq!(
        config.get::<BoolOrText>("flag").unwrap(),
        BoolOrText::Bool(true)
    );
    assert_eq!(
        config.get::<BoolOrText>("text").unwrap(),
        BoolOrText::Text("hello".to_owned())
    );
}

#[test]
fn serializer_rejects_non_string_keys_and_large_integers() {
    use std::collections::BTreeMap;

    let mut map = BTreeMap::new();
    map.insert(1_u32, "value");
    assert!(matches!(
        Value::from_serialize(&map),
        Err(Error::Serialize { .. })
    ));
    assert!(matches!(
        Value::from_serialize(&i128::MAX),
        Err(Error::Serialize { .. })
    ));
}

#[test]
fn serializer_handles_unit_variants_and_arrays() {
    use serde::Serialize;

    #[derive(Serialize)]
    enum Mode {
        Development,
    }

    assert_eq!(
        Value::from_serialize(&Mode::Development).unwrap(),
        Value::String("Development".to_owned())
    );
    assert_eq!(
        Value::from_serialize(&vec![1_u8, 2_u8]).unwrap(),
        Value::Array(vec![Value::Int(1), Value::Int(2)])
    );
}

#[test]
fn section_deserialize_and_nested_sections() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Tls {
        enabled: bool,
    }
    #[derive(Deserialize, Debug, PartialEq)]
    struct Server {
        host: String,
        tls: Tls,
    }

    let source = StaticSource::new(map([(
        "server",
        Value::Map(map([
            ("host", Value::from("example.com")),
            ("tls", Value::Map(map([("enabled", Value::Bool(true))]))),
        ])),
    )]));
    let config = Config::builder().source(source).build().unwrap();
    let server = config.section("server").unwrap();
    let tls = server.section("tls").unwrap();
    assert!(tls.get::<bool>("enabled").unwrap());
    assert_eq!(
        server.deserialize::<Server>().unwrap(),
        Server {
            host: "example.com".to_owned(),
            tls: Tls { enabled: true },
        }
    );
}
