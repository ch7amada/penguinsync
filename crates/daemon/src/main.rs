//! `penguinsyncd` — the PenguinSync background daemon.
//!
//! Runs as a systemd **user** service (clipboard is per-session), started at
//! login and kept running. Not D-Bus-activated and not idle-exiting: a clipboard
//! daemon that idle-exits is not a clipboard daemon.
//!
//! Serves `org.penguinsync.Daemon1` at `/org/penguinsync/Daemon`, implementing
//! `org.freedesktop.DBus.ObjectManager` with one object per paired device. The
//! TUI, the Nautilus extension and any future client all consume that.
//!
//! Must start and run successfully **without** the GNOME Shell extension
//! present — file transfer and notification mirroring do not need it. Clipboard
//! is then reported as unavailable (docs/design.md §4.4).
//!
//! # M0
//!
//! No clipboard, no files, no notifications yet — just identity, pairing,
//! QUIC, and the D-Bus surface those need (docs/design.md §9).

mod clipboard;
mod config;
mod dbus;
mod net_addrs;
mod notify;
mod orchestrator;
mod shared;
mod state;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::Mutex;

use penguinsync_net::{Endpoint, Identity, TrustStore};

use crate::config::Config;
use crate::dbus::Daemon1;
use crate::shared::Shared;
use crate::state::PersistedState;

fn xdg_config_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME must be set"))
                .join(".config")
        })
        .join("penguinsync")
}

fn xdg_data_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME must be set"))
                .join(".local/share")
        })
        .join("penguinsync")
}

/// The user's actual Downloads directory — not an XDG base directory at all,
/// but the one `xdg-user-dirs` maintains in `~/.config/user-dirs.dirs`
/// (docs/design.md §4.3: received files land in
/// `$XDG_DOWNLOAD_DIR/PenguinSync/`). Falls back to `~/Downloads` if the
/// env var isn't set and the user-dirs file is missing or unparseable —
/// same "never refuse to start over this" posture as [`Config::load`].
fn xdg_download_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("XDG_DOWNLOAD_DIR") {
        return std::path::PathBuf::from(dir);
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME must be set"));
    let user_dirs = home.join(".config/user-dirs.dirs");
    if let Ok(text) = std::fs::read_to_string(&user_dirs) {
        for line in text.lines() {
            if let Some(value) = line.trim().strip_prefix("XDG_DOWNLOAD_DIR=") {
                let value = value
                    .trim_matches('"')
                    .replace("$HOME", &home.to_string_lossy());
                return std::path::PathBuf::from(value);
            }
        }
    }
    home.join("Downloads")
}

fn init_tracing() {
    use tracing_subscriber::prelude::*;
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new("penguinsyncd=info,penguinsync_net=info")
    });

    let registry = tracing_subscriber::registry().with(env_filter);
    match tracing_journald::layer() {
        Ok(journald) => registry.with(journald).init(),
        // Not running under systemd (e.g. a terminal during development) —
        // fall back to stderr rather than refusing to start.
        Err(_) => registry.with(tracing_subscriber::fmt::layer()).init(),
    }
}

#[tokio::main]
async fn main() {
    init_tracing();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "penguinsyncd starting");

    let config_dir = xdg_config_dir();
    let data_dir = xdg_data_dir();
    let config = Config::load(&config_dir.join("config.toml"));

    let identity = match Identity::load_or_generate(&data_dir) {
        Ok(identity) => identity,
        Err(e) => {
            tracing::error!(error = %e, "failed to load or generate device identity");
            std::process::exit(1);
        }
    };
    tracing::info!(
        device_id = %penguinsync_protocol::message::short_fingerprint(&identity.device_id),
        "device identity ready"
    );

    let state_path = data_dir.join("devices.json");
    let state = PersistedState::load(&state_path);
    let trust = Arc::new(TrustStore::new(state.device_ids()));

    let listen_addr: SocketAddr = SocketAddr::new([0, 0, 0, 0].into(), config.listen_port);
    let endpoint = match Endpoint::listening(&identity, trust.clone(), listen_addr) {
        Ok(e) => Arc::new(e),
        Err(e) => {
            tracing::error!(error = %e, addr = %listen_addr, "failed to bind QUIC listener");
            std::process::exit(1);
        }
    };
    let bound_addr = endpoint.local_addr().unwrap_or(listen_addr);
    tracing::info!(addr = %bound_addr, "listening for QUIC connections");

    let device_name = config.device_name();
    let shared = Arc::new(Shared {
        identity,
        name: device_name,
        listen_addr: bound_addr,
        trust,
        state_path,
        state: Mutex::new(state.clone()),
        current_token: Mutex::new(None),
        pending_confirmations: Mutex::new(HashMap::new()),
        remote_to_device: Mutex::new(HashMap::new()),
        remote_handles: Mutex::new(HashMap::new()),
        connected_devices: Mutex::new(HashMap::new()),
        clipboard: crate::shared::ClipboardState::default(),
        transfers: Mutex::new(HashMap::new()),
    });

    let connection = match zbus::connection::Builder::session()
        .and_then(|b| b.name(dbus::BUS_NAME))
        .and_then(|b| b.serve_at(dbus::ROOT_PATH, zbus::fdo::ObjectManager))
        .and_then(|b| {
            b.serve_at(
                dbus::ROOT_PATH,
                Daemon1 {
                    shared: shared.clone(),
                },
            )
        }) {
        Ok(builder) => match builder.build().await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "failed to connect to the D-Bus session bus");
                std::process::exit(1);
            }
        },
        Err(e) => {
            tracing::error!(error = %e, "failed to configure D-Bus server");
            std::process::exit(1);
        }
    };

    // Restore Device1 objects for every already-paired device, so the TUI's
    // first GetManagedObjects() sees them even before any of them reconnect.
    for device in &state.devices {
        if let Some(id) = penguinsync_protocol::message::from_hex(&device.device_id) {
            let _ = connection
                .object_server()
                .at(
                    dbus::device_path(&id),
                    dbus::Device1 {
                        name: device.name.clone(),
                        device_id: device.device_id.clone(),
                        connected: false,
                        shared: shared.clone(),
                    },
                )
                .await;
        }
    }

    let download_dir = xdg_download_dir().join("PenguinSync");
    tracing::info!(dir = %download_dir.display(), "received files land here");
    let transfer_sink: Arc<dyn penguinsync_net::TransferSink> =
        Arc::new(penguinsync_net::FsSink::new(download_dir));

    let (listener_tx, listener_rx) = tokio::sync::mpsc::unbounded_channel();
    let local_identity = penguinsync_protocol::LocalIdentity {
        device_id: shared.identity.device_id,
        name: shared.name.clone(),
        capabilities: vec![],
    };
    tokio::spawn(penguinsync_net::listener::run(
        endpoint,
        local_identity,
        std::time::Duration::from_secs(20),
        transfer_sink,
        listener_tx,
    ));

    let orchestrator = tokio::spawn(orchestrator::run(
        listener_rx,
        shared.clone(),
        connection.clone(),
    ));

    // Optional: the daemon must run fine without it (docs/design.md §4.4).
    match clipboard::GnomeClipboardBackend::probe(&connection).await {
        Some(backend) => {
            tracing::info!("GNOME clipboard extension found; clipboard sync active");
            let backend: Arc<dyn penguinsync_net::ClipboardBackend> = Arc::new(backend);
            // Stashed on `Shared` too (not just moved into the broadcast
            // loop below) so the orchestrator's M2 receive path can write
            // an incoming clipboard update to the same backend.
            *shared.clipboard.backend.lock().await = Some(backend.clone());
            tokio::spawn(clipboard::watch_and_broadcast(backend, shared.clone()));
        }
        None => {
            tracing::info!("GNOME clipboard extension not found; clipboard sync unavailable");
        }
    }

    tracing::info!(name = %dbus::BUS_NAME, "D-Bus service ready");

    wait_for_shutdown_signal().await;
    tracing::info!("shutting down");
    orchestrator.abort();
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
