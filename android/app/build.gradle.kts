plugins {
    id("com.android.application")
}

android {
    namespace = "com.example.ttlsigner"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.example.ttlsigner"
        minSdk = 26 // must match the API level in scripts/android/build-native.sh
        targetSdk = 35
        versionCode = 2
        versionName = "1.1"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.recyclerview:recyclerview:1.3.2")
    implementation("com.google.android.material:material:1.14.0")
    implementation("androidx.fragment:fragment-ktx:1.9.0")
    implementation("androidx.viewpager2:viewpager2:1.1.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-ktx:2.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    // SuperTonic 3 on-device TTS (ported example from mewmix/nabu): ONNX
    // Runtime sessions + Gson for the model JSON sidecars.
    implementation("com.microsoft.onnxruntime:onnxruntime-android:1.22.0")
    implementation("com.google.code.gson:gson:2.10.1")

    testImplementation("junit:junit:4.13.2")
    // Unit tests run against android.jar stubs whose methods throw; the real org.json
    // makes Discovery's parser genuinely testable on the JVM.
    testImplementation("org.json:json:20250107")

    androidTestImplementation("androidx.test:core:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test:rules:1.7.0")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.7.0")
    androidTestImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
}
