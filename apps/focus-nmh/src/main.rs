//! Native messaging host: bridges the browser extension (stdin/stdout JSON) to `focus-daemon` IPC.
//!
//! - `focus-nmh chrome-extension://<id>/`: started by the browser; relays messages until it closes.
//! - `focus-nmh --register --extension-id <id> [--dry-run]`: tells Chromium browsers about the host.
//! - `focus-nmh --unregister [--dry-run]`: undoes `--register`.
//!
//! In host mode stdout carries the protocol, so diagnostics only ever go to stderr.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use focus_nmh::register::{self, Action, Machine, Platform};
use ipc_transport::Endpoint;

const USAGE: &str = "usage:
  focus-nmh --register --extension-id <id> [--dry-run]
  focus-nmh --unregister [--dry-run]
(the browser starts focus-nmh itself once it's registered)";

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(install) = install_folder() else {
        eprintln!("focus-nmh: couldn't find its own folder");
        return ExitCode::FAILURE;
    };
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let (home, xdg_config_home) = (var("HOME"), var("XDG_CONFIG_HOME"));
    let machine = |install| Machine {
        platform: Platform::current(),
        install,
        home: home.as_deref(),
        xdg_config_home: xdg_config_home.as_deref(),
        exists: &|path: &Path| path.exists(),
    };
    match args.first().map(String::as_str) {
        Some(origin) if origin.starts_with("chrome-extension://") => relay(&install).await,
        Some("--register") => {
            let id = args
                .windows(2)
                .find(|pair| pair[0] == "--extension-id")
                .map(|pair| pair[1].as_str());
            let Some(id) = id else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            apply(register::register_plan(&machine(&install), id), dry_run)
        }
        Some("--unregister") => apply(register::unregister_plan(&machine(&install)), dry_run),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Relays between the browser on stdin/stdout and the daemon of this install.
async fn relay(install: &Path) -> ExitCode {
    let endpoint = Endpoint::for_install(install);
    let (mut stdin, mut stdout) = (tokio::io::stdin(), tokio::io::stdout());
    match focus_nmh::host::run(&mut stdin, &mut stdout, &endpoint).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("focus-nmh: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Prints the plan, then carries it out unless this is a dry run.
fn apply(plan: Result<Vec<Action>, register::RegisterError>, dry_run: bool) -> ExitCode {
    let actions = match plan {
        Ok(actions) => actions,
        Err(error) => {
            eprintln!("focus-nmh: {error}");
            return ExitCode::FAILURE;
        }
    };
    for action in &actions {
        println!("{}{action}", if dry_run { "would " } else { "" });
    }
    if dry_run {
        return ExitCode::SUCCESS;
    }
    match register::execute(&actions) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("focus-nmh: {error}");
            ExitCode::FAILURE
        }
    }
}

fn install_folder() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}
