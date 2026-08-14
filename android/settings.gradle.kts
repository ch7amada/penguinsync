pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "penguinsync"

// :app — Compose UI, foreground service, permissions, platform integrations.
// :core — generated UniFFI bindings + jniLibs (docs/design.md §4.6).
include(":app", ":core")
