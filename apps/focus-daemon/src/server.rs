//! Accepts connections and answers each request line with the [`Service`].

use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ipc_proto::{DaemonError, Request, Response, decode, encode};
use ipc_transport::{Connection, Listener};
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;

use crate::service::Service;

/// How often the session timer and due config changes are checked.
const TICK: Duration = Duration::from_secs(1);

/// The settings file is re-read every this many ticks (and on `ListConfigs` / `StartSession`).
const RELOAD_EVERY_TICKS: u32 = 5;

/// Blocked apps and unwatched browsers are checked every this many ticks.
const ENFORCE_EVERY_TICKS: u32 = 5;

/// Pause after a failed `accept`, so a persistent error doesn't spin the CPU.
const ACCEPT_RETRY: Duration = Duration::from_millis(100);

/// The service shared by every task; `None` once shutdown has begun.
type Shared = Arc<Mutex<Option<Service>>>;

/// Serves `listener` until `shutdown` completes, then stops every task before returning.
///
/// The service does blocking file I/O (reading settings, saving state), so it only ever runs on
/// tokio's blocking-thread pool, never on the async worker threads that accept connections. Its
/// `Mutex` is locked only inside those blocking calls, never across an `.await`.
pub async fn serve(listener: Listener, service: Service, shutdown: impl Future<Output = ()>) {
    tracing::info!("listening at {}", listener.endpoint());
    let shared: Shared = Arc::new(Mutex::new(Some(service)));
    let mut tasks = JoinSet::new();
    tasks.spawn(tick_forever(Arc::clone(&shared)));
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok(connection) => {
                    tasks.spawn(answer(connection, Arc::clone(&shared)));
                }
                Err(error) => {
                    tracing::warn!("accepting a connection failed: {error}");
                    tokio::time::sleep(ACCEPT_RETRY).await;
                }
            },
        }
        // Drop finished connection tasks so the set doesn't grow forever.
        while tasks.try_join_next().is_some() {}
    }
    tasks.shutdown().await;
    // A blocking call already started can't be aborted. Taking the service waits for it (it
    // holds the lock), and any call that starts later finds `None`, so nothing saves after
    // `serve` returns and a restarted daemon can't be overwritten by this one.
    let _ = tokio::task::spawn_blocking(move || lock(&shared).take()).await;
    tracing::info!("stopped");
}

/// Answers requests on one connection until the client disconnects.
async fn answer(mut connection: Connection, shared: Shared) {
    loop {
        let line = match connection.recv_line().await {
            Ok(Some(line)) => line,
            Ok(None) => return,
            Err(error) => {
                tracing::debug!("dropping a connection: {error}");
                return;
            }
        };
        let response = match decode::<Request>(&line) {
            Ok(request) => handle(&shared, request).await,
            Err(error) => Response::Error {
                error: error.into(),
            },
        };
        if connection.send_line(&encode(&response)).await.is_err() {
            return;
        }
    }
}

/// Runs one request on the blocking pool.
async fn handle(shared: &Shared, request: Request) -> Response {
    let shared = Arc::clone(shared);
    let shutting_down = || Response::Error {
        error: DaemonError::ShuttingDown,
    };
    tokio::task::spawn_blocking(move || match lock(&shared).as_mut() {
        Some(service) => service.handle(request),
        None => shutting_down(),
    })
    .await
    .unwrap_or_else(|error| {
        tracing::error!("handling a request panicked: {error}");
        Response::Error {
            error: DaemonError::Internal {
                message: "handling the request panicked".into(),
            },
        }
    })
}

/// Expires sessions and applies due changes every tick, re-reading settings now and then.
async fn tick_forever(shared: Shared) {
    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut ticks: u32 = 0;
    loop {
        interval.tick().await;
        let reload = ticks.is_multiple_of(RELOAD_EVERY_TICKS);
        let enforce = ticks.is_multiple_of(ENFORCE_EVERY_TICKS);
        let shared = Arc::clone(&shared);
        let ticked = tokio::task::spawn_blocking(move || {
            if let Some(service) = lock(&shared).as_mut() {
                if reload {
                    service.reload_settings();
                }
                service.tick();
                if enforce {
                    service.enforce();
                    service.watch_ui();
                    service.ensure_autostart();
                }
            }
        })
        .await;
        if let Err(error) = ticked {
            tracing::error!("a tick panicked: {error}");
        }
        ticks = ticks.wrapping_add(1);
    }
}

/// Locks the service. A panic while it was locked can't have left it half-changed, because every
/// change is made to a copy and swapped in only after saving, so a poisoned lock is safe to reuse.
fn lock(shared: &Mutex<Option<Service>>) -> MutexGuard<'_, Option<Service>> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}
