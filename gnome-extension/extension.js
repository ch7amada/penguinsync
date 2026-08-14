/*
 * PenguinSync Clipboard — exposes the clipboard selection over D-Bus.
 *
 * Keep this minimal. All logic stays in Rust; this file does selection
 * access and byte transfer, and nothing else (see ../gnome-extension/
 * README.md). It runs inside the gnome-shell process, where a bug hangs the
 * user's desktop — every Meta.Selection call is wrapped so a failure here
 * is a logged error, never an uncaught exception.
 *
 * Modeled on GSConnect's src/shell/clipboard.js (docs/design.md §4.4):
 *   - Watch: global.display.get_selection() -> 'owner-changed', filtered to
 *     Meta.SelectionType.SELECTION_CLIPBOARD
 *   - Read:  selection.transfer_async(...)
 *   - Write: Meta.SelectionSourceMemory.new(mimetype, bytes) ->
 *            selection.set_owner(...)
 *
 * D-Bus interface (consumed by penguinsyncd via zbus — see
 * crates/daemon/src/clipboard.rs, which must match this exactly):
 *   Bus name:    org.penguinsync.Clipboard
 *   Object path: /org/penguinsync/Clipboard
 *   Methods:     GetMimetypes() -> as, GetText() -> s, SetText(s),
 *                GetValue(s) -> ay, SetValue(ay, s)
 *   Signal:      OwnerChange()
 */

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const BUS_NAME = 'org.penguinsync.Clipboard';
const OBJECT_PATH = '/org/penguinsync/Clipboard';
const MIME_TEXT_PLAIN = 'text/plain';

const IFACE_XML = `
<node>
  <interface name="org.penguinsync.Clipboard">
    <method name="GetMimetypes">
      <arg type="as" direction="out" name="mimetypes"/>
    </method>
    <method name="GetText">
      <arg type="s" direction="out" name="text"/>
    </method>
    <method name="SetText">
      <arg type="s" direction="in" name="text"/>
    </method>
    <method name="GetValue">
      <arg type="s" direction="in" name="mimetype"/>
      <arg type="ay" direction="out" name="value"/>
    </method>
    <method name="SetValue">
      <arg type="ay" direction="in" name="value"/>
      <arg type="s" direction="in" name="mimetype"/>
    </method>
    <signal name="OwnerChange"/>
  </interface>
</node>`;

export default class PenguinSyncClipboardExtension extends Extension {
    enable() {
        this._selection = global.display.get_selection();

        this._dbusImpl = Gio.DBusExportedObject.wrapJSObject(IFACE_XML, this);
        this._dbusImpl.export(Gio.DBus.session, OBJECT_PATH);

        this._ownerId = Gio.bus_own_name(
            Gio.BusType.SESSION,
            BUS_NAME,
            Gio.BusNameOwnerFlags.NONE,
            null,
            null,
            null,
        );

        this._ownerChangedId = this._selection.connect('owner-changed', (selection, selectionType) => {
            if (selectionType !== Meta.SelectionType.SELECTION_CLIPBOARD)
                return;

            try {
                this._dbusImpl.emit_signal('OwnerChange', null);
            } catch (e) {
                console.error(`penguinsync-clipboard: failed to emit OwnerChange: ${e}`);
            }
        });
    }

    disable() {
        if (this._ownerChangedId) {
            this._selection.disconnect(this._ownerChangedId);
            this._ownerChangedId = null;
        }
        this._selection = null;

        if (this._ownerId) {
            Gio.bus_unown_name(this._ownerId);
            this._ownerId = null;
        }

        if (this._dbusImpl) {
            this._dbusImpl.unexport();
            this._dbusImpl = null;
        }
    }

    // --- D-Bus method implementations -------------------------------------
    //
    // Gio.DBusExportedObject dispatches `Foo(params)` to a `FooAsync(params,
    // invocation)` method when one exists, which is what every method here
    // needs: reading the selection is itself async
    // (selection.transfer_async), and even the synchronous ones return
    // through `invocation` so a thrown error becomes a proper D-Bus error
    // reply instead of an uncaught exception inside gnome-shell.

    GetMimetypesAsync(params, invocation) {
        try {
            const mimetypes = this._selection.get_mimetypes(Meta.SelectionType.SELECTION_CLIPBOARD) ?? [];
            invocation.return_value(new GLib.Variant('(as)', [mimetypes]));
        } catch (e) {
            invocation.return_error_literal(Gio.DBusError, Gio.DBusError.FAILED, `${e}`);
        }
    }

    GetTextAsync(params, invocation) {
        this._readSelection(MIME_TEXT_PLAIN)
            .then(bytes => {
                const text = new TextDecoder().decode(bytes.toArray());
                invocation.return_value(new GLib.Variant('(s)', [text]));
            })
            .catch(e => invocation.return_error_literal(Gio.DBusError, Gio.DBusError.FAILED, `${e}`));
    }

    SetTextAsync(params, invocation) {
        const [text] = params;
        try {
            this._writeSelection(MIME_TEXT_PLAIN, new TextEncoder().encode(text));
            invocation.return_value(null);
        } catch (e) {
            invocation.return_error_literal(Gio.DBusError, Gio.DBusError.FAILED, `${e}`);
        }
    }

    GetValueAsync(params, invocation) {
        const [mimetype] = params;
        this._readSelection(mimetype)
            .then(bytes => {
                invocation.return_value(new GLib.Variant('(ay)', [bytes.toArray()]));
            })
            .catch(e => invocation.return_error_literal(Gio.DBusError, Gio.DBusError.FAILED, `${e}`));
    }

    SetValueAsync(params, invocation) {
        const [value, mimetype] = params;
        try {
            this._writeSelection(mimetype, Uint8Array.from(value));
            invocation.return_value(null);
        } catch (e) {
            invocation.return_error_literal(Gio.DBusError, Gio.DBusError.FAILED, `${e}`);
        }
    }

    // --- Selection access ---------------------------------------------------

    /**
     * Resolves to a GLib.Bytes with the clipboard's current content.
     *
     * `Meta.Selection.transfer_async` writes straight into the output
     * stream passed to it (it does not hand back a stream to read from —
     * an easy assumption to get wrong, since most GIO async-read patterns
     * work the other way around) and `transfer_finish` returns a plain
     * success boolean.
     */
    _readSelection(mimetype) {
        return new Promise((resolve, reject) => {
            const output = Gio.MemoryOutputStream.new_resizable();
            this._selection.transfer_async(
                Meta.SelectionType.SELECTION_CLIPBOARD,
                mimetype,
                -1,
                output,
                null,
                (selection, result) => {
                    try {
                        selection.transfer_finish(result);
                        resolve(output.steal_as_bytes());
                    } catch (e) {
                        reject(e);
                    }
                },
            );
        });
    }

    _writeSelection(mimetype, bytes) {
        const source = Meta.SelectionSourceMemory.new(mimetype, new GLib.Bytes(bytes));
        this._selection.set_owner(Meta.SelectionType.SELECTION_CLIPBOARD, source);
    }
}
