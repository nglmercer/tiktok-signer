# 10 — Authorized API feasibility

Question this answers: can any **officially authorized** TikTok API replace the WebView-signed
`/webcast/im/fetch/` path this project reverse-engineers, so that native signing convergence
stops being on the critical path?

Short answer: **no official surface exposes real-time LIVE webcast events** (chat, gifts, likes,
member joins, room state). Every "TikTok LIVE API" that does is a third-party reverse-engineered
library or a paid managed service in the same category as this project — not a TikTok-sanctioned
product. The authorized surfaces cover profiles, published videos, and content posting, none of
which carry the live event stream.

## Authorized surfaces and what they cover

| Surface | Auth | Covers | Live events? |
|---|---|---|---|
| **Research API** | Vetted application; non-commercial | Public accounts (profiles, followers/following, liked/pinned/reposted videos), content, and shops | No |
| **Display API** | User OAuth (Login Kit) | `GET /v2/user/info/` (`open_id`, `union_id`, `avatar_url`, `display_name`); `POST /v2/video/list/` and `/v2/video/query/` (published video metadata) | No |
| **Login Kit / Content Posting API** | User OAuth | Identity, and publishing videos on the user's behalf | No |

### Research API eligibility (narrow)

The Research API is not generally available. Access is limited to:

- academic institutions in the US, EEA, UK, or Switzerland;
- EU-based non-profit / independent research organizations (beta, select countries);
- Brazilian institutions studying online youth safety.

Applicants must show research expertise, independence from commercial interests, a non-commercial
basis, disclosed funding, a proportionate proposal, ethics review, and a data-security commitment.
A connector shipping LIVE event data to end users does not fit this program, and even if admitted,
the data it returns is not live webcast events.

### Display API (closest thing to "official", still wrong shape)

The Display API requires each end user to authorize your app via Login Kit. It returns that user's
own profile and their published (VOD) video metadata — `id`, `title`, `duration`,
`cover_image_url` (expiring), `share_url`, `embed_link`. It has no live-stream, webcast, or
real-time event capability, and it only ever sees the authorizing user's own account, not an
arbitrary creator's live room.

## Why this matters for the signing workstream

There is no authorized shortcut that removes the WebView oracle. The live event stream this project
targets is reachable, today, only through the signed `/webcast/im/fetch/` + WebSocket path, which is
exactly why the WebView remains the reference signer and why native signing stays unsupported until
it converges (see [09](09-signing-research.md)). If the goal is durable access to live events, the
realistic authorized routes are business/partnership channels with TikTok, not a public API — and
those are a product/legal decision, not an engineering one.

## Practical implication

- Keep the WebView oracle as the supported signer. Do not treat any public API as a drop-in
  replacement for it — none exposes the data.
- If a use case only needs **published** content or profile data (not live events), the Display API
  (per-user OAuth) or the Research API (if eligible) is the correct, sanctioned path and avoids
  signing entirely. Route those needs there rather than through the webcast signer.
- Anything requiring the live event stream at scale, sanctioned, points to a TikTok partnership
  conversation rather than an API integration.

## Sources

- [TikTok Research API — access & eligibility](https://developers.tiktok.com/products/research-api/)
- [TikTok Display API — getting started](https://developers.tiktok.com/doc/display-api-get-started/)
- [TikTok Login Kit overview](https://developers.tiktok.com/doc/login-kit-overview)
- [TikTok scopes overview](https://developers.tiktok.com/doc/scopes-overview)

_Reviewed August 2026. The absence of an official live-events API is a point-in-time finding; TikTok's
developer surface changes, so re-check the developer portal before acting on it._
