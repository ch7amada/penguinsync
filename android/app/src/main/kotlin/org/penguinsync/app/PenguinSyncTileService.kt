package org.penguinsync.app

import android.app.PendingIntent
import android.content.Intent
import android.graphics.drawable.Icon
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

/// QS tile for M2's manual clipboard-send path (docs/design.md §6.1's
/// Baseline row, §9). Tapping it can't read the clipboard directly — a tile
/// has no window focus of its own (§3.1) — so it launches
/// [ClipboardReadActivity], which waits for focus itself. There's no
/// meaningful on/off state to track here (unlike, say, a mute toggle); the
/// tile is a one-shot action, always shown the same way.
class PenguinSyncTileService : TileService() {
    override fun onStartListening() {
        super.onStartListening()
        qsTile?.apply {
            label = "Send clipboard"
            icon = Icon.createWithResource(this@PenguinSyncTileService, R.drawable.ic_penguinsync_clipboard)
            state = Tile.STATE_INACTIVE
            updateTile()
        }
    }

    override fun onClick() {
        super.onClick()
        val intent =
            Intent(this, ClipboardReadActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            }
        // The non-deprecated, PendingIntent-based overload only exists from
        // API 34; below that, the Intent overload is all there is.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            val pendingIntent =
                PendingIntent.getActivity(
                    this,
                    0,
                    intent,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                )
            startActivityAndCollapse(pendingIntent)
        } else {
            // Below API 34 there is no PendingIntent overload to call instead —
            // this branch only runs below the SDK level lint is warning about.
            @Suppress("DEPRECATION", "StartActivityAndCollapseDeprecated")
            startActivityAndCollapse(intent)
        }
    }
}
