//! macOS: matikan App Nap selama stream berjalan (moonlight-qt melakukan hal yang sama lewat `NSProcessInfo`).
//! Tanpa ini, saat jendela stream tertutup jendela lain (mis. chat TikTok LIVE di atasnya), macOS boleh memperlambat
//! timer dan jaringan proses ini: gambar yang diambil Window Capture jadi patah-patah/beku dan stream tertinggal.
//! Jendela yang di-MINIMIZE tetap tidak bisa diambil aplikasi perekam mana pun (macOS tidak menggambar isinya).

use std::ffi::{c_char, c_void};

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

#[link(name = "Foundation", kind = "framework")]
extern "C" {}

/// NSActivityUserInitiated (0x00FFFFFF, termasuk mencegah tidur sistem) | NSActivityLatencyCritical (0xFF00000000).
const OPTIONS: u64 = 0x00FF_FFFF | 0xFF_0000_0000;

/// Selama nilai ini hidup, App Nap mati untuk proses ini.
pub struct Activity {
    token: *mut c_void,
}

unsafe impl Send for Activity {}

impl Activity {
    pub fn begin(reason: &str) -> Option<Activity> {
        unsafe {
            let pool = objc_autoreleasePoolPush();
            let send0: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let send_str: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> *mut c_void = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let send_begin: unsafe extern "C" fn(*mut c_void, *mut c_void, u64, *mut c_void) -> *mut c_void = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let info = send0(objc_getClass(c"NSProcessInfo".as_ptr()), sel_registerName(c"processInfo".as_ptr()));
            let reason = std::ffi::CString::new(reason).unwrap_or_default();
            let text = send_str(objc_getClass(c"NSString".as_ptr()), sel_registerName(c"stringWithUTF8String:".as_ptr()), reason.as_ptr());
            let token = if info.is_null() || text.is_null() {
                std::ptr::null_mut()
            } else {
                send_begin(info, sel_registerName(c"beginActivityWithOptions:reason:".as_ptr()), OPTIONS, text)
            };
            // Token dikembalikan autoreleased: simpan (retain) sampai stream selesai.
            let token = if token.is_null() { token } else { send0(token, sel_registerName(c"retain".as_ptr())) };
            objc_autoreleasePoolPop(pool);
            (!token.is_null()).then_some(Activity { token })
        }
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        unsafe {
            let pool = objc_autoreleasePoolPush();
            let send0: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let send1: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let info = send0(objc_getClass(c"NSProcessInfo".as_ptr()), sel_registerName(c"processInfo".as_ptr()));
            if !info.is_null() {
                send1(info, sel_registerName(c"endActivity:".as_ptr()), self.token);
            }
            let _ = send0(self.token, sel_registerName(c"release".as_ptr()));
            objc_autoreleasePoolPop(pool);
        }
    }
}
