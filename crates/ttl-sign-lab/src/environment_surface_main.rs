//! Record which browser properties the signing bundle touches while it signs.
//!
//! This is Phase 0 of `docs/11-webview-removal.md`. Both routes to a browser-free build need the
//! same list: an embedded JS engine has to shim these properties, and a native VM interpreter has
//! to resolve them. Producing the list by measurement rather than by guesswork is what turns a
//! rejected transport into a named missing property.
//!
//! The recorder installs proxies over the environment objects of an offscreen iframe, evaluates
//! the **unmodified** bundle inside it, and drives the patched-fetch path against a stubbed
//! transport. No signed request leaves the process.
//!
//! Only shapes are recorded: property path, operation counts, `typeof` class, and byte length.
//! `document.cookie` is recorded as a length; the cookie never crosses the bridge.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;
use ttl_sign_lab::webview_support::{download_bundle, engine_config, load_selected_case};
use ttl_sign_lab::{
    build_environment_surface, collect_sdk_evidence, environment_surface_json, AccessOps,
    InstrumentationCoverage, PropertyAccess, SurfaceRoot, SurfaceSource, ValueDigest, ValueType,
};
use ttl_sign_webview::run;

/// Environment roots replaced with a recording proxy.
///
/// `window` itself cannot be proxied — the global object is not replaceable — so it is covered by
/// the explicit property list below instead.
const PROXIED_ROOTS: [&str; 7] = [
    "navigator",
    "screen",
    "location",
    "crypto",
    "localStorage",
    "sessionStorage",
    "document",
];

/// Global properties instrumented individually, because the global object cannot be proxied.
///
/// Kept deliberately broad: a property nobody reads costs one unused accessor, while a missing one
/// costs a silent gap in the shim specification.
const WINDOW_PROPERTIES: [&str; 24] = [
    "innerWidth",
    "innerHeight",
    "outerWidth",
    "outerHeight",
    "devicePixelRatio",
    "screenX",
    "screenY",
    "origin",
    "isSecureContext",
    "performance",
    "TextEncoder",
    "TextDecoder",
    "Uint8Array",
    "ArrayBuffer",
    "Math",
    "Date",
    "Intl",
    "JSON",
    "Function",
    "Error",
    "Promise",
    "URL",
    "URLSearchParams",
    "WebSocket",
];

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ttl_sign_lab=info,ttl_sign_webview=warn".into()),
        )
        .init();
    let (plan_path, case_id) = match arguments() {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    };
    let selected = match load_selected_case(&plan_path, &case_id) {
        Ok(case) => case,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    };
    let unsigned_url = match selected.signing_url() {
        Ok(url) => url,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let config = match engine_config(&selected) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    };

    run(config, move |signer| {
        let shutdown = signer.clone();
        let runtime = tokio::runtime::Runtime::new().expect("could not create Tokio runtime");
        let result = runtime.block_on(async move {
            let sdk = collect_sdk_evidence(&signer, &unsigned_url)
                .await
                .context("could not identify webmssdk")?;
            let endpoint = sdk
                .resources
                .iter()
                .find(|resource| {
                    resource.endpoint.contains("/webmssdk/")
                        && resource.status == ttl_sign_lab::SdkResourceStatus::Downloaded
                })
                .context("the loaded page did not expose a downloadable webmssdk bundle")?
                .endpoint
                .clone();
            let source = download_bundle(&endpoint, &signer.preset().user_agent()).await?;
            let bundle = ValueDigest::of(&source);
            let source = String::from_utf8(source).context("webmssdk bundle is not UTF-8")?;

            let script = surface_script(&source, &unsigned_url)?;
            let raw = signer
                .eval(&script)
                .await
                .map_err(|error| anyhow::anyhow!("surface recording failed: {error:?}"))?;
            let recorded: RawSurface =
                serde_json::from_str(&raw).context("invalid sanitized surface recording")?;

            let document = build_environment_surface(
                SurfaceSource {
                    case_id: selected.id.clone(),
                    bundle_endpoint: endpoint,
                    bundle,
                    product: "fetch".into(),
                    clock_ms: recorded.clock_ms,
                },
                recorded
                    .instrumentation
                    .into_iter()
                    .map(|coverage| InstrumentationCoverage {
                        root: SurfaceRoot::of(&coverage.root),
                        installed: coverage.installed,
                        note: coverage.note,
                    })
                    .collect(),
                recorded.accesses.into_iter().map(Into::into).collect(),
            );

            let uninstrumented = document.uninstrumented_roots();
            if !uninstrumented.is_empty() {
                eprintln!(
                    "warning: {} root(s) could not be instrumented; this surface is incomplete \
                     and must not be used as a shim specification: {uninstrumented:?}",
                    uninstrumented.len()
                );
            }
            print!("{}", environment_surface_json(&document));
            Result::<()>::Ok(())
        });
        match result {
            Ok(()) => shutdown.shutdown(),
            Err(error) => {
                eprintln!("environment surface recording failed: {error:#}");
                shutdown.shutdown_with_code(1);
            }
        }
    })
}

#[derive(Debug, Deserialize)]
struct RawSurface {
    clock_ms: u64,
    instrumentation: Vec<RawCoverage>,
    accesses: Vec<RawAccess>,
}

#[derive(Debug, Deserialize)]
struct RawCoverage {
    root: String,
    installed: bool,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAccess {
    path: String,
    op: String,
    #[serde(rename = "type")]
    value_type: String,
    value_class: Option<String>,
    bytes: Option<usize>,
}

impl From<RawAccess> for PropertyAccess {
    fn from(raw: RawAccess) -> Self {
        let mut operations = AccessOps::default();
        match raw.op.as_str() {
            "get" => operations.gets = 1,
            "set" => operations.sets = 1,
            "call" => operations.calls = 1,
            "has" => operations.has = 1,
            // An unrecognized operation still counts as a touch: the shim must provide the
            // property either way.
            _ => operations.gets = 1,
        }
        PropertyAccess {
            root: SurfaceRoot::of(&raw.path),
            path: raw.path,
            operations,
            value_type: ValueType::from_js(&raw.value_type),
            value_class: raw.value_class,
            byte_lengths: raw.bytes.into_iter().collect(),
        }
    }
}

/// Build the in-page recorder.
///
/// The bundle source and URL are embedded as JSON literals, so neither can break out of the
/// script regardless of their content.
fn surface_script(source: &str, unsigned_url: &str) -> Result<String> {
    let source = js_string_literal(source)?;
    let unsigned_url = js_string_literal(unsigned_url)?;
    let proxied_roots = serde_json::to_string(&PROXIED_ROOTS)?;
    let window_properties = serde_json::to_string(&WINDOW_PROPERTIES)?;

    Ok(format!(
        r#"(async function(){{
  var frame=document.createElement('iframe');
  frame.style.display='none';
  document.documentElement.appendChild(frame);
  try {{
    var view=frame.contentWindow;
    var accesses=[];
    var instrumentation=[];
    var recording=false;

    // Describe a value without ever retaining it. Strings contribute a byte length; everything
    // else contributes only its type. The recorder must not touch a trapped object, so it never
    // reads properties of `value` beyond `typeof` and `length`.
    var describe=function(value){{
      var type=typeof value, bytes=null, valueClass=null;
      try {{
        if(type==='string') bytes=value.length;
        else if(value instanceof view.Uint8Array){{ bytes=value.byteLength; valueClass='typed_array'; }}
      }} catch(ignored) {{}}
      return {{type:type,bytes:bytes,value_class:valueClass}};
    }};
    var record=function(path,op,value){{
      if(!recording||accesses.length>=8192) return;
      var shape=describe(value);
      accesses.push({{path:path,op:op,type:shape.type,bytes:shape.bytes,value_class:shape.value_class}});
    }};

    // A recording proxy over one environment object. Functions are bound to the real target so
    // the SDK observes native behaviour, not a wrapper.
    var proxyFor=function(name,target){{
      return new Proxy(target,{{
        get:function(object,property,receiver){{
          var key=typeof property==='symbol'?String(property):property;
          var value;
          try {{ value=Reflect.get(object,property,object); }} catch(error) {{ value=void 0; }}
          record(name+'.'+key,'get',value);
          if(typeof value==='function'){{
            return function(){{
              record(name+'.'+key,'call',void 0);
              return value.apply(object,arguments);
            }};
          }}
          return value;
        }},
        set:function(object,property,value){{
          var key=typeof property==='symbol'?String(property):property;
          record(name+'.'+key,'set',value);
          try {{ object[property]=value; }} catch(error) {{}}
          return true;
        }},
        has:function(object,property){{
          var key=typeof property==='symbol'?String(property):property;
          record(name+'.'+key,'has',void 0);
          return Reflect.has(object,property);
        }}
      }});
    }};

    // Install a proxy per root. A failure is recorded rather than swallowed: an uninstrumented
    // root produces an empty surface, which is indistinguishable from an untouched one.
    {proxied_roots}.forEach(function(name){{
      var installed=false, note=null;
      try {{
        var target=view[name];
        if(target===void 0||target===null){{
          note='root_absent';
        }} else {{
          var proxy=proxyFor(name,target);
          Object.defineProperty(view,name,{{
            configurable:true,
            get:function(){{ return proxy; }}
          }});
          installed=true;
        }}
      }} catch(error) {{ note='trap_install_failed'; }}
      instrumentation.push({{root:name,installed:installed,note:note}});
    }});

    // The global object cannot be proxied, so instrument a fixed list of its properties.
    var windowInstalled=0, windowFailed=0;
    {window_properties}.forEach(function(name){{
      try {{
        var current=view[name];
        Object.defineProperty(view,name,{{
          configurable:true,
          get:function(){{ record('window.'+name,'get',current); return current; }},
          set:function(value){{ record('window.'+name,'set',value); current=value; }}
        }});
        windowInstalled++;
      }} catch(error) {{ windowFailed++; }}
    }});
    instrumentation.push({{
      root:'window',
      installed:windowInstalled>0,
      note:windowFailed>0?'partial_property_coverage':null
    }});

    // Stub the transport before evaluating: the bundle must sign, but nothing may be sent.
    var capturedUrl=null;
    view._mssdk=window._mssdk;
    view.fetch=async function(input){{
      capturedUrl=typeof input==='string'?input:String(input&&input.url||input);
      return new view.Response('',{{status:200}});
    }};

    // Record from here on: bundle evaluation and signing are both in scope, because a shim has to
    // satisfy the SDK at load time as well as at sign time.
    recording=true;
    view.eval({source});
    var sdk=view.byted_acrawler;
    if(!sdk) throw new Error('sdk_not_exposed');

    var cachedConfigs=window._mssdk&&window._mssdk.cacheOpts;
    var cachedAids=cachedConfigs?Object.keys(cachedConfigs):[];
    if(cachedAids.length===0) throw new Error('mssdk_config_not_found');
    await Promise.resolve(sdk.init(cachedConfigs[cachedAids[0]]));

    await Promise.resolve(view.fetch({unsigned_url},{{method:'GET'}}));
    if(!capturedUrl) throw new Error('fetch_not_captured');
    recording=false;

    return JSON.stringify({{
      clock_ms:Date.now(),
      instrumentation:instrumentation,
      accesses:accesses
    }});
  }} finally {{ frame.remove(); }}
}})()"#
    ))
}

/// Encode a value as a JavaScript string literal.
///
/// JSON encoding already makes quotes, backslashes, and newlines inert. `<` is escaped on top of
/// that so the generated script stays safe if it is ever delivered inside an HTML `<script>`
/// element rather than through the `eval` bridge; `\u003c` is a valid escape in both.
fn js_string_literal(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?.replace('<', "\\u003c"))
}

fn arguments() -> Result<(PathBuf, String)> {
    let usage = "usage: ttl-sign-env-surface <plan.json> <case-id>";
    let mut args = std::env::args_os().skip(1);
    let plan = PathBuf::from(args.next().context(usage)?);
    let case_id = args.next().context(usage)?.to_string_lossy().into_owned();
    if args.next().is_some() {
        anyhow::bail!(usage);
    }
    Ok((plan, case_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_probe_never_embeds_an_unescaped_url() {
        let script = surface_script("bundle", "https://example.test/?secret=\"value\n").unwrap();
        assert!(!script.contains("secret=\"value\n"));
        assert!(script.contains("secret=\\\"value\\n"));
    }

    #[test]
    fn generated_probe_never_embeds_unescaped_bundle_source() {
        let script = surface_script(
            "</script><script>alert(1)\n\"quoted\"\\",
            "https://example.test/",
        )
        .unwrap();
        // JSON encoding neutralizes quotes, newlines, and backslashes; `<` is escaped on top so
        // the script is also inert inside an HTML script element.
        assert!(!script.contains("</script>"));
        assert!(script.contains("\\u003c/script>"));
        assert!(!script.contains("\n\"quoted\""));
    }

    /// Every root the recorder proxies must also be reported in `instrumentation`, or a failed
    /// trap would be invisible.
    #[test]
    fn every_proxied_root_is_reported() {
        let script = surface_script("bundle", "https://example.test/").unwrap();
        for root in PROXIED_ROOTS {
            assert!(
                script.contains(&format!("\"{root}\"")),
                "{root} is not installed by the recorder"
            );
        }
        assert!(script.contains("root:'window'"));
    }

    /// The recorder must be able to describe a value without reading it.
    #[test]
    fn the_probe_records_lengths_not_values() {
        let script = surface_script("bundle", "https://example.test/").unwrap();
        assert!(script.contains("bytes=value.length"));
        // The access record has no field that could carry a value.
        assert!(script.contains("path:path,op:op,type:shape.type,bytes:shape.bytes"));
        assert!(!script.contains("value:value"));
    }

    #[test]
    fn raw_accesses_map_onto_bounded_operations() {
        let get = PropertyAccess::from(RawAccess {
            path: "document.cookie".into(),
            op: "get".into(),
            value_type: "string".into(),
            value_class: None,
            bytes: Some(214),
        });
        assert_eq!(get.operations.gets, 1);
        assert_eq!(get.root, SurfaceRoot::Document);
        assert_eq!(get.value_type, ValueType::String);
        assert_eq!(get.byte_lengths, vec![214]);

        let call = PropertyAccess::from(RawAccess {
            path: "crypto.getRandomValues".into(),
            op: "call".into(),
            value_type: "function".into(),
            value_class: None,
            bytes: None,
        });
        assert_eq!(call.operations.calls, 1);
        assert!(call.byte_lengths.is_empty());

        // An unrecognized operation still records a touch.
        let odd = PropertyAccess::from(RawAccess {
            path: "window.Math".into(),
            op: "deleteProperty".into(),
            value_type: "object".into(),
            value_class: None,
            bytes: None,
        });
        assert_eq!(odd.operations.total(), 1);
    }
}
