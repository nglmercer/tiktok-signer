# Signing replay fixtures

Each directory is one explicit version-1 replay case. Non-empty values that could otherwise
carry session or signed transport material must use the `fixture-*` prefix; the loader rejects
raw values. Empty transport values remain empty so encoding edge cases can be represented.
`push_server` must not contain a query string.

These cases are deterministic and intentionally non-live. They exercise backend and HTTP
contracts without TikTok, a display server, browser libraries, cookies, or credentials.
