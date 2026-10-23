//! Accepts connections and answers each request line with the [`Service`].

use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ipc_proto::{DaemonError, DecodeError, PROTOCOL_VERSION, Request, Response, decode, encode};
use ipc_transport::{Connection, Listener};
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;

use crate::service::Service;

/// How often the session timer and due config changes are checked.
const TICK: Duration = Duration::from_secs(1);

/// The settings file is re-read every this many ticks (and on `ListConfigs` / `StartSession`).
const RELOAD_EVERY_TICKS: u32 = 5;

/// Pause after a failed `accept`, so a persistent error doesn't spin the CPU.
const ACCEPT_RETRY: Duration = Duration::from_millis(100);

/// Serves `listener` until `shutdown` completes, then stops every task before returning.
///
/// All tasks share one [`Service`] behind a `Mutex`. It is locked only for the synchronous
/// handling of one request, never across an `.await`.
pub async fn serve(listener: Listener, service: Service, shutdown: impl Future<Output = ()>) {
    tracing::info!("listening at {}", listener.endpoint());
    let service = Arc::new(Mutex::new(service));
    let mut tasks = JoinSet::new();
    tasks.spawn(tick_forever(Arc::clone(&service)));
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok(connection) => {
                    tasks.spawn(answer(connection, Arc::clone(&service)));
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
    tracing::info!("stopped");
}

/// Answers requests on one connection until the client disconnects.
async fn answer(mut connection: Connection, service: Arc<Mutex<Service>>) {
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
            Ok(request) => lock(&service).handle(request),
            Err(DecodeError::UnsupportedVersion { version }) => Response::Error {
                error: DaemonError::UnsupportedVersion {
                    daemon: PROTOCOL_VERSION,
                    client: version,
                },
            },
            Err(DecodeError::Malformed { message }) => Response::Error {
                error: DaemonError::BadRequest { message },
            },
        };
        if connection.send_line(&encode(&response)).await.is_err() {
            return;
        }
    }
}

/// Expires sessions and applies due changes every tick, re-reading settings now and then.
async fn tick_forever(service: Arc<Mutex<Service>>) {
    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut ticks: u32 = 0;
    loop {
        interval.tick().await;
        let mut service = lock(&service);
        if ticks % RELOAD_EVERY_TICKS == 0 {
            service.reload_settings();
        }
        service.tick();
        ticks = ticks.wrapping_add(1);
    }
}

/// Locks the service. A panic while it was locked can't have left it half-changed, because every
/// change is made to a copy and swapped in only after saving, so a poisoned lock is safe to reuse.
fn lock(service: &Mutex<Service>) -> MutexGuard<'_, Service> {
    service.lock().unwrap_or_else(PoisonError::into_inner)
}
