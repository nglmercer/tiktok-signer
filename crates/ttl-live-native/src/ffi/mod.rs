use std::{
    cell::RefCell,
    ffi::{c_char, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    ptr,
};
mod client;
mod event;
mod schema;
pub use client::*;
pub use event::*;
pub use schema::*;

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultCode {
    Ok = 0,
    InvalidArgument = 1,
    InvalidUtf8 = 2,
    InvalidState = 3,
    Discovery = 4,
    Offline = 5,
    Signer = 6,
    Auth = 7,
    Socket = 8,
    Protocol = 9,
    Decode = 10,
    Timeout = 11,
    Shutdown = 12,
    Internal = 13,
}
thread_local! {
    static ERROR: RefCell<(ResultCode, CString)> = RefCell::new((ResultCode::Ok, CString::default()));
}
fn fail(code: ResultCode, message: &str) {
    ERROR.with(|e| *e.borrow_mut() = (code, cstring(message)));
}
fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "\\0")).expect("NUL escaped")
}
fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(_) => {
            fail(ResultCode::Internal, "native operation panicked");
            fallback
        }
    }
}
/// NULL plus zero length denotes empty input. Non-NULL inputs must reference
/// initialized memory for their length; handles must originate from this library.
unsafe fn bytes<'a>(p: *const u8, n: usize, max: usize) -> Option<&'a [u8]> {
    if n > max || (n != 0 && p.is_null()) {
        fail(
            ResultCode::InvalidArgument,
            "invalid pointer or input length",
        );
        return None;
    }
    Some(if n == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(p, n)
    })
}
unsafe fn text<'a>(p: *const c_char, n: usize, max: usize) -> Option<&'a str> {
    let b = bytes(p.cast(), n, max)?;
    match std::str::from_utf8(b) {
        Ok(s) => Some(s),
        Err(_) => {
            fail(ResultCode::InvalidUtf8, "input is not UTF-8");
            None
        }
    }
}
#[no_mangle]
pub extern "C" fn ttl_live_abi_version() -> u32 {
    guard(0, || 1)
}
#[no_mangle]
pub extern "C" fn ttl_live_version() -> *const c_char {
    guard(ptr::null(), || {
        concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
    })
}
#[no_mangle]
pub extern "C" fn ttl_last_error() -> *const c_char {
    guard(ptr::null(), || ERROR.with(|e| e.borrow().1.as_ptr()))
}
#[no_mangle]
pub extern "C" fn ttl_last_error_code() -> ResultCode {
    guard(ResultCode::Internal, || ERROR.with(|e| e.borrow().0))
}
#[no_mangle]
pub unsafe extern "C" fn ttl_string_free(p: *mut c_char) {
    guard((), || {
        if !p.is_null() {
            drop(CString::from_raw(p));
        }
    });
}
