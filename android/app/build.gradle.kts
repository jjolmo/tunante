plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.tunante.android"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.tunante.android"
        // 26 is the floor for cpal's AAudio backend, so it is ours.
        minSdk = 26
        targetSdk = 34
        // From the release tag when there is one, so an installed app can say
        // which build it is. The APK used to be *named* after the version while
        // reporting versionName 0.1.0 from inside, which is the wrong half:
        // the name is lost the moment it is installed.
        //
        // versionCode matters more than it looks. Android decides whether an
        // install is an upgrade by that number, and it was pinned at 1, so
        // every release looked like the same one. Packed as
        // major*1e6 + minor*1e3 + patch, which stays monotonic while minor and
        // patch are under a thousand -- this project is at 0.1.262.
        val release = (System.getenv("TUNANTE_VERSION") ?: "0.1.0").removePrefix("v")
        val parts = release.split(".")
        versionName = release
        versionCode = (
            (parts.getOrNull(0)?.toIntOrNull() ?: 0) * 1_000_000 +
            (parts.getOrNull(1)?.toIntOrNull() ?: 0) * 1_000 +
            (parts.getOrNull(2)?.toIntOrNull() ?: 0)
        ).coerceAtLeast(1)

        ndk {
            // The phone and the emulator, unless build.sh was told otherwise.
            //
            // Read from the same $ABIS it uses, and not hardcoded, because the
            // two must agree: AGP packages an AndroidX .so for every ABI named
            // here whether or not our own libraries were staged for it. Listing
            // x86_64 while staging only arm64 produces an APK that *claims*
            // x86_64, installs happily on an emulator, and then dies in
            // System.loadLibrary.
            abiFilters += (System.getenv("ABIS") ?: "arm64-v8a x86_64")
                .split(" ").filter { it.isNotBlank() }.toSet()
        }
    }

    packaging {
        jniLibs {
            // Not a size optimisation to turn off, and not optional.
            //
            // AGP defaults this to false for minSdk >= 23, which leaves the
            // native libraries mapped straight out of the (uncompressed) APK
            // rather than unpacked into nativeLibraryDir. That is fine for a
            // library you dlopen and fatal for one you exec: there is no file
            // on disk to hand to execve.
            useLegacyPackaging = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    signingConfigs {
        create("release") {
            // Only wired up when the keystore is actually there. CI decodes it
            // from a secret; a fresh clone has none and must still build.
            val store = System.getenv("TUNANTE_KEYSTORE")
            if (store != null && file(store).exists()) {
                storeFile = file(store)
                storePassword = System.getenv("TUNANTE_KEYSTORE_PASSWORD")
                keyAlias = "tunante"
                keyPassword = System.getenv("TUNANTE_KEYSTORE_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // The whole point of a stable key: Android refuses to install an
            // APK over one signed with a different certificate
            // (INSTALL_FAILED_UPDATE_INCOMPATIBLE), and the only way through is
            // to uninstall — losing the scanned library. A debug key generated
            // per CI run made every release a fresh install.
            val store = System.getenv("TUNANTE_KEYSTORE")
            if (store != null && file(store).exists()) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.09.03")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.2")
}
