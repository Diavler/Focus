//! The host relaying between a pretend browser (an in-memory pipe) and an in-process daemon.

// Test helpers outside `#[test]` functions aren't covered by clippy's allow-unwrap-in-tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use config_toml::TomlConfigSource;
use focus_core::ConfigState;
use focus_core::ports::NoopHistory;
use focus_core::testing::{
    FakeAutostart, FakeCalendar, FakeClock, FakeEnforcer, FakeForegroundApp, FakeNotifier,
    FakeTasks, FakeUi, MemorySecrets, MemoryStateStore,
};
use focus_daemon::{Ports, Service, serve};
use focus_nmh::framing::{Frame, read_message};
use ipc_transport::{Endpoint, bind};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncWriteExt, DuplexStream, duplex};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

fn unique_endpoint() -> Endpoint {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    Endpoint::Namespaced(format!("focus-nmh-test-{}-{n}", std::process::id()))
}

/// A daemon serving `endpoint` until dropped via `stop`.
struct Daemon {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
    _dir: TempDir,
}

impl Daemon {
    async fn start(endpoint: &Endpoint) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.toml");
        std::fs::write(&settings, "[configs.coding]\nblock = [\"x.com\"]\n").unwrap();
        let ports = Ports {
            clock: Arc::new(FakeClock::default()),
            configs: Arc::new(TomlConfigSource::new(&settings)),
            store: Arc::new(MemoryStateStore::default()),
            history: Arc::new(NoopHistory),
            enforcer: Arc::new(FakeEnforcer::default()),
            processes: Arc::new(FakeEnforcer::default()),
            foreground: Arc::new(FakeForegroundApp::default()),
            notifier: Arc::new(FakeNotifier::default()),
            browsers: Vec::new(),
            autostart: Arc::new(FakeAutostart::default()),
            ui: Arc::new(FakeUi::running()),
            calendar: Arc::new(FakeCalendar::default()),
            tasks: Arc::new(FakeTasks::default()),
            secrets: Arc::new(MemorySecrets::default()),
            settings: Arc::new(TomlConfigSource::new(&settings)),
            windowed: Arc::new(focus_core::testing::FakeWindowedApps::with([
                "discord", "notepad",
            ])),
        };
        let service = Service::open(ports, ConfigState::DEFAULT_COOLING_OFF).unwrap();
        let listener = bind(endpoint).await.unwrap();
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(serve(listener, service, async {
            let _ = stopped.await;
        }));
        Self {
            stop,
            task,
            _dir: dir,
        }
    }

    async fn stop(self) {
        let _ = self.stop.send(());
        self.task.await.unwrap();
    }
}

/// The browser's end of the pipe, with the host running on the other end.
struct Browser {
    pipe: DuplexStream,
    host: JoinHandle<std::io::Result<()>>,
}

impl Browser {
    /// A browser whose host sees a session saved as running whenever the daemon is gone.
    fn connect(endpoint: &Endpoint) -> Self {
        Self::connect_with_saved_session(endpoint, true)
    }

    fn connect_with_saved_session(endpoint: &Endpoint, session_saved: bool) -> Self {
        let (browser, host_side) = duplex(64 * 1024);
        let endpoint = endpoint.clone();
        let host = tokio::spawn(async move {
            let (mut reader, mut writer) = tokio::io::split(host_side);
            focus_nmh::host::run(&mut reader, &mut writer, &endpoint, move || !session_saved).await
        });
        Self {
            pipe: browser,
            host,
        }
    }

    /// Sends raw bytes as one message and returns the parsed answer.
    async fn send_raw(&mut self, bytes: &[u8]) -> Value {
        // Framed by hand: browsers may send hosts more than the 1 MiB `write_message` allows.
        let length = u32::try_from(bytes.len()).unwrap().to_ne_bytes();
        self.pipe.write_all(&length).await.unwrap();
        self.pipe.write_all(bytes).await.unwrap();
        let Some(Frame::Message(answer)) = read_message(&mut self.pipe).await.unwrap() else {
            panic!("expected an answer from the host");
        };
        serde_json::from_slice(&answer).unwrap()
    }

    /// Sends `message` with `id` in the current protocol version.
    async fn send(&mut self, id: u64, message: Value) -> Value {
        let request = json!({"id": id, "version": ipc_proto::PROTOCOL_VERSION, "message": message});
        let answer = self.send_raw(&serde_json::to_vec(&request).unwrap()).await;
        assert_eq!(answer["id"], id, "answers carry the request's id");
        answer["message"].clone()
    }

    /// Closes the pipe like a browser does and waits for the host to exit.
    async fn close(self) {
        drop(self.pipe);
        self.host.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn relays_extension_requests_to_the_daemon() {
    let endpoint = unique_endpoint();
    let daemon = Daemon::start(&endpoint).await;
    let mut browser = Browser::connect(&endpoint);

    let done = browser
        .send(
            1,
            json!({"type": "heartbeat", "browser": "chrome", "incognito_allowed": false}),
        )
        .await;
    assert_eq!(done, json!({"type": "done"}));

    let verdict = browser
        .send(
            2,
            json!({"type": "rules_for", "url": "https://x.com", "config": "coding"}),
        )
        .await;
    assert_eq!(verdict["type"], "verdict");
    assert_eq!(verdict["verdict"], json!({"restrict": "block"}));

    let status = browser.send(3, json!({"type": "status"})).await;
    assert_eq!(status["browsers"][0]["browser"], "chrome");
    assert_eq!(status["friction"]["tab_cap"], 10);

    // Relayed (not refused), with the shapes the extension parses.
    let unlock = browser
        .send(
            4,
            json!({"type": "unlock", "url": "https://x.com/", "reason": "docs"}),
        )
        .await;
    assert_eq!(
        unlock,
        json!({"type": "error", "error": {"kind": "unlock", "error": "no_session"}})
    );
    let delay = browser
        .send(5, json!({"type": "delay_visit", "url": "https://x.com/"}))
        .await;
    assert_eq!(delay, json!({"type": "delay", "seconds": 0}));

    browser.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn refuses_requests_the_browser_may_not_send() {
    let endpoint = unique_endpoint();
    let daemon = Daemon::start(&endpoint).await;
    let mut browser = Browser::connect(&endpoint);

    let refused = browser
        .send(1, json!({"type": "start_session", "config": "coding", "strictness": "light", "duration_secs": 60}))
        .await;
    assert_eq!(refused["error"]["kind"], "bad_request");
    let refused = browser.send(2, json!({"type": "end_session"})).await;
    assert_eq!(refused["error"]["kind"], "bad_request");
    let refused = browser
        .send(
            3,
            json!({"type": "set_secret", "key": "calendar_url", "value": "https://evil.example/"}),
        )
        .await;
    assert_eq!(
        refused["error"]["kind"], "bad_request",
        "a page can't swap the calendar"
    );

    let malformed = browser.send_raw(b"not json").await;
    assert_eq!(malformed["id"], 0);
    assert_eq!(malformed["message"]["error"]["kind"], "bad_request");

    let unversioned = browser
        .send_raw(br#"{"id":9,"message":{"type":"status"}}"#)
        .await;
    assert_eq!(
        unversioned["id"], 9,
        "a readable id is kept even when the rest is malformed"
    );
    assert_eq!(unversioned["message"]["error"]["kind"], "bad_request");

    let huge = vec![b' '; focus_nmh::framing::MAX_INCOMING + 1];
    let too_large = browser.send_raw(&huge).await;
    assert_eq!(too_large["message"]["error"]["kind"], "bad_request");
    let status = browser.send(10, json!({"type": "status"})).await;
    assert_eq!(
        status["type"], "status",
        "the relay survives an oversized message"
    );

    let newer = browser
        .send_raw(br#"{"id":5,"version":99,"message":{"type":"status"}}"#)
        .await;
    assert_eq!(newer["message"]["error"]["kind"], "unsupported_version");

    browser.close().await;
    daemon.stop().await;
}

#[tokio::test]
async fn reports_focus_stopped_when_no_session_is_saved() {
    let endpoint = unique_endpoint();
    let mut browser = Browser::connect_with_saved_session(&endpoint, false);
    let status = browser.send(1, json!({"type": "status"})).await;
    assert_eq!(
        status["error"]["kind"], "stopped",
        "the extension may lift its blocks"
    );
}

#[tokio::test]
async fn reports_an_unreachable_daemon_and_reconnects_when_it_returns() {
    let endpoint = unique_endpoint();
    let mut browser = Browser::connect(&endpoint);
    let status = browser.send(1, json!({"type": "status"})).await;
    assert_eq!(status["error"]["kind"], "unreachable");

    let daemon = Daemon::start(&endpoint).await;
    let status = browser.send(2, json!({"type": "status"})).await;
    assert_eq!(status["type"], "status");

    daemon.stop().await;
    let daemon = Daemon::start(&endpoint).await;
    let status = browser.send(3, json!({"type": "status"})).await;
    assert_eq!(
        status["type"], "status",
        "reconnects after a daemon restart"
    );

    browser.close().await;
    daemon.stop().await;
}
