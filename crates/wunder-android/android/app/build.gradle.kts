import java.io.File
import java.util.Locale
import java.util.Properties

plugins {
    id("com.android.application")
}

// ---------------------------------------------------------------------------
// Version
//
// Every wunder form shares config/app_version.json at the repository root.
// This build lives three levels below it:
//   crates/wunder-android/android -> crates/wunder-android -> crates -> repository root
// ---------------------------------------------------------------------------
val repoRoot: File = rootProject.projectDir.resolve("../../..").canonicalFile

fun readAppVersion(): String {
    val file = File(repoRoot, "config/app_version.json")
    check(file.isFile) { "wunder-android: version file is missing: $file" }
    val match = Regex("\"version\"\\s*:\\s*\"([^\"]+)\"").find(file.readText())
        ?: error("wunder-android: no version field in $file")
    return match.groupValues[1].trim()
}

/**
 * versionCode from the semantic version:
 *   major * 1_000_000 + minor * 10_000 + patch * 100
 * 0.4.0 -> 40000. Every build of one version shares the code; the in-app
 * updater (UiBridge) accepts an equal versionCode, so a nightly installs over
 * the previous nightly of the same version. WUNDER_ANDROID_VERSION_CODE
 * overrides the derived value when a build needs a distinct code.
 */
fun semanticVersionCode(version: String): Int {
    val match = Regex("^(\\d+)\\.(\\d+)\\.(\\d+)").find(version)
        ?: error("wunder-android: cannot derive a versionCode from '$version'")
    val (major, minor, patch) = match.destructured
    return major.toInt() * 1_000_000 + minor.toInt() * 10_000 + patch.toInt() * 100
}

val appVersion = readAppVersion()
val appVersionCode: Int = System.getenv("WUNDER_ANDROID_VERSION_CODE")
    ?.trim()
    ?.takeIf { it.isNotEmpty() }
    ?.let { override ->
        override.toIntOrNull() ?: error("wunder-android: WUNDER_ANDROID_VERSION_CODE '$override' is not an integer")
    }
    ?: semanticVersionCode(appVersion)

// ---------------------------------------------------------------------------
// Release signing
//
// Priority: environment variables, then local.properties keys:
//   WUNDER_ANDROID_KEYSTORE          / wunder.android.keystore
//   WUNDER_ANDROID_KEYSTORE_PASSWORD / wunder.android.keystore.password
//   WUNDER_ANDROID_KEY_ALIAS         / wunder.android.key.alias
//   WUNDER_ANDROID_KEY_PASSWORD      / wunder.android.key.password
//
// Without a configured keystore the release APK is signed with the pinned
// repository keystore packaging/android/debug.keystore (store/key password
// "android", alias "androiddebugkey"). That is the same certificate earlier
// debug nightlies shipped, so existing installs keep the upgrade path; the
// build prints a warning. WUNDER_ANDROID_REQUIRE_RELEASE_SIGNING=1 turns the
// silent fallback into a build failure.
// ---------------------------------------------------------------------------
val localProperties = Properties().apply {
    val file = rootProject.file("local.properties")
    if (file.isFile) {
        file.inputStream().use { load(it) }
    }
}

fun signingValue(environment: String, property: String): String? =
    System.getenv(environment)?.trim()?.takeIf { it.isNotEmpty() }
        ?: localProperties.getProperty(property)?.trim()?.takeIf { it.isNotEmpty() }

val requireReleaseSigning: Boolean = System.getenv("WUNDER_ANDROID_REQUIRE_RELEASE_SIGNING")
    ?.trim()
    ?.lowercase(Locale.ROOT)
    ?.let { it == "1" || it == "true" || it == "yes" || it == "on" }
    ?: false

android {
    namespace = "dev.wunder.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.wunder.android"
        minSdk = 26
        targetSdk = 36
        versionCode = appVersionCode
        versionName = appVersion
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk {
            // Shipping ABI is arm64. Emulator verification can request more
            // slices with -PwunderAbis=x86_64 (the build script builds the
            // matching Rust targets).
            val requested = (project.findProperty("wunderAbis") as String?)
                ?.split(",")
                ?.map { it.trim() }
                ?.filter { it.isNotEmpty() }
                ?: listOf("arm64-v8a")
            abiFilters += requested
        }
    }

    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")

    signingConfigs {
        create("release") {
            val keystore = signingValue("WUNDER_ANDROID_KEYSTORE", "wunder.android.keystore")
            val storePassword = signingValue("WUNDER_ANDROID_KEYSTORE_PASSWORD", "wunder.android.keystore.password")
            val keyAlias = signingValue("WUNDER_ANDROID_KEY_ALIAS", "wunder.android.key.alias")
            val keyPassword = signingValue("WUNDER_ANDROID_KEY_PASSWORD", "wunder.android.key.password")
            if (keystore == null) {
                val stray = buildList {
                    if (storePassword != null) add("WUNDER_ANDROID_KEYSTORE_PASSWORD (wunder.android.keystore.password)")
                    if (keyAlias != null) add("WUNDER_ANDROID_KEY_ALIAS (wunder.android.key.alias)")
                    if (keyPassword != null) add("WUNDER_ANDROID_KEY_PASSWORD (wunder.android.key.password)")
                }
                check(stray.isEmpty()) {
                    "wunder-android: release signing is partially configured; " +
                        "WUNDER_ANDROID_KEYSTORE (wunder.android.keystore) is missing while " +
                        stray.joinToString() + " is set"
                }
                check(!requireReleaseSigning) {
                    "wunder-android: WUNDER_ANDROID_REQUIRE_RELEASE_SIGNING is set but no release " +
                        "keystore is configured (WUNDER_ANDROID_KEYSTORE / wunder.android.keystore)"
                }
                val fallback = File(repoRoot, "packaging/android/debug.keystore")
                check(fallback.isFile) { "wunder-android: compatibility keystore is missing: $fallback" }
                logger.warn(
                    "wunder-android: release signing is not configured; signing the release APK with " +
                        "packaging/android/debug.keystore (fixed compatibility signature). Set " +
                        "WUNDER_ANDROID_KEYSTORE / WUNDER_ANDROID_KEYSTORE_PASSWORD / " +
                        "WUNDER_ANDROID_KEY_ALIAS / WUNDER_ANDROID_KEY_PASSWORD for a release key."
                )
                storeFile = fallback
                this.storePassword = "android"
                this.keyAlias = "androiddebugkey"
                this.keyPassword = "android"
            } else {
                val missing = buildList {
                    if (storePassword == null) add("WUNDER_ANDROID_KEYSTORE_PASSWORD (wunder.android.keystore.password)")
                    if (keyAlias == null) add("WUNDER_ANDROID_KEY_ALIAS (wunder.android.key.alias)")
                    if (keyPassword == null) add("WUNDER_ANDROID_KEY_PASSWORD (wunder.android.key.password)")
                }
                check(missing.isEmpty()) {
                    "wunder-android: keystore is configured but " + missing.joinToString() + " is missing"
                }
                val configured = File(keystore)
                val resolved = if (configured.isAbsolute) configured else File(repoRoot, keystore)
                check(resolved.isFile) { "wunder-android: keystore not found: $resolved" }
                storeFile = resolved
                this.storePassword = requireNotNull(storePassword) {
                    "wunder-android: WUNDER_ANDROID_KEYSTORE_PASSWORD (wunder.android.keystore.password) is missing"
                }
                this.keyAlias = requireNotNull(keyAlias) {
                    "wunder-android: WUNDER_ANDROID_KEY_ALIAS (wunder.android.key.alias) is missing"
                }
                this.keyPassword = requireNotNull(keyPassword) {
                    "wunder-android: WUNDER_ANDROID_KEY_PASSWORD (wunder.android.key.password) is missing"
                }
            }
        }
    }

    buildTypes {
        getByName("debug") {
            isMinifyEnabled = false
        }
        getByName("release") {
            isDebuggable = false
            isMinifyEnabled = false
            // Signed with the configured release key, or the pinned compatibility
            // keystore so debug-era installs can keep upgrading in place.
            signingConfig = signingConfigs.getByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        // Android 10+ refuses to execve files under the app's private data
        // directory. The bundled proot/busybox must therefore live in the
        // package-managed native library directory and be symlinked from
        // files/usr/bin at install time. Legacy packaging extracts every
        // jniLibs entry to that directory; the Rust binary is already
        // stripped, so extraction only costs installed size.
        jniLibs.useLegacyPackaging = true
    }

    androidResources {
        // The offline tool pack is copied verbatim into filesDir before proot
        // extracts it; keeping it uncompressed gives stable length reporting
        // and avoids an extra asset decompression pass.
        noCompress += "xz"
    }

    lint {
        // assembleRelease otherwise wires lintVital into the release build and
        // resolves the whole com.android.tools.lint toolchain. The offline
        // build kits only cache what assembleDebug needs, so a nightly or an
        // offline Gitea build would fail on a missing lint-gradle artifact.
        checkReleaseBuilds = false
    }
}

dependencies {
    // Android system trust-store verification for rustls. The Rust side
    // (rustls-platform-verifier 0.7.1) initializes through this Kotlin
    // component; without it every HTTPS request panics on a Tokio worker.
    // Keep the AAR version in lockstep with rustls-platform-verifier-android
    // in Cargo.lock (0.2.0).
    implementation(files("libs/rustls-platform-verifier-0.2.0.aar"))
    // The verifier AAR is Kotlin; the app has no Kotlin plugin, so vendor the
    // standard library next to it instead of adding a remote Maven repository.
    implementation(files("libs/kotlin-stdlib-2.0.21.jar"))

    // Instrumented tests only (androidTest); none of these ship in the APK.
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:rules:1.6.1")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:core:1.6.1")
    androidTestImplementation("androidx.test.uiautomator:uiautomator:2.3.0")
}
