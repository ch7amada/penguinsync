"""PenguinSync Nautilus extension — "Send with PenguinSync" context menu.

Modeled directly on GSConnect's nautilus-gsconnect.py (docs/design.md §4.5,
see ../nautilus/README.md for the route rationale). Two rules from that
file are non-negotiable and both are load-bearing here:

  1. Never call gi.require_version("Nautilus", ...) — just import it.
  2. get_file_items() takes *args and reads the file list as args[-1].

Nautilus 49 bumped its introspection namespace 4.0 -> 4.1 and broke every
extension that hardcoded the version or the old two-argument (window,
files) signature. GSConnect survived because of exactly these two lines,
nothing else does.

Devices come from penguinsyncd over D-Bus via Gio.DBusProxy — GSConnect's
own route, no pydbus, no extra Python dependency beyond PyGObject (which
nautilus-python already pulls in). The contract (frozen, see the README):

  Bus:      session bus, name org.penguinsync.Daemon1
  Root:     /org/penguinsync/Daemon, org.freedesktop.DBus.ObjectManager
  Devices:  /org/penguinsync/Daemon/devices/<hex-device-id>,
            interface org.penguinsync.Device1, properties
            Name (s), DeviceId (s), Connected (b), method SendFiles(as)

This file runs inside the Nautilus/Files process — a bug here takes the
file manager down with it. Every D-Bus call and every callback body is
wrapped so failure is a logged error, never an uncaught exception.
"""

import logging

from gi.repository import Gio, GLib, GObject
from gi.repository import Nautilus

logging.basicConfig()
log = logging.getLogger("penguinsync-nautilus")

BUS_NAME = "org.penguinsync.Daemon1"
OBJECT_MANAGER_PATH = "/org/penguinsync/Daemon"
DEVICE_IFACE = "org.penguinsync.Device1"
OBJECT_MANAGER_IFACE = "org.freedesktop.DBus.ObjectManager"


class DeviceRegistry(GObject.GObject):
    """Tracks paired devices by watching the daemon's ObjectManager.

    One instance, shared by every PenguinSyncMenuProvider (Nautilus may
    instantiate the provider more than once, e.g. one per window). Watches
    the bus name itself, since the daemon is not guaranteed to be running.
    """

    def __init__(self):
        super().__init__()
        self._devices = {}  # object path -> {"Name": str, "DeviceId": str, "Connected": bool}
        self._object_manager = None
        self._watch_id = Gio.bus_watch_name(
            Gio.BusType.SESSION,
            BUS_NAME,
            Gio.BusNameWatcherFlags.NONE,
            self._on_name_appeared,
            self._on_name_vanished,
        )

    def connected_devices(self):
        """Return {object_path: {Name, DeviceId, Connected}} for connected devices."""
        try:
            return {
                path: props
                for path, props in self._devices.items()
                if props.get("Connected")
            }
        except Exception:
            log.exception("penguinsync: failed to filter connected devices")
            return {}

    # -- bus name watch ----------------------------------------------

    def _on_name_appeared(self, connection, name, owner):
        try:
            self._object_manager = Gio.DBusProxy.new_sync(
                connection,
                Gio.DBusProxyFlags.NONE,
                None,
                BUS_NAME,
                OBJECT_MANAGER_PATH,
                OBJECT_MANAGER_IFACE,
                None,
            )
            self._object_manager.connect("g-signal", self._on_signal)
            result = self._object_manager.call_sync(
                "GetManagedObjects",
                None,
                Gio.DBusCallFlags.NONE,
                -1,
                None,
            )
            managed = result.unpack()[0]
            self._devices = {}
            for path, ifaces in managed.items():
                if DEVICE_IFACE in ifaces:
                    self._devices[path] = dict(ifaces[DEVICE_IFACE])
            log.debug("penguinsync: daemon appeared, %d device(s) known", len(self._devices))
        except GLib.Error:
            log.exception("penguinsync: GetManagedObjects failed")
        except Exception:
            log.exception("penguinsync: unexpected error while querying daemon")

    def _on_name_vanished(self, connection, name):
        log.debug("penguinsync: daemon vanished from the bus")
        self._object_manager = None
        self._devices = {}

    def _on_signal(self, proxy, sender_name, signal_name, parameters):
        try:
            if signal_name == "InterfacesAdded":
                path, ifaces = parameters.unpack()
                if DEVICE_IFACE in ifaces:
                    self._devices[path] = dict(ifaces[DEVICE_IFACE])
            elif signal_name == "InterfacesRemoved":
                path, iface_names = parameters.unpack()
                if DEVICE_IFACE in iface_names:
                    self._devices.pop(path, None)
        except Exception:
            log.exception("penguinsync: failed to handle %s", signal_name)


# One registry, shared across provider instances / windows.
_registry = DeviceRegistry()


def _send_files(object_path, uris):
    """Call SendFiles(as) on the device at object_path. Never raises."""
    try:
        connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        connection.call_sync(
            BUS_NAME,
            object_path,
            DEVICE_IFACE,
            "SendFiles",
            GLib.Variant("(as)", (uris,)),
            None,
            Gio.DBusCallFlags.NONE,
            -1,
            None,
        )
    except GLib.Error:
        log.exception("penguinsync: SendFiles failed for %s", object_path)
    except Exception:
        log.exception("penguinsync: unexpected error sending files to %s", object_path)


class PenguinSyncMenuProvider(GObject.GObject, Nautilus.MenuProvider):
    """Adds "Send with PenguinSync" to the file context menu."""

    def __init__(self):
        super().__init__()

    def get_file_items(self, *args):
        # Rule 2: never trust the argument count/order across Nautilus
        # versions — the file list is always the last positional arg.
        try:
            files = args[-1]
        except Exception:
            log.exception("penguinsync: could not read file list from args")
            return []

        try:
            return self._build_menu(files)
        except Exception:
            log.exception("penguinsync: get_file_items failed")
            return []

    def _build_menu(self, files):
        if not files:
            return []

        # v1 has no directory transfer (docs/design.md §6.2) — only offer
        # the menu for a selection of one or more regular files.
        for f in files:
            try:
                if f.is_directory():
                    return []
            except Exception:
                log.exception("penguinsync: is_directory() check failed")
                return []

        try:
            uris = [f.get_uri() for f in files]
        except Exception:
            log.exception("penguinsync: get_uri() failed")
            return []

        devices = _registry.connected_devices()

        if not devices:
            item = Nautilus.MenuItem(
                name="PenguinSync::no-devices",
                label="No devices connected",
                tip="No PenguinSync devices are currently connected",
            )
            item.set_property("sensitive", False)
            return [item]

        if len(devices) == 1:
            (object_path, props), = devices.items()
            name = props.get("Name", "device")
            item = Nautilus.MenuItem(
                name="PenguinSync::send-single",
                label="Send to %s" % name,
                tip="Send %d file(s) via PenguinSync" % len(uris),
            )
            item.connect("activate", self._on_activate, object_path, uris)
            return [item]

        top = Nautilus.MenuItem(
            name="PenguinSync::submenu",
            label="Send with PenguinSync ▸",
            tip="Send %d file(s) via PenguinSync" % len(uris),
        )
        submenu = Nautilus.Menu()
        top.set_submenu(submenu)

        for object_path, props in devices.items():
            name = props.get("Name", "device")
            device_item = Nautilus.MenuItem(
                name="PenguinSync::send-%s" % object_path,
                label=name,
                tip="Send %d file(s) to %s" % (len(uris), name),
            )
            device_item.connect("activate", self._on_activate, object_path, uris)
            submenu.append_item(device_item)

        return [top]

    def _on_activate(self, menu_item, object_path, uris):
        try:
            _send_files(object_path, uris)
        except Exception:
            log.exception("penguinsync: activate handler failed")
