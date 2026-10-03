use ttl_live_discovery::{interpret_room_lookup, DiscoveryError};
#[test]
fn shared_presence_cases() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/events/presence/cases.json")).unwrap();
    for case in fixtures.as_array().unwrap() {
        let result = interpret_room_lookup(
            case["id"].as_str().unwrap(),
            case["httpStatus"].as_u64().unwrap() as u16,
            case["body"].as_str().unwrap(),
        );
        let status = match result {
            Ok(room) => {
                if room.is_live() {
                    "LIVE"
                } else {
                    "OFFLINE"
                }
            }
            Err(DiscoveryError::NoRoom(_)) => "OFFLINE",
            Err(_) => "UNKNOWN",
        };
        assert_eq!(
            status,
            case["expected"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
}
