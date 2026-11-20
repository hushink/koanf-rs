#![cfg(feature = "http")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use koanf_rs::http::Http;
use koanf_rs::prelude::*;

struct TestServer {
    address: SocketAddr,
    state: Arc<Mutex<ServerState>>,
}

struct ServerState {
    status: u16,
    body: Vec<u8>,
    content_type: String,
    etag: String,
    requests: usize,
    conditional_requests: usize,
    last_authorization: Option<String>,
}

impl TestServer {
    fn start(body: &str, etag: &str, content_type: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let state = Arc::new(Mutex::new(ServerState {
            status: 200,
            body: body.as_bytes().to_vec(),
            content_type: content_type.to_owned(),
            etag: etag.to_owned(),
            requests: 0,
            conditional_requests: 0,
            last_authorization: None,
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

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn set_response(&self, status: u16, body: &str, etag: &str, content_type: &str) {
        let mut state = self.state.lock().expect("test server state");
        state.status = status;
        state.body = body.as_bytes().to_vec();
        state.etag = etag.to_owned();
        state.content_type = content_type.to_owned();
    }

    fn requests(&self) -> usize {
        self.state.lock().expect("test server state").requests
    }

    fn conditional_requests(&self) -> usize {
        self.state
            .lock()
            .expect("test server state")
            .conditional_requests
    }

    fn last_authorization(&self) -> Option<String> {
        self.state
            .lock()
            .expect("test server state")
            .last_authorization
            .clone()
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
    let if_none_match = header_value(&request_text, "if-none-match");
    let authorization = header_value(&request_text, "authorization");

    let (status, body, etag, content_type) = {
        let mut state = state.lock().expect("test server state");
        state.requests += 1;
        if if_none_match.is_some() {
            state.conditional_requests += 1;
        }
        if authorization.is_some() {
            state.last_authorization = authorization;
        }
        if state.status == 200 && if_none_match.as_deref() == Some(state.etag.as_str()) {
            (
                304,
                Vec::new(),
                state.etag.clone(),
                state.content_type.clone(),
            )
        } else {
            (
                state.status,
                state.body.clone(),
                state.etag.clone(),
                state.content_type.clone(),
            )
        }
    };

    let response = if status == 304 {
        format!(
            "HTTP/1.1 304 Not Modified\r\nETag: {etag}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
    } else {
        format!(
            "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            reason(status),
            body.len()
        )
    };
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

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        304 => "Not Modified",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

#[cfg(feature = "json")]
fn wait_for_event<F>(receiver: &Receiver<WatchEvent>, predicate: F) -> WatchEvent
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

#[cfg(feature = "json")]
#[test]
fn fetches_and_revalidates() {
    let server = TestServer::start(r#"{"value":1}"#, "\"v1\"", "application/json");
    let source = Http::new(server.url("/config.json")).unwrap();
    let config = Config::builder().source(source).build().unwrap();

    assert_eq!(config.get::<u16>("value").unwrap(), 1);
    assert_eq!(server.requests(), 1);

    config.reload().unwrap();
    assert_eq!(config.get::<u16>("value").unwrap(), 1);
    assert_eq!(server.requests(), 2);
    assert_eq!(server.conditional_requests(), 1);

    server.set_response(200, r#"{"value":2}"#, "\"v2\"", "application/json");
    config.reload().unwrap();
    assert_eq!(config.get::<u16>("value").unwrap(), 2);
}

#[cfg(feature = "json")]
#[test]
fn explicit_format_and_auth_headers() {
    let server = TestServer::start(r#"{"value":3}"#, "\"v1\"", "text/plain");
    let source = Http::new(server.url("/config.data"))
        .unwrap()
        .format(koanf_rs::format::Json)
        .bearer_token("secret")
        .header("X-Test", "yes")
        .unwrap();

    let config = Config::builder().source(source).build().unwrap();
    assert_eq!(config.get::<u16>("value").unwrap(), 3);
    assert_eq!(
        server.last_authorization().as_deref(),
        Some("Bearer secret")
    );
}

#[test]
fn unknown_content_type_requires_an_explicit_format() {
    let server = TestServer::start(r#"{"value":1}"#, "\"v1\"", "text/plain");
    let source = Http::new(server.url("/config.data")).unwrap();
    assert!(matches!(
        Config::builder().source(source).build(),
        Err(Error::UnknownFormat { .. })
    ));
}

#[test]
fn non_success_status_is_an_error() {
    let server = TestServer::start("server error", "\"v1\"", "text/plain");
    server.set_response(500, "server error", "\"v1\"", "text/plain");
    let source = Http::new(server.url("/config.data")).unwrap();
    assert!(matches!(
        Config::builder().source(source).build(),
        Err(Error::Source { .. })
    ));
}

#[cfg(feature = "json")]
#[test]
fn watch_polls_revalidates_and_recovers_from_errors() {
    let server = TestServer::start(r#"{"value":1}"#, "\"v1\"", "application/json");
    let source = Http::new(server.url("/config.json"))
        .unwrap()
        .poll_interval(Duration::from_millis(20));
    let config = Config::builder().watched(source).build().unwrap();
    let (guard, receiver) = config.watch_channel().unwrap();

    server.set_response(200, r#"{"value":2}"#, "\"v2\"", "application/json");
    wait_for_event(&receiver, |event| {
        matches!(
            event,
            WatchEvent::Changed { snapshot, .. } if snapshot.get::<u16>("value").ok() == Some(2)
        )
    });

    server.set_response(200, r#"{"value":[invalid}"#, "\"v3\"", "application/json");
    wait_for_event(&receiver, |event| matches!(event, WatchEvent::Error { .. }));
    assert_eq!(config.get::<u16>("value").unwrap(), 2);

    server.set_response(200, r#"{"value":3}"#, "\"v4\"", "application/json");
    wait_for_event(&receiver, |event| {
        matches!(
            event,
            WatchEvent::Changed { snapshot, .. } if snapshot.get::<u16>("value").ok() == Some(3)
        )
    });

    drop(guard);
}
