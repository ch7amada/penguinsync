//! Client of `org.penguinsync.Daemon1` (docs/design.md §4.3).
//!
//! Both the TUI and the non-interactive CLI verbs are thin frontends over
//! this one module. It talks to the session bus only — no protocol logic
//! lives here, all of that runs in the daemon.

use std::collections::HashMap;

use zbus::names::OwnedInterfaceName;
use zbus::zvariant::OwnedObjectPath;

/// Must match `penguinsync-daemon`'s `dbus::BUS_NAME`/`dbus::ROOT_PATH`
/// exactly — there's no shared crate between the two binaries for two
/// string constants.
pub const BUS_NAME: &str = "org.penguinsync.Daemon1";
pub const ROOT_PATH: &str = "/org/penguinsync/Daemon";
const DEVICE_INTERFACE: &str = "org.penguinsync.Device1";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("D-Bus error: {0}")]
    Zbus(#[from] zbus::Error),
    #[error("D-Bus error: {0}")]
    Fdo(#[from] zbus::fdo::Error),
    #[error("invalid bus name: {0}")]
    BusName(#[from] zbus::names::Error),
    #[error(
        "penguinsyncd isn't running, or hasn't claimed {BUS_NAME} yet — is the systemd user service up?"
    )]
    DaemonNotRunning,
}

#[zbus::proxy(
    interface = "org.penguinsync.Daemon1",
    default_service = "org.penguinsync.Daemon1",
    default_path = "/org/penguinsync/Daemon"
)]
pub trait Daemon1 {
    /// Returns `(qr_uri, fingerprint)`.
    async fn start_pairing(&self) -> zbus::Result<(String, String)>;
    async fn confirm_pairing(&self, device_id: &str, accept: bool) -> zbus::Result<()>;
    async fn unpair(&self, device_id: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn pairing_requested(
        &self,
        device_id: String,
        fingerprint: String,
        name: String,
    ) -> zbus::Result<()>;

    /// A file transfer is under way, either direction (docs/design.md §6.2,
    /// §9's "TUI progress"). `direction` is `"send"` or `"receive"`.
    #[zbus(signal)]
    fn transfer_progress(
        &self,
        device_id: String,
        transfer_id: u64,
        name: String,
        bytes: u64,
        total: u64,
        direction: String,
    ) -> zbus::Result<()>;

    /// A file transfer ended. `error` is empty when `ok` is true.
    #[zbus(signal)]
    fn transfer_finished(
        &self,
        device_id: String,
        transfer_id: u64,
        name: String,
        ok: bool,
        error: String,
        direction: String,
    ) -> zbus::Result<()>;
}

#[zbus::proxy(interface = "org.penguinsync.Device1")]
pub trait Device1 {
    #[zbus(property)]
    fn name(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn device_id(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn connected(&self) -> zbus::Result<bool>;

    /// `uris` are `file://…`. Auto-accepted on the far end, no prompt
    /// (docs/protocol.md §6.4) — progress and outcome arrive as
    /// `Daemon1::transfer_progress`/`transfer_finished` signals, not as this
    /// call's return value.
    async fn send_files(&self, uris: Vec<String>) -> zbus::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub device_id: String,
    pub name: String,
    pub connected: bool,
}

pub async fn connect() -> Result<zbus::Connection, ClientError> {
    let connection = zbus::Connection::session().await?;
    // Fail fast with a clear message rather than every subsequent call
    // timing out against a name nobody owns (docs/design.md §8's
    // connectivity-self-check philosophy, applied to the client side too).
    let dbus_proxy = zbus::fdo::DBusProxy::new(&connection).await?;
    if !dbus_proxy.name_has_owner(BUS_NAME.try_into()?).await? {
        return Err(ClientError::DaemonNotRunning);
    }
    Ok(connection)
}

pub async fn daemon_proxy(connection: &zbus::Connection) -> Result<Daemon1Proxy<'_>, ClientError> {
    Ok(Daemon1Proxy::new(connection).await?)
}

pub fn device_path(device_id_hex: &str) -> String {
    format!("{ROOT_PATH}/devices/{device_id_hex}")
}

/// Sends `uris` (`file://…`) to the paired device identified by
/// `device_id_hex` (docs/design.md §6.2). Fire-and-forget over D-Bus, same
/// as the daemon's own `Device1::send_files` — progress and outcome are the
/// `transfer_progress`/`transfer_finished` signals, not this call's result.
pub async fn send_files(
    connection: &zbus::Connection,
    device_id_hex: &str,
    uris: Vec<String>,
) -> Result<(), ClientError> {
    let proxy = Device1Proxy::builder(connection)
        .destination(BUS_NAME)?
        .path(device_path(device_id_hex))?
        .build()
        .await?;
    proxy.send_files(uris).await?;
    Ok(())
}

/// Every paired device, freshly read via `GetManagedObjects()`. No caching —
/// the TUI just polls this on a timer, which is simple and plenty fast for a
/// handful of devices (docs/design.md §4.3's ObjectManager shape is what
/// makes this a few lines).
pub async fn list_devices(connection: &zbus::Connection) -> Result<Vec<DeviceInfo>, ClientError> {
    let om = zbus::fdo::ObjectManagerProxy::builder(connection)
        .destination(BUS_NAME)?
        .path(ROOT_PATH)?
        .build()
        .await?;
    let objects: HashMap<
        OwnedObjectPath,
        HashMap<OwnedInterfaceName, HashMap<String, zbus::zvariant::OwnedValue>>,
    > = om.get_managed_objects().await?;

    let mut devices = Vec::new();
    for (path, ifaces) in objects {
        if !ifaces.keys().any(|k| k.as_str() == DEVICE_INTERFACE) {
            continue;
        }
        let proxy = Device1Proxy::builder(connection)
            .destination(BUS_NAME)?
            .path(path)?
            .build()
            .await?;
        devices.push(DeviceInfo {
            device_id: proxy.device_id().await.unwrap_or_default(),
            name: proxy.name().await.unwrap_or_default(),
            connected: proxy.connected().await.unwrap_or(false),
        });
    }
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(devices)
}
