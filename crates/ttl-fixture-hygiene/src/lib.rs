//! Enforced artifact-hygiene gate for committed fixtures.
//!
//! The research corpus is only allowed to contain sanitized, synthetic material: hashes,
//! byte lengths, field names, field ordering, fixed-vocabulary labels, and synthetic test
//! identifiers. This crate refuses a fixture tree that has accidentally captured live
//! secrets — session cookies, reusable signed URLs, or raw signature values — so the mistake
//! fails CI instead of landing in git history.
//!
//! # Sanitization of the gate itself
//!
//! A [`Finding`] never carries the matched secret. It reports the rule, the file, a line
//! number, the (non-secret) field or cookie name, and the byte length of the offending value.
//! That is enough to locate and fix the leak without the gate's own output becoming a second
//! copy of it.
//!
//! # What it deliberately does not do
//!
//! It does not try to classify a bare 19-digit integer as a "real" device or room id: the
//! corpus legitimately uses synthetic 19-digit ids, and a raw device id only becomes
//! transport-sensitive once embedded in a signed URL — which [`Rule::SignedUrlToRealHost`]
//! already catches. Detecting secrets is a one-way filter: a clean scan is necessary, not
//! sufficient, evidence of hygiene.

use std::fmt;
use std::path::{Path, PathBuf};

use regex::Regex;

/// A single hygiene violation, described without reproducing the secret it found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub path: PathBuf,
    /// 1-based line number for text files; `None` for binary files (no line concept).
    pub line: Option<usize>,
    pub rule: Rule,
    /// Non-secret name involved (cookie name, query/field key), when the rule has one.
    pub field: Option<String>,
    /// Byte length of the offending value. The value itself is never retained.
    pub value_len: usize,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let where_ = match self.line {
            Some(line) => format!("{}:{}", self.path.display(), line),
            None => format!("{} (binary)", self.path.display()),
        };
        match &self.field {
            Some(field) => write!(
                f,
                "{where_}: {} — `{field}` with non-synthetic value (len {})",
                self.rule.id(),
                self.value_len
            ),
            None => write!(f, "{where_}: {} (len {})", self.rule.id(), self.value_len),
        }
    }
}

/// The classes of leak the gate refuses. Bounded on purpose: every rule targets a concrete,
/// high-signal transport secret rather than a fuzzy "looks random" heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// A known sensitive cookie (`msToken`, `ttwid`, `sessionid`, …) carrying a value that is
    /// not a recognizable synthetic placeholder.
    SensitiveCookieValue,
    /// A signing parameter (`X-Gnarly`, `X-Dynosaur`, `_signature`, or `X-Bogus` other than the
    /// confirmed literal `1`) present as a query parameter with a non-trivial value.
    SignedQueryParameter,
    /// A raw signature/signing-field value stored under a signing key in a JSON/kv fixture.
    SignedFieldRawValue,
    /// A URL to a real TikTok/ByteDance host whose query string carries a signing parameter —
    /// i.e. a reusable signed URL.
    SignedUrlToRealHost,
    /// A VM research artifact retaining raw operand values, operand byte strings, or string/
    /// bytecode table contents instead of the sanitized widths, slots, and shapes.
    VmOperandValue,
}

impl Rule {
    pub fn id(self) -> &'static str {
        match self {
            Rule::SensitiveCookieValue => "sensitive-cookie-value",
            Rule::SignedQueryParameter => "signed-query-parameter",
            Rule::SignedFieldRawValue => "signed-field-raw-value",
            Rule::SignedUrlToRealHost => "signed-url-to-real-host",
            Rule::VmOperandValue => "vm-operand-value",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Rule::SensitiveCookieValue => {
                "a session/anti-bot cookie was committed with a non-synthetic value"
            }
            Rule::SignedQueryParameter => {
                "a signing query parameter was committed with a captured value"
            }
            Rule::SignedFieldRawValue => {
                "a raw signature/signing-field value was committed instead of a digest+length"
            }
            Rule::SignedUrlToRealHost => {
                "a reusable signed URL to a real TikTok/ByteDance host was committed"
            }
            Rule::VmOperandValue => {
                "a VM artifact committed raw operand values or table contents instead of \
                 operand widths, opcode slots, and value shapes"
            }
        }
    }
}

/// Cookie names that must never appear with a real value in a committed fixture.
const SENSITIVE_COOKIES: &[&str] = &[
    "msToken",
    "ttwid",
    "sessionid",
    "sessionid_ss",
    "sid_tt",
    "sid_guard",
    "uid_tt",
    "uid_tt_ss",
    "tt_csrf_token",
    "tt_chain_token",
    "passport_csrf_token",
    "passport_auth_status",
    "odin_tt",
    "s_v_web_id",
    "store-idc",
    "store-country-code",
];

/// Host fragments that identify a real (non-`.invalid`) TikTok/ByteDance endpoint.
const REAL_HOSTS: &[&str] = &[
    "tiktok.com",
    "tiktokv.com",
    "tiktokcdn",
    "ttwstatic.com",
    "byteoversea.com",
    "bytedance",
    "musical.ly",
    "ibytedtos.com",
    "ipstatp.com",
];

/// Values recognized as deliberately synthetic. Case-insensitive substring match.
const SYNTHETIC_MARKERS: &[&str] = &[
    "fixture",
    "synthetic",
    "placeholder",
    "example",
    "sample",
    "do-not-serialize",
    "never-observed",
    "redacted",
    "sanitized",
    "dummy",
    "test-token",
    "test_token",
];

/// Is `value` a recognizable synthetic placeholder rather than a captured secret?
///
/// A value is treated as synthetic if it is empty, short (secrets of interest are long,
/// high-entropy strings), or contains one of the [`SYNTHETIC_MARKERS`].
pub fn is_synthetic_value(value: &str) -> bool {
    let value = value.trim();
    if value.len() < 12 {
        return true;
    }
    let lower = value.to_ascii_lowercase();
    SYNTHETIC_MARKERS.iter().any(|m| lower.contains(m))
}

struct Patterns {
    /// `name=value` and JSON `["name", "value"]` / `"name": "value"` cookie forms.
    cookie_kv: Regex,
    /// JSON `"key": "value"` and `["key", "value"]` for signing keys.
    signing_field: Regex,
    /// `?name=value` or `&name=value` query parameter (signing keys + X-Bogus + msToken).
    query_param: Regex,
    /// A URL token: scheme://host...[?query].
    url: Regex,
    /// A JSON key that carries raw VM operand or table contents rather than sanitized shapes.
    vm_raw_key: Regex,
    /// A hex byte string stored under a `bytes` key — raw bytecode operands.
    vm_raw_bytes: Regex,
}

impl Patterns {
    fn new() -> Self {
        // A cookie value ends at `;`, `"`, whitespace, or end. Covers `Set-Cookie` style,
        // `document.cookie`, and JSON pair encodings.
        let cookie_kv = Regex::new(
            r#"(?i)(?:"?\b(msToken|ttwid|sessionid|sessionid_ss|sid_tt|sid_guard|uid_tt|uid_tt_ss|tt_csrf_token|tt_chain_token|passport_csrf_token|passport_auth_status|odin_tt|s_v_web_id|store-idc|store-country-code)"?\s*[=:,]\s*"?)([^;"'\s,}\]]*)"#,
        )
        .expect("cookie_kv regex");

        let signing_field = Regex::new(
            r#"(?i)"(X-Gnarly|X-Dynosaur|_signature|signature|x-tt-params|X-Argus|X-Ladon|X-Khronos|X-Bogus)"\s*[:,]\s*"([^"]*)""#,
        )
        .expect("signing_field regex");

        let query_param = Regex::new(
            r#"(?i)[?&](X-Gnarly|X-Dynosaur|X-Bogus|_signature|signature|msToken|x-tt-params|X-Argus|X-Ladon|X-Khronos)=([^&"'\s]*)"#,
        )
        .expect("query_param regex");

        let url = Regex::new(r#"(?i)\b(?:https?|wss?)://[^\s"'`)\]}<>]+"#).expect("url regex");

        // Keys are matched whole (`"key":`), so sanitized neighbours such as
        // `"operand_widths"`, `"string_table_slots"`, and `"bytecode_sha256"` do not trip it.
        let vm_raw_key = Regex::new(
            r#"(?i)"(operands|operand_values|operand_examples|string_table|strings|numbers|bytecode)"\s*:"#,
        )
        .expect("vm_raw_key regex");

        let vm_raw_bytes =
            Regex::new(r#"(?i)"bytes"\s*:\s*"([0-9a-f]{8,})""#).expect("vm_raw_bytes regex");

        Self {
            cookie_kv,
            signing_field,
            query_param,
            url,
            vm_raw_key,
            vm_raw_bytes,
        }
    }
}

/// True when a signing value is the confirmed, non-secret constant `X-Bogus=1`.
fn is_confirmed_x_bogus(key: &str, value: &str) -> bool {
    key.eq_ignore_ascii_case("X-Bogus") && value.trim() == "1"
}

/// Scan raw bytes attributed to `path`. Text is scanned line-by-line; a file containing NUL
/// bytes is treated as binary and scanned as one lossy-UTF-8 unit (line = `None`).
pub fn scan_bytes(path: &Path, bytes: &[u8]) -> Vec<Finding> {
    let patterns = Patterns::new();
    let mut findings = Vec::new();

    let is_binary = bytes.contains(&0);
    if is_binary {
        let text = String::from_utf8_lossy(bytes);
        scan_line(&patterns, path, None, &text, &mut findings);
    } else {
        let text = String::from_utf8_lossy(bytes);
        for (index, line) in text.lines().enumerate() {
            scan_line(&patterns, path, Some(index + 1), line, &mut findings);
        }
    }

    findings
}

fn scan_line(
    patterns: &Patterns,
    path: &Path,
    line: Option<usize>,
    text: &str,
    findings: &mut Vec<Finding>,
) {
    // 1. Sensitive cookies with a non-synthetic value.
    for capture in patterns.cookie_kv.captures_iter(text) {
        let name = capture.get(1).map(|m| m.as_str()).unwrap_or_default();
        let value = capture.get(2).map(|m| m.as_str()).unwrap_or_default();
        // Only flag names we actually treat as sensitive (regex alternation already limits
        // this, but keep the check explicit for auditability).
        if !SENSITIVE_COOKIES
            .iter()
            .any(|c| c.eq_ignore_ascii_case(name))
        {
            continue;
        }
        if !is_synthetic_value(value) {
            findings.push(Finding {
                path: path.to_path_buf(),
                line,
                rule: Rule::SensitiveCookieValue,
                field: Some(name.to_string()),
                value_len: value.len(),
            });
        }
    }

    // 2. Signing fields stored as raw JSON string values.
    for capture in patterns.signing_field.captures_iter(text) {
        let key = capture.get(1).map(|m| m.as_str()).unwrap_or_default();
        let value = capture.get(2).map(|m| m.as_str()).unwrap_or_default();
        if is_confirmed_x_bogus(key, value) || is_synthetic_value(value) {
            continue;
        }
        findings.push(Finding {
            path: path.to_path_buf(),
            line,
            rule: Rule::SignedFieldRawValue,
            field: Some(key.to_string()),
            value_len: value.len(),
        });
    }

    // 3. Signing query parameters with a captured value.
    for capture in patterns.query_param.captures_iter(text) {
        let key = capture.get(1).map(|m| m.as_str()).unwrap_or_default();
        let value = capture.get(2).map(|m| m.as_str()).unwrap_or_default();
        if is_confirmed_x_bogus(key, value) {
            continue;
        }
        // A bare `msToken=` or empty value in prose is not a leak; the value must be
        // substantial. Signing signatures/tokens of interest are always long.
        if value.len() < 12 {
            continue;
        }
        if is_synthetic_value(value) {
            continue;
        }
        findings.push(Finding {
            path: path.to_path_buf(),
            line,
            rule: Rule::SignedQueryParameter,
            field: Some(key.to_string()),
            value_len: value.len(),
        });
    }

    // 4. Reusable signed URLs to real hosts.
    for url_match in patterns.url.find_iter(text) {
        let url = url_match.as_str();
        let Some((_, query)) = url.split_once('?') else {
            continue;
        };
        let host = url
            .split_once("://")
            .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
            .unwrap_or_default();
        let host_lower = host.to_ascii_lowercase();
        if host_lower.ends_with(".invalid") || host_lower.contains("fixture") {
            continue;
        }
        if !REAL_HOSTS.iter().any(|h| host_lower.contains(h)) {
            continue;
        }
        // Only a signing parameter in the query makes this a *signed* URL. A plain URL to a
        // real host with an ordinary query is not a transport secret. The first query pair has
        // no leading separator once the `?` is stripped, so re-add one before matching.
        let query = format!("&{query}");
        let carries_signing = patterns.query_param.captures_iter(&query).any(|capture| {
            let key = capture.get(1).map(|m| m.as_str()).unwrap_or_default();
            let value = capture.get(2).map(|m| m.as_str()).unwrap_or_default();
            !is_confirmed_x_bogus(key, value) && value.len() >= 12 && !is_synthetic_value(value)
        });
        if carries_signing {
            findings.push(Finding {
                path: path.to_path_buf(),
                line,
                rule: Rule::SignedUrlToRealHost,
                field: Some(host.to_string()),
                value_len: query.len().saturating_sub(1),
            });
        }
    }

    // 5. VM research artifacts that kept raw operand or table contents. Operand values index the
    // bundle's string and numeric constant tables; committing them reconstructs bundle internals
    // that the sanitized model deliberately reduces to widths, slots, and shapes.
    for capture in patterns.vm_raw_key.captures_iter(text) {
        let key = capture.get(1).map(|m| m.as_str()).unwrap_or_default();
        findings.push(Finding {
            path: path.to_path_buf(),
            line,
            rule: Rule::VmOperandValue,
            field: Some(key.to_string()),
            value_len: 0,
        });
    }
    for capture in patterns.vm_raw_bytes.captures_iter(text) {
        let value = capture.get(1).map(|m| m.as_str()).unwrap_or_default();
        findings.push(Finding {
            path: path.to_path_buf(),
            line,
            rule: Rule::VmOperandValue,
            field: Some("bytes".to_string()),
            value_len: value.len(),
        });
    }
}

/// Read and scan a single file.
pub fn scan_path(path: &Path) -> std::io::Result<Vec<Finding>> {
    let bytes = std::fs::read(path)?;
    Ok(scan_bytes(path, &bytes))
}

/// Directory entries skipped entirely (never contain committed fixtures).
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules"];

/// Recursively scan every file under `root`, returning findings sorted deterministically by
/// (path, line, rule) so output and tests do not depend on filesystem iteration order.
pub fn scan_dir(root: &Path) -> std::io::Result<Vec<Finding>> {
    let mut findings = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                let name = entry.file_name();
                if SKIP_DIRS.iter().any(|s| *s == name) {
                    continue;
                }
                stack.push(path);
            } else if file_type.is_file() {
                findings.extend(scan_path(&path)?);
            }
        }
    }
    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.rule.id().cmp(b.rule.id()))
            .then(a.field.cmp(&b.field))
    });
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(text: &str) -> Vec<Finding> {
        scan_bytes(Path::new("mem.json"), text.as_bytes())
    }

    #[test]
    fn synthetic_cookie_values_pass() {
        assert!(scan(r#"["msToken", "fixture-ms-token"]"#).is_empty());
        assert!(scan(r#"["ttwid", "fixture-ttwid"]"#).is_empty());
        assert!(scan(r#"msToken=fixture-token; ttwid=fixture"#).is_empty());
        assert!(
            scan(r#""sessionid": """#).is_empty(),
            "empty value is synthetic"
        );
    }

    #[test]
    fn captured_cookie_value_is_flagged() {
        // A realistic (long, non-synthetic) msToken value.
        let leak = "msToken=Ab3xK9pQ7wLmN2vR8sT4uYc6ZdE1fG0hJ5iK8lM3nO7pQ2rS9tU4vW";
        let findings = scan(leak);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, Rule::SensitiveCookieValue);
        assert_eq!(findings[0].field.as_deref(), Some("msToken"));
        // The gate must not echo the secret.
        assert!(!format!("{}", findings[0]).contains("Ab3xK9"));
    }

    #[test]
    fn json_cookie_pair_leak_is_flagged() {
        let leak = r#"["sessionid", "9f8e7d6c5b4a39281706f5e4d3c2b1a0ffeeddccbbaa9988"]"#;
        let findings = scan(leak);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, Rule::SensitiveCookieValue);
    }

    #[test]
    fn confirmed_x_bogus_constant_passes() {
        assert!(scan("X-Bogus=1").is_empty());
        assert!(scan(r#""X-Bogus": "1""#).is_empty());
        assert!(scan(r#"https://webcast.tiktok.com/im/fetch/?X-Bogus=1"#).is_empty());
    }

    #[test]
    fn captured_signing_query_parameter_is_flagged() {
        let leak = "path/?X-Gnarly=Zx9Yw8Vu7Ts6Rq5Po4Nm3Lk2Ji1Hg0Fe9Dc8Ba7";
        let findings = scan(leak);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, Rule::SignedQueryParameter);
        assert_eq!(findings[0].field.as_deref(), Some("X-Gnarly"));
    }

    #[test]
    fn prose_signing_names_without_values_pass() {
        // The research notes mention field names in prose; those are not leaks.
        assert!(scan("Query signatures: X-Gnarly (~332 chars), X-Dynosaur (~392).").is_empty());
        assert!(scan(r#""added_parameters_in_order": ["X-Dynosaur", "msToken"]"#).is_empty());
        assert!(scan(r#""name": "X-Gnarly""#).is_empty());
    }

    #[test]
    fn raw_vm_operand_values_are_flagged() {
        // A VM trace pasted into a fixture: operand values index the bundle's string table.
        let leak =
            r#"{"function_entry": 48886, "opcode": 11, "operands": [{"kind": "N", "value": 658}]}"#;
        let findings = scan(leak);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, Rule::VmOperandValue);
        assert_eq!(findings[0].field.as_deref(), Some("operands"));

        // Raw operand byte strings are the same leak in hex form.
        let bytes = scan(r#"{"width": 4, "bytes": "92020a01"}"#);
        assert_eq!(bytes.len(), 1, "{bytes:?}");
        assert_eq!(bytes[0].rule, Rule::VmOperandValue);

        // So are the VM tables themselves.
        for table in [
            r#""string_table": ["#,
            r#""strings": ["#,
            r#""bytecode": ""#,
        ] {
            assert!(!scan(table).is_empty(), "{table} should be flagged");
        }
    }

    #[test]
    fn sanitized_vm_shape_fields_pass() {
        // The subgraph model's own vocabulary must not trip the rule.
        for sanitized in [
            r#""operand_widths": [2, 4]"#,
            r#""operand_helpers": ["N", "j"]"#,
            r#""string_table_slots": 1001"#,
            r#""bytecode_sha256": "bc791ca2d4704d407ed36269b9bb758f807377915f90d34d007216de6620e8ff""#,
            r#""operand_helper_widths": [0, 2, 3]"#,
            r#""catalogue_fields": ["operand_widths", "operand_examples"]"#,
            r#""bytes": 235357"#,
            r#""byte_lengths": [332]"#,
        ] {
            assert!(scan(sanitized).is_empty(), "{sanitized} should pass");
        }
    }

    #[test]
    fn signed_field_raw_value_is_flagged() {
        let leak = r#"{"X-Gnarly": "MDEyMzQ1Njc4OWFiY2RlZmdoaWprbG1ub3BxcnN0dXZ3eHl6"}"#;
        let findings = scan(leak);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, Rule::SignedFieldRawValue);
    }

    #[test]
    fn signed_url_to_real_host_is_flagged() {
        let leak = "wss://webcast-ws.tiktok.com/ws/?X-Gnarly=Zx9Yw8Vu7Ts6Rq5Po4Nm3Lk2Ji1Hg0Fe";
        let findings = scan(leak);
        // Both the query-parameter rule and the signed-URL rule fire; both are correct.
        assert!(findings.iter().any(|f| f.rule == Rule::SignedUrlToRealHost));
    }

    #[test]
    fn real_host_without_signing_query_passes() {
        // The push_server and bundle endpoints are real hosts but carry no signing query.
        assert!(scan("wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse/").is_empty());
        assert!(scan(
            "https://sf16-website-login.neutral.ttwstatic.com/obj/webmssdk/1.0.0.388/webmssdk.js"
        )
        .is_empty());
    }

    #[test]
    fn fixture_invalid_host_passes_even_with_query() {
        assert!(scan("wss://fixture.invalid/ws/?signed=fixture-value-here-1234").is_empty());
    }

    #[test]
    fn binary_content_is_scanned_without_line_numbers() {
        let mut bytes = vec![0u8, 1, 2, 3];
        bytes.extend_from_slice(b"msToken=Ab3xK9pQ7wLmN2vR8sT4uYc6ZdE1fG0hJ5iK8lM3nO7pQ2rS9tU4vW");
        let findings = scan_bytes(Path::new("blob.pb"), &bytes);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].line, None);
    }
}
