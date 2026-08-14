// Root build script: only plugin version declarations. Everything else
// lives in :app/:core (docs/design.md §4.6).
// AGP 9.0+ has built-in Kotlin support (kotl.in/gradle/agp-built-in-kotlin)
// — no separate org.jetbrains.kotlin.android plugin needed or allowed.
plugins {
    id("com.android.application") version "9.3.1" apply false
    id("com.android.library") version "9.3.1" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.4.10" apply false
}
