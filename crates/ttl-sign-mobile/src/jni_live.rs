//! JNI entry points for live streaming (`com.example.ttlsigner.TtlNative`).
//!
//! `nConnect` opens a [`LiveClient`](crate::live::LiveClient) whose worker thread pushes
//! states and event JSON back through the `TtlEvents` callback object. The worker is not
//! a Java thread, so every callback attaches it first ([`JavaVM::attach_current_thread`]
//! stays attached across calls — only the first attach pays). If the Kotlin callback
//! throws, the exception is described to logcat, cleared, and delivery continues: a UI
//! bug must not kill the stream.
//!
//! `nDisconnect` blocks until the worker has stopped (within ~1 s); call it from
//! `Dispatchers.IO`.

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{Global, JClass, JObject, JString};
use jni::sys::jlong;
use jni::{EnvUnowned, JavaVM};

use crate::live::{LiveClient, LiveSink};

/// Calls `TtlEvents.onEvent` / `onState` from the worker thread.
struct JniSink {
    vm: JavaVM,
    callback: Global<JObject<'static>>,
}

impl JniSink {
    /// Deliver one callback, attaching the worker thread first. The `jni_str!` /
    /// `jni_sig!` macros need literals, so the two methods keep their own call sites.
    fn call_event(&self, json: &str) {
        let outcome: std::result::Result<(), jni::errors::Error> =
            self.vm.attach_current_thread(|env| {
                let json = env.new_string(json)?;
                let result = env.call_method(
                    self.callback.as_obj(),
                    jni::jni_str!("onEvent"),
                    jni::jni_sig!("(Ljava/lang/String;)V"),
                    &[(&json).into()],
                );
                self.settle(env, result)
            });
        if let Err(error) = outcome {
            eprintln!("ttl-live-mobile: onEvent callback failed: {error}");
        }
    }

    fn call_state(&self, kind: &str, detail: &str) {
        let outcome: std::result::Result<(), jni::errors::Error> =
            self.vm.attach_current_thread(|env| {
                let kind = env.new_string(kind)?;
                let detail = env.new_string(detail)?;
                let result = env.call_method(
                    self.callback.as_obj(),
                    jni::jni_str!("onState"),
                    jni::jni_sig!("(Ljava/lang/String;Ljava/lang/String;)V"),
                    &[(&kind).into(), (&detail).into()],
                );
                self.settle(env, result)
            });
        if let Err(error) = outcome {
            eprintln!("ttl-live-mobile: onState callback failed: {error}");
        }
    }

    /// A throwing callback must not take the stream down with it: describe what
    /// happened for logcat, clear it, and keep delivering.
    fn settle(
        &self,
        env: &mut jni::Env,
        result: std::result::Result<jni::objects::JValueOwned<'_>, jni::errors::Error>,
    ) -> std::result::Result<(), jni::errors::Error> {
        if env.exception_check() {
            env.exception_describe();
            env.exception_clear();
            return Ok(());
        }
        result.map(|_| ())
    }
}

impl LiveSink for JniSink {
    fn on_state(&self, kind: &str, detail: &str) {
        self.call_state(kind, detail);
    }

    fn on_event(&self, json: &str) {
        self.call_event(json);
    }
}

/// Open a live event stream. Returns the handle, or 0 with an exception pending.
///
/// `bundle`/`optionsJson` feed the signer; `roomId` selects the room; `cookies` is the
/// guest session (`k=v; k=v`); `callback` is the `TtlEvents` implementation receiving
/// `onState`/`onEvent` from the worker thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_example_ttlsigner_TtlNative_nConnect<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _cls: JClass<'caller>,
    bundle: JString<'caller>,
    options_json: JString<'caller>,
    room_id: JString<'caller>,
    cookies: JString<'caller>,
    callback: JObject<'caller>,
) -> jlong {
    unowned
        .with_env(|env| -> Result<jlong> {
            let bundle: String = bundle.mutf8_chars(env)?.into();
            let options: String = options_json.mutf8_chars(env)?.into();
            let room: String = room_id.mutf8_chars(env)?.into();
            let cookies: String = cookies.mutf8_chars(env)?.into();
            if callback.is_null() {
                return Err(crate::MobileError::Sign("connect needs a callback".into()).into());
            }
            let vm = env.get_java_vm()?;
            let callback = env.new_global_ref(&callback)?;
            let sink = JniSink { vm, callback };
            let client = LiveClient::connect(&bundle, &options, &room, &cookies, sink)?;
            Ok(Box::into_raw(Box::new(client)) as jlong)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Stop the stream opened by `nConnect` and wait for its worker. Blocking; zero is a no-op.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_example_ttlsigner_TtlNative_nDisconnect<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _cls: JClass<'caller>,
    handle: jlong,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            if handle != 0 {
                // SAFETY: the handle came from `nConnect` and is disconnected once.
                let client = unsafe { Box::from_raw(handle as *mut LiveClient) };
                client.disconnect();
            }
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[derive(Debug, thiserror::Error)]
enum CallError {
    #[error(transparent)]
    Jni(#[from] jni::errors::Error),
    #[error(transparent)]
    Mobile(#[from] crate::MobileError),
}

type Result<T> = std::result::Result<T, CallError>;
