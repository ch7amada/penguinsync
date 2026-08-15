import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

// Release signing, from a keystore this repository never contains.
//
// Two sources, in order: `android/keystore.properties` for a human at a
// laptop, and environment variables for CI, where the keystore arrives as a
// base64 secret and is written to disk by the workflow. If neither is
// present the release build still succeeds and produces an *unsigned* APK —
// deliberately, so that `assembleRelease` stays runnable by anyone who just
// wants to check the app compiles in release mode. Only the person cutting
// the release needs the key.
//
// See docs/RELEASING.md for generating the keystore. Losing it means every
// user has to uninstall and reinstall to take an update, so it belongs in a
// password manager, not a drawer.
val keystoreProperties =
    Properties().apply {
        val file = rootProject.file("keystore.properties")
        if (file.exists()) file.inputStream().use { load(it) }
    }

fun signingValue(
    key: String,
    env: String,
): String? = keystoreProperties.getProperty(key) ?: System.getenv(env)

val keystorePath = signingValue("storeFile", "PENGUINSYNC_KEYSTORE_FILE")
val hasSigningKey = keystorePath != null && rootProject.file(keystorePath).exists()

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
        // versionCode is monotonic across *released* builds and unrelated to
        // versionName. It starts at 2 because locally-installed debug builds
        // already carried 1, and Android refuses a downgrade.
        versionCode = 2
        versionName = "0.1.0"
    }

    buildFeatures {
        compose = true
    }

    signingConfigs {
        if (hasSigningKey) {
            create("release") {
                storeFile = rootProject.file(keystorePath!!)
                storePassword = signingValue("storePassword", "PENGUINSYNC_KEYSTORE_PASSWORD")
                keyAlias = signingValue("keyAlias", "PENGUINSYNC_KEY_ALIAS")
                keyPassword = signingValue("keyPassword", "PENGUINSYNC_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        debug {
            // x86_64 for the emulator, on top of the two that ship.
            ndk { abiFilters += setOf("arm64-v8a", "armeabi-v7a", "x86_64") }
        }
        release {
            // Without this the APK also carries armeabi, mips, mips64 and x86
            // copies of libjnidispatch — JNA's AAR ships every ABI it has ever
            // supported, and nothing else prunes them.
            ndk { abiFilters += setOf("arm64-v8a", "armeabi-v7a") }
            // Not optional: material-icons-extended alone puts ~55 MB of dex
            // in an unshrunk build. The UniFFI/JNA boundary is reflective and
            // needs the keep rules in proguard-rules.pro — see the reasoning
            // there, and verify a release APK on a real device before
            // shipping one, because a wrongly-stripped FFI symbol fails at
            // the first native call rather than at build time.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            signingConfig = if (hasSigningKey) signingConfigs.getByName("release") else null
        }
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
