# Third-party notices

## TikTok Webcast Protobuf v3 schemas

`crates/ttl-live-proto/proto/v3/` is vendored verbatim from
[isaackogan/TikTok-Webcast-Protobuf](https://github.com/isaackogan/TikTok-Webcast-Protobuf),
path `src/slim/v3`, commit `cf7bcd49d59926b44c1c4e2632df5558bf3e8169` (2026-07-22). It is the
schema source behind the `tiktok-live-proto` npm package used by `TikTok-Live-Connector`.

Copyright (c) Isaac Kogan and contributors.

**These files are not MIT.** They are licensed under a modified AGPL-3.0 — the stock AGPL text
plus Section 7 additional permissions — reproduced unmodified in
`crates/ttl-live-proto/LICENSE.upstream`.

Section 18 grants an exception allowing the schemas and their generated bindings to be
integrated into downstream client libraries and applications, commercial or not, without those
downstream projects becoming AGPL. Section 19 withdraws that exception where the code powers a
commercial, closed-source or hosted service offered to third parties over a network — such as a
WebSocket relay or a managed API — in which case the unmodified AGPL, including its network-use
provision, applies instead. Section 21 exempts two named parties from Section 19.

Because this repository ships a server (`ttl-sign-server`) and a Dockerfile, Section 19 is the
operative clause for anyone hosting it for third parties. See
`crates/ttl-live-proto/README.md` for the full account.

The schemas are confined to `ttl-live-proto` (which sets `publish = false`), but that crate is
**not** isolated from the rest of the workspace: `ttl-sign-webview` depends on it through
`ttl-live-events` for page-message decoding, and `ttl-sign-server` depends on `ttl-sign-webview`.
The shipped server binary therefore contains AGPL-licensed schema material, which is what makes
Section 19 operative for anyone hosting it. Only `ttl-sign-core` and `ttl-live-ws` remain free of
that dependency.

## TikTokLiveSharp schema snapshot (removed)

`crates/ttl-sign-core/proto/tiktok_schema.proto`, vendored from
[frankvHoof93/TikTokLiveSharp](https://github.com/frankvHoof93/TikTokLiveSharp) under the MIT
License (Copyright (c) 2024 Frank van Hoof), was **removed** once the v3 schemas above became
the single schema source. `ttl-sign-core` no longer bundles or builds any protobuf schema.

The note is kept because the file is still reachable in this repository's git history.
