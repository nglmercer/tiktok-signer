plugins { application }
dependencies { implementation(project(":live-presence")) }
application { mainClass.set("io.github.nglmercer.tiktoklive.examples.Main") }
tasks.named<JavaExec>("run") { standardInput = System.`in` }
