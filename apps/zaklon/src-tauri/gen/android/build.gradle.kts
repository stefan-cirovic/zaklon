buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:8.11.0")
        // Kotlin 2.1: new enough for CameraX and zxing-cpp (the barcode
        // scanner), and still able to build Tauri 2.11's Android library,
        // whose kotlinOptions block is an error from Kotlin 2.2 on.
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.1.21")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

tasks.register("clean").configure {
    delete("build")
}

