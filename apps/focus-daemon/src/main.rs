//! Per-user daemon that owns sessions and enforcement; the composition root that wires adapters
//! into `focus-core`.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use config_toml::TomlConfigSource;
use focus_core::ConfigState;
use focus_core::ports::NoopHistory;
use focus_daemon::{InstanceLock, Paths, Ports, Service, SystemClock, serve};
use ipc_transport::{BindError, Endpoint, bind};
use state_json::JsonStateStore;

/// Written to the settings path on first run, so there is something to start a session with.
const STARTER_SETTINGS: &str = include_str!("../starter-settings.toml");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let exe = std::env::current_exe().context("couldn't find the daemon's own path")?;
    let install = exe
        .parent()
        .context("the daemon's path has no parent folder")?;
    let paths = Paths::portable(install);
    std::fs::create_dir_all(&paths.data).with_context(|| {
        format!(
            "can't write to {}; put Focus in a folder you can write to without admin \
             (not Program Files)",
            paths.data.display()
        )
    })?;

    // Take the install's lock before anything else, so a second daemon exits without binding,
    // reading or writing anything. The lock is held until the process exits.
    let Some(_instance) = InstanceLock::acquire(&paths.lock)
        .with_context(|| format!("locking {}", paths.lock.display()))?
    else {
        tracing::info!("a Focus daemon is already running for this install; exiting");
        return Ok(());
    };
    let endpoint = Endpoint::for_install(install);
    let listener = match bind(&endpoint).await {
        Err(BindError::AlreadyRunning(endpoint)) => {
            tracing::info!("another Focus daemon is already running at {endpoint}; exiting");
            return Ok(());
        }
        other => other?,
    };

    write_starter_settings(&paths.settings)?;
    let ports = Ports {
        clock: Arc::new(SystemClock),
        configs: Arc::new(TomlConfigSource::new(&paths.settings)),
        store: Arc::new(JsonStateStore::new(&paths.state)),
        history: Arc::new(NoopHistory),
    };
    let service = Service::open(ports, ConfigState::DEFAULT_COOLING_OFF)?;
    tracing::info!("data: {}", paths.data.display());

    serve(listener, service, async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!("can't listen for Ctrl+C: {error}");
            std::future::pending::<()>().await;
        }
    })
    .await;
    Ok(())
}

/// Creates the settings file with starter content if it doesn't exist; never overwrites one.
///
/// The content goes to a temp file first and is renamed into place, so an interrupted first run
/// leaves no half-written settings. Only the instance-lock holder calls this, so nothing else
/// creates the file between the check and the rename.
fn write_starter_settings(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        return Ok(());
    }
    let temp = path.with_extension("toml.tmp");
    let mut file =
        std::fs::File::create(&temp).with_context(|| format!("creating {}", temp.display()))?;
    file.write_all(STARTER_SETTINGS.as_bytes())
        .and_then(|()| file.sync_all())
        .with_context(|| format!("writing {}", temp.display()))?;
    drop(file);
    std::fs::rename(&temp, path).with_context(|| format!("creating {}", path.display()))?;
    tracing::info!("created starter settings at {}", path.display());
    Ok(())
}
