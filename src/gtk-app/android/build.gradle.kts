// AGP 9 compiles Kotlin itself (built-in Kotlin).
plugins {
    id("com.android.application") version "9.4.1" apply false
}

// ice-commander/app: this project lives in src/gtk-app/android.
layout.buildDirectory.set(File(rootDir.parentFile.parentFile.parentFile, "bin/android-gtk-gradle-root"))
