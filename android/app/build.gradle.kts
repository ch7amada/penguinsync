plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "org.penguinsync.app"
    compileSdk = 37

    defaultConfig {
        applicationId = "org.penguinsync.app"
        // Android will not let a background app read the clipboard without
        // one of a short list of exemptions (docs/design.md §3.1) — none of
        // that is M0's problem, but minSdk/targetSdk are set for the whole
        // project from day one.
        minSdk = 31
        targetSdk = 37
        versionCode = 1
        versionName = "0.0.0"
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        // uniffi's generated JNA loader ends up with duplicate metadata
        // entries across dependencies; excluded rather than fought.
        resources.excludes += setOf("META-INF/AL2.0", "META-INF/LGPL2.1")
    }
}

dependencies {
    implementation(project(":core"))

    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.9.1")
    implementation(platform("androidx.compose:compose-bom:2025.06.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.navigation:navigation-compose:2.9.0")
    // JNA (uniffi's Kotlin bindings need it) comes in transitively via
    // :core's `api` dependency.
}
