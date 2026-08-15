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
    // Non-deprecated `LocalLifecycleOwner` for QrScanner.kt's CameraX bind
    // (the one on androidx.compose.ui.platform is deprecated).
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.1")
    implementation(platform("androidx.compose:compose-bom:2026.08.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    // Pinned above the BOM on purpose. Material 3 Expressive — the theme, the
    // motion scheme, ShortNavigationBar, the flexible title+subtitle app bars
    // — ships inside material3 1.4.0 (what the BOM resolves) but every one of
    // those declarations is `internal` there, so none of it can be called.
    // 1.5.0-alpha is the first release that makes the API public.
    //
    // The alpha is contained: it is the only pre-release artifact in the
    // graph. It asks for foundation/animation 1.12.0-beta01 and the BOM's
    // stable 1.12.0 wins those, so nothing else in the app is on a
    // pre-release version. Revisit when 1.5.0 goes stable.
    implementation("androidx.compose.material3:material3:1.5.0-alpha26")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.navigation:navigation-compose:2.9.0")
    // JNA (uniffi's Kotlin bindings need it) comes in transitively via
    // :core's `api` dependency.

    // QR scanning for the Pair screen (docs/design.md §4.6 / §9's four
    // screens): CameraX for the preview + frame stream, zxing `core` to
    // decode them. Both are plain Apache-2.0 jars/AARs with no Google Play
    // Services dependency — unlike ML Kit's barcode scanner, this doesn't
    // compromise the F-Droid target (docs/design.md §1).
    val cameraxVersion = "1.6.1"
    implementation("androidx.camera:camera-core:$cameraxVersion")
    implementation("androidx.camera:camera-camera2:$cameraxVersion")
    implementation("androidx.camera:camera-lifecycle:$cameraxVersion")
    implementation("androidx.camera:camera-view:$cameraxVersion")
    implementation("com.google.zxing:core:3.5.4")
}
