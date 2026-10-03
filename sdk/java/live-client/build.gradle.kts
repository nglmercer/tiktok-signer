dependencies {
    api(project(":live-core"))
    api("com.squareup.okhttp3:okhttp:4.12.0")
    compileOnly(project(":live-proto"))
    implementation("com.google.protobuf:protobuf-java:3.25.5")
    testImplementation(project(":live-proto"))
    implementation("com.fasterxml.jackson.core:jackson-databind:2.18.6")
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
}
// Bundle internal generated schemas so the published client requires no unpublished module.
tasks.jar { from(project(":live-proto").sourceSets.main.get().output) }
tasks.named("sourcesJar", Jar::class) { from(project(":live-proto").sourceSets.main.get().allSource) }
