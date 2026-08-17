//! `penguinsync` — terminal UI and command-line client.
//!
//! A thin client over D-Bus; all protocol logic lives in the daemon.
//!
//! TUI: status, device list, pairing QR display, confirm/revoke.
//! CLI: non-interactive verbs (`penguinsync send file.pdf`, `penguinsync debug`).
//!
//! Both are frontends over one shared D-Bus client module. Room to grow into a
//! full dashboard with transfer progress at M4 — but not before the protocol
//! works (docs/design.md §4.3).

mod dbus_client;
mod qr;
mod tui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "penguinsync",
    version,
    about = "PenguinSync — pair, sync, done."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start pairing: prints a QR code and waits for confirmation. Useful
    /// over SSH or when the TUI isn't handy.
    Pair,
    /// Revoke a paired device by its device id (or a unique hex prefix).
    Unpair { device_id: String },
    /// Send one or more files to a paired, connected device
    /// (docs/design.md §6.2). `device` matches a device id prefix or an
    /// exact name.
    Send {
        device: String,
        files: Vec<std::path::PathBuf>,
    },
    /// Dump daemon state as seen over D-Bus — devices, connection status.
    /// The instrument of last resort when a phone won't reconnect
    /// (docs/design.md §8).
    Debug,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let result = match cli.command {
        None => run_tui().await,
        Some(Command::Pair) => run_pair().await,
        Some(Command::Unpair { device_id }) => run_unpair(&device_id).await,
        Some(Command::Send { device, files }) => run_send(&device, &files).await,
        Some(Command::Debug) => run_debug().await,
    };

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run_tui() -> Result<(), dbus_client::ClientError> {
    let connection = dbus_client::connect().await?;
    tui::run(connection).await
}

/// Terminal width in columns, or `None` when stdout isn't a terminal (piped
/// or redirected) — in which case there is nothing to wrap against and no
/// reason to withhold the code.
fn terminal_width() -> Option<u16> {
    crossterm::terminal::size().ok().map(|(w, _)| w)
}

async fn run_pair() -> Result<(), dbus_client::ClientError> {
    let connection = dbus_client::connect().await?;
    let daemon = dbus_client::daemon_proxy(&connection).await?;

    let (qr_uri, fingerprint) = daemon.start_pairing().await?;
    match qr::render(&qr_uri) {
        // A QR wider than the terminal is wrapped by the terminal itself,
        // which shifts every line after the first and destroys the code —
        // and it still *looks* like a QR, so the failure shows up as a phone
        // that just never scans. Say so instead, and print the URI for the
        // phone's manual-entry field.
        Ok(qr) if terminal_width().is_some_and(|w| w < qr.width) => {
            let width = terminal_width().unwrap_or(0);
            println!(
                "This terminal is {width} columns wide; the pairing QR needs {}. \
                 A wrapped QR cannot be scanned, so it isn't shown.\n\
                 Widen the window and run `penguinsync pair` again, or enter this \
                 on the phone by hand:\n{qr_uri}",
                qr.width,
            );
        }
        Ok(qr) => println!("{}", qr.text),
        Err(e) => println!("(could not render QR: {e})\n{qr_uri}"),
    }
    println!("Fingerprint: {fingerprint}");
    println!("Scan this on your phone, then confirm the fingerprint matches there.");
    println!("Waiting for the phone to connect (up to 60s)...");

    let mut requests = daemon.receive_pairing_requested().await?;
    let signal = {
        use futures_util::StreamExt;
        tokio::time::timeout(std::time::Duration::from_secs(65), requests.next()).await
    };
    let Ok(Some(signal)) = signal else {
        println!("No device connected in time.");
        return Ok(());
    };
    let Ok(args) = signal.args() else {
        return Ok(());
    };

    println!("\nDevice: {}", args.name());
    println!("Fingerprint: {}", args.fingerprint());
    print!("Does this match the phone's screen? [y/N] ");
    use std::io::Write;
    std::io::stdout().flush().ok();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).ok();
    let accept = matches!(answer.trim().to_lowercase().as_str(), "y" | "yes");

    daemon.confirm_pairing(args.device_id(), accept).await?;
    println!("{}", if accept { "Paired." } else { "Rejected." });
    Ok(())
}

async fn run_unpair(device_id: &str) -> Result<(), dbus_client::ClientError> {
    let connection = dbus_client::connect().await?;
    let daemon = dbus_client::daemon_proxy(&connection).await?;
    let devices = dbus_client::list_devices(&connection).await?;

    let matches: Vec<_> = devices
        .iter()
        .filter(|d| d.device_id.starts_with(device_id))
        .collect();
    match matches.as_slice() {
        [] => println!("No paired device matches '{device_id}'."),
        [device] => {
            daemon.unpair(&device.device_id).await?;
            println!("Unpaired {}.", device.name);
        }
        _ => println!("'{device_id}' matches more than one device; use a longer prefix."),
    }
    Ok(())
}

/// Matches `device` against a device id prefix or an exact (case-insensitive)
/// name — same lookup shape as [`run_unpair`], plus a name match since a
/// hex prefix is nobody's idea of a friendly command line.
async fn run_send(
    device: &str,
    files: &[std::path::PathBuf],
) -> Result<(), dbus_client::ClientError> {
    let connection = dbus_client::connect().await?;
    let devices = dbus_client::list_devices(&connection).await?;

    let matches: Vec<_> = devices
        .iter()
        .filter(|d| d.device_id.starts_with(device) || d.name.eq_ignore_ascii_case(device))
        .collect();
    let target = match matches.as_slice() {
        [] => {
            println!("No paired device matches '{device}'.");
            return Ok(());
        }
        [d] => *d,
        _ => {
            println!("'{device}' matches more than one device; use a longer id prefix.");
            return Ok(());
        }
    };
    if !target.connected {
        println!("{} is not currently connected.", target.name);
        return Ok(());
    }

    let mut uris = Vec::with_capacity(files.len());
    for file in files {
        let canonical = match std::fs::canonicalize(file) {
            Ok(p) => p,
            Err(e) => {
                println!("skipping {}: {e}", file.display());
                continue;
            }
        };
        match url::Url::from_file_path(&canonical) {
            Ok(url) => uris.push(url.to_string()),
            Err(()) => println!("skipping {}: not a plain file path", file.display()),
        }
    }
    if uris.is_empty() {
        println!("nothing to send.");
        return Ok(());
    }

    let count = uris.len();
    dbus_client::send_files(&connection, &target.device_id, uris).await?;
    println!("Sending {count} file(s) to {}...", target.name);
    Ok(())
}

async fn run_debug() -> Result<(), dbus_client::ClientError> {
    let connection = dbus_client::connect().await?;
    let devices = dbus_client::list_devices(&connection).await?;
    println!("bus name: {}", dbus_client::BUS_NAME);
    println!("devices ({}):", devices.len());
    for d in devices {
        println!(
            "  {} {}  {}",
            if d.connected { "\u{25cf}" } else { "\u{25cb}" },
            d.device_id,
            d.name
        );
    }
    Ok(())
}
