use std::ffi::CString;
use std::os::raw::c_char;

#[no_mangle]
pub extern "C" fn aether_version() -> *const c_char {
    static VERSION: &str = "1.4.0-phase4\0";
    VERSION.as_ptr() as *const c_char
}

fn default_daemon_port() -> u16 {
    std::env::var("AETHER_DAEMON_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7433)
}

fn default_daemon_addr() -> String {
    std::env::var("AETHER_DAEMON_ADDR")
        .unwrap_or_else(|_| format!("127.0.0.1:{}", default_daemon_port()))
}

/// Default TCP port for aether-daemon (override via AETHER_DAEMON_PORT).
#[no_mangle]
pub extern "C" fn aether_daemon_default_port() -> u16 {
    default_daemon_port()
}

/// Phase 1 IPC contract: TCP JSON-lines on localhost (see docs/DAEMON_IPC.md).
#[no_mangle]
pub extern "C" fn aether_ffi_daemon_ipc() -> *mut c_char {
    let contract = format!("tcp-json-lines:{}", default_daemon_addr());
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

    #[test]
    fn default_port_is_7433() {
        assert_eq!(default_daemon_port(), 7433);
    }
}
