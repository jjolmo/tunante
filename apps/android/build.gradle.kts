plugins {
    id("com.android.application") version "8.5.2" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    // Since Kotlin 2.0 the Compose compiler ships with Kotlin itself, so this
    // plugin's version tracks the Kotlin version rather than having one of its own.
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
}
