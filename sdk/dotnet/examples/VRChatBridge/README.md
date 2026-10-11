# TikTok LIVE Activity Stage

This is source for a VRChat world example, not a native client or a UPM package.
The .NET relay connects to TikTok on the server. Udon polls a fixed finite JSON URL
and renders activity. No TikTok cookies or signing bundle are uploaded into the world.

## Local project

The configured Unity 2022.3.22f1 project on this workstation is
`/home/meme/UnityProjects/VRChatStarterWorld`. Open `Assets/Scenes/StarterWorld.unity`.
It contains a collidable stage, world spawn, LIVE activity panel, start/pause polling
buttons, and a beacon that pulses when new events arrive.

1. Start `sdk/dotnet/examples/AspNetCoreRelay/run-local.sh` from this repository.
2. Open `http://127.0.0.1:15050` and enter an active `@creator`, then Connect.
   The offline fixtures button checks all activity categories, including an unknown method without credentials.
3. The panel polls `http://127.0.0.1:15050/vrchat/live` every six seconds.
4. In VRChat enable **Allow Untrusted URLs** for a relay domain that is not on the
   VRChat allowlist. Use the VRChat SDK's Build & Test to run the world locally.

The operator page can only be accessed from localhost, with host/origin checks.
For a remote server, operate it through an SSH tunnel. Keep `TTL_SESSION_FILE` and
`TTL_BUNDLE` on the server; their paths and contents are not feed fields.

## Public or multiplayer deployment

Localhost works only when the relay and VRChat run on the same computer.
For other players or Quest, deploy the relay behind HTTPS and set `feedUrl` in the
TikTokLivePanel inspector to the fixed public URL **before building the world**.
Use `VRCHAT_ALLOWED_IPS` when a deployment can restrict reader addresses. The feed
is read-only and contains no privileged secret; do not put server credentials in
uploaded world URLs. The operator endpoints remain local-only.

## Event behavior

`/vrchat/live` returns `streamId`, `connection`, and `events`. The inner snapshot
retains the existing `chat` and `gifts` arrays and adds an ordered `activity` array.
Every accepted SDK event has an activity row with a monotonic string ID, native
method, event type, message ID, timestamp, history flag, support, flags, payload
length and bounded summary. Common chat/gift/like/member/social/viewer events have
human-readable summaries. Unknown methods are displayed with their metadata;
the world does not decode their complete protobuf payload. Full native event
representations remain accessible through the SDK and SSE endpoint.

Gift summaries preserve gift ID and repeat/combo information. The display disables
rich text for untrusted strings. Deduplication resets when the relay restarts.
Each player polls independently. This provides activity rendering, not video playback.

History and payloads are bounded by `VRCHAT_HISTORY_EVENTS` (default 256) and
`VRCHAT_MAX_PAYLOAD_BYTES` (default 65536). Old entries are removed under pressure;
`DroppedSnapshotEvents` records losses. Polling does not guarantee every event
under sustained traffic. The world processes at most the newest 256 rows per poll,
retains 512 dedup IDs and a bounded text log, and rejects JSON over 131072 characters.
Keep the relay payload limit below that world limit. Pausing stops subsequent
requests; it does not disconnect the server's LIVE session.

## Recreate the panel

Copy `TikTokLivePanel.cs` and `Editor/TikTokPanelSetup.cs` into
`Assets/TikTokBridge` and `Assets/TikTokBridge/Editor` in a VRChat Worlds project.
Import TMP Essential Resources, open a saved world scene, and run
**World Setup > Add TikTok LIVE Panel**. Existing panels are preserved.
`TikTokPanelSetup.UpgradeAndVerify` rebuilds the example panel in StarterWorld,
so configure custom URLs again after using it.

## Validation

The example has native fixture regression tests, including unknown methods and
ordered snapshots. The local project has passed UdonSharp compilation and editor
fixture display/deduplication checks. Real LIVE chat reached the relay during local
validation. Actual VRChat gameplay and world upload have not been validated.
