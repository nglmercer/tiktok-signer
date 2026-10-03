dependencies {
    api(project(":live-core"))
    api("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("com.fasterxml.jackson.core:jackson-databind:2.18.6")
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
}
