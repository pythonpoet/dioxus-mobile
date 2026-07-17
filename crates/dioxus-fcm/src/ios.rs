use std::ffi::{c_char, CStr};

extern "C" {
    /// Implemented in Swift (FcmBridge.swift)
    fn fcm_request_token();
}

pub fn request_token() {
    unsafe { fcm_request_token() }
}

/// Called FROM Swift when APNs/FCM token is available.
#[no_mangle]
pub extern "C" fn fcm_on_token(token: *const c_char) {
    if token.is_null() {
        return;
    }
    let s = unsafe { CStr::from_ptr(token) }
        .to_string_lossy()
        .into_owned();
    crate::on_native_token(s);
}

#[no_mangle]
pub extern "C" fn fcm_on_error(msg: *const c_char) {
    if msg.is_null() {
        return;
    }
    let s = unsafe { CStr::from_ptr(msg) }
        .to_string_lossy()
        .into_owned();
    crate::on_native_error(s);
}
