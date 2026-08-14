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
// TODO(M1): key the cargo profile off the Gradle build variant (debug vs.
// release) instead of always building debug — release ABI stripping/LTO
// matters once this ships, not for the first bring-up.
plugins {
    id("com.android.library")
}

// crates/ffi's cdylib name (`[lib] name = "penguinsync"` in its Cargo.toml).
val rustLibName = "penguinsync"
val repoRoot = rootDir.resolve("..")
val rustJniLibsDir = file("src/main/jniLibs")
val uniffiKotlinDir = file("src/main/kotlin")

// arm64-v8a + armeabi-v7a ship; x86_64 added here too so the emulator works
// without a phone (docs/design.md §4.6).
val abis = listOf("arm64-v8a", "armeabi-v7a", "x86_64")

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
        commandLine(listOf("cargo") + args)
    }

val uniffiBindgen =
    tasks.register<Exec>("uniffiBindgen") {
        description = "Generates Kotlin bindings from the built cdylib's embedded UniFFI metadata."
        dependsOn(cargoNdkBuild)
        workingDir = repoRoot
        val libraryPath = rustJniLibsDir.resolve("arm64-v8a/lib$rustLibName.so")
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
    dependsOn(uniffiBindgen)
}
