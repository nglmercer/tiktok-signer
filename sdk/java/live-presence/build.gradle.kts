dependencies {
    api(project(":live-core"))
    api("com.squareup.okhttp3:okhttp:5.5.1")
    implementation("com.fasterxml.jackson.core:jackson-databind:2.22.4")
    testImplementation("com.squareup.okhttp3:mockwebserver:5.5.1")
}
