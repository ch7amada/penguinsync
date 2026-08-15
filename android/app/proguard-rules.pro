# R8 rules for the release build.
#
# Why R8 is on at all: `material-icons-extended` declares every Material icon
# as a Kotlin function, and Compose is large. Unshrunk, this app's dex weighs
# ~55 MB for a UI that uses about fifteen icons. R8 is not an optimisation
# here, it is the difference between a 60 MB download and a sane one.
#
# Why the rules below are not optional: everything the app does eventually
# crosses into Rust through UniFFI, which reaches the native library via JNA.
# JNA resolves classes, fields and method signatures reflectively at runtime,
# so R8 sees no references to them and removes them — and the failure is a
# crash on the user's phone at the first FFI call, not a build error here.

# --- JNA -------------------------------------------------------------------
# JNA maps Java/Kotlin declarations onto native symbols by name. Renaming or
# removing any of it breaks that mapping.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-keep class * implements com.sun.jna.** { *; }

# JNA's own optional integrations reference classes that are not on this
# app's classpath. They are never reached at runtime; silence the warnings
# rather than dragging the dependencies in.
-dontwarn java.awt.**
-dontwarn com.sun.jna.**

# --- UniFFI ----------------------------------------------------------------
# The generated bindings (uniffi.penguinsync, produced into :core by
# uniffi-bindgen) declare the JNA interface, its callback interfaces and the
# structs passed across the boundary. Callbacks in particular are invoked
# *from* Rust, so nothing on the Kotlin side appears to call them.
-keep class uniffi.** { *; }
-keep interface uniffi.** { *; }

# The Kotlin objects the app hands to Rust as event listeners are called back
# by name from native code.
-keep class org.penguinsync.app.** implements uniffi.penguinsync.CoreEventListener { *; }
