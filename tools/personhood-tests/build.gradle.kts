plugins {
    kotlin("jvm") version "2.1.0"
}

repositories {
    mavenCentral()
}

kotlin {
    jvmToolchain(21)
    sourceSets {
        main {
            kotlin.srcDir("../../src-tauri/gen/android/app/src/main/java")
            kotlin.include("org/alexandria/node/PersonhoodKeyTransfer.kt")
        }
        test {
            kotlin.srcDir("../../src-tauri/gen/android/app/src/test/java")
            kotlin.include("org/alexandria/node/PersonhoodKeyTransferTest.kt")
        }
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}
