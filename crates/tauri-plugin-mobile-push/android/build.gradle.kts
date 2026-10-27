// Alexandria modification: Firebase messaging dependency and native command regression tests.
plugins {
    id("com.android.library")
    kotlin("android")
}

android {
    namespace = "app.tauri.mobilepush"
    compileSdk = 34

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("proguard-rules.pro")
    }

    testOptions {
        unitTests.isIncludeAndroidResources = true
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
}

dependencies {
    compileOnly(project(":tauri-android"))
    testImplementation(project(":tauri-android"))
    testImplementation("junit:junit:4.13.2")
    testImplementation("com.fasterxml.jackson.core:jackson-databind:2.15.3")
    testImplementation("org.robolectric:robolectric:4.14.1")
    implementation("com.google.firebase:firebase-messaging:23.4.1")
}
