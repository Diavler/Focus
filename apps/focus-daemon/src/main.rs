//! Per-user daemon that owns sessions and enforcement; the composition root that wires adapters
//! into `focus-core`.

use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use config_toml::TomlConfigSource;
use focus_core::ConfigState;
use focus_core::ports::NoopHistory;
use focus_daemon::{Paths, Ports, Service, SystemClock, serve};
use ipc_transport::{BindError, Endpoint, bind};
use state_json::JsonStateStore;

/// Written to the settings path on first run, so there is something to start a session with.
const STARTER_SETTINGS: &str = include_str!("../starter-settings.toml");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let Paths { settings, state } = Paths::from_env(|name| std::env::var(name).ok())
        .context("couldn't work out where to keep settings: set APPDATA or HOME")?;
    write_starter_settings(&settings)?;

    // Bind before touching state, so a second daemon exits without reading or writing anything.
    let endpoint = Endpoint::for_current_user();
    let listener = match bind(&endpoint).await {
        Err(BindError::AlreadyRunning(endpoint)) => {
            tracing::info!("a Focus daemon is already running at {endpoint}; exiting");
            return Ok(());
        }
        other => other?,
    };

    let ports = Ports {
        clock: Arc::new(SystemClock),
        configs: Arc::new(TomlConfigSource::new(&settings)),
        store: Arc::new(JsonStateStore::new(&state)),
        history: Arc::new(NoopHistory),
    };
    let service = Service::open(ports, ConfigState::DEFAULT_COOLING_OFF)?;
    tracing::info!("settings: {}", settings.display());
    tracing::info!("state: {}", state.display());

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
fn write_starter_settings(path: &Path) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path);
    match file {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(STARTER_SETTINGS.as_bytes())
                .with_context(|| format!("writing {}", path.display()))?;
            tracing::info!("created starter settings at {}", path.display());
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error).with_context(|| format!("creating {}", path.display())),
    }
}
