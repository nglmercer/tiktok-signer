#!/usr/bin/env node

// Read-only structural inspection for a locally downloaded TikTok webmssdk bundle.
// It emits hashes, VM entry offsets, and bytecode metadata; it never emits source,
// decrypted strings, signed URLs, cookies, or signature values.

import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { inflateRawSync } from "node:zlib";

const usage = "usage: node scripts/inspect-webmssdk.mjs <webmssdk.js>";
if (process.argv.length !== 3) {
  console.error(usage);
  process.exit(64);
}

const path = process.argv[2];
const source = await readFile(path);
const text = source.toString("utf8");
const payload = text.match(
  /var C=\{\},I=dwInfl\.dwAbA\(D\("([A-Za-z0-9+/=]+)"\)\)/,
);
if (!payload) {
  throw new Error("unsupported bundle: compressed VM bytecode was not found");
}

const compressedBytecode = Buffer.from(payload[1], "base64");
const bytecode = inflateRawSync(compressedBytecode);
const vmFunctions = new Map();
for (const match of text.matchAll(
  /function ([A-Za-z_$][\w$]*)\([^)]*\)\{return L\((\d+),t,this,arguments,0,(\d+)\)\}/g,
)) {
  vmFunctions.set(match[1], {
    offset: Number(match[2]),
    frame_size: Number(match[3]),
  });
}

const exportedEntryPoints = {};
for (const match of text.matchAll(
  /prototype\.(frontierSign|registerWsSigner)=([A-Za-z_$][\w$]*)/g,
)) {
  const entry = vmFunctions.get(match[2]);
  if (entry) exportedEntryPoints[match[1]] = entry;
}
for (const match of text.matchAll(
  /r\.(init|report|setTTWebid|setTTWebidV2|setTTWid)=function\([^)]*\)\{return L\(([\deE+.]+),t,this,arguments,0,(\d+)\)\}/g,
)) {
  exportedEntryPoints[match[1]] = {
    offset: Number(match[2]),
    frame_size: Number(match[3]),
  };
}

const markers = [
  "byted_acrawler",
  "frontierSign",
  "registerWsSigner",
  "msToken",
  "webmssdk",
].filter((marker) => text.includes(marker));

console.log(
  JSON.stringify(
    {
      inspector_version: 1,
      source: {
        sha256: sha256(source),
        bytes: source.length,
      },
      vm: {
        compressed_bytecode_bytes: compressedBytecode.length,
        bytecode_bytes: bytecode.length,
        bytecode_sha256: sha256(bytecode),
        initial_offset: 0,
        exported_entry_points: exportedEntryPoints,
      },
      markers,
    },
    null,
    2,
  ),
);

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}
