//! Trace the public signer VM entry points without retaining source or signature values.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ttl_sign_lab::webview_support::{engine_config, load_selected_case};
use ttl_sign_lab::{
    collect_sdk_evidence, TraceProduct, ValueDigest, VmEnvironmentEvidence, VmTrace, VmTraceReport,
    VM_TRACE_VERSION,
};
use ttl_sign_webview::run;

const VM_EXPORT_NEEDLE: &str = "L(0,void 0,y,[],0,14)}()}();})();";
const VM_EXPORT_REPLACEMENT: &str =
    "y.__ttlVm={ops:b,bytecode:I,strings:M,numbers:S};L(0,void 0,y,[],0,14)}()}();})();";
const VM_CALL_TAIL: &str = "return R(u,4)}L(0,void 0,y,[],0,14)";
const VM_DISPATCH_NEEDLE: &str = "for(u.u[0]=null,u.u[1]=void 0,u.u[2]=!0,u.u[3]=!1,u.u[4]=C,u.u[5]=r,u.u[6]=i;u.o<I.length&&R(u,4)===C;){var c=I[u.o++]|I[u.o++]<<8;try{b[c](u)}catch(n){if(0===u.C.length)throw n;u.I=[],u.I.push({t:\"0\",v:n}),u.o=u.C[u.C.length-1].h}}";
const VM_DISPATCH_REPLACEMENT: &str = "for(u.u[0]=null,u.u[1]=void 0,u.u[2]=!0,u.u[3]=!1,u.u[4]=C,u.u[5]=r,u.u[6]=i;u.o<I.length&&R(u,4)===C;){var c=I[u.o++]|I[u.o++]<<8,__ttlOffset=u.o-2,__ttlOperandStart=u.o;u.__ttlOperandBytes=0,u.__ttlOperandValues=[];var __ttlPreviousEntry=y.__ttlVmActiveEntry,__ttlPreviousOpcode=y.__ttlVmActiveOpcode,__ttlPreviousFunction=y.__ttlVmFunctionEntry;y.__ttlVmActiveEntry=__ttlOffset,y.__ttlVmActiveOpcode=c,y.__ttlVmFunctionEntry=u.__ttlEntry;try{b[c](u)}catch(n){if(0===u.C.length)throw n;u.I=[],u.I.push({t:\"0\",v:n}),u.o=u.C[u.C.length-1].h}finally{if(y.__ttlTraceEntries&&y.__ttlTraceEntries[String(u.__ttlEntry)]&&y.__ttlFunctionSteps&&y.__ttlFunctionSteps.length<4096){var __ttlWidth=u.__ttlOperandBytes||0,__ttlStepBytes=[];for(var __ttlStepByteIndex=__ttlOperandStart;__ttlStepByteIndex<Math.min(__ttlOperandStart+__ttlWidth,__ttlOperandStart+16);__ttlStepByteIndex++)__ttlStepBytes.push(I[__ttlStepByteIndex].toString(16).padStart(2,\"0\"));y.__ttlFunctionSteps.push({function_entry:u.__ttlEntry,offset:__ttlOffset,opcode:c,width:__ttlWidth,bytes:__ttlStepBytes.join(\"\"),operands:u.__ttlOperandValues||[]})}y.__ttlVmActiveEntry=__ttlPreviousEntry,y.__ttlVmActiveOpcode=__ttlPreviousOpcode,y.__ttlVmFunctionEntry=__ttlPreviousFunction}}";
const VM_CALL_HEAD: &str = "function L(n,t,r,i,o,e){var u={o:n,u:[],C:[],I:[],A:t,M:e};";
const VM_CALL_HEAD_REPLACEMENT: &str = "function L(n,t,r,i,o,e){var __ttlParent=y.__ttlVmParents&&t&&typeof t==='object'&&y.__ttlVmParents.has(t)?String(y.__ttlVmParents.get(t)):'root';var u={o:n,u:[],C:[],I:[],A:t,M:e,__ttlEntry:n};if(y.__ttlVmParents)y.__ttlVmParents.set(u,n);if(y.__ttlVmInputs&&y.__ttlTraceEntries&&y.__ttlTraceEntries[String(n)]&&y.__ttlVmInputs.length<4096){var __ttlShape=function(__ttlValue){var __ttlType=typeof __ttlValue,__ttlBytes=0,__ttlKeys=[],__ttlClass=null;try{if(__ttlType==='string')__ttlBytes=__ttlValue.length;else if(__ttlValue instanceof Uint8Array){__ttlBytes=__ttlValue.byteLength;__ttlClass='typed_array'}if(__ttlValue&&__ttlType==='object')__ttlKeys=Object.keys(__ttlValue).sort().slice(0,32)}catch(__ttlIgnore){}return {type:__ttlType,bytes:__ttlBytes,value_class:__ttlClass,object_keys:__ttlKeys}};var __ttlArgs=[];try{var __ttlCount=i&&typeof i.length==='number'?Math.min(Number(i.length),16):0;for(var __ttlArgIndex=0;__ttlArgIndex<__ttlCount;__ttlArgIndex++)__ttlArgs.push(__ttlShape(i[__ttlArgIndex]))}catch(__ttlIgnoreArgs){}y.__ttlVmInputs.push({entry:n,parent:__ttlParent,args:__ttlArgs,this_value:__ttlShape(r),context_value:__ttlShape(t)})}";
const VM_CALL_REPLACEMENT: &str = "var __ttlResult=R(u,4);if(y.__ttlVmCalls){var __ttlEntry=String(n),__ttlType=typeof __ttlResult,__ttlBytes=0,__ttlKeys=[],__ttlPhase=String(y.__ttlVmPhase||'unknown');try{if(__ttlType==='string')__ttlBytes=__ttlResult.length;else if(__ttlResult instanceof Uint8Array)__ttlBytes=__ttlResult.byteLength;if(__ttlResult&&__ttlType==='object')__ttlKeys=Object.keys(__ttlResult).sort().slice(0,32)}catch(__ttlIgnore){}var __ttlRecord=y.__ttlVmCalls.entries[__ttlEntry]||(y.__ttlVmCalls.entries[__ttlEntry]={calls:0,types:{},byte_lengths:{},object_keys:[],parents:{},phases:{}});__ttlRecord.calls++;__ttlRecord.types[__ttlType]=(__ttlRecord.types[__ttlType]||0)+1;__ttlRecord.byte_lengths[__ttlBytes]=(__ttlRecord.byte_lengths[__ttlBytes]||0)+1;__ttlRecord.parents[__ttlParent]=(__ttlRecord.parents[__ttlParent]||0)+1;__ttlRecord.phases[__ttlPhase]=(__ttlRecord.phases[__ttlPhase]||0)+1;for(var __ttlKeyIndex=0;__ttlKeyIndex<__ttlKeys.length;__ttlKeyIndex++)if(__ttlRecord.object_keys.indexOf(__ttlKeys[__ttlKeyIndex])<0)__ttlRecord.object_keys.push(__ttlKeys[__ttlKeyIndex]);var __ttlCall={entry:__ttlEntry,parent:__ttlParent,type:__ttlType,bytes:__ttlBytes,phase:__ttlPhase,object_keys:__ttlKeys};if(y.__ttlVmCalls.sequence.length>=8192)y.__ttlVmCalls.sequence.shift();y.__ttlVmCalls.sequence.push(__ttlCall);if(y.__ttlVmPhase==='invocation'&&y.__ttlVmInvocation&&y.__ttlVmInvocation.length<8192)y.__ttlVmInvocation.push(__ttlCall);if(__ttlType==='string'&&y.__ttlVmCalls.strings.length<256)y.__ttlVmCalls.strings.push({entry:__ttlEntry,parent:__ttlParent,bytes:__ttlBytes,phase:__ttlPhase})}return __ttlResult}L(0,void 0,y,[],0,14)";
const VM_OPERAND_HELPERS: [(&str, &str); 3] = [
    (
        "function N(n){return I[n.o++]|I[n.o++]<<8}",
        "function N(n){var v=I[n.o++]|I[n.o++]<<8;n.__ttlOperandBytes=(n.__ttlOperandBytes||0)+2;if(n.__ttlOperandValues&&n.__ttlOperandValues.length<32)n.__ttlOperandValues.push({kind:'N',value:v});return v}",
    ),
    (
        "function j(n){return I[n.o++]}",
        "function j(n){var v=I[n.o++];n.__ttlOperandBytes=(n.__ttlOperandBytes||0)+1;if(n.__ttlOperandValues&&n.__ttlOperandValues.length<32)n.__ttlOperandValues.push({kind:'j',value:v});return v}",
    ),
    (
        "function x(n){return I[n.o++]|I[n.o++]<<8|I[n.o++]<<16}",
        "function x(n){var v=I[n.o++]|I[n.o++]<<8|I[n.o++]<<16;n.__ttlOperandBytes=(n.__ttlOperandBytes||0)+3;if(n.__ttlOperandValues&&n.__ttlOperandValues.length<32)n.__ttlOperandValues.push({kind:'x',value:v});return v}",
    ),
];
const VM_REGISTER_HELPERS: [(&str, &str); 2] = [
    (
        "function Q(n,t,r){t>=n.M?n.u[t].v=r:n.u[t]=r}",
        "function Q(n,t,r){t>=n.M?n.u[t].v=r:n.u[t]=r;if(y.__ttlRegisterTrace&&y.__ttlTraceEntries&&y.__ttlTraceEntries[String(n.__ttlEntry)]&&y.__ttlRegisterTrace.length<8192){var __ttlType=typeof r,__ttlBytes=0,__ttlKeys=[],__ttlClass=null;try{if(__ttlType==='string'){__ttlBytes=r.length;__ttlClass=y.__ttlClassify?y.__ttlClassify(r):null}else if(r instanceof Uint8Array){__ttlBytes=r.byteLength;__ttlClass='typed_array'}if(r&&__ttlType==='object')__ttlKeys=Object.keys(r).sort().slice(0,32)}catch(__ttlIgnore){}y.__ttlRegisterTrace.push({function_entry:n.__ttlEntry,op:'write',register:t,type:__ttlType,bytes:__ttlBytes,value_class:__ttlClass,object_keys:__ttlKeys})}}",
    ),
    (
        "function R(n,t){return t>=n.M?n.u[t].v:n.u[t]}",
        "function R(n,t){var r=t>=n.M?n.u[t].v:n.u[t];if(y.__ttlRegisterTrace&&y.__ttlTraceEntries&&y.__ttlTraceEntries[String(n.__ttlEntry)]&&y.__ttlRegisterTrace.length<8192){var __ttlType=typeof r,__ttlBytes=0,__ttlKeys=[],__ttlClass=null;try{if(__ttlType==='string'){__ttlBytes=r.length;__ttlClass=y.__ttlClassify?y.__ttlClassify(r):null}else if(r instanceof Uint8Array){__ttlBytes=r.byteLength;__ttlClass='typed_array'}if(r&&__ttlType==='object')__ttlKeys=Object.keys(r).sort().slice(0,32)}catch(__ttlIgnore){}y.__ttlRegisterTrace.push({function_entry:n.__ttlEntry,op:'read',register:t,type:__ttlType,bytes:__ttlBytes,value_class:__ttlClass,object_keys:__ttlKeys})}return r}",
    ),
];
const STRING_DECODE_NEEDLE: &str =
    "function m(n,t){n=new E(\"utf-8\").decode(D(n));for(var r=\"\",i=0;i<n.length;i++)r+=String.fromCharCode(n.charCodeAt(i)^t.charCodeAt(i%t.length));return r}";
const STRING_DECODE_REPLACEMENT: &str =
    "function m(n,t){var __ttlEncoded=n;n=new E(\"utf-8\").decode(D(n));for(var r=\"\",i=0;i<n.length;i++)r+=String.fromCharCode(n.charCodeAt(i)^t.charCodeAt(i%t.length));if(y.__ttlDecodedTargets){var __ttlSlot=M.indexOf(__ttlEncoded),__ttlTargets=['X-Bogus','X-Gnarly','X-Dynosaur','msToken','1'];for(var __ttlTargetIndex=0;__ttlTargetIndex<__ttlTargets.length;__ttlTargetIndex++)if(r===__ttlTargets[__ttlTargetIndex]&&__ttlSlot>=0){var __ttlTarget=__ttlTargets[__ttlTargetIndex];if(y.__ttlDecodedTargets[__ttlTarget].length<32)y.__ttlDecodedTargets[__ttlTarget].push(__ttlSlot);if(y.__ttlDecodedUses&&y.__ttlDecodedUses[__ttlTarget].length<128)y.__ttlDecodedUses[__ttlTarget].push({slot:__ttlSlot,function_entry:y.__ttlVmFunctionEntry===void 0?null:y.__ttlVmFunctionEntry,entry:y.__ttlVmActiveEntry===void 0?null:y.__ttlVmActiveEntry,opcode:y.__ttlVmActiveOpcode===void 0?null:y.__ttlVmActiveOpcode})}}return r}";
const MAX_BUNDLE_BYTES: usize = 2 * 1024 * 1024;

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ttl_sign_lab=info,ttl_sign_webview=warn".into()),
        )
        .init();
    let (plan_path, case_id, product) = match arguments() {
        Ok(arguments) => arguments,
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
            let source_digest = ValueDigest::of(&source);
            let source = String::from_utf8(source).context("webmssdk bundle is not UTF-8")?;
            let patched = patch_vm_export(&source)?;
            let script = vm_trace_script(&patched, &unsigned_url, product)?;
            let raw = signer.eval(&script).await.map_err(|error| {
                anyhow::anyhow!("VM evaluation failed: {}", error_class(&error))
            })?;
            let trace: VmTrace =
                serde_json::from_str(&raw).context("invalid sanitized VM trace")?;
            let effective_environment = VmEnvironmentEvidence::from(&signer.preset());
            let report = VmTraceReport {
                trace_version: VM_TRACE_VERSION,
                case_id: selected.id,
                bundle_endpoint: endpoint,
                bundle: source_digest,
                effective_environment,
                trace,
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
            Result::<()>::Ok(())
        });
        match result {
            Ok(()) => shutdown.shutdown(),
            Err(error) => {
                eprintln!("VM trace failed: {error:#}");
                shutdown.shutdown_with_code(1);
            }
        }
    })
}

async fn download_bundle(endpoint: &str, user_agent: &str) -> Result<Vec<u8>> {
    let response = reqwest::Client::builder()
        .user_agent(user_agent)
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .get(endpoint)
        .send()
        .await?
        .error_for_status()?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BUNDLE_BYTES as u64)
    {
        anyhow::bail!("webmssdk bundle exceeds the inspection limit");
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_BUNDLE_BYTES {
        anyhow::bail!("webmssdk bundle exceeds the inspection limit");
    }
    Ok(bytes.to_vec())
}

fn patch_vm_export(source: &str) -> Result<String> {
    if source.matches(VM_CALL_TAIL).count() != 1 {
        anyhow::bail!("unsupported webmssdk VM call wrapper shape");
    }
    if source.matches(VM_CALL_HEAD).count() != 1 {
        anyhow::bail!("unsupported webmssdk VM call head shape");
    }
    let mut patched = source.replacen(VM_CALL_HEAD, VM_CALL_HEAD_REPLACEMENT, 1);
    if patched.matches(VM_CALL_TAIL).count() != 1 {
        anyhow::bail!("unsupported webmssdk VM call wrapper shape");
    }
    patched = patched.replacen(VM_CALL_TAIL, VM_CALL_REPLACEMENT, 1);
    if patched.matches(VM_EXPORT_NEEDLE).count() != 1 {
        anyhow::bail!("unsupported webmssdk VM wrapper shape");
    }
    patched = patched.replacen(VM_EXPORT_NEEDLE, VM_EXPORT_REPLACEMENT, 1);
    if patched.matches(STRING_DECODE_NEEDLE).count() != 1 {
        anyhow::bail!("unsupported webmssdk string decoder shape");
    }
    patched = patched.replacen(STRING_DECODE_NEEDLE, STRING_DECODE_REPLACEMENT, 1);
    if patched.matches(VM_DISPATCH_NEEDLE).count() != 1 {
        anyhow::bail!("unsupported webmssdk VM dispatch loop shape");
    }
    patched = patched.replacen(VM_DISPATCH_NEEDLE, VM_DISPATCH_REPLACEMENT, 1);
    for (needle, replacement) in VM_OPERAND_HELPERS {
        if patched.matches(needle).count() != 1 {
            anyhow::bail!("unsupported webmssdk VM operand helper shape");
        }
        patched = patched.replacen(needle, replacement, 1);
    }
    for (needle, replacement) in VM_REGISTER_HELPERS {
        if patched.matches(needle).count() != 1 {
            anyhow::bail!("unsupported webmssdk VM register helper shape");
        }
        patched = patched.replacen(needle, replacement, 1);
    }
    Ok(patched)
}

fn vm_trace_script(source: &str, unsigned_url: &str, product: TraceProduct) -> Result<String> {
    let source = serde_json::to_string(source)?;
    let unsigned_url = serde_json::to_string(unsigned_url)?;
    let product_name = serde_json::to_string(&product)?;
    let setup = match product {
        TraceProduct::Frontier => String::new(),
        TraceProduct::Fetch => r#"
    var capturedUrl=null;
    var fetchAssignments=[];
    var fetchPhase='setup';
    var fetchDescriptorInstalled=false;
    var describeFetch=function(value){
      var type=typeof value, source="", name="", length=null;
      if(type==='function'){
        try{name=String(value.name||'');}catch(e){}
        try{length=Number(value.length);}catch(e){length=null;}
        try{source=Function.prototype.toString.call(value);}catch(e){source="";}
      }
      return {
        type:type,
        name:name,
        length:length,
        source_bytes:source.length,
        contains_fetch:source.indexOf('fetch')!==-1,
        contains_l:source.indexOf('L(')!==-1,
        contains_apply:source.indexOf('apply')!==-1,
        contains_call:source.indexOf('call')!==-1,
        contains_arguments:source.indexOf('arguments')!==-1,
        contains_new:source.indexOf('new ')!==-1,
        contains_window:source.indexOf('window')!==-1,
        contains_url:source.indexOf('URL')!==-1||source.indexOf('url')!==-1,
        contains_search:source.indexOf('search')!==-1,
        contains_query:source.indexOf('query')!==-1,
        contains_header:source.indexOf('header')!==-1,
        contains_set:source.indexOf('set')!==-1,
        contains_append:source.indexOf('append')!==-1,
        contains_crypto:source.indexOf('crypto')!==-1,
        contains_bogus:source.indexOf('Bogus')!==-1||source.indexOf('bogus')!==-1,
        contains_gnarly:source.indexOf('Gnarly')!==-1||source.indexOf('gnarly')!==-1,
        contains_dynosaur:source.indexOf('Dynosaur')!==-1||source.indexOf('dynosaur')!==-1,
        contains_ms_token:source.indexOf('msToken')!==-1||source.indexOf('mstoken')!==-1
      };
    };
    var installFetchDescriptor=function(){
      var current;
      try{current=frame.contentWindow.fetch;}catch(e){current=void 0;}
      try{
        Object.defineProperty(frame.contentWindow,'fetch',{
          configurable:true,
          enumerable:true,
          get:function(){return current;},
          set:function(value){
            if(fetchAssignments.length<32){
              var metadata=describeFetch(value);
              metadata.assignment=fetchAssignments.length;
              metadata.phase=fetchPhase;
              fetchAssignments.push(metadata);
            }
            current=value;
          }
        });
        fetchDescriptorInstalled=true;
      }catch(e){fetchDescriptorInstalled=false;}
    };
    installFetchDescriptor();
    var activeOpcode=null;
    var fieldEvents=[];
    var recordField=function(action,name,value){
      if(fieldEvents.length>=512) return;
      var bytes=0;
      try { bytes=new TextEncoder().encode(String(value==null?'':value)).length; } catch(e) {}
      fieldEvents.push({action:action,name:String(name),bytes:bytes,opcode:activeOpcode});
    };
    var urlSearchParams=frame.contentWindow.URLSearchParams;
    if(urlSearchParams&&urlSearchParams.prototype){
      var nativeAppend=urlSearchParams.prototype.append;
      var nativeSet=urlSearchParams.prototype.set;
      var nativeDelete=urlSearchParams.prototype.delete;
      urlSearchParams.prototype.append=function(name,value){recordField('append',name,value);return nativeAppend.call(this,name,value);};
      urlSearchParams.prototype.set=function(name,value){recordField('set',name,value);return nativeSet.call(this,name,value);};
      urlSearchParams.prototype.delete=function(name){recordField('delete',name,'');return nativeDelete.call(this,name);};
    }
    var urlPrototype=frame.contentWindow.URL&&frame.contentWindow.URL.prototype;
    if(urlPrototype){
      var searchDescriptor=Object.getOwnPropertyDescriptor(urlPrototype,'search');
      if(searchDescriptor&&searchDescriptor.set&&searchDescriptor.get){
        Object.defineProperty(urlPrototype,'search',{
          configurable:true,
          get:function(){return searchDescriptor.get.call(this);},
          set:function(value){recordField('url_search', 'search', value);return searchDescriptor.set.call(this,value);}
        });
      }
    }
    frame.contentWindow._mssdk=window._mssdk;
    frame.contentWindow.fetch=async function(input){
      capturedUrl=typeof input==='string'?input:String(input&&input.url||input);
      return new frame.contentWindow.Response('',{status:200});
    };"#
        .into(),
    };
    let invocation = match product {
        TraceProduct::Frontier => format!(
            r#"var result=await Promise.resolve(sdk.frontierSign({{url:{unsigned_url}}}));
    var parameters=Object.keys(result||{{}}).sort().map(function(name){{
      return {{name:name,bytes:new TextEncoder().encode(String(result[name])).length}};
    }});"#
        ),
        TraceProduct::Fetch => format!(
            r#"await Promise.resolve(frame.contentWindow.fetch({unsigned_url},{{method:'GET'}}));
    if(!capturedUrl) throw new Error('fetch_not_captured');
    var inputCounts={{}};
    new URL({unsigned_url}).searchParams.forEach(function(value,name){{inputCounts[name]=(inputCounts[name]||0)+1;}});
    var seen={{}}, parameters=[];
    new URL(capturedUrl).searchParams.forEach(function(value,name){{
      var occurrence=seen[name]||0;seen[name]=occurrence+1;
      if(occurrence>=(inputCounts[name]||0)) parameters.push({{
        name:name,bytes:new TextEncoder().encode(value).length
      }});
    }});"#
        ),
    };
    let initialization = match product {
        TraceProduct::Frontier => String::new(),
        TraceProduct::Fetch => r#"
    var cachedConfigs=window._mssdk&&window._mssdk.cacheOpts;
    var cachedAids=cachedConfigs?Object.keys(cachedConfigs):[];
    if(cachedAids.length===0) throw new Error('mssdk_config_not_found');
    await Promise.resolve(sdk.init(cachedConfigs[cachedAids[0]]));"#
            .into(),
    };
    Ok(format!(
        r#"(async function(){{
  var frame=document.createElement('iframe');
  frame.style.display='none';
  document.documentElement.appendChild(frame);
  try {{
    var activeOpcode=null, fieldEvents=[];
    frame.contentWindow.__ttlVmCalls={{entries:{{}},sequence:[],strings:[]}};
    frame.contentWindow.__ttlVmPhase='setup';
    frame.contentWindow.__ttlVmInvocation=[];
    frame.contentWindow.__ttlVmInputs=[];
    frame.contentWindow.__ttlSourceDispatch=true;
    frame.contentWindow.__ttlFunctionSteps=[];
    frame.contentWindow.__ttlRegisterTrace=[];
    frame.contentWindow.__ttlTraceEntries={{'56':true,'91717':true,'94000':true,'58628':true,'55188':true,'48886':true,'92825':true,'8039':true,'69021':true,'69171':true,'67569':true,'68501':true,'8685':true}};
    frame.contentWindow.__ttlDecodedTargets={{'X-Bogus':[],'X-Gnarly':[],'X-Dynosaur':[],'msToken':[],'1':[]}};
    frame.contentWindow.__ttlDecodedUses={{'X-Bogus':[],'X-Gnarly':[],'X-Dynosaur':[],'msToken':[],'1':[]}};
    frame.contentWindow.__ttlClassify=function(value){{
      if(typeof value!=='string') return null;
      if(value==='1') return 'literal_one';
      if(value==='X-Bogus') return 'field_key_x_bogus';
      if(value==='X-Dynosaur') return 'field_key_x_dynosaur';
      if(value==='X-Gnarly') return 'field_key_x_gnarly';
      if(value==='msToken') return 'field_key_ms_token';
      return null;
    }};
    frame.contentWindow.__ttlVmParents=new WeakMap();
    {setup}
    fetchPhase='before_eval';
    frame.contentWindow.__ttlVmPhase='eval';
    frame.contentWindow.eval({source});
    fetchPhase='after_eval';
    frame.contentWindow.__ttlVmPhase='post_eval';
    var vm=frame.contentWindow.__ttlVm;
    var sdk=frame.contentWindow.byted_acrawler;
    if(!vm||!sdk||typeof sdk.frontierSign!=='function') throw new Error('vm_not_ready');
    var knownStringSlots={{}};
    ['X-Bogus','X-Gnarly','X-Dynosaur','msToken','1'].forEach(function(target){{
      knownStringSlots[target]=[];
      for(var stringIndex=0;stringIndex<vm.strings.length;stringIndex++)
        if(String(vm.strings[stringIndex])===target) knownStringSlots[target].push(stringIndex);
    }});
    var mssdkObject=frame.contentWindow._mssdk||null;
    var mssdkKeys=mssdkObject?Object.keys(mssdkObject).sort().slice(0,128):[];
    var mssdkFunctions=mssdkObject?mssdkKeys.filter(function(name){{return typeof mssdkObject[name]==='function';}}):[];
    var mssdkFunctionPaths=[],mssdkSeen=new WeakSet();
    var inspectMssdk=function(value,path,depth){{
      if(!value||depth>3||(typeof value!=='object'&&typeof value!=='function')||mssdkSeen.has(value)) return;
      mssdkSeen.add(value);
      Object.keys(value).slice(0,128).forEach(function(name){{
        var child;try{{child=value[name];}}catch(e){{return;}}
        var childPath=path+'.'+name;
        if(typeof child==='function') mssdkFunctionPaths.push(childPath);
        else inspectMssdk(child,childPath,depth+1);
      }});
    }};
    inspectMssdk(mssdkObject,'_mssdk',0);
    var mssdkOwnFunctionPaths=[],mssdkAccessorPaths=[],mssdkOwnSeen=new WeakSet();
    var inspectMssdkOwn=function(value,path,depth){{
      if(!value||depth>4||(typeof value!=='object'&&typeof value!=='function')||mssdkOwnSeen.has(value)) return;
      mssdkOwnSeen.add(value);
      var names=[];
      try{{names=Object.getOwnPropertyNames(value).slice(0,128);}}catch(e){{return;}}
      for(var nameIndex=0;nameIndex<names.length;nameIndex++){{
        var name=names[nameIndex],descriptor=null;
        try{{descriptor=Object.getOwnPropertyDescriptor(value,name);}}catch(e){{continue;}}
        if(!descriptor) continue;
        var childPath=path+'.'+name;
        if(!Object.prototype.hasOwnProperty.call(descriptor,'value')){{
          if(mssdkAccessorPaths.length<256) mssdkAccessorPaths.push(childPath);
          continue;
        }}
        var child=descriptor.value, childType=typeof child;
        if(childType==='function'){{
          if(mssdkOwnFunctionPaths.length<256) mssdkOwnFunctionPaths.push(childPath);
        }}else if(child&&childType==='object'){{
          inspectMssdkOwn(child,childPath,depth+1);
        }}
      }}
    }};
    inspectMssdkOwn(mssdkObject,'_mssdk',0);
    frame.contentWindow.__ttlSdkCalls=[];
    var recordSdkReturn=function(name,result){{
      try{{
        var fields=[];
        var seenResults=new WeakSet();
        var inspectResult=function(value,path,depth){{
          var type=typeof value,bytes=0;
          if(type==='string') bytes=value.length;
          else if(value instanceof Uint8Array) bytes=value.byteLength;
          if(fields.length>=128) return;
          if(value&&type==='object'){{
            if(seenResults.has(value)||depth>3) return;
            seenResults.add(value);
            Object.keys(value).sort().slice(0,64).forEach(function(fieldName){{
              var child;try{{child=value[fieldName];}}catch(e){{return;}}
              inspectResult(child,path+'.'+fieldName,depth+1);
            }});
            return;
          }}
          fields.push({{name:path,type:type,bytes:bytes}});
        }};
        inspectResult(result,'$return',0);
        if(frame.contentWindow.__ttlSdkCalls.length<32)
          frame.contentWindow.__ttlSdkCalls.push({{name:name,fields:fields}});
      }}catch(e){{}}
    }};
    var wrapSdkFunction=function(name){{
      try{{
        var nativeFunction=sdk[name];
        if(typeof nativeFunction!=='function') return;
        sdk[name]=function(){{
          var result=nativeFunction.apply(this,arguments);
          recordSdkReturn(name,result);
          if(typeof result==='function'){{
            var callback=result;
            result=function(){{
              var callbackResult=callback.apply(this,arguments);
              recordSdkReturn(name+'#callback',callbackResult);
              return callbackResult;
            }};
          }}
          return result;
        }};
      }}catch(e){{}}
    }};
    ['frontierSign','registerWsSigner','init','report'].forEach(wrapSdkFunction);
    var counts={{}}, transitions={{}}, functionEntries={{}}, callEdges={{}}, opcodeCatalog={{}}, first=[], last=[];
    var total=0, minOffset=null, maxOffset=null, previous=null, rolling=2166136261;
    var states=new WeakMap(), nextStateId=1;
    var originals=vm.ops.slice();
    for(let opcode=0;opcode<originals.length;opcode++){{
      if(typeof originals[opcode]!=='function') continue;
      var sourceText=Function.prototype.toString.call(originals[opcode]);
      var handlerTags=[];
      if(sourceText.indexOf('R(n,')!==-1) handlerTags.push('read_register');
      if(sourceText.indexOf('Q(n,')!==-1) handlerTags.push('write_register');
      if(sourceText.indexOf('n.A')!==-1) handlerTags.push('read_context');
      if(sourceText.indexOf('M[')!==-1) handlerTags.push('read_string_table');
      if(sourceText.indexOf('S[')!==-1) handlerTags.push('read_numeric_table');
      if(sourceText.indexOf('throw')!==-1) handlerTags.push('throw');
      if(sourceText.indexOf('return')!==-1) handlerTags.push('return');
      opcodeCatalog[opcode]={{
        source_bytes:sourceText.length,
        helper_reads:{{N:(sourceText.match(/\bN\(/g)||[]).length,j:(sourceText.match(/\bj\(/g)||[]).length,x:(sourceText.match(/\bx\(/g)||[]).length}},
        calls_vm:sourceText.indexOf('L(')!==-1,
        reads_window:sourceText.indexOf('window')!==-1,
        reads_document:sourceText.indexOf('document')!==-1,
        reads_storage:sourceText.indexOf('Storage')!==-1||sourceText.indexOf('storage')!==-1,
        reads_crypto:sourceText.indexOf('crypto')!==-1,
        reads_fetch:sourceText.indexOf('fetch')!==-1,
        handler_tags:handlerTags,
        visited:0,
        operand_widths:{{}},
        examples:[]
      }};
      vm.ops[opcode]=(function(opcode,original){{return function(state){{
        var offset=state.o-2;
        var operandStart=state.o;
        state.__ttlOperandBytes=0;
        state.__ttlOperandValues=[];
        var stateMeta=states.get(state);
        if(!stateMeta){{
          var parentMeta=state.A&&typeof state.A==='object'?states.get(state.A):null;
          stateMeta={{id:nextStateId++,entry:offset,parent:parentMeta?parentMeta.entry:null}};
          states.set(state,stateMeta);
          functionEntries[offset]=(functionEntries[offset]||0)+1;
          var callEdge=(stateMeta.parent===null?'root':stateMeta.parent)+'>'+offset;
          callEdges[callEdge]=(callEdges[callEdge]||0)+1;
        }}
        total++;
        counts[opcode]=(counts[opcode]||0)+1;
        var catalog=opcodeCatalog[opcode];
        catalog.visited++;
        if(previous!==null){{var edge=previous+'>'+opcode;transitions[edge]=(transitions[edge]||0)+1;}}
        previous=opcode;
        minOffset=minOffset===null?offset:Math.min(minOffset,offset);
        maxOffset=maxOffset===null?offset:Math.max(maxOffset,offset);
        rolling=Math.imul((rolling^opcode)>>>0,16777619)>>>0;
        rolling=Math.imul((rolling^(offset&65535))>>>0,16777619)>>>0;
        var step={{offset:offset,opcode:opcode}};
        if(first.length<64) first.push(step);
        if(last.length===64) last.shift();
        last.push(step);
        var previousOpcode=activeOpcode;
        activeOpcode=opcode;
        var previousActiveEntry=frame.contentWindow.__ttlVmActiveEntry;
        var previousActiveOpcode=frame.contentWindow.__ttlVmActiveOpcode;
        var previousFunctionEntry=frame.contentWindow.__ttlVmFunctionEntry;
        frame.contentWindow.__ttlVmActiveEntry=offset;
        frame.contentWindow.__ttlVmActiveOpcode=opcode;
        frame.contentWindow.__ttlVmFunctionEntry=stateMeta.entry;
        var result;
        try {{ result=original(state); }} finally {{ activeOpcode=previousOpcode;frame.contentWindow.__ttlVmActiveEntry=previousActiveEntry;frame.contentWindow.__ttlVmActiveOpcode=previousActiveOpcode;frame.contentWindow.__ttlVmFunctionEntry=previousFunctionEntry; }}
        var width=state.__ttlOperandBytes||0;
        catalog.operand_widths[width]=(catalog.operand_widths[width]||0)+1;
        if(!frame.contentWindow.__ttlSourceDispatch&&frame.contentWindow.__ttlTraceEntries[String(stateMeta.entry)]&&frame.contentWindow.__ttlFunctionSteps.length<4096){{
          var stepBytes=[];
          for(var stepByteIndex=operandStart;stepByteIndex<Math.min(operandStart+width,operandStart+16);stepByteIndex++)
            stepBytes.push(vm.bytecode[stepByteIndex].toString(16).padStart(2,'0'));
          frame.contentWindow.__ttlFunctionSteps.push({{function_entry:stateMeta.entry,offset:offset,opcode:opcode,width:width,bytes:stepBytes.join(''),operands:state.__ttlOperandValues||[]}});
        }}
        if(catalog.examples.length<8){{
          var bytes=[];
          for(var byteIndex=operandStart;byteIndex<Math.min(operandStart+width,operandStart+16);byteIndex++)
            bytes.push(vm.bytecode[byteIndex].toString(16).padStart(2,'0'));
          catalog.examples.push({{offset:offset,width:width,bytes:bytes.join('')}});
        }}
        return result;
      }};}})(opcode,originals[opcode]);
    }}
    frame.contentWindow.__ttlVmPhase='init';
    fetchPhase='before_init';
    {initialization}
    fetchPhase='after_init';
    frame.contentWindow.__ttlVmPhase='after_init';
    var vmCallCountsBeforeInvocation={{}};
    Object.keys(frame.contentWindow.__ttlVmCalls.entries).forEach(function(entry){{
      vmCallCountsBeforeInvocation[entry]=frame.contentWindow.__ttlVmCalls.entries[entry].calls||0;
    }});
    fetchPhase='before_invocation';
    frame.contentWindow.__ttlVmPhase='invocation';
    {invocation}
    frame.contentWindow.__ttlVmPhase='done';
    var vmCallDelta={{}};
    Object.keys(frame.contentWindow.__ttlVmCalls.entries).forEach(function(entry){{
      var current=frame.contentWindow.__ttlVmCalls.entries[entry].calls||0;
      var delta=current-(vmCallCountsBeforeInvocation[entry]||0);
      if(delta>0) vmCallDelta[entry]=delta;
    }});
    var fetchMetadataAfterInit=typeof describeFetch==='function'?describeFetch(frame.contentWindow.fetch):null;
    if(fetchMetadataAfterInit) fetchMetadataAfterInit.phase=fetchPhase;
    var topTransitions=Object.keys(transitions).map(function(edge){{return {{edge:edge,count:transitions[edge]}};}})
      .sort(function(a,b){{return b.count-a.count||a.edge.localeCompare(b.edge);}}).slice(0,32);
    var topFunctionEntries=Object.keys(functionEntries).map(function(offset){{
      return {{offset:Number(offset),count:functionEntries[offset]}};
    }}).sort(function(a,b){{return b.count-a.count||a.offset-b.offset;}}).slice(0,128);
    var topCallEdges=Object.keys(callEdges).map(function(edge){{return {{edge:edge,count:callEdges[edge]}};}})
      .sort(function(a,b){{return b.count-a.count||a.edge.localeCompare(b.edge);}}).slice(0,128);
    return JSON.stringify({{
      product:{product_name},
      clock_ms:Date.now(),
      bytecode_bytes:vm.bytecode.length,
      opcode_table_slots:vm.ops.length,
      string_table_slots:vm.strings.length,
      numeric_constant_slots:vm.numbers.length,
      opcode_executions:total,
      distinct_opcodes:Object.keys(counts).length,
      distinct_opcode_counts:counts,
      top_transitions:topTransitions,
      function_invocations:nextStateId-1,
      top_function_entries:topFunctionEntries,
      top_call_edges:topCallEdges,
      opcode_catalog:opcodeCatalog,
      vm_call_entries:frame.contentWindow.__ttlVmCalls.entries,
      vm_call_delta:vmCallDelta,
      vm_call_sequence:frame.contentWindow.__ttlVmCalls.sequence,
      vm_call_invocation_sequence:frame.contentWindow.__ttlVmInvocation,
      vm_call_inputs:frame.contentWindow.__ttlVmInputs,
      vm_string_returns:frame.contentWindow.__ttlVmCalls.strings,
      decoded_string_slots:frame.contentWindow.__ttlDecodedTargets,
      decoded_string_uses:frame.contentWindow.__ttlDecodedUses,
      function_steps:frame.contentWindow.__ttlFunctionSteps,
      register_trace:frame.contentWindow.__ttlRegisterTrace,
      sdk_call_returns:frame.contentWindow.__ttlSdkCalls,
      known_string_slots:knownStringSlots,
      mssdk_keys:mssdkKeys,
      mssdk_functions:mssdkFunctions,
      mssdk_function_paths:mssdkFunctionPaths.slice(0,256),
      mssdk_own_function_paths:mssdkOwnFunctionPaths,
      mssdk_accessor_paths:mssdkAccessorPaths,
      fetch_descriptor_installed:typeof fetchDescriptorInstalled==='boolean'?fetchDescriptorInstalled:false,
      fetch_assignments:typeof fetchAssignments==='undefined'?[]:fetchAssignments,
      fetch_metadata_after_init:fetchMetadataAfterInit,
      min_visited_offset:minOffset,
      max_visited_offset:maxOffset,
      rolling_trace_hash:rolling.toString(16).padStart(8,'0'),
      first_steps:first,
      last_steps:last,
      result_parameters:parameters
      ,field_events:typeof fieldEvents==='undefined'?[]:fieldEvents
    }});
  }} finally {{ frame.remove(); }}
}})()"#
    ))
}

fn arguments() -> Result<(PathBuf, String, TraceProduct)> {
    let usage = "usage: ttl-sign-vm-trace <plan.json> <case-id> [frontier|fetch]";
    let mut args = std::env::args_os().skip(1);
    let plan = PathBuf::from(args.next().context(usage)?);
    let case_id = args.next().context(usage)?.to_string_lossy().into_owned();
    let product = match args.next().as_deref().and_then(|value| value.to_str()) {
        None | Some("frontier") => TraceProduct::Frontier,
        Some("fetch") => TraceProduct::Fetch,
        Some(_) => anyhow::bail!(usage),
    };
    if args.next().is_some() {
        anyhow::bail!(usage);
    }
    Ok((plan, case_id, product))
}

fn error_class(error: &ttl_sign_core::SignError) -> &'static str {
    use ttl_sign_core::SignError;
    match error {
        SignError::SdkNotReady => "sdk_not_ready",
        SignError::NoInstanceAvailable => "no_instance_available",
        SignError::BackendUnavailable(_) => "backend_unavailable",
        SignError::Bridge(_) => "bridge",
        SignError::EngineGone(_) => "engine_gone",
        SignError::LoginTimeout(_) => "login_timeout",
        SignError::Timeout(_) => "timeout",
        SignError::Transport(_) => "transport",
        SignError::Decode(_) => "decode",
        SignError::Refused(_) => "refused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_requires_one_known_vm_tail() {
        let helpers: String = VM_OPERAND_HELPERS
            .iter()
            .map(|(needle, _)| *needle)
            .collect::<Vec<_>>()
            .join(";");
        let register_helpers: String = VM_REGISTER_HELPERS
            .iter()
            .map(|(needle, _)| *needle)
            .collect::<Vec<_>>()
            .join(";");
        let source = format!(
            "{VM_CALL_HEAD}{helpers};{register_helpers};{STRING_DECODE_NEEDLE};{VM_DISPATCH_NEEDLE};{VM_CALL_TAIL};{VM_EXPORT_NEEDLE}"
        );
        let patched = patch_vm_export(&source).unwrap();
        assert_ne!(patched, source);
        assert!(patched.contains("__ttlVm"));
        assert!(patched.contains("__ttlVmCalls"));
        assert!(patched.contains("__ttlFunctionSteps"));
        assert!(patch_vm_export("not a VM").is_err());
    }

    #[test]
    fn generated_probe_never_embeds_an_unescaped_url() {
        let script = vm_trace_script(
            "bundle",
            "https://example.test/?secret=\"value\n",
            TraceProduct::Frontier,
        )
        .unwrap();
        assert!(!script.contains("secret=\"value\n"));
        assert!(script.contains("secret=\\\"value\\n"));
    }
}
