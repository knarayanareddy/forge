use aether_core::default_daemon_addr;
use std::ffi::CString;
use std::os::raw::c_char;

#[no_mangle]
pub extern "C" fn aether_version() -> *const c_char {
    static VERSION: &str = "1.2.0-phase1\0";
    VERSION.as_ptr() as *const c_char
}

/// Phase 1 IPC contract: TCP JSON-lines on localhost (see docs/DAEMON_IPC.md).
#[no_mangle]
pub extern "C" fn aether_ffi_daemon_ipc() -> *mut c_char {
    let addr = aether_core::default_daemon_addr();
    let contract = format!("tcp-json-lines:{}", addr);
    CString::new(contract).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn aether_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn daemon_ipc_contract_is_tcp() {
        let ptr = aether_ffi_daemon_ipc();
        let s = unsafe { CStr::from_ptr(ptr).to_str().unwrap() };
        assert!(s.starts_with("tcp-json-lines:"));
        aether_free_string(ptr);
    }
}
