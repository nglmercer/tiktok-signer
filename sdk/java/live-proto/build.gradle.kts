plugins { id("com.google.protobuf") }
dependencies { api("com.google.protobuf:protobuf-java:3.25.5") }
sourceSets { main { proto { srcDir("../../../crates/ttl-live-proto/proto/v3") } } }
protobuf { protoc { artifact = "com.google.protobuf:protoc:3.25.5" } }
// Internal implementation classes are bundled into live-client, not a separate public artifact.
