// No console window on Windows (see the module docs).
#![cfg_attr(windows, windows_subsystem = "windows")]

//! Per-user daemon that owns sessions and enforcement; the composition root that wires adapters
//! into `focus-core`.
//!
//! On Windows it's a windowed program, so starting it (at login, or from the UI) never opens a
//! console window that could be closed to stop it. It logs to `data/daemon.log` instead.
//!
//! `focus-daemon --dry-run` logs which apps it would close (and the Do Not Disturb prompt it would
//! show) instead of acting, for trying enforcement safely on a real machine.
//! `focus-daemon --at-login` is how the login entry starts it: it also opens the UI, minimised.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use config_toml::TomlConfigSource;
use focus_core::ConfigState;
use focus_core::ports::NoopHistory;
use focus_daemon::{InstalledUi, Ports, Service, SystemClock, serve};
use focus_install::{InstanceLock, Paths};
use integrations::{IcalCalendar, TodoistTasks};
use ipc_transport::{BindError, Endpoint, bind};
use secrets_keyring::KeyringSecrets;
use state_json::JsonStateStore;

/// Written to the settings path on first run, so there is something to start a session with.
const STARTER_SETTINGS: &str = include_str!("../starter-settings.toml");

/// How long shutdown waits for blocking work still running (such as a plan fetch stuck on the
/// network) before the process exits anyway.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

fn main() -> anyhow::Result<()> {
    // Built by hand instead of `#[tokio::main]`, so shutdown doesn't wait out a slow HTTP fetch
    // on the blocking pool (it's abandoned after `SHUTDOWN_GRACE`; nothing saves after `serve`).
    let runtime = tokio::runtime::Runtime::new().context("couldn't start the async runtime")?;
    let result = runtime.block_on(run());
    runtime.shutdown_timeout(SHUTDOWN_GRACE);
    result
}

async fn run() -> anyhow::Result<()> {
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

    init_logging(&paths.data, &paths.daemon_lock);

    // Take the install's lock before anything else, so a second daemon exits without binding,
    // reading or writing anything. The lock is held until the process exits.
    let Some(_instance) = InstanceLock::acquire(&paths.daemon_lock)
        .with_context(|| format!("locking {}", paths.daemon_lock.display()))?
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
    let dry_run = std::env::args().skip(1).any(|arg| arg == "--dry-run");
    if dry_run {
        tracing::info!("dry run: apps won't be closed, only logged");
    }
    let at_login = std::env::args().skip(1).any(|arg| arg == "--at-login");
    let enforcement = platform::enforcement(dry_run, install);
    // The settings file is both read (`ConfigSource`) and edited for the window
    // (`SettingsEditor`) through one adapter.
    let settings = Arc::new(TomlConfigSource::new(&paths.settings));
    // One keychain adapter, shared (`Arc`) by the two sources that read from it and the service
    // that writes to it.
    let secrets = Arc::new(KeyringSecrets::new());
    let ports = Ports {
        clock: Arc::new(SystemClock),
        configs: settings.clone(),
        store: Arc::new(JsonStateStore::new(&paths.state)),
        history: Arc::new(NoopHistory),
        enforcer: enforcement.enforcer,
        processes: enforcement.processes,
        foreground: enforcement.foreground,
        notifier: enforcement.notifier,
        browsers: enforcement.browsers,
        autostart: enforcement.autostart,
        ui: Arc::new(InstalledUi {
            install: install.to_path_buf(),
            lock: paths.ui_lock.clone(),
            dry_run,
        }),
        calendar: Arc::new(IcalCalendar::new(secrets.clone())),
        tasks: Arc::new(TodoistTasks::new(secrets.clone())),
        secrets,
        settings: settings.clone(),
    };
    let mut service = Service::open(ports, ConfigState::DEFAULT_COOLING_OFF)?;
    if at_login {
        service.ensure_ui();
    }
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

/// Most a log file grows before the next start moves it to `daemon.log.old`.
const LOG_LIMIT: u64 = 1024 * 1024;

/// Sends logs to `data/daemon.log` (appending). If it's grown past [`LOG_LIMIT`] and no other
/// daemon is running, it's moved to `daemon.log.old` first. Without a log file, logs are dropped.
fn init_logging(data: &Path, daemon_lock: &Path) {
    let log = data.join("daemon.log");
    let too_big = std::fs::metadata(&log).is_ok_and(|meta| meta.len() > LOG_LIMIT);
    if too_big && !focus_install::is_held(daemon_lock).unwrap_or(true) {
        let _ = std::fs::rename(&log, data.join("daemon.log.old"));
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log);
    if let Ok(file) = file {
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init();
    }
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

/// The platform's enforcement adapters. The only `#[cfg]` in the daemon (CLAUDE.md).
mod platform {
    use std::sync::Arc;

    use focus_core::KnownBrowser;
    use std::path::Path;

    use focus_core::ports::{Autostart, Enforcer, ForegroundApp, Notifier, ProcessList};

    /// One adapter per enforcement port, plus the browsers to watch.
    pub struct Enforcement {
        /// Closes blocked apps.
        pub enforcer: Arc<dyn Enforcer>,
        /// Tells which apps are running.
        pub processes: Arc<dyn ProcessList>,
        /// Tells which app is in front.
        pub foreground: Arc<dyn ForegroundApp>,
        /// Silences notifications during sessions.
        pub notifier: Arc<dyn Notifier>,
        /// The browsers to watch.
        pub browsers: Vec<KnownBrowser>,
        /// Registers Focus to start at login.
        pub autostart: Arc<dyn Autostart>,
    }

    /// Windows: the real adapters, acting or (`dry_run`) only logging.
    #[cfg(windows)]
    pub fn enforcement(dry_run: bool, install: &Path) -> Enforcement {
        use enforce_windows::{
            Mode, WindowsAutostart, WindowsEnforcer, WindowsForeground, WindowsNotifier,
        };
        let mode = if dry_run { Mode::DryRun } else { Mode::Live };
        // One enforcer serves both ports; `Arc` lets it be shared.
        let enforcer = Arc::new(WindowsEnforcer::new(mode));
        Enforcement {
            enforcer: enforcer.clone(),
            processes: enforcer,
            foreground: Arc::new(WindowsForeground),
            notifier: Arc::new(WindowsNotifier::new(mode)),
            browsers: enforce_windows::known_browsers(),
            autostart: Arc::new(WindowsAutostart::new(mode, install)),
        }
    }

    /// Elsewhere: placeholders until the KDE Plasma adapters (build step 9).
    #[cfg(not(windows))]
    pub fn enforcement(_dry_run: bool, _install: &Path) -> Enforcement {
        let pending = Arc::new(enforce_linux_kde::PendingEnforcer::default());
        Enforcement {
            enforcer: pending.clone(),
            processes: pending.clone(),
            foreground: pending.clone(),
            notifier: pending.clone(),
            browsers: enforce_linux_kde::known_browsers(),
            autostart: pending,
        }
    }
}
