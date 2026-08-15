package org.penguinsync.app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalContext

/// The app's theme.
///
/// [MaterialExpressiveTheme] rather than plain `MaterialTheme`: it swaps in
/// the expressive motion scheme (springier, shorter, more overshoot) and the
/// expressive shape scale, which is what the "modern Material" look actually
/// consists of — the colours alone don't get you there.
///
/// [dynamicColor] is a user preference, not an automatic capability check.
/// Every device this app runs on supports Material You (minSdk 31), so
/// "is it available" is never the question — "does this user want their
/// wallpaper's colours or PenguinSync's" is. Default is the brand palette:
/// an app that has a deliberate identity should show it unless asked not to.
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun PenguinSyncTheme(
    dynamicColor: Boolean = false,
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val context = LocalContext.current
    val colorScheme =
        when {
            dynamicColor && darkTheme -> dynamicDarkColorScheme(context)
            dynamicColor -> dynamicLightColorScheme(context)
            darkTheme -> PenguinSyncDarkColors
            else -> PenguinSyncLightColors
        }

    CompositionLocalProvider(
        LocalStatusColors provides statusColorsFor(darkTheme, colorScheme),
    ) {
        MaterialExpressiveTheme(
            colorScheme = colorScheme,
            motionScheme = MotionScheme.expressive(),
            content = content,
        )
    }
}
