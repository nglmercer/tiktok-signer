plugins { id("com.google.protobuf") version "0.9.5" apply false }
allprojects { group = "io.github.nglmercer"; version = "0.1.0-SNAPSHOT"; repositories { mavenCentral() } }
subprojects {
    val moduleName = name
    apply(plugin = "java-library")
    extensions.configure<JavaPluginExtension> {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        withSourcesJar()
        if (project.name != "live-proto") withJavadocJar()
    }
    tasks.withType<JavaCompile>().configureEach { options.release.set(17); options.encoding = "UTF-8" }
    dependencies {
        "testImplementation"("org.junit.jupiter:junit-jupiter:5.11.4")
        "testRuntimeOnly"("org.junit.platform:junit-platform-launcher")
    }
    tasks.withType<Javadoc>().configureEach {
        (options as StandardJavadocDocletOptions).addBooleanOption("Xdoclint:none", true)
        if (project.name == "live-proto") enabled = false
    }
    tasks.withType<Test>().configureEach {
        useJUnitPlatform()
        systemProperty("repositoryRoot", rootProject.projectDir.parentFile.parentFile.absolutePath)
    }
    if (name.startsWith("live-") && name != "live-proto") {
        apply(plugin = "maven-publish")
        extensions.configure<PublishingExtension> {
            publications { create<MavenPublication>("library") {
                from(components["java"])
                artifactId = "tiktok-$moduleName"
                pom { name.set(artifactId); description.set("TikTok LIVE Java SDK"); url.set("https://github.com/nglmercer/tiktok-signer")
                    licenses {
                        license { name.set("MIT"); url.set("https://opensource.org/licenses/MIT") }
                        if (moduleName == "live-client") license {
                            name.set("Modified AGPL-3.0 with additional permissions (protobuf bindings)")
                            url.set("https://github.com/nglmercer/tiktok-signer/blob/main/crates/ttl-live-proto/LICENSE.upstream")
                        }
                    }
                }
            } }
        }
    }
}
