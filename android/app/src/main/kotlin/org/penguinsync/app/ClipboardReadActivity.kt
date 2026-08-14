package org.penguinsync.app

import android.app.Activity
import android.os.Bundle
import android.widget.Toast

/// Transparent trampoline for M2's manual clipboard-send path
/// (docs/design.md §6.1's Baseline row, §9). The QS tile and the
/// notification action can't read the clipboard themselves — Android
/// requires window focus to read it, and neither has any (docs/design.md
/// §3.1) — so both launch this instead. No layout, no visible frame: it
/// does nothing until [onWindowFocusChanged] reports focus, then reads,
/// sends, and finishes.
class ClipboardReadActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || isFinishing) return

        val app = application as PenguinSyncApp
        val message =
            when (val result = app.sendClipboardFromFocusedContext(this)) {
                is SendResult.Sent -> "Clipboard sent to Linux"
                is SendResult.NothingToSend -> "Clipboard is empty or marked sensitive"
                is SendResult.Failed -> "Couldn't send: ${result.reason}"
            }
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
        finish()
    }
}
