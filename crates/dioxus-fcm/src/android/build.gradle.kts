plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.dioxus.main.fcm"
    compileSdk = 34
    defaultConfig { minSdk = 24 }
}

dependencies {
    implementation(platform("com.google.firebase:firebase-bom:33.5.1"))
    implementation("com.google.firebase:firebase-messaging")
    implementation("com.google.android.gms:play-services-tasks:18.2.0")
}
