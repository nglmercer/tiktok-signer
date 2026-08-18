// Sign one URL under the pinned profile, in V8, through the shipped bootstrap.
//
//   node scripts/headless/tools/sign-pinned.mjs <webmssdk.js> <url> [fetch|frontier|ws]
//
// This exists to be the other half of a comparison. `crates/ttl-sign-embedded` runs
// `crates/ttl-sign-embedded/bootstrap.js` in QuickJS; this runs the same file in V8, under the same
// frozen clock and entropy, so the two outputs must be byte-identical. Its integration test drives
// exactly this, which is what turns "the embedded signer seems to work" into a check that fails
// when an engine, or the sandbox, starts behaving differently.
//
// The bootstrap is loaded into a bare `vm` context on purpose: with Node's globals kept out, the
// only platform the sandbox gets is the one the bootstrap installs for itself — the same platform
// QuickJS gets.
//
// Only the signed URL is printed, on stdout, with nothing else.

import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(HERE, '..', '..', '..');
const BOOTSTRAP = path.join(ROOT, 'crates', 'ttl-sign-embedded', 'bootstrap.js');

const [, , bundlePath, url, product = 'fetch'] = process.argv;
if (!bundlePath || !url) {
  console.error('usage: node scripts/headless/tools/sign-pinned.mjs <webmssdk.js> <url> [product]');
  process.exit(2);
}

const context = vm.createContext({});
vm.runInContext(fs.readFileSync(BOOTSTRAP, 'utf8'), context);

const prepared = JSON.parse(
  vm.runInContext('ttlPrepare', context)(
    fs.readFileSync(bundlePath, 'utf8'),
    JSON.stringify({ pinned: true }),
  ),
);
if (prepared.error) {
  console.error(prepared.error);
  process.exit(1);
}

const signed = JSON.parse(vm.runInContext('ttlSignUrl', context)(url, product));
if (signed.error) {
  console.error(signed.error);
  process.exit(1);
}
process.stdout.write(signed.signed);
