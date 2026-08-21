// Just enough protobuf to read what the socket sends.
//
// The schema is known and small — a push frame, a batch envelope, and six event messages — so this
// is a wire reader rather than generated protobuf code: no runtime dependency and nothing to
// regenerate when a message grows a field it does not care about. Unknown fields are skipped,
// which is what makes that safe.
//
// The *encoders* live in `player.ts`, next to the constants they serialise, because those bytes
// are covered by a signature and belong beside the transcription of the player that produces them.

export const WIRE = Object.freeze({
  varint: 0,
  fixed64: 1,
  lengthDelimited: 2,
  fixed32: 5,
});

export type WireValue = bigint | Uint8Array;

export interface WireField {
  number: number;
  wire: number;
  value: WireValue;
}

export type FieldReader<T = unknown> = (value: WireValue) => T;
export type MessageShape = Readonly<Record<number, readonly [name: string, reader: FieldReader]>>;

/// Walk the fields of one message, yielding `{ number, wire, value }`.
///
/// `value` is a `bigint` for varints and fixed-width fields, and a `Uint8Array` view for
/// length-delimited ones. Views, not copies: a batch is a few hundred kilobytes and every event
/// payload inside it would otherwise be copied twice.
export function* fields(buffer: ArrayBuffer | ArrayBufferView): Generator<WireField> {
  const bytes = buffer instanceof Uint8Array
    ? buffer
    : ArrayBuffer.isView(buffer)
      ? new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength)
      : new Uint8Array(buffer);
  let at = 0;
  while (at < bytes.length) {
    const [tag, afterTag] = varint(bytes, at);
    if (afterTag === at) return;
    const number = Number(tag >> 3n);
    const wire = Number(tag & 7n);
    if (number === 0) return;
    at = afterTag;
    switch (wire) {
      case WIRE.varint: {
        const [value, next] = varint(bytes, at);
        if (next === at) return;
        yield { number, wire, value };
        at = next;
        break;
      }
      case WIRE.fixed64: {
        if (at + 8 > bytes.length) return;
        yield { number, wire, value: view64(bytes, at) };
        at += 8;
        break;
      }
      case WIRE.lengthDelimited: {
        const [length, next] = varint(bytes, at);
        const end = next + Number(length);
        // A truncated frame is a transport failure, not an event: stop rather than decode noise.
        if (end > bytes.length) return;
        yield { number, wire, value: bytes.subarray(next, end) };
        at = end;
        break;
      }
      case WIRE.fixed32: {
        if (at + 4 > bytes.length) return;
        yield { number, wire, value: BigInt(new DataView(bytes.buffer, bytes.byteOffset + at, 4).getUint32(0, true)) };
        at += 4;
        break;
      }
      default:
        // Groups (3, 4) were removed in proto3 and nothing here emits them.
        return;
    }
  }
}

function varint(bytes: Uint8Array, at: number): readonly [bigint, number] {
  let result = 0n;
  let shift = 0n;
  let index = at;
  while (index < bytes.length) {
    const byte = bytes[index];
    if (byte === undefined) break;
    index += 1;
    result |= BigInt(byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) return [result, index];
    shift += 7n;
    if (shift >= 70n) break; // malformed: a varint is at most ten bytes
  }
  return [result, at];
}

function view64(bytes: Uint8Array, at: number): bigint {
  return new DataView(bytes.buffer, bytes.byteOffset + at, 8).getBigUint64(0, true);
}

const decoder = new TextDecoder();

/// A length-delimited field as UTF-8.
export const asString = (value: WireValue): string =>
  (value instanceof Uint8Array ? decoder.decode(value) : '');

/// A varint as a JavaScript number, read as the signed integer protobuf encodes.
///
/// A negative `int64` travels as its two's complement, so reading it unsigned turns `-1` into
/// 18446744073709551615 — a number that looks like data. Ids beyond 2^53 stay exact as strings;
/// see `asId`.
export const asNumber = (value: WireValue): number =>
  (typeof value === 'bigint' ? Number(BigInt.asIntN(64, value)) : 0);

/// A count, clamped at zero. These fields are never negative in practice; one that is means a
/// misread field, and a huge wrapped number would be far harder to notice than a 0.
export const asCount = (value: WireValue): number => {
  const number = asNumber(value);
  return number > 0 ? number : 0;
};

/// A 64-bit id as a decimal string. Room and user ids exceed `Number.MAX_SAFE_INTEGER`, and one
/// rounded digit is a different user.
export const asId = (value: WireValue): string =>
  (typeof value === 'bigint' ? BigInt.asIntN(64, value).toString() : '0');

/// Read a message into an object by field number.
///
/// `shape` maps a field number to `[name, reader]`. Repeated fields are collected when the name
/// ends with `[]`.
export function read<T extends object>(
  buffer: ArrayBuffer | ArrayBufferView,
  shape: MessageShape,
): Partial<T> {
  const out: Record<string, unknown> = {};
  for (const { number, value } of fields(buffer)) {
    const entry = shape[number];
    if (!entry) continue;
    const [name, reader] = entry;
    if (name.endsWith('[]')) {
      const key = name.slice(0, -2);
      const values = (out[key] ??= []) as unknown[];
      values.push(reader(value));
    } else {
      out[name] = reader(value);
    }
  }
  return out as Partial<T>;
}
