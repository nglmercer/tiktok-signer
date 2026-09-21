//! C ABI for callers that cannot speak JNI.
//!
//! All functions are panic-safe (a panic becomes a null return, never an unwind across
//! the boundary) and all strings cross as explicit pointer+length pairs — the bundle and
//! the signed URLs are not NUL-terminated on the way in.
//!
//! Ownership: `ttl_mobile_open` returns a handle freed by `ttl_mobile_close`;
//! `ttl_mobile_sign`, `ttl_mobile_socket_url`, and `ttl_mobile_user_agent` return
//! NUL-terminated strings freed by `ttl_mobile_free_str`. A null return means the call
//! failed; [`ttl_mobile_last_error`] says why.

use std::cell::RefCell;
#[cfg(test)]
use std::ffi::CStr;
use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::slice;
use std::str;

use crate::{MobileError, MobileSigner};

thread_local! {
    static LAST_ERROR: RefCell<CString> =
        RefCell::new(CString::new("no error recorded").expect("static"));
}

fn record_error(message: String) {
    let message = if message.is_empty() {
        "unknown error".to_string()
    } else {
        message
    };
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = CString::new(message)
            .unwrap_or_else(|_| CString::new("unrepresentable error").expect("static"));
    });
}

/// Read bytes the caller guarantees valid for `len`. Null on any violation.
fn borrow_bytes<'a>(ptr: *const c_char, len: usize) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees `ptr..ptr+len` is readable for this call.
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
    str::from_utf8(bytes).ok()
}

/// Move a Rust string across the boundary. The caller frees it with `ttl_mobile_free_str`.
fn hand_out(text: String) -> *mut c_char {
    match CString::new(text) {
        Ok(owned) => owned.into_raw(),
        Err(_) => {
            record_error("the signer produced a string with an interior NUL".into());
            ptr::null_mut()
        }
    }
}

/// Open a signer. `bundle` is the webmssdk source, `options` a [`crate::SignOptions`]
/// JSON object. Returns null on failure; see [`ttl_mobile_last_error`].
///
/// # Safety
///
/// `bundle..bundle+bundle_len` and `options..options+options_len` must be readable
/// for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ttl_mobile_open(
    bundle: *const c_char,
    bundle_len: usize,
    options: *const c_char,
    options_len: usize,
) -> *mut MobileSigner {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let bundle = borrow_bytes(bundle, bundle_len).ok_or_else(|| {
            MobileError::Bundle("bundle pointer/length are invalid or not UTF-8".into())
        })?;
        let options = borrow_bytes(options, options_len).ok_or_else(|| {
            MobileError::Options("options pointer/length are invalid or not UTF-8".into())
        })?;
        MobileSigner::new(bundle, options)
    }));
    match outcome {
        Ok(Ok(signer)) => Box::into_raw(Box::new(signer)),
        Ok(Err(error)) => {
            record_error(error.to_string());
            ptr::null_mut()
        }
        Err(_) => {
            record_error("the signer panicked while opening".into());
            ptr::null_mut()
        }
    }
}

/// Sign one URL. `product` is `fetch`, `frontier`, or `ws`. Returns a NUL-terminated
/// string the caller frees with [`ttl_mobile_free_str`], or null on failure.
///
/// # Safety
///
/// `handle` must come from [`ttl_mobile_open`] and be live; the string ranges must be
/// readable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ttl_mobile_sign(
    handle: *const MobileSigner,
    url: *const c_char,
    url_len: usize,
    product: *const c_char,
    product_len: usize,
) -> *mut c_char {
    if handle.is_null() {
        record_error("sign called on a null handle".into());
        return ptr::null_mut();
    }
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the caller guarantees the handle came from `ttl_mobile_open` and is live.
        let signer = unsafe { &*handle };
        let url = borrow_bytes(url, url_len)
            .ok_or_else(|| MobileError::Sign("url pointer/length are invalid".into()))?;
        let product = borrow_bytes(product, product_len)
            .ok_or_else(|| MobileError::Sign("product pointer/length are invalid".into()))?;
        let product: crate::Product = product.parse()?;
        signer.sign(url, product)
    }));
    match outcome {
        Ok(Ok(signed)) => hand_out(signed),
        Ok(Err(error)) => {
            record_error(error.to_string());
            ptr::null_mut()
        }
        Err(_) => {
            record_error("the signer panicked while signing".into());
            ptr::null_mut()
        }
    }
}

/// Close a signer opened by [`ttl_mobile_open`]. Null is a no-op.
///
/// # Safety
///
/// `handle` must come from [`ttl_mobile_open`] and be closed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ttl_mobile_close(handle: *mut MobileSigner) {
    if handle.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the handle came from `ttl_mobile_open` and is closed once.
        let _ = unsafe { Box::from_raw(handle) };
    }));
}

/// Free a string handed out by this library. Null is a no-op.
///
/// # Safety
///
/// `text` must come from this library's string-returning functions and be freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ttl_mobile_free_str(text: *mut c_char) {
    if text.is_null() {
        return;
    }
    // SAFETY: the pointer came from `hand_out` and is freed once.
    let _ = unsafe { CString::from_raw(text) };
}

/// Why the last call on this thread failed. Valid until the next failing call on the
/// same thread; never null.
#[unsafe(no_mangle)]
pub extern "C" fn ttl_mobile_last_error() -> *const c_char {
    LAST_ERROR.with(|slot| slot.borrow().as_ptr())
}

/// The unsigned direct-socket URL for `room_id` ([`crate::direct_socket_url`]).
/// An empty `device_id` mints one. The caller frees the result.
///
/// # Safety
///
/// Both string ranges must be readable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ttl_mobile_socket_url(
    room_id: *const c_char,
    room_id_len: usize,
    device_id: *const c_char,
    device_id_len: usize,
) -> *mut c_char {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        borrow_bytes(room_id, room_id_len).map(|room| {
            let device = borrow_bytes(device_id, device_id_len)
                .filter(|id| !id.is_empty())
                .map(Some)
                .unwrap_or(None);
            crate::direct_socket_url(room, device)
        })
    }));
    match outcome {
        Ok(Some(url)) => hand_out(url),
        _ => {
            record_error("socket_url arguments are invalid or not UTF-8".into());
            ptr::null_mut()
        }
    }
}

/// The User-Agent every request in the flow must present. The caller frees the result.
#[unsafe(no_mangle)]
pub extern "C" fn ttl_mobile_user_agent() -> *mut c_char {
    match catch_unwind(crate::user_agent) {
        Ok(ua) => hand_out(ua),
        Err(_) => {
            record_error("the signer panicked while reading the User-Agent".into());
            ptr::null_mut()
        }
    }
}

/// Read back a handed-out string in tests.
#[cfg(test)]
fn take_back(ptr: *mut c_char) -> String {
    assert!(!ptr.is_null(), "{}", unsafe {
        CStr::from_ptr(ttl_mobile_last_error())
            .to_string_lossy()
            .into_owned()
    });
    // SAFETY: `ptr` came from `hand_out` in this test and is freed once, here.
    let owned = unsafe { CString::from_raw(ptr) };
    owned.into_string().expect("valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_handles_fail_with_an_error_rather_than_crashing() {
        let url = c"http://x.invalid/";
        let product = c"ws";
        // SAFETY: the string ranges are live; the null handle is the case under test.
        let out = unsafe {
            ttl_mobile_sign(
                ptr::null(),
                url.as_ptr(),
                url.count_bytes(),
                product.as_ptr(),
                product.count_bytes(),
            )
        };
        assert!(out.is_null());
        let error = unsafe { CStr::from_ptr(ttl_mobile_last_error()) }
            .to_string_lossy()
            .into_owned();
        assert!(error.contains("null handle"), "unexpected error: {error}");

        // Null closes and frees are no-ops, not crashes.
        // SAFETY: null is explicitly a no-op for both.
        unsafe {
            ttl_mobile_close(ptr::null_mut());
            ttl_mobile_free_str(ptr::null_mut());
        }
    }

    #[test]
    fn pure_builders_cross_the_boundary() {
        let room = c"7300000000000000001";
        let device = c"1234567890123456789";
        // SAFETY: the string ranges are live for the call.
        let url = take_back(unsafe {
            ttl_mobile_socket_url(
                room.as_ptr(),
                room.count_bytes(),
                device.as_ptr(),
                device.count_bytes(),
            )
        });
        assert!(url.contains("room_id=7300000000000000001"), "url: {url}");
        assert!(url.contains("device_id=1234567890123456789"), "url: {url}");

        // An empty device id mints one rather than sending an empty parameter.
        let empty = c"";
        // SAFETY: the string ranges are live for the call.
        let url = take_back(unsafe {
            ttl_mobile_socket_url(room.as_ptr(), room.count_bytes(), empty.as_ptr(), 0)
        });
        let id = url
            .split("device_id=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        assert_eq!(id.len(), 19, "url: {url}");

        let ua = take_back(ttl_mobile_user_agent());
        assert!(ua.contains("Android"), "ua: {ua}");
    }
}
