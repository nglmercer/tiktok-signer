dependencies {
    api(project(":live-core"))
    api("com.squareup.okhttp3:okhttp:5.5.1")
    compileOnly(project(":live-proto"))
    implementation("com.google.protobuf:protobuf-java:4.36.2")
    testImplementation(project(":live-proto"))
    implementation("com.fasterxml.jackson.core:jackson-databind:2.22.4")
    testImplementation("com.squareup.okhttp3:mockwebserver:5.5.1")
}
// Bundle internal generated schemas so the published client requires no unpublished module.
tasks.jar { from(project(":live-proto").sourceSets.main.get().output) }
tasks.named("sourcesJar", Jar::class) { from(project(":live-proto").sourceSets.main.get().allSource) }
