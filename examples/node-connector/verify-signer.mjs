// Verify that `tiktok-live-connector` can use this project as its sign server.
//
// The Node client normally signs through Euler Stream. Pointing `SignConfig.basePath` at
// the local server makes it call our `GET /webcast/rooms/{room_id}/connect` instead, so the
// signature is produced by the WebView in this repository and nothing leaves for a
// third-party signing service.
//
//   cargo run -p ttl-sign-server            # terminal 1
//   node verify-signer.mjs [username]       # terminal 2
//
// Exits 0 only when the connector reached TikTok's WebSocket through our signature.

import {
  TikTokLiveConnection,
  SignConfig,
  WebcastEvent,
  WebSocketConfigDefaults,
} from 'tiktok-live-connector';

const SIGN_SERVER = process.env.TTL_SIGN_SERVER ?? 'http://127.0.0.1:8080';
const USERNAME = process.argv[2] ?? process.env.TTL_USERNAME;
const EVENT_TIMEOUT_MS = 60_000;

if (!USERNAME) {
  console.error('usage: node verify-signer.mjs <username>   (a channel that is live now)');
  process.exit(2);
}

// Everything the connector would send to Euler Stream now goes here. The API key is
// deliberately unset: our server does not authenticate, and requiring one would defeat the
// point of self-hosting.
SignConfig.basePath = SIGN_SERVER;
SignConfig.apiKey = undefined;

// The signer returns the query TikTok's own player signed. The connector would otherwise
// merge ~27 parameters of its own and append `&version_code=270000`, sending a query that
// does not match the signature. Emptying the defaults leaves only our `route_params`.
WebSocketConfigDefaults.DEFAULT_WS_CLIENT_PARAMS = {};
WebSocketConfigDefaults.DEFAULT_WS_CLIENT_PARAMS_APPEND_PARAMETER = '';

async function checkServer() {
  const response = await fetch(`${SIGN_SERVER}/healthz`);
  if (!response.ok) throw new Error(`sign server unhealthy: HTTP ${response.status}`);
  const health = await response.json();
  console.log(`sign server: ${SIGN_SERVER}`);
  console.log(`  ready=${health.ready} signs_ok=${health.signs_ok} rejects=${health.rejects}`);
  console.log(`  user_agent=${health.user_agent}`);
}

async function main() {
  await checkServer();

  console.log(`\nconnecting to @${USERNAME} through the local signer…`);
  const connection = new TikTokLiveConnection(USERNAME, {
    // The signature is bound to the signer's own device preset, so let the server decide.
    signApiKey: undefined,
  });

  let events = 0;
  const seen = new Set();
  for (const [label, event] of [
    ['chat', WebcastEvent.CHAT],
    ['gift', WebcastEvent.GIFT],
    ['like', WebcastEvent.LIKE],
    ['member', WebcastEvent.MEMBER],
    ['follow', WebcastEvent.FOLLOW],
    ['roomUser', WebcastEvent.ROOM_USER],
  ]) {
    connection.on(event, (data) => {
      events += 1;
      seen.add(label);
      if (events <= 10) {
        // v2 hands back the decoded protobuf, so these are proto field names:
        // `content` rather than `comment`, `displayId` rather than `uniqueId`.
        const who = data?.user?.displayId ?? data?.user?.nickname ?? '-';
        const detail =
          label === 'chat' ? `: ${data.content}`
          : label === 'gift' ? `: gift ${data.giftId} ×${data.repeatCount}`
          : label === 'like' ? `: +${data.count}`
          : label === 'roomUser' ? `: ${data.total} viewers`
          : '';
        console.log(`  [${label}] @${who}${detail}`);
      }
    });
  }

  connection.on(WebcastEvent.STREAM_END, () => console.log('  stream ended'));

  const state = await connection.connect();
  console.log(`\nconnected: roomId=${state.roomId}`);
  console.log('the signature came from the local WebView, not from Euler Stream\n');

  await new Promise((resolve) => setTimeout(resolve, EVENT_TIMEOUT_MS));
  connection.disconnect();

  console.log(`\nreceived ${events} events (${[...seen].join(', ') || 'none'})`);
  if (events === 0) {
    console.error('FAILED: connected but no events arrived within the timeout');
    process.exit(1);
  }
  console.log('OK: tiktok-live-connector works against this signer');
}

main().catch((error) => {
  const message = String(error?.message ?? error);
  console.error(`\nFAILED: ${message}`);

  // Separate "the connector could not talk to us" from "we talked fine, TikTok refused".
  // Only the first is an integration problem; the second is the known architectural limit
  // below, and reaching it proves the routing and error contract already work.
  if (message.includes('502') || message.includes('silent rejection')) {
    console.error(`
This is the expected architectural limit, not a wiring problem:

  tiktok-live-connector requires a ProtoMessageFetchResult from
  GET /webcast/rooms/{room_id}/connect, which means signing TikTok's
  /webcast/im/fetch/ endpoint. TikTok answers that endpoint with HTTP 200 and an
  empty body for this signer, logged-in or not — the silent rejection this project
  documents, and the reason it switched to relaying the page's own WebSocket.

  The page relay cannot fill the gap either: the player's own /webcast/im/fetch/
  response is opaque to the page (no CORS headers), so its bytes cannot be read and
  handed to the connector.

  What this run does prove: routing, room-id extraction, headers, and the typed
  error contract are all correct — the connector reached this signer and parsed its
  response. Use live-check for a working end-to-end path.`);
  } else if (message.includes('ECONNREFUSED')) {
    console.error('\n  Is the sign server running? cargo run -p ttl-sign-server');
  }
  process.exit(1);
});
