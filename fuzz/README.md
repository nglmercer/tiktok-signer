# Fuzz targets

Run with `cargo fuzz run ws_uri_parser`, `cargo fuzz run protobuf_transport`, or
`cargo fuzz run fixture_loader`. Targets accept arbitrary bytes and focus on panic resistance
for transport URI parsing, protobuf envelope decoding, and fixture deserialization.
