// Verify that `tiktok-live-connector` can use this project as its sign server.
//
// The Node client normally signs through Euler Stream. Pointing `SignConfig.basePath` at
// the local server makes it call our `GET /webcast/rooms/{room_id}/connect` instead, so the
// signature is produced by this repository's headless signer and nothing leaves for a
// third-party signing service.
//
//   cargo run -p ttl-sign-server            # terminal 1
//   bun run verify-signer.ts <username>     # terminal 2  (or: npx tsx verify-signer.ts)
//
// TypeScript is not decoration here: the connector types its event handlers, so the payload
// of each event is inferred from the schema. An earlier JavaScript version of this file read
// `data.comment` and printed `undefined` at runtime — the field is `content`. `bun run
// typecheck` catches that class of mistake before a live channel is involved.
//
// Exits 0 only when the connector reached TikTok's WebSocket through our signature.

import {
  TikTokLiveConnection,
  SignConfig,
  WebcastEvent,
  WebSocketConfigDefaults,
} from 'tiktok-live-connector';

const SIGN_SERVER: string = process.env.TTL_SIGN_SERVER ?? 'http://127.0.0.1:8080';
const USERNAME: string | undefined = process.argv[2] ?? process.env.TTL_USERNAME;
const EVENT_TIMEOUT_MS = 60_000;
const MAX_PRINTED = 10;

/** Subset of `GET /healthz` this check reports. */
interface SignServerHealth {
  ready: boolean;
  signs_ok: number;
  rejects: number;
  user_agent: string;
}

if (!USERNAME) {
  console.error('usage: bun run verify-signer.ts <username>   (a channel that is live now)');
  process.exit(2);
}

// Everything the connector would send to Euler Stream now goes here. The API key is
// deliberately unset: our server does not authenticate, and requiring one would defeat the
// point of self-hosting.
SignConfig.basePath = SIGN_SERVER;
SignConfig.apiKey = undefined;

// The signer returns the query TikTok's own player signed, as `route_params`. The connector
// would otherwise merge ~27 parameters of its own and append `&version_code=270000`, sending
// a query with two sets of browser fields in it. Emptying the defaults leaves ours.
//
// The connector still rebuilds the query from that map — reordered, re-encoded, and with the
// duplicated `version_code` collapsed. Measured 2026-08-18: the socket accepts that and pushes
// frames, so the signature does not have to survive as bytes, only as values.
//
// The declared type pins each known key, so replacing the whole record needs a cast; the
// runtime contract is only "an object of query parameters".
WebSocketConfigDefaults.DEFAULT_WS_CLIENT_PARAMS =
  {} as typeof WebSocketConfigDefaults.DEFAULT_WS_CLIENT_PARAMS;
WebSocketConfigDefaults.DEFAULT_WS_CLIENT_PARAMS_APPEND_PARAMETER = '';

async function checkServer(): Promise<void> {
  const response = await fetch(`${SIGN_SERVER}/healthz`);
  if (!response.ok) throw new Error(`sign server unhealthy: HTTP ${response.status}`);
  const health = (await response.json()) as SignServerHealth;
  console.log(`sign server: ${SIGN_SERVER}`);
  console.log(`  ready=${health.ready} signs_ok=${health.signs_ok} rejects=${health.rejects}`);
  console.log(`  user_agent=${health.user_agent}`);
}

async function main(): Promise<void> {
  await checkServer();

  console.log(`\nconnecting to @${USERNAME} through the local signer…`);
  // Options are required by the constructor. `useMobile: false` selects the plain
  // unauthenticated variant of the session union, which is what a guest listener wants.
  const connection = new TikTokLiveConnection(USERNAME!, { useMobile: false });

  let events = 0;
  const seen = new Set<string>();

  /** Print the first few events, then keep counting quietly. */
  const record = (label: string, detail: string): void => {
    events += 1;
    seen.add(label);
    if (events <= MAX_PRINTED) console.log(`  [${label}] ${detail}`);
  };

  // Each handler's payload type comes from the connector's event map, so these field names
  // are checked against the protobuf schema rather than guessed.
  connection.on(WebcastEvent.CHAT, (data) => {
    record('chat', `@${data.user?.displayId ?? '-'}: ${data.content}`);
  });
  connection.on(WebcastEvent.GIFT, (data) => {
    record('gift', `@${data.user?.displayId ?? '-'}: gift ${data.giftId} ×${data.repeatCount}`);
  });
  connection.on(WebcastEvent.LIKE, (data) => {
    record('like', `@${data.user?.displayId ?? '-'}: +${data.count}`);
  });
  connection.on(WebcastEvent.MEMBER, (data) => {
    record('member', `@${data.user?.displayId ?? '-'} joined`);
  });
  connection.on(WebcastEvent.FOLLOW, (data) => {
    record('follow', `@${data.user?.displayId ?? '-'}`);
  });
  connection.on(WebcastEvent.ROOM_USER, (data) => {
    record('roomUser', `${data.total} viewers`);
  });
  connection.on(WebcastEvent.STREAM_END, () => console.log('  stream ended'));

  const state = await connection.connect();
  console.log(`\nconnected: roomId=${state.roomId}`);
  console.log('the signature came from the local signer, not from Euler Stream\n');

  await new Promise((resolve) => setTimeout(resolve, EVENT_TIMEOUT_MS));
  connection.disconnect();

  console.log(`\nreceived ${events} events (${[...seen].join(', ') || 'none'})`);
  if (events === 0) {
    console.error('FAILED: connected but no events arrived within the timeout');
    process.exit(1);
  }
  console.log('OK: tiktok-live-connector works against this signer');
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`\nFAILED: ${message}`);

  // Separate "the connector could not talk to us" from "we talked fine, TikTok refused".
  if (message.includes('502') || message.includes('silent rejection')) {
    console.error(`
The sign server reached TikTok but the signature was refused. Check its log: a room that
ended between discovery and connection produces this, as does a stale captured URI.`);
  } else if (message.includes('ECONNREFUSED')) {
    console.error('\n  Is the sign server running? cargo run -p ttl-sign-server');
  }
  process.exit(1);
});
