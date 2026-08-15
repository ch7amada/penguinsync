// Generated UniFFI bindings + jniLibs — nothing hand-written here except
// this build glue (docs/design.md §4.2 "Build glue", §4.6).
//
// Two steps, explicit, no third-party Gradle plugin:
//   1. cargo-ndk cross-compiles crates/ffi into a .so per ABI.
//   2. uniffi-bindgen reads that .so's embedded metadata and generates the
//      matching Kotlin into this module's source set.
//
// Outputs land directly in the *conventional* `src/main/jniLibs` and
// `src/main/kotlin` directories rather than being registered as extra
// source dirs — AGP 9.3.1's `AndroidSourceSet.jniLibs`/`.kotlin` accessors
// throw a ClassCastException from Kotlin build scripts on this project
// (`DefaultAndroidLibrarySourceSet_Decorated` vs. `AndroidLibrarySourceSet`),
// and the convention path sidesteps it entirely — this module has no
// hand-written Kotlin to collide with anyway. Both directories are
// git-ignored; `./gradlew :core:uniffiBindgen` (or a full build) regenerates
// them from a clean checkout.
//
plugins {
    id("com.android.library")
}

// crates/ffi's cdylib name (`[lib] name = "penguinsync"` in its Cargo.toml).
val rustLibName = "penguinsync"
val repoRoot = rootDir.resolve("..")
val rustJniLibsDir = file("src/main/jniLibs")
val uniffiKotlinDir = file("src/main/kotlin")

// Which cargo profile the `.so` is built with, and therefore which ABIs are
// produced (docs/design.md §4.6: arm64-v8a + armeabi-v7a ship, x86_64 is a
// debug-only convenience so the emulator works without a phone).
//
// A debug-profile `.so` in a released APK would be several times the size and
// meaningfully slower, so this is not cosmetic. It is decided from the
// requested task names, with `-Ppenguinsync.release=true` as the explicit
// override — which is what packaging/build-android-release.sh and the release
// workflow pass, rather than relying on the heuristic. The heuristic is only
// there so that a plain `./gradlew :app:assembleRelease` by hand does the
// right thing; note that a single invocation building *both* variants (bare
// `./gradlew build`) gets one profile for both, and is not how releases are
// cut.
val releaseRust =
    project.findProperty("penguinsync.release") == "true" ||
        gradle.startParameter.taskNames.any { it.endsWith("Release") || it.endsWith("release") }

// `release-android` (not plain `release`) — panic = "abort", because a Rust
// panic unwinding across the JNI boundary is undefined behaviour, and it
// surfaces on Android as an unattributable native crash either way
// (docs/design.md §8).
val cargoProfile = if (releaseRust) "release-android" else "dev"

val abis =
    if (releaseRust) {
        listOf("arm64-v8a", "armeabi-v7a")
    } else {
        listOf("arm64-v8a", "armeabi-v7a", "x86_64")
    }

android {
    namespace = "org.penguinsync.core"
    compileSdk = 37

    defaultConfig {
        minSdk = 31
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    // uniffi's generated Kotlin talks to the .so through JNA.
    api("net.java.dev.jna:jna:5.17.0@aar")
}

val cargoNdkBuild =
    tasks.register<Exec>("cargoNdkBuild") {
        description = "Cross-compiles crates/ffi for Android via cargo-ndk."
        workingDir = repoRoot
        outputs.dir(rustJniLibsDir)
        inputs.dir(repoRoot.resolve("crates")).withPropertyName("rustSources")
        inputs.file(repoRoot.resolve("Cargo.lock")).withPropertyName("cargoLock")
        // The workspace manifest defines the profiles this task builds with,
        // so a profile edit has to invalidate it.
        inputs.file(repoRoot.resolve("Cargo.toml")).withPropertyName("cargoManifest")
        // Without this the task is considered up to date when only the
        // profile changed, and a release APK quietly ships the debug `.so`
        // left behind by the last debug build.
        inputs.property("cargoProfile", cargoProfile)

        // Switching debug → release drops x86_64 from `abis`, but a stale
        // x86_64 directory from the previous build would still be sitting
        // here and would still be packaged. cargo's own cache means the
        // rebuild after this is a recompile of nothing and a recopy.
        doFirst { rustJniLibsDir.deleteRecursively() }

        val ndkPath =
            System.getenv("ANDROID_NDK_HOME")
                ?: System.getenv("ANDROID_NDK_ROOT")
                ?: System.getenv("ANDROID_HOME")?.let { sdkDir ->
                    file("$sdkDir/ndk").listFiles()?.maxByOrNull { it.name }?.absolutePath
                }
                ?: error("No NDK found — set ANDROID_NDK_HOME or ANDROID_HOME (with an ndk/ subdir installed)")
        environment("ANDROID_NDK_HOME", ndkPath)

        val args = mutableListOf("ndk")
        abis.forEach { args += listOf("-t", it) }
        args += listOf("-o", rustJniLibsDir.absolutePath, "build", "-p", "penguinsync-ffi")
        args += listOf("--profile", cargoProfile)
        commandLine(listOf("cargo") + args)
    }

// The bindings are generated from a *host* build of the same crate, not from
// the Android `.so` that ships.
//
// `uniffi-bindgen generate --library` recovers the interface definition from
// symbols the crate exports, and the release profile strips exactly those —
// "No UniFFI metadata found", at build time, with nothing pointing at the
// cause. Leaving symbols in the shipped library instead costs about 5 MB per
// ABI, and AGP does not strip them back out. Since the metadata describes the
// source, not the target, a host build answers the question just as well and
// costs one compile that `cargo build --workspace` is doing anyway.
val hostTargetDir =
    System.getenv("CARGO_TARGET_DIR")?.let { file(it) } ?: repoRoot.resolve("target")
val hostLibrary = hostTargetDir.resolve("debug/lib$rustLibName.so")

val cargoHostBuild =
    tasks.register<Exec>("cargoHostBuild") {
        description = "Builds crates/ffi for the host, purely so uniffi-bindgen can read its metadata."
        workingDir = repoRoot
        inputs.dir(repoRoot.resolve("crates")).withPropertyName("rustSources")
        inputs.file(repoRoot.resolve("Cargo.lock")).withPropertyName("cargoLock")
        outputs.file(hostLibrary)
        commandLine("cargo", "build", "-p", "penguinsync-ffi")
    }

val uniffiBindgen =
    tasks.register<Exec>("uniffiBindgen") {
        description = "Generates Kotlin bindings from the built cdylib's embedded UniFFI metadata."
        dependsOn(cargoHostBuild)
        workingDir = repoRoot
        val libraryPath = hostLibrary
        inputs.file(libraryPath)
        outputs.dir(uniffiKotlinDir)

        commandLine(
            "cargo", "run", "-p", "penguinsync-ffi", "--bin", "uniffi-bindgen", "--features", "uniffi-cli", "--",
            "generate", "--library", libraryPath.absolutePath,
            "--language", "kotlin",
            "--out-dir", uniffiKotlinDir.absolutePath,
            "--no-format",
        )
    }

tasks.named("preBuild") {
    dependsOn(cargoNdkBuild, uniffiBindgen)
}
