java {
    sourceCompatibility = JavaVersion.VERSION_21
    targetCompatibility = JavaVersion.VERSION_21
}
tasks.withType<JavaCompile>().configureEach { options.release.set(21) }
dependencies {
    api(project(":live-core"))
    // The signing engine: Rust QuickJS (`ttl-sign-mobile`) over JNI.
    api(project(":live-native"))
    implementation(project(":live-presence"))
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    testImplementation(project(":live-client"))
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
    implementation("com.fasterxml.jackson.core:jackson-databind:2.18.6")
}
tasks.register("runtimeFootprint") {
    doLast {
        val files = configurations.runtimeClasspath.get().files.filter { it.isFile }
        println("Runtime artifacts: ${files.size}; bytes: ${files.sumOf { it.length() }}")
    }
}
