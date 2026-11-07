plugins {
    id("com.android.application")
}

android {
    namespace = "dev.wunder.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.wunder.android"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.4.0"
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

    buildTypes {
        getByName("debug") {
            isMinifyEnabled = false
        }
        getByName("release") {
            isMinifyEnabled = false
            // Release signing is supplied by the caller; the prototype only
            // needs the debug keystore.
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
}
