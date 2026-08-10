# 06 — Risks and operations

## Primary risks

- UA and query parameters diverge. Mitigate by deriving both from one `Preset`.
- TikTok detection is mistaken for a network error. Treat empty 200 bodies, empty
  `push_server`, and 200 WebSocket handshakes as typed rejections.
- Queueing makes signatures expire before the handshake. Rate-limit before signing.
- TikTok changes `webmssdk.js`. Keep the WebView bridge small and monitor rejection rates.
- Long-lived WebViews accumulate state. Recycle instances by age or signing count.
- Repeated signing from one IP triggers anti-bot limits. Keep volume low and avoid retry
  loops; this behavior may also implicate TikTok's Terms of Service.

## Instrumentation

Track signing latency, rejection rate, session age, WebView recycling, and WebSocket
handshake outcomes. Reload once when the readiness symbol disappears; mark the instance
unavailable if the problem persists.

## Session handling

A session file contains real account cookies and must remain outside the repository with
mode 0600. The login example polls `sessionid`, and the server can load either
`TTL_SESSION_ID` or `TTL_SESSION_FILE`. Logs redact cookie values.

One WebView instance owns one cookie session. Do not share it between rooms.
