// Measure anonymous guest identity on the direct live WebSocket.
//
//   npm --prefix packages/tiktok-live run build
//   node scripts/headless/ws-guest-probe.mjs /tmp/webmssdk.js <room_id> [seconds]
//
// The probe deliberately starts from an empty jar. `--mode empty` is the control; `--mode guest`
// performs the same `/live` bootstrap as `find-live.mjs` and then uses the resulting jar for both
// the signer and the WebSocket. `--remove NAME` and `--only NAME` are conservative cookie-bisect
// helpers. They print names, never values.
//
// AUTHORIZED USE ONLY: use one public room you are authorized to test against. This script does
// not read the stored session file and never prints cookies, tokens, signatures, or URLs.

let session;
let player;
let signerModule;
let framesModule;
try {
  // The repository is TypeScript-first now. Build the package before running this probe so all
  // helpers come from the package's single implementation rather than a second parser/serializer.
  [session, player, signerModule, framesModule] = await Promise.all([
    import('../../packages/tiktok-live/dist/session.js'),
    import('../../packages/tiktok-live/dist/player.js'),
    import('../../packages/tiktok-live/dist/signer.js'),
    import('../../packages/tiktok-live/dist/frames.js'),
  ]);
} catch {
  console.error('package build missing; run: npm --prefix packages/tiktok-live run build');
  process.exitCode = 2;
  process.exit();
}

const {
  USER_AGENT,
  absorbCookies,
  cookieHeader,
  parseCookies,
} = session;
const {
  PATH,
  SOCKET_HOST,
  enterRoomFrame,
  heartbeatFrame,
  socketConfig,
  socketQuery,
} = player;
const { PRODUCT, Signer } = signerModule;
const { ackFrame, decodeBatch, decodePushFrame, decompress } = framesModule;

const args = process.argv.slice(2);
const bundlePath = args.shift();
const roomId = args.shift();
const seconds = Number(args.shift() || 20);
const mode = takeValue(args, '--mode') || 'guest';
const endpoint = takeValue(args, '--endpoint') || 'https://www.tiktok.com/live';
const bootstrapOnly = takeFlag(args, '--bootstrap-only');
const additions = takeAllValues(args, '--add-cookie');
const removals = takeAllValues(args, '--remove');
const only = takeAllValues(args, '--only');

if (!bundlePath || !roomId || !Number.isFinite(seconds) || seconds <= 0
  || !['empty', 'guest'].includes(mode)
  || args.length > 0 || additions.some((raw) => !raw.includes('='))
  || removals.some((name) => !name) || only.some((name) => !name)) {
  console.error(
    'usage: node scripts/headless/ws-guest-probe.mjs <webmssdk.js> <room_id> [seconds] '
      + '[--mode empty|guest] [--endpoint URL] [--bootstrap-only] '
      + '[--add-cookie NAME=VALUE ...] [--remove NAME ...] [--only NAME ...]',
  );
  process.exit(2);
}

const report = {
  mode,
  roomId,
  bootstrapStatus: null,
  bootstrapCookieNames: [],
  addedCookieNames: [],
  cookieNames: [],
  sessionidPresent: false,
  webidSource: 'fallback',
  signerAndSocketCookieIdentity: 'shared',
  handshake: 'not_attempted',
  openMs: null,
  enterRoom: 'not_attempted',
  frames: 0,
  validPushFrames: 0,
  eventFrames: 0,
  decodedEvents: 0,
  invalidFrames: 0,
  bytes: 0,
  closeCode: null,
  elapsedMs: 0,
  classification: 'UNKNOWN',
};

const jar = new Map();
if (mode === 'guest') {
  try {
    const response = await fetch(endpoint, {
      headers: {
        'user-agent': USER_AGENT,
        'accept-language': 'en-US,en;q=0.9',
      },
      signal: AbortSignal.timeout(15_000),
    });
    report.bootstrapStatus = response.status;
    report.bootstrapCookieNames = unique(absorbCookies(jar, response)).sort();
    await response.arrayBuffer();
  } catch {
    report.bootstrapStatus = 'request_failed';
    report.classification = 'RATE_LIMIT_OR_VERIFICATION';
    printReport(report);
    process.exit(0);
  }
} else {
  report.bootstrapStatus = 'skipped';
}

for (const raw of additions) {
  for (const [name, value] of parseCookies(raw)) {
    jar.set(name, value);
    report.addedCookieNames.push(name);
  }
}
report.addedCookieNames = unique(report.addedCookieNames).sort();

const originalNames = new Set(jar.keys());
for (const name of removals) jar.delete(name);
if (only.length) {
  const keep = new Set(only);
  for (const name of [...jar.keys()]) if (!keep.has(name)) jar.delete(name);
}

report.cookieNames = [...jar.keys()].sort();
report.sessionidPresent = jar.has('sessionid');
if (jar.has('tt_webid_v2')) report.webidSource = 'tt_webid_v2';
else if (jar.has('tt_webid')) report.webidSource = 'tt_webid';
report.removedCookieNames = [...originalNames].filter((name) => !jar.has(name)).sort();

if (bootstrapOnly) {
  report.classification = jar.size ? 'BOOTSTRAP_ONLY' : 'BOOTSTRAP_NO_COOKIES';
  printReport(report);
  process.exit(0);
}

if (mode === 'guest' && jar.size === 0) {
  report.classification = 'BOOTSTRAP_NO_COOKIES';
  printReport(report);
  process.exit(0);
}

const cookie = cookieHeader(jar);
const config = socketConfig({
  roomId,
  deviceId: jar.get('tt_webid_v2') || jar.get('tt_webid') || '7300000000000000001',
});
const query = socketQuery(config);

// The signer receives the exact cookie header that will be supplied to the socket. Keep these as
// two references to the same immutable string so a probe cannot accidentally sign one identity
// and present another during the handshake.
const signer = await Signer.create({
  bundlePath,
  userAgent: USER_AGENT,
  cookie,
});
const signedUrl = signer.sign(
  `${config.socketHost}${PATH.wsReuseSupplement}?${query}`,
  PRODUCT.ws,
);

const WebSocketImpl = globalThis.WebSocket;
if (typeof WebSocketImpl !== 'function') {
  report.classification = 'UNKNOWN';
  console.error('this Node runtime has no global WebSocket');
  printReport(report);
  process.exit(2);
}

const started = Date.now();
let heartbeat;
let socket;
try {
  // Node's built-in WebSocket accepts this header-bearing options object. No URL is printed.
  socket = new WebSocketImpl(signedUrl, {
    headers: {
      cookie,
      'user-agent': USER_AGENT,
      origin: 'https://www.tiktok.com',
    },
  });
  socket.binaryType = 'arraybuffer';
} catch {
  report.handshake = 'constructor_error';
  report.classification = 'WS_HANDSHAKE_REJECTED';
  report.elapsedMs = Date.now() - started;
  printReport(report);
  process.exit(0);
}

await new Promise((resolve) => {
  let done = false;
  let opened = false;
  let timer;

  const finish = () => {
    if (done) return;
    done = true;
    clearInterval(heartbeat);
    clearTimeout(timer);
    report.elapsedMs = Date.now() - started;
    if (!opened) {
      report.handshake = 'rejected';
      report.classification = 'WS_HANDSHAKE_REJECTED';
    } else if (report.enterRoom !== 'accepted') {
      report.classification = 'ENTER_ROOM_FAILURE';
    } else if (!report.eventFrames) {
      report.classification = 'WS_OPEN_NO_EVENTS';
    } else {
      report.classification = 'SUCCESS';
    }
    printReport(report);
    resolve();
  };

  timer = setTimeout(() => {
    try { socket.close(); } catch { /* already closed */ }
    finish();
  }, seconds * 1000);

  socket.addEventListener('open', () => {
    opened = true;
    report.handshake = 'opened';
    report.openMs = Date.now() - started;
    try {
      socket.send(enterRoomFrame({ roomId: config.roomId, identity: config.identity }));
      report.enterRoom = 'sent';
      heartbeat = setInterval(() => {
        try { socket.send(heartbeatFrame(config.roomId)); } catch { /* close handler reports it */ }
      }, Number(config.heartbeatDuration) || 10_000);
    } catch {
      report.enterRoom = 'send_error';
      report.classification = 'ENTER_ROOM_FAILURE';
    }
  });

  socket.addEventListener('message', async (event) => {
    const body = await asBytes(event.data);
    report.frames += 1;
    report.bytes += body.byteLength;
    try {
      const frame = decodePushFrame(body);
      report.validPushFrames += 1;
      if (frame.payloadType === 'im_enter_room_resp') report.enterRoom = 'accepted';
      if (!frame.carriesEvents) return;
      report.eventFrames += 1;
      const batch = decodeBatch(decompress(frame));
      report.decodedEvents += batch.messages.length;
      if (batch.needAck) socket.send(ackFrame(frame, batch.internalExt));
    } catch {
      report.invalidFrames += 1;
    }
  });

  socket.addEventListener('error', () => {
    // The close event is the useful observation. Do not print the runtime's error text because it
    // can include request details on some WebSocket implementations.
  });

  socket.addEventListener('close', (event) => {
    report.closeCode = Number(event.code);
    finish();
  });
});

function takeValue(values, flag) {
  const at = values.indexOf(flag);
  if (at < 0) return undefined;
  const value = values[at + 1];
  values.splice(at, 2);
  return value;
}

function takeFlag(values, flag) {
  const at = values.indexOf(flag);
  if (at < 0) return false;
  values.splice(at, 1);
  return true;
}

function takeAllValues(values, flag) {
  const found = [];
  let at = values.indexOf(flag);
  while (at >= 0) {
    const value = values[at + 1];
    if (value !== undefined) found.push(value);
    values.splice(at, 2);
    at = values.indexOf(flag);
  }
  return found;
}

async function asBytes(data) {
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  if (data && typeof data.arrayBuffer === 'function') return new Uint8Array(await data.arrayBuffer());
  throw new TypeError('unsupported WebSocket message body');
}

function unique(values) {
  return [...new Set(values)];
}

function printReport(value) {
  console.log(JSON.stringify(value, null, 2));
}
