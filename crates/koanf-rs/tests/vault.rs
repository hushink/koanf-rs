#![cfg(feature = "vault")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use koanf_rs::prelude::*;
use koanf_rs::vault::Vault;

struct TestServer {
    address: SocketAddr,
    state: Arc<Mutex<ServerState>>,
}

struct ServerState {
    status: u16,
    body: Vec<u8>,
    last_token: Option<String>,
    last_namespace: Option<String>,
    last_path: Option<String>,
    requests: usize,
}

impl TestServer {
    fn start(body: serde_json::Value) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let state = Arc::new(Mutex::new(ServerState {
            status: 200,
            body: serde_json::to_vec(&body).expect("serialize body"),
            last_token: None,
            last_namespace: None,
            last_path: None,
            requests: 0,
        }));
        let server_state = Arc::clone(&state);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    break;
                };
                let state = Arc::clone(&server_state);
                thread::spawn(move || handle_connection(stream, state));
            }
        });
        Self { address, state }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn set_body(&self, body: serde_json::Value) {
        self.state.lock().expect("test server state").body =
            serde_json::to_vec(&body).expect("serialize body");
    }

    fn set_status(&self, status: u16) {
        self.state.lock().expect("test server state").status = status;
    }

    fn last_token(&self) -> Option<String> {
        self.state
            .lock()
            .expect("test server state")
            .last_token
            .clone()
    }

    fn last_namespace(&self) -> Option<String> {
        self.state
            .lock()
            .expect("test server state")
            .last_namespace
            .clone()
    }

    fn last_path(&self) -> Option<String> {
        self.state
            .lock()
            .expect("test server state")
            .last_path
            .clone()
    }

    fn requests(&self) -> usize {
        self.state.lock().expect("test server state").requests
    }
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<ServerState>>) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => {
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => return,
        }
    }

    let request_text = String::from_utf8_lossy(&request);
    let path = request_text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .map(ToOwned::to_owned);
    let token = header_value(&request_text, "x-vault-token");
    let namespace = header_value(&request_text, "x-vault-namespace");

    let (status, body) = {
        let mut state = state.lock().expect("test server state");
        state.requests += 1;
        state.last_path = path;
        state.last_token = token;
        state.last_namespace = namespace;
        if state.status == 200 {
            (200, state.body.clone())
        } else {
            (state.status, Vec::new())
        }
    };

    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn header_value(request: &str, name: &str) -> Option<String> {
    request.lines().find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        header_name
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

fn v2_body(value: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "data": {
            "data": value,
            "metadata": {"version": 1},
        }
    })
}

#[test]
fn reads_v2_secrets() {
    let server = TestServer::start(v2_body(serde_json::json!({
        "server": {"host": "localhost", "port": 8080},
        "enabled": true,
    })));
    let source = Vault::new(server.url(), "secret", "app/config")
        .unwrap()
        .token("hvs.token")
        .namespace("team-a");
    let config = Config::builder().source(source).build().unwrap();

    assert_eq!(config.get::<String>("server.host").unwrap(), "localhost");
    assert_eq!(config.get::<u16>("server.port").unwrap(), 8080);
    assert!(config.get::<bool>("enabled").unwrap());
    assert_eq!(server.last_token().as_deref(), Some("hvs.token"));
    assert_eq!(server.last_namespace().as_deref(), Some("team-a"));
    assert_eq!(
        server.last_path().as_deref(),
        Some("/v1/secret/data/app/config")
    );
}

#[test]
fn reads_v1_secrets() {
    let server = TestServer::start(serde_json::json!({
        "data": {"value": 7},
    }));
    let source = Vault::new(server.url(), "secret", "app").unwrap().kv_v1();
    let config = Config::builder().source(source).build().unwrap();

    assert_eq!(config.get::<u16>("value").unwrap(), 7);
    assert_eq!(server.last_path().as_deref(), Some("/v1/secret/app"));
}

#[test]
fn optional_missing_secret_is_empty() {
    let server = TestServer::start(serde_json::json!({}));
    server.set_status(404);

    let source = Vault::new(server.url(), "secret", "missing")
        .unwrap()
        .optional();
    let config = Config::builder().source(source).build().unwrap();
    assert!(config.snapshot().as_map().is_empty());

    let source = Vault::new(server.url(), "secret", "missing").unwrap();
    assert!(matches!(
        Config::builder().source(source).build(),
        Err(Error::Source { .. })
    ));
}

#[test]
fn non_success_status_is_an_error() {
    let server = TestServer::start(serde_json::json!({}));
    server.set_status(500);
    let source = Vault::new(server.url(), "secret", "app").unwrap();
    assert!(matches!(
        Config::builder().source(source).build(),
        Err(Error::Source { .. })
    ));
}

#[test]
fn watch_polls_for_changes() {
    let server = TestServer::start(v2_body(serde_json::json!({"value": 1})));
    let source = Vault::new(server.url(), "secret", "app")
        .unwrap()
        .poll_interval(Duration::from_millis(20));
    let config = Config::builder().watched(source).build().unwrap();
    let (guard, receiver) = config.watch_channel().unwrap();

    server.set_body(v2_body(serde_json::json!({"value": 2})));
    wait_for(&receiver, |event| {
        matches!(
            event,
            WatchEvent::Changed { snapshot, .. }
                if snapshot.get::<u16>("value").ok() == Some(2)
        )
    });

    drop(guard);
}

#[test]
fn source_uses_one_request_to_build() {
    let server = TestServer::start(v2_body(serde_json::json!({"value": 1})));
    let source = Vault::new(server.url(), "secret", "app").unwrap();
    let _config = Config::builder().source(source).build().unwrap();
    assert_eq!(server.requests(), 1);
}

fn wait_for<F>(receiver: &Receiver<WatchEvent>, predicate: F) -> WatchEvent
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
