//! JNI entry points for the Java SDK and the compatibility Android demo.
//!
//! The Kotlin side holds an opaque `Long` handle; zero means the open failed. Every error —
//! Rust or JNI — arrives in Kotlin as a `RuntimeException` via
//! [`ThrowRuntimeExAndDefault`](jni::errors::ThrowRuntimeExAndDefault), never as a crash.
//! Signing blocks the calling thread: call it from `Dispatchers.IO`, not the UI thread.

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JObject, JString};
use jni::sys::jlong;
use jni::{Env, EnvUnowned};

use crate::{MobileError, MobileSigner};

/// Everything a native call can fail with. `ThrowRuntimeExAndDefault` turns it into a
/// `RuntimeException` carrying the message, and returns the default (0 / null).
#[derive(Debug, thiserror::Error)]
enum CallError {
    #[error(transparent)]
    Jni(#[from] jni::errors::Error),
    #[error(transparent)]
    Mobile(#[from] MobileError),
}

type Result<T> = std::result::Result<T, CallError>;

fn read(env: &Env, value: &JString) -> Result<String> {
    Ok(value.mutf8_chars(env)?.into())
}

/// Open a signer. `bundle` is the webmssdk source, `optionsJson` a
/// [`crate::SignOptions`] object. Returns the handle, or 0 with an exception pending.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_nglmercer_tiktoklive_internal_NativeBindings_nOpen<
    'caller,
>(
    mut unowned: EnvUnowned<'caller>,
    _cls: JClass<'caller>,
    bundle: JString<'caller>,
    options_json: JString<'caller>,
) -> jlong {
    unowned
        .with_env(|env| -> Result<jlong> {
            let bundle = read(env, &bundle)?;
            let options = read(env, &options_json)?;
            let signer = MobileSigner::new(&bundle, &options)?;
            Ok(Box::into_raw(Box::new(signer)) as jlong)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Sign one URL. `product` is `fetch`, `frontier`, or `ws`. Returns the signed URL
/// as a Java string, or null with an exception pending.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_nglmercer_tiktoklive_internal_NativeBindings_nSign<
    'caller,
>(
    mut unowned: EnvUnowned<'caller>,
    _cls: JClass<'caller>,
    handle: jlong,
    url: JString<'caller>,
    product: JString<'caller>,
) -> JObject<'caller> {
    unowned
        .with_env(|env| -> Result<JObject<'caller>> {
            if handle == 0 {
                return Err(MobileError::Sign("sign on a closed signer".into()).into());
            }
            // SAFETY: the handle came from `nOpen` and Kotlin closes it once.
            let signer = unsafe { &*(handle as *const MobileSigner) };
            let url = read(env, &url)?;
            let product = read(env, &product)?;
            let signed = signer.sign(&url, product.parse()?)?;
            Ok(env.new_string(signed)?.into())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Close a signer opened by `nOpen`. Zero is a no-op.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_nglmercer_tiktoklive_internal_NativeBindings_nClose<
    'caller,
>(
    mut unowned: EnvUnowned<'caller>,
    _cls: JClass<'caller>,
    handle: jlong,
) {
    // No Env needed, but `with_env` is still the panic-safe boundary: an unwind into
    // the JVM aborts the process.
    unowned
        .with_env(|_env| -> Result<()> {
            if handle != 0 {
                // SAFETY: the handle came from `nOpen` and is closed once.
                let _ = unsafe { Box::from_raw(handle as *mut MobileSigner) };
            }
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// The unsigned direct-socket URL for `roomId`. An empty `deviceId` mints one.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_example_ttlsigner_TtlNative_nSocketUrl<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _cls: JClass<'caller>,
    room_id: JString<'caller>,
    device_id: JString<'caller>,
) -> JObject<'caller> {
    unowned
        .with_env(|env| -> Result<JObject<'caller>> {
            let room = read(env, &room_id)?;
            let device = read(env, &device_id)?;
            let device = (!device.is_empty()).then_some(device.as_str());
            Ok(env
                .new_string(crate::direct_socket_url(&room, device))?
                .into())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// The User-Agent every request in the flow must present.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_example_ttlsigner_TtlNative_nUserAgent<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _cls: JClass<'caller>,
) -> JObject<'caller> {
    unowned
        .with_env(|env| -> Result<JObject<'caller>> {
            Ok(env.new_string(crate::user_agent())?.into())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `ttl-sign-mobile <version> (<engine>)`, for the demo app's about line.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_example_ttlsigner_TtlNative_nVersion<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _cls: JClass<'caller>,
) -> JObject<'caller> {
    unowned
        .with_env(|env| -> Result<JObject<'caller>> {
            let version = format!("ttl-sign-mobile {} ({})", crate::VERSION, crate::ENGINE);
            Ok(env.new_string(version)?.into())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

// Android demo compatibility namespace. The SDK entry points above are the implementation;
// both namespaces enter the same panic-safe JNI implementation and Rust signing engine.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_example_ttlsigner_TtlNative_nOpen<'caller>(
    unowned: EnvUnowned<'caller>,
    cls: JClass<'caller>,
    bundle: JString<'caller>,
    options: JString<'caller>,
) -> jlong {
    Java_io_github_nglmercer_tiktoklive_internal_NativeBindings_nOpen(unowned, cls, bundle, options)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_example_ttlsigner_TtlNative_nSign<'caller>(
    unowned: EnvUnowned<'caller>,
    cls: JClass<'caller>,
    handle: jlong,
    url: JString<'caller>,
    product: JString<'caller>,
) -> JObject<'caller> {
    Java_io_github_nglmercer_tiktoklive_internal_NativeBindings_nSign(
        unowned, cls, handle, url, product,
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_example_ttlsigner_TtlNative_nClose<'caller>(
    unowned: EnvUnowned<'caller>,
    cls: JClass<'caller>,
    handle: jlong,
) {
    Java_io_github_nglmercer_tiktoklive_internal_NativeBindings_nClose(unowned, cls, handle)
}
