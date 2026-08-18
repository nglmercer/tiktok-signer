# Fuzz targets

Run with `cargo fuzz run uri_sanitizer`, `cargo fuzz run protobuf_transport`, or
`cargo fuzz run fixture_loader`. Targets accept arbitrary bytes and focus on panic resistance
for transport URI escaping, protobuf envelope decoding, and fixture deserialization.
