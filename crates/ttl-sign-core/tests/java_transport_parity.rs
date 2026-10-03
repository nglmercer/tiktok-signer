use ttl_sign_core::proto::PushFrame;
fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
#[test]
fn node_oracle_transport_semantics() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/events/transport-parity.json"
    ))
    .unwrap();
    let decode = |key: &str| PushFrame::decode(&hex(fixture[key].as_str().unwrap())).unwrap();
    let incoming = decode("incoming");
    assert_eq!(
        incoming.log_id.to_string(),
        fixture["logId"].as_str().unwrap()
    );
    for (key, generated) in [
        ("ack", incoming.ack("fixture-ext")),
        ("ackEmpty", incoming.ack("")),
        (
            "enter",
            PushFrame::enter_room(fixture["roomId"].as_str().unwrap().parse().unwrap()),
        ),
    ] {
        let expected = decode(key);
        assert_eq!(generated.payload_type, expected.payload_type);
        assert_eq!(generated.payload_encoding, expected.payload_encoding);
        assert_eq!(generated.log_id, expected.log_id);
        assert_eq!(generated.payload, expected.payload);
    }
    // Rust carries a sequence counter; Node/Java omit it (proto3 default zero).
    let expected = decode("heartbeat");
    let generated = PushFrame::heartbeat(fixture["roomId"].as_str().unwrap().parse().unwrap(), 0);
    assert_eq!(generated.payload_type, expected.payload_type);
    assert_eq!(generated.payload, expected.payload);
}
