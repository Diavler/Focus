//! The daemon in-process over real local sockets, with the real settings and state adapters.

// Test helpers outside `#[test]` functions aren't covered by clippy's allow-unwrap-in-tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use config_toml::TomlConfigSource;
use focus_core::ports::{Clock, NoopHistory};
use focus_core::testing::FakeClock;
use focus_core::{ConfigState, EndReason, SessionError, Strictness};
use focus_daemon::{Ports, Service, serve};
use ipc_proto::{DaemonError, PROTOCOL_VERSION, Request, Response, decode, encode};
use ipc_transport::{BindError, Client, ClientError, Connection, Endpoint, bind};
use state_json::JsonStateStore;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const MINUTE: Duration = Duration::from_secs(60);

/// A fresh endpoint per test, so tests running in parallel don't meet.
fn unique_endpoint() -> Endpoint {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    Endpoint::Namespaced(format!("focus-test-{}-{n}", std::process::id()))
}

/// Settings and state files in a temporary directory, plus a fake clock shared by every daemon
/// started from it, as if the same user restarted the daemon.
struct Home {
    dir: TempDir,
    settings: PathBuf,
    state: PathBuf,
    clock: Arc<FakeClock>,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.toml");
        std::fs::write(
            &settings,
            "[configs.coding]\nfeed_only = [\"reddit.com\"]\n",
        )
        .unwrap();
        Self {
            state: dir.path().join("state").join("state.json"),
            settings,
            dir,
            clock: Arc::new(FakeClock::default()),
        }
    }

    /// Binds `endpoint` and serves it in a background task, as `main` does.
    async fn start_daemon(&self, endpoint: &Endpoint) -> RunningDaemon {
        let listener = bind(endpoint).await.unwrap();
        let ports = Ports {
            clock: self.clock.clone(),
            configs: Arc::new(TomlConfigSource::new(&self.settings)),
            store: Arc::new(JsonStateStore::new(&self.state)),
            history: Arc::new(NoopHistory),
        };
        let service = Service::open(ports, ConfigState::DEFAULT_COOLING_OFF).unwrap();
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(serve(listener, service, async {
            let _ = stopped.await;
        }));
        RunningDaemon { stop, task }
    }
}

struct RunningDaemon {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl RunningDaemon {
    /// Stops the daemon and waits until all of its tasks have finished.
    async fn stop(self) {
        let _ = self.stop.send(());
        self.task.await.unwrap();
    }
}

#[tokio::test]
async fn a_session_survives_a_daemon_restart() {
    let home = Home::new();
    let endpoint = unique_endpoint();
    let daemon = home.start_daemon(&endpoint).await;

    let mut client = Client::connect(&endpoint).await.unwrap();
    let names: Vec<_> = client
        .list_configs()
        .await
        .unwrap()
        .into_iter()
        .map(|config| config.name)
        .collect();
    assert_eq!(names, ["coding"]);
    client
        .start_session("coding", Strictness::Locked, 25 * MINUTE)
        .await
        .unwrap();
    assert!(client.status().await.unwrap().session.running().is_some());
    drop(client);

    daemon.stop().await;
    assert!(
        Client::connect(&endpoint).await.is_err(),
        "nothing listens after stopping"
    );

    home.clock.advance(10 * MINUTE);
    let daemon = home.start_daemon(&endpoint).await;
    let mut client = Client::connect(&endpoint).await.unwrap();
    let status = client.status().await.unwrap();
    let running = status.session.running().expect("the session resumed");
    assert_eq!(running.time_left(status.now), 15 * MINUTE);
    assert!(matches!(
        client.end_session().await,
        Err(ClientError::Daemon(DaemonError::Session {
            error: SessionError::Locked
        }))
    ));

    home.clock.advance(20 * MINUTE);
    let status = client.status().await.unwrap();
    assert_eq!(
        status.session.ended().map(|ended| ended.reason()),
        Some(&EndReason::Completed)
    );
    daemon.stop().await;
}

#[tokio::test]
async fn only_one_daemon_per_endpoint() {
    let home = Home::new();
    let endpoint = unique_endpoint();
    let daemon = home.start_daemon(&endpoint).await;
    assert!(matches!(
        bind(&endpoint).await,
        Err(BindError::AlreadyRunning(_))
    ));
    daemon.stop().await;

    let daemon = home.start_daemon(&endpoint).await;
    daemon.stop().await;
}

#[tokio::test]
async fn bad_lines_get_errors_and_the_connection_stays_usable() {
    let home = Home::new();
    let endpoint = unique_endpoint();
    let daemon = home.start_daemon(&endpoint).await;
    let mut connection = Connection::connect(&endpoint).await.unwrap();

    let mut ask = async |line: &str| -> Response {
        connection.send_line(line).await.unwrap();
        let answer = connection.recv_line().await.unwrap().unwrap();
        decode(&answer).unwrap()
    };
    assert!(matches!(
        ask("this is not json\n").await,
        Response::Error {
            error: DaemonError::BadRequest { .. }
        }
    ));
    assert_eq!(
        ask("{\"version\":99,\"message\":{\"type\":\"status\"}}\n").await,
        Response::Error {
            error: DaemonError::UnsupportedVersion {
                daemon: PROTOCOL_VERSION,
                client: 99
            }
        }
    );
    assert!(matches!(
        ask(&encode(&Request::Status)).await,
        Response::Status(_)
    ));
    daemon.stop().await;
}

#[tokio::test]
async fn heartbeats_and_rules_over_ipc() {
    let home = Home::new();
    let endpoint = unique_endpoint();
    let daemon = home.start_daemon(&endpoint).await;
    let mut client = Client::connect(&endpoint).await.unwrap();

    client.heartbeat("chrome", true).await.unwrap();
    let status = client.status().await.unwrap();
    assert_eq!(status.browsers.len(), 1);
    assert_eq!(status.browsers[0].last_seen, home.clock.now());

    let (verdict, config) = client
        .rules_for("https://reddit.com/", Some("coding"))
        .await
        .unwrap();
    assert_eq!(verdict.to_string(), "Hide feed");
    assert_eq!(config.as_deref(), Some("coding"));
    daemon.stop().await;
}

#[tokio::test]
async fn install_socket_replaces_a_file_left_by_a_crash() {
    // On Windows the install endpoint is the user's real named pipe, which a running daemon
    // may hold; the socket-file behaviour under test only exists on Linux.
    if std::env::consts::FAMILY == "windows" {
        return;
    }
    let home = Home::new();
    let endpoint = Endpoint::for_install(home.dir.path());
    let Endpoint::Path(socket) = &endpoint else {
        panic!("expected a socket file, got {endpoint}");
    };
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    std::fs::write(socket, "left behind by a crash").unwrap();

    let daemon = home.start_daemon(&endpoint).await;
    let mut client = Client::connect(&endpoint).await.unwrap();
    assert!(client.status().await.is_ok());
    daemon.stop().await;
}
