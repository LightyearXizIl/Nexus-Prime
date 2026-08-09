//! 物理热键监视器 — WH_KEYBOARD_LL 低级键盘钩子
//!
//! 使用 SetWindowsHookExW 安装钩子，过滤 SendInput 注入事件，回调物理按键。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind { Press, Release }

#[derive(Debug, Clone)]
pub struct PhysicalKeyEvent {
    pub vk_code: u32,
    pub scan_code: u32,
    pub kind: KeyEventKind,
}

type HookCallback = Arc<Mutex<dyn FnMut(PhysicalKeyEvent) + Send + 'static>>;

pub struct PhysicalHotkeyMonitor {
    running: Arc<AtomicBool>,
    thread_id: Arc<AtomicU32>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl PhysicalHotkeyMonitor {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread_id: Arc::new(AtomicU32::new(0)),
            thread_handle: None,
        }
    }

    pub fn start<F>(&mut self, callback: F)
    where F: FnMut(PhysicalKeyEvent) + Send + 'static
    {
        if self.running.load(Ordering::SeqCst) { return; }
        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let thread_id = Arc::clone(&self.thread_id);
        let cb: HookCallback = Arc::new(Mutex::new(callback));
        self.thread_handle = Some(thread::spawn(move || {
            #[cfg(target_os = "windows")]
            hook_thread_impl(running, thread_id, cb);
            #[cfg(not(target_os = "windows"))]
            { log::warn!("HotkeyMonitor only on Windows"); let _ = (running, thread_id, cb); }
        }));
        log::info!("PhysicalHotkeyMonitor started");
    }

    pub fn stop(&mut self) {
        if !self.running.load(Ordering::SeqCst) { return; }
        self.running.store(false, Ordering::SeqCst);
        #[cfg(target_os = "windows")]
        {
            let thread_id = self.thread_id.load(Ordering::SeqCst);
            if thread_id != 0 {
                unsafe { win32::PostThreadMessageW(thread_id, win32::WM_QUIT, 0, 0); }
            }
        }
        if let Some(h) = self.thread_handle.take() { let _ = h.join(); }
        self.thread_id.store(0, Ordering::SeqCst);
        log::info!("PhysicalHotkeyMonitor stopped");
    }

    pub fn is_running(&self) -> bool { self.running.load(Ordering::SeqCst) }
}

impl Default for PhysicalHotkeyMonitor {
    fn default() -> Self { Self::new() }
}

impl Drop for PhysicalHotkeyMonitor {
    fn drop(&mut self) { if self.is_running() { self.stop(); } }
}

// ============================================================
// Windows raw FFI
// ============================================================

#[cfg(target_os = "windows")]
#[allow(non_snake_case, non_camel_case_types, dead_code)]
mod win32 {
    use std::ffi::c_void;
    pub type HINSTANCE = *mut c_void;
    pub type HMODULE = *mut c_void;
    pub type HHOOK = *mut c_void;
    pub type LRESULT = isize;
    pub type WPARAM = usize;
    pub type LPARAM = isize;
    pub type DWORD = u32;
    pub type UINT = u32;
    pub type LONG_PTR = isize;

    pub const WM_QUIT: u32 = 0x0012;
    pub const WM_KEYDOWN: u32 = 0x0100;
    pub const WM_KEYUP: u32 = 0x0101;
    pub const WM_SYSKEYDOWN: u32 = 0x0104;
    pub const WM_SYSKEYUP: u32 = 0x0105;
    pub const WH_KEYBOARD_LL: i32 = 13;
    pub const LLKHF_INJECTED: u32 = 0x10;
    pub const PM_NOREMOVE: u32 = 0x0000;

    #[repr(C)]
    pub struct KBDLLHOOKSTRUCT {
        pub vkCode: DWORD,
        pub scanCode: DWORD,
        pub flags: DWORD,
        pub time: DWORD,
        pub dwExtraInfo: usize,
    }

    #[repr(C)]
    pub struct MSG {
        pub hwnd: *mut c_void,
        pub message: UINT,
        pub wParam: WPARAM,
        pub lParam: LPARAM,
        pub time: DWORD,
        pub pt: POINT,
    }

    #[repr(C)]
    pub struct POINT { pub x: i32, pub y: i32 }

    extern "system" {
        pub fn GetModuleHandleW(lpModuleName: *const u16) -> HMODULE;
        pub fn GetCurrentThreadId() -> DWORD;
        pub fn SetWindowsHookExW(
            idHook: i32, lpfn: Option<unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT>,
            hmod: HINSTANCE, dwThreadId: DWORD,
        ) -> HHOOK;
        pub fn UnhookWindowsHookEx(hhk: HHOOK) -> i32;
        pub fn CallNextHookEx(hhk: HHOOK, nCode: i32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        pub fn GetMessageW(lpMsg: *mut MSG, hWnd: *mut c_void, wMsgFilterMin: UINT, wMsgFilterMax: UINT) -> i32;
        pub fn PeekMessageW(lpMsg: *mut MSG, hWnd: *mut c_void, wMsgFilterMin: UINT, wMsgFilterMax: UINT, wRemoveMsg: UINT) -> i32;
        pub fn TranslateMessage(lpMsg: *const MSG) -> i32;
        pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
        pub fn PostThreadMessageW(idThread: DWORD, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> i32;
    }
}

// Global storage for hook callback
static mut HOOK_CALLBACK: *const HookCallback = std::ptr::null();

#[cfg(target_os = "windows")]
fn set_hook_callback(ptr: *const HookCallback) { unsafe { HOOK_CALLBACK = ptr; } }

#[cfg(target_os = "windows")]
fn get_hook_callback() -> *const HookCallback { unsafe { HOOK_CALLBACK } }

#[cfg(target_os = "windows")]
fn hook_thread_impl(running: Arc<AtomicBool>, thread_id: Arc<AtomicU32>, callback: HookCallback) {
    use win32::*;
    use std::mem;
    use std::ptr;

    // Create this thread's message queue before publishing its id, otherwise an
    // immediate stop can race with the first GetMessageW and lose WM_QUIT.
    let mut bootstrap: MSG = unsafe { mem::zeroed() };
    unsafe { PeekMessageW(&mut bootstrap, ptr::null_mut(), 0, 0, PM_NOREMOVE); }
    thread_id.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

    let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
    let cb_raw = Arc::into_raw(Arc::new(callback));
    set_hook_callback(cb_raw);

    let hook = unsafe {
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_proc), hinstance, 0)
    };

    if hook.is_null() {
        log::error!("SetWindowsHookExW failed");
        unsafe { let _ = Arc::from_raw(cb_raw); }
        set_hook_callback(ptr::null());
        thread_id.store(0, Ordering::SeqCst);
        return;
    }

    log::debug!("WH_KEYBOARD_LL hook installed");

    let mut msg: MSG = unsafe { mem::zeroed() };
    loop {
        if !running.load(Ordering::SeqCst) { break; }
        let ret = unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) };
        if ret <= 0 { break; }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe { UnhookWindowsHookEx(hook); }
    log::debug!("WH_KEYBOARD_LL hook uninstalled");

    let old = get_hook_callback();
    if !old.is_null() {
        unsafe { let _ = Arc::from_raw(old as *const HookCallback); }
        set_hook_callback(ptr::null());
    }
    thread_id.store(0, Ordering::SeqCst);

    unsafe extern "system" fn low_level_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
        if n_code >= 0 {
            let kb = &*(l_param as *const KBDLLHOOKSTRUCT);
            let is_injected = (kb.flags & LLKHF_INJECTED) != 0;
            if !is_injected {
                let cb_ptr = get_hook_callback();
                if !cb_ptr.is_null() {
                    let cb_arc = unsafe { &*(cb_ptr as *const HookCallback) };
                    if let Ok(mut cb) = cb_arc.lock() {
                        let kind = match w_param as u32 {
                            WM_KEYDOWN | WM_SYSKEYDOWN => KeyEventKind::Press,
                            WM_KEYUP | WM_SYSKEYUP => KeyEventKind::Release,
                            _ => return unsafe { CallNextHookEx(ptr::null_mut(), n_code, w_param, l_param) },
                        };
                        cb(PhysicalKeyEvent { vk_code: kb.vkCode, scan_code: kb.scanCode, kind });
                    }
                }
            }
        }
        unsafe { CallNextHookEx(ptr::null_mut(), n_code, w_param, l_param) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_create_monitor() { assert!(!PhysicalHotkeyMonitor::new().is_running()); }
    #[test]
    fn test_start_stop() {
        let mut m = PhysicalHotkeyMonitor::new();
        m.start(|_| {});
        assert!(m.is_running());
        std::thread::sleep(std::time::Duration::from_millis(50));
        m.stop();
        assert!(!m.is_running());
    }
}
