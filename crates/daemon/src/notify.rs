//! Desktop notification on file arrival, with **Open** and **Show in
//! Files** actions (docs/design.md §6.2).
//!
//! A minimal hand-rolled `org.freedesktop.Notifications` proxy — same style
//! as `crate::clipboard`'s `ClipboardProxy`, no new crate dependency for
//! something this small. "Open" runs `xdg-open`; "Show in Files" calls
//! `org.freedesktop.FileManager1.ShowItems`, which Nautilus implements, so
//! this never has to shell out to a specific file manager by name.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: std::collections::HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.FileManager1",
    default_service = "org.freedesktop.FileManager1",
    default_path = "/org/freedesktop/FileManager1"
)]
trait FileManager1 {
    fn show_items(&self, uris: &[&str], startup_id: &str) -> zbus::Result<()>;
}

/// Fires a notification for a successfully received file, and handles its
/// **Open**/**Show in Files** actions for as long as the notification stays
/// open. Runs to completion on its own — the caller (`crate::orchestrator`)
/// doesn't wait on this; a notification nobody ever clicks just times out.
pub async fn file_received(name: &str, path: &Path) {
    let Ok(connection) = zbus::Connection::session().await else {
        tracing::debug!("no session bus; skipping file-arrival notification");
        return;
    };
    let Ok(proxy) = NotificationsProxy::new(&connection).await else {
        return;
    };

    let id = match proxy
        .notify(
            "PenguinSync",
            0,
            "folder-download",
            "File received",
            name,
            &["open", "Open", "show", "Show in Files"],
            std::collections::HashMap::new(),
            -1,
        )
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::debug!(error = %e, "failed to post file-arrival notification");
            return;
        }
    };

    let path = path.to_path_buf();
    tokio::spawn(handle_actions(connection, id, path));
}

async fn handle_actions(connection: zbus::Connection, id: u32, path: PathBuf) {
    let Ok(proxy) = NotificationsProxy::new(&connection).await else {
        return;
    };
    let (Ok(mut actions), Ok(mut closed)) = (
        proxy.receive_action_invoked().await,
        proxy.receive_notification_closed().await,
    ) else {
        return;
    };

    loop {
        tokio::select! {
            Some(signal) = actions.next() => {
                let Ok(args) = signal.args() else { continue };
                if *args.id() != id {
                    continue;
                }
                match args.action_key().as_str() {
                    "open" => {
                        let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
                    }
                    "show" => show_in_files(&connection, &path).await,
                    _ => {}
                }
            }
            Some(signal) = closed.next() => {
                let Ok(args) = signal.args() else { continue };
                if *args.id() == id {
                    return;
                }
            }
            else => return,
        }
    }
}

async fn show_in_files(connection: &zbus::Connection, path: &Path) {
    let Ok(proxy) = FileManager1Proxy::new(connection).await else {
        return;
    };
    let uri = format!("file://{}", path.display());
    let _ = proxy.show_items(&[uri.as_str()], "").await;
}
