package org.penguinsync.app.ui.theme

import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

/// The brand palette, generated from a single seed rather than hand-picked.
///
/// Seed `#01579B` — the deep ocean blue already used for the launcher icon's
/// background (`res/values/colors.xml`), so the icon and the app agree without
/// anything being re-drawn. Run through Material's *expressive* scheme
/// variant, which rotates the secondary and tertiary hues away from the
/// primary instead of keeping them in the same blue family. That rotation is
/// what makes the palette useful here and not just decorative: tertiary comes
/// out green, and green is exactly what a "connected" indicator wants — so
/// [StatusColors.connected] is a real theme role rather than the hardcoded
/// `Color(0xFF2E7D32)` this app used to draw, which never adapted to dark
/// mode.
///
/// Every role below is generated output, not a judgement call. Regenerate the
/// whole block rather than tweaking individual values by hand: the tones are
/// contrast-paired (each `onX` is guaranteed readable on its `X`), and editing
/// one side of a pair silently breaks that guarantee.

// ---------------------------------------------------------------- light

private val LightPrimary = Color(0xFF1861A6)
private val LightOnPrimary = Color(0xFFF8F8FF)
private val LightPrimaryContainer = Color(0xFF99C4FF)
private val LightOnPrimaryContainer = Color(0xFF003D70)
private val LightInversePrimary = Color(0xFF79B3FE)
private val LightSecondary = Color(0xFF426751)
private val LightOnSecondary = Color(0xFFE7FFEC)
private val LightSecondaryContainer = Color(0xFFD1FBDE)
private val LightOnSecondaryContainer = Color(0xFF3D624C)
private val LightTertiary = Color(0xFF006E3B)
private val LightOnTertiary = Color(0xFFE8FFE9)
private val LightTertiaryContainer = Color(0xFF99F7B5)
private val LightOnTertiaryContainer = Color(0xFF005F32)
private val LightBackground = Color(0xFFF8F9FF)
private val LightOnBackground = Color(0xFF163354)
private val LightSurface = Color(0xFFF8F9FF)
private val LightOnSurface = Color(0xFF163354)
private val LightSurfaceVariant = Color(0xFFD3E4FF)
private val LightOnSurfaceVariant = Color(0xFF466084)
private val LightInverseSurface = Color(0xFF020F1F)
private val LightInverseOnSurface = Color(0xFF909EB4)
private val LightError = Color(0xFFAC3434)
private val LightOnError = Color(0xFFFFF7F6)
private val LightErrorContainer = Color(0xFFF56965)
private val LightOnErrorContainer = Color(0xFF65000B)
private val LightOutline = Color(0xFF627CA1)
private val LightOutlineVariant = Color(0xFF99B4DB)
private val LightSurfaceBright = Color(0xFFF8F9FF)
private val LightSurfaceDim = Color(0xFFC5DCFF)
private val LightSurfaceContainer = Color(0xFFE6EEFF)
private val LightSurfaceContainerHigh = Color(0xFFDDE9FF)
private val LightSurfaceContainerHighest = Color(0xFFD3E4FF)
private val LightSurfaceContainerLow = Color(0xFFEFF4FF)
private val LightSurfaceContainerLowest = Color(0xFFFFFFFF)

// ----------------------------------------------------------------- dark

private val DarkPrimary = Color(0xFFBBD6FF)
private val DarkOnPrimary = Color(0xFF224B78)
private val DarkPrimaryContainer = Color(0xFFA3C9FE)
private val DarkOnPrimaryContainer = Color(0xFF16416F)
private val DarkInversePrimary = Color(0xFF3A6190)
private val DarkSecondary = Color(0xFFB5CCBB)
private val DarkOnSecondary = Color(0xFF304538)
private val DarkSecondaryContainer = Color(0xFF162A1E)
private val DarkOnSecondaryContainer = Color(0xFF92A999)
private val DarkTertiary = Color(0xFFC2FFD0)
private val DarkOnTertiary = Color(0xFF006838)
private val DarkTertiaryContainer = Color(0xFF99F7B5)
private val DarkOnTertiaryContainer = Color(0xFF005F32)
private val DarkBackground = Color(0xFF060F1B)
private val DarkOnBackground = Color(0xFFD8E6FF)
private val DarkSurface = Color(0xFF060F1B)
private val DarkOnSurface = Color(0xFFD8E6FF)
private val DarkSurfaceVariant = Color(0xFF13273E)
private val DarkOnSurfaceVariant = Color(0xFF9AACC9)
private val DarkInverseSurface = Color(0xFFF8F9FF)
private val DarkInverseOnSurface = Color(0xFF4C5664)
private val DarkError = Color(0xFFFF716C)
private val DarkOnError = Color(0xFF490006)
private val DarkErrorContainer = Color(0xFF8A1A1E)
private val DarkOnErrorContainer = Color(0xFFFF9993)
private val DarkOutline = Color(0xFF647792)
private val DarkOutlineVariant = Color(0xFF374962)
private val DarkSurfaceBright = Color(0xFF172D47)
private val DarkSurfaceDim = Color(0xFF060F1B)
private val DarkSurfaceContainer = Color(0xFF0C1A2C)
private val DarkSurfaceContainerHigh = Color(0xFF102035)
private val DarkSurfaceContainerHighest = Color(0xFF13273E)
private val DarkSurfaceContainerLow = Color(0xFF081422)
private val DarkSurfaceContainerLowest = Color(0xFF000000)

val PenguinSyncLightColors =
    lightColorScheme(
        primary = LightPrimary,
        onPrimary = LightOnPrimary,
        primaryContainer = LightPrimaryContainer,
        onPrimaryContainer = LightOnPrimaryContainer,
        inversePrimary = LightInversePrimary,
        secondary = LightSecondary,
        onSecondary = LightOnSecondary,
        secondaryContainer = LightSecondaryContainer,
        onSecondaryContainer = LightOnSecondaryContainer,
        tertiary = LightTertiary,
        onTertiary = LightOnTertiary,
        tertiaryContainer = LightTertiaryContainer,
        onTertiaryContainer = LightOnTertiaryContainer,
        background = LightBackground,
        onBackground = LightOnBackground,
        surface = LightSurface,
        onSurface = LightOnSurface,
        surfaceVariant = LightSurfaceVariant,
        onSurfaceVariant = LightOnSurfaceVariant,
        surfaceTint = LightPrimary,
        inverseSurface = LightInverseSurface,
        inverseOnSurface = LightInverseOnSurface,
        error = LightError,
        onError = LightOnError,
        errorContainer = LightErrorContainer,
        onErrorContainer = LightOnErrorContainer,
        outline = LightOutline,
        outlineVariant = LightOutlineVariant,
        surfaceBright = LightSurfaceBright,
        surfaceDim = LightSurfaceDim,
        surfaceContainer = LightSurfaceContainer,
        surfaceContainerHigh = LightSurfaceContainerHigh,
        surfaceContainerHighest = LightSurfaceContainerHighest,
        surfaceContainerLow = LightSurfaceContainerLow,
        surfaceContainerLowest = LightSurfaceContainerLowest,
    )

val PenguinSyncDarkColors =
    darkColorScheme(
        primary = DarkPrimary,
        onPrimary = DarkOnPrimary,
        primaryContainer = DarkPrimaryContainer,
        onPrimaryContainer = DarkOnPrimaryContainer,
        inversePrimary = DarkInversePrimary,
        secondary = DarkSecondary,
        onSecondary = DarkOnSecondary,
        secondaryContainer = DarkSecondaryContainer,
        onSecondaryContainer = DarkOnSecondaryContainer,
        tertiary = DarkTertiary,
        onTertiary = DarkOnTertiary,
        tertiaryContainer = DarkTertiaryContainer,
        onTertiaryContainer = DarkOnTertiaryContainer,
        background = DarkBackground,
        onBackground = DarkOnBackground,
        surface = DarkSurface,
        onSurface = DarkOnSurface,
        surfaceVariant = DarkSurfaceVariant,
        onSurfaceVariant = DarkOnSurfaceVariant,
        surfaceTint = DarkPrimary,
        inverseSurface = DarkInverseSurface,
        inverseOnSurface = DarkInverseOnSurface,
        error = DarkError,
        onError = DarkOnError,
        errorContainer = DarkErrorContainer,
        onErrorContainer = DarkOnErrorContainer,
        outline = DarkOutline,
        outlineVariant = DarkOutlineVariant,
        surfaceBright = DarkSurfaceBright,
        surfaceDim = DarkSurfaceDim,
        surfaceContainer = DarkSurfaceContainer,
        surfaceContainerHigh = DarkSurfaceContainerHigh,
        surfaceContainerHighest = DarkSurfaceContainerHighest,
        surfaceContainerLow = DarkSurfaceContainerLow,
        surfaceContainerLowest = DarkSurfaceContainerLowest,
    )

/// Connection-state colours, kept next to the scheme instead of scattered
/// through the screens as literals.
///
/// Three of the four states map onto real Material roles — that's the point of
/// having picked an expressive palette. "Reconnecting" is the exception: it
/// means *transient trouble, don't panic*, and Material 3 has no role for it
/// (`error` is too final, `secondary` in this palette is a green almost
/// indistinguishable from `tertiary`). So amber is supplied here, as a
/// contrast-paired container/on-container set generated from `#F2A20C` the
/// same way the rest of the palette was generated.
///
/// Note these amber values stay fixed under dynamic colour rather than being
/// harmonised toward the wallpaper. Harmonisation would be nicer, but the
/// blend utility that does it isn't part of the Compose Material 3 surface —
/// and a warning colour that drifts is worse than one that doesn't match.
@Immutable
data class StatusColors(
    val connected: Color,
    val connectedContainer: Color,
    val onConnectedContainer: Color,
    val warning: Color,
    val warningContainer: Color,
    val onWarningContainer: Color,
    val offline: Color,
)

internal fun statusColorsFor(
    dark: Boolean,
    scheme: androidx.compose.material3.ColorScheme,
): StatusColors =
    if (dark) {
        StatusColors(
            connected = scheme.tertiary,
            connectedContainer = scheme.tertiaryContainer,
            onConnectedContainer = scheme.onTertiaryContainer,
            warning = Color(0xFFEABF89),
            warningContainer = Color(0xFF6B4D21),
            onWarningContainer = Color(0xFFFFDEB6),
            offline = scheme.outline,
        )
    } else {
        StatusColors(
            connected = scheme.tertiary,
            connectedContainer = scheme.tertiaryContainer,
            onConnectedContainer = scheme.onTertiaryContainer,
            warning = Color(0xFF7E581D),
            warningContainer = Color(0xFFFCC983),
            onWarningContainer = Color(0xFF634005),
            offline = scheme.outline,
        )
    }

/// Reachable from any composable under [PenguinSyncTheme] as
/// `LocalStatusColors.current`. `staticCompositionLocalOf` rather than
/// `compositionLocalOf`: this changes only when the whole theme changes, so
/// there's no reason to pay for fine-grained invalidation tracking.
val LocalStatusColors =
    staticCompositionLocalOf {
        statusColorsFor(dark = false, scheme = PenguinSyncLightColors)
    }
