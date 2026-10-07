//! Raw Input API 监听器 — Windows 原始输入事件
//!
//! 使用 RegisterRawInputDevices 通过 raw FFI 调用，避免 windows-rs 版本冲突。
//! 创建隐藏消息窗口，在独立线程中运行消息循环。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

// Windows FFI 声明
#[cfg(target_os = "windows")]
#[allow(non_snake_case, non_camel_case_types, dead_code)]
mod win32 {
    use std::ffi::c_void;
    pub type HINSTANCE = *mut c_void;
    pub type HWND = *mut c_void;
    pub type HMODULE = *mut c_void;
    pub type HRAWINPUT = *mut c_void;
    pub type ATOM = u16;
    pub type LRESULT = isize;
    pub type WPARAM = usize;
    pub type LPARAM = isize;
    pub type LPVOID = *mut c_void;
    pub type DWORD = u32;
    pub type UINT = u32;
    #[allow(non_camel_case_types)]
    pub type LONG_PTR = isize;
    #[allow(non_camel_case_types)]
    pub type ULONG_PTR = usize;

    pub const WM_INPUT: u32 = 0x00FF;
    pub const WM_DESTROY: u32 = 0x0002;
    pub const WM_QUIT: u32 = 0x0012;
    pub const HWND_MESSAGE: isize = -3;
    pub const GWLP_USERDATA: i32 = -21;
    pub const RIDEV_INPUTSINK: DWORD = 0x100;
    pub const RIDEV_REMOVE: DWORD = 0x00000001;
    pub const RID_INPUT: DWORD = 0x10000003;
    pub const RIM_TYPEKEYBOARD: DWORD = 1;
    pub const RIM_TYPEMOUSE: DWORD = 0;
    pub const RI_KEY_BREAK: u16 = 1;
    pub const RI_MOUSE_LEFT_BUTTON_DOWN: u16 = 0x0001;
    pub const RI_MOUSE_LEFT_BUTTON_UP: u16 = 0x0002;
    pub const RI_MOUSE_RIGHT_BUTTON_DOWN: u16 = 0x0004;
    pub const RI_MOUSE_RIGHT_BUTTON_UP: u16 = 0x0008;
    pub const RI_MOUSE_MIDDLE_BUTTON_DOWN: u16 = 0x0010;
    pub const RI_MOUSE_MIDDLE_BUTTON_UP: u16 = 0x0020;

    #[repr(C)]
    pub struct RAWINPUTDEVICE {
        pub usUsagePage: u16,
        pub usUsage: u16,
        pub dwFlags: DWORD,
        pub hwndTarget: HWND,
    }

    #[repr(C)]
    pub struct RAWINPUTHEADER {
        pub dwType: DWORD,
        pub dwSize: DWORD,
        pub hDevice: *mut c_void,
        pub wParam: usize,
    }

    #[derive(Copy, Clone)]
    #[repr(C)]
    pub struct RAWKEYBOARD {
        pub MakeCode: u16,
        pub Flags: u16,
        pub Reserved: u16,
        pub VKey: u16,
        pub Message: UINT,
        pub ExtraInformation: DWORD,
    }

    #[derive(Copy, Clone)]
    #[repr(C)]
    pub struct RAWMOUSE {
        pub usFlags: u16,
        pub Anonymous: RAWMOUSE_UNION,
        pub ulRawButtons: DWORD,
        pub lLastX: i32,
        pub lLastY: i32,
        pub ulExtraInformation: DWORD,
    }

    #[derive(Copy, Clone)]
    #[repr(C)]
    pub union RAWMOUSE_UNION {
        pub ulButtons: DWORD,
        pub usButtonFlags: u16,
    }

    #[repr(C)]
    pub struct RAWINPUT {
        pub header: RAWINPUTHEADER,
        pub data: RAWINPUT_UNION,
    }

    #[repr(C)]
    pub union RAWINPUT_UNION {
        pub mouse: RAWMOUSE,
        pub keyboard: RAWKEYBOARD,
    }

    #[repr(C)]
    pub struct MSG {
        pub hwnd: HWND,
        pub message: UINT,
        pub wParam: WPARAM,
        pub lParam: LPARAM,
        pub time: DWORD,
        pub pt: POINT,
    }

    #[repr(C)]
    pub struct POINT { pub x: i32, pub y: i32 }

    #[repr(C)]
    pub struct WNDCLASSEXW {
        pub cbSize: UINT,
        pub style: UINT,
        pub lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>,
        pub cbClsExtra: i32,
        pub cbWndExtra: i32,
        pub hInstance: HINSTANCE,
        pub hIcon: *mut c_void,
        pub hCursor: *mut c_void,
        pub hbrBackground: *mut c_void,
        pub lpszMenuName: *const u16,
        pub lpszClassName: *const u16,
        pub hIconSm: *mut c_void,
    }

    extern "system" {
        pub fn GetModuleHandleW(lpModuleName: *const u16) -> HMODULE;
        pub fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> ATOM;
        pub fn UnregisterClassW(lpClassName: *const u16, hInstance: HINSTANCE) -> i32;
        pub fn CreateWindowExW(
            dwExStyle: DWORD, lpClassName: *const u16, lpWindowName: *const u16,
            dwStyle: DWORD, x: i32, y: i32, nWidth: i32, nHeight: i32,
            hWndParent: HWND, hMenu: *mut c_void, hInstance: HINSTANCE, lpParam: LPVOID,
        ) -> HWND;
        pub fn DestroyWindow(hWnd: HWND) -> i32;
        pub fn DefWindowProcW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        pub fn RegisterRawInputDevices(
            pRawInputDevices: *const RAWINPUTDEVICE, uiNumDevices: UINT, cbSize: UINT,
        ) -> i32;
        pub fn GetRawInputData(
            hRawInput: HRAWINPUT, uiCommand: UINT, pData: LPVOID,
            pcbSize: *mut UINT, cbSizeHeader: UINT,
        ) -> UINT;
        pub fn GetMessageW(
            lpMsg: *mut MSG, hWnd: HWND, wMsgFilterMin: UINT, wMsgFilterMax: UINT,
        ) -> i32;
        pub fn TranslateMessage(lpMsg: *const MSG) -> i32;
        pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
        pub fn GetCurrentThreadId() -> DWORD;
        pub fn GetLastError() -> DWORD;
        pub fn PostThreadMessageW(idThread: DWORD, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> i32;
        pub fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dwNewLong: LONG_PTR) -> LONG_PTR;
        pub fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> LONG_PTR;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_input_bridge_can_start_stop_and_start_again() {
        let mut bridge = RawInputBridge::new();
        assert!(!bridge.is_running());
        let first = bridge.start(|_| {});
        if cfg!(target_os = "windows") {
            assert!(first.is_ok(), "Raw Input startup failed: {first:?}");
            assert!(bridge.is_running());
            bridge.stop();
            assert!(!bridge.is_running());

            let second = bridge.start(|_| {});
            assert!(second.is_ok(), "Raw Input restart failed: {second:?}");
            bridge.stop();
            assert!(!bridge.is_running());
        } else {
            assert!(first.is_err());
        }
    }
}

// ============================================================
// 公共类型
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawInputDeviceType { Keyboard, Mouse, HID }

#[derive(Debug, Clone)]
pub struct RawInputEvent {
    pub device_type: RawInputDeviceType,
    pub usage_id: u16,
    pub usage_page: u16,
    pub pressed: bool,
    pub device_handle: u64,
    pub delta_x: i32,
    pub delta_y: i32,
}

type EventCallback = Arc<Mutex<dyn FnMut(RawInputEvent) + Send + 'static>>;

pub struct RawInputBridge {
    running: Arc<AtomicBool>,
    thread_id: Arc<AtomicU32>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl RawInputBridge {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread_id: Arc::new(AtomicU32::new(0)),
            thread_handle: None,
        }
    }

    pub fn start<F>(&mut self, callback: F) -> Result<(), String>
    where F: FnMut(RawInputEvent) + Send + 'static
    {
        if self.running.load(Ordering::SeqCst) {
            return Err("RawInputBridge 已在运行".into());
        }
        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let thread_id = Arc::clone(&self.thread_id);
        let cb: EventCallback = Arc::new(Mutex::new(callback));
        let (startup_tx, startup_rx) = mpsc::channel();

        self.thread_handle = Some(thread::spawn(move || {
            #[cfg(target_os = "windows")]
            raw_input_thread_impl(running, thread_id, cb, startup_tx);
            #[cfg(not(target_os = "windows"))]
            {
                log::warn!("RawInput only on Windows");
                let _ = startup_tx.send(Err("RawInput only on Windows".into()));
                let _ = (running, thread_id, cb);
            }
        }));

        match startup_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => {
                log::info!("RawInputBridge started");
                Ok(())
            }
            Ok(Err(error)) => {
                if let Some(handle) = self.thread_handle.take() {
                    let _ = handle.join();
                }
                self.thread_id.store(0, Ordering::SeqCst);
                self.running.store(false, Ordering::SeqCst);
                Err(error)
            }
            Err(_) => {
                self.running.store(false, Ordering::SeqCst);
                let thread_id = self.thread_id.load(Ordering::SeqCst);
                #[cfg(target_os = "windows")]
                if thread_id != 0 {
                    unsafe { win32::PostThreadMessageW(thread_id, win32::WM_QUIT, 0, 0); }
                }
                if let Some(handle) = self.thread_handle.take() {
                    let _ = handle.join();
                }
                self.thread_id.store(0, Ordering::SeqCst);
                Err("RawInputBridge startup timed out".into())
            }
        }
    }

    pub fn stop(&mut self) {
        if !self.running.load(Ordering::SeqCst) && self.thread_handle.is_none() {
            return;
        }
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
        log::info!("RawInputBridge stopped");
    }

    pub fn is_running(&self) -> bool { self.running.load(Ordering::SeqCst) }
}

// ============================================================
// Windows 实现
// ============================================================

#[cfg(target_os = "windows")]
fn raw_input_thread_impl(
    running: Arc<AtomicBool>,
    thread_id: Arc<AtomicU32>,
    callback: EventCallback,
    startup_tx: mpsc::Sender<Result<(), String>>,
) {
    use win32::*;
    use std::mem;
    use std::ptr;

    // 全局 WNDPROC 存储（通过 GWLP_USERDATA 传递）
    // 由于 extern fn 不能捕获闭包，使用线程局部存储或 GWLP_USERDATA

    let class_name: Vec<u16> = "RawInputBridgeClass\0".encode_utf16().collect();
    let window_name: Vec<u16> = "RawInputBridge\0".encode_utf16().collect();

    let hinstance = unsafe { GetModuleHandleW(ptr::null()) };

    let mut wc: WNDCLASSEXW = unsafe { mem::zeroed() };
    wc.cbSize = mem::size_of::<WNDCLASSEXW>() as u32;
    wc.lpfnWndProc = Some(raw_input_wndproc);
    wc.hInstance = hinstance;
    wc.lpszClassName = class_name.as_ptr();

    if unsafe { RegisterClassExW(&wc) } == 0 {
        let error = unsafe { GetLastError() };
        log::error!("RegisterClassExW failed error={error}");
        let _ = startup_tx.send(Err(format!("RegisterClassExW failed error={error}")));
        running.store(false, Ordering::SeqCst);
        thread_id.store(0, Ordering::SeqCst);
        return;
    }

    let hwnd = unsafe {
        CreateWindowExW(
            0, class_name.as_ptr(), window_name.as_ptr(),
            0, 0, 0, 0, 0,
            HWND_MESSAGE as HWND, ptr::null_mut(), hinstance, ptr::null_mut(),
        )
    };

    if hwnd.is_null() {
        let error = unsafe { GetLastError() };
        log::error!("CreateWindowExW failed error={error}");
        unsafe { UnregisterClassW(class_name.as_ptr(), hinstance); }
        let _ = startup_tx.send(Err(format!("CreateWindowExW failed error={error}")));
        running.store(false, Ordering::SeqCst);
        thread_id.store(0, Ordering::SeqCst);
        return;
    }
    // Creating the message-only window establishes this thread's message queue.
    // Publish the id only after PostThreadMessageW can reliably wake it.
    thread_id.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

    // Store callback Arc pointer in GWLP_USERDATA
    let cb_raw = Arc::into_raw(Arc::new(callback));
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, cb_raw as LONG_PTR); }

    // Register raw input devices
    let devices = [
        RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06,      // Keyboard
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        },
        RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x02,      // Mouse
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        },
    ];

    if unsafe {
        RegisterRawInputDevices(devices.as_ptr(), devices.len() as u32, mem::size_of::<RAWINPUTDEVICE>() as u32)
    } == 0 {
        let error = unsafe { GetLastError() };
        log::error!("RegisterRawInputDevices failed error={error}");
        unsafe {
            let cb_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if cb_ptr != 0 {
                let _ = Arc::from_raw(cb_ptr as *const EventCallback);
            }
            DestroyWindow(hwnd);
            UnregisterClassW(class_name.as_ptr(), hinstance);
        }
        let _ = startup_tx.send(Err(format!("RegisterRawInputDevices failed error={error}")));
        running.store(false, Ordering::SeqCst);
        thread_id.store(0, Ordering::SeqCst);
        return;
    }

    log::info!("RawInput registered, message loop started");
    let _ = startup_tx.send(Ok(()));

    // Message loop
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

    // Cleanup
    unsafe {
        let remove_devices = [
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x06,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: ptr::null_mut(),
            },
            RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x02,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: ptr::null_mut(),
            },
        ];
        if RegisterRawInputDevices(remove_devices.as_ptr(), remove_devices.len() as u32, mem::size_of::<RAWINPUTDEVICE>() as u32) == 0 {
            log::warn!(
                "RegisterRawInputDevices remove failed error={}",
                GetLastError()
            );
        }
        let cb_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        if cb_ptr != 0 {
            let _ = Arc::from_raw(cb_ptr as *const EventCallback);
        }
        DestroyWindow(hwnd);
        if UnregisterClassW(class_name.as_ptr(), hinstance) == 0 {
            log::warn!("UnregisterClassW failed error={}", GetLastError());
        }
    }

    log::info!("RawInput message loop exited");
    running.store(false, Ordering::SeqCst);
    thread_id.store(0, Ordering::SeqCst);

    // ======== Window Procedure ========
    unsafe extern "system" fn raw_input_wndproc(
        hwnd: HWND, msg: UINT, w_param: WPARAM, l_param: LPARAM,
    ) -> LRESULT {
        if msg == WM_INPUT {
            let cb_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if cb_ptr != 0 {
                let cb_arc = &*(cb_ptr as *const EventCallback);

                let mut size: UINT = 0;
                let header_size = mem::size_of::<RAWINPUTHEADER>() as UINT;

                if GetRawInputData(l_param as HRAWINPUT, RID_INPUT, ptr::null_mut(), &mut size, header_size) == 0
                {
                    let mut buf: Vec<u8> = vec![0u8; size as usize];
                    let written = GetRawInputData(
                        l_param as HRAWINPUT, RID_INPUT,
                        buf.as_mut_ptr() as LPVOID, &mut size, header_size,
                    );
                    if written == size {
                        let raw = &*(buf.as_ptr() as *const RAWINPUT);
                        let device = raw.header.hDevice as u64;

                        let mut events = Vec::new();
                        if raw.header.dwType == RIM_TYPEKEYBOARD {
                            let kb = &raw.data.keyboard;
                            let vk = kb.VKey;
                            let pressed = (kb.Flags & RI_KEY_BREAK) == 0;
                            events.push(RawInputEvent {
                                device_type: RawInputDeviceType::Keyboard,
                                usage_id: vk, usage_page: 0x01, pressed,
                                device_handle: device, delta_x: 0, delta_y: 0,
                            });
                        } else if raw.header.dwType == RIM_TYPEMOUSE {
                            let mouse = &raw.data.mouse;
                            let flags = unsafe { mouse.Anonymous.usButtonFlags };

                            if (flags & RI_MOUSE_LEFT_BUTTON_DOWN) != 0 {
                                events.push(RawInputEvent {
                                    device_type: RawInputDeviceType::Mouse,
                                    usage_id: 1, usage_page: 0x01, pressed: true,
                                    device_handle: device, delta_x: 0, delta_y: 0,
                                });
                            }
                            if (flags & RI_MOUSE_LEFT_BUTTON_UP) != 0 {
                                events.push(RawInputEvent {
                                    device_type: RawInputDeviceType::Mouse,
                                    usage_id: 1, usage_page: 0x01, pressed: false,
                                    device_handle: device, delta_x: 0, delta_y: 0,
                                });
                            }
                            if (flags & RI_MOUSE_RIGHT_BUTTON_DOWN) != 0 {
                                events.push(RawInputEvent {
                                    device_type: RawInputDeviceType::Mouse,
                                    usage_id: 2, usage_page: 0x01, pressed: true,
                                    device_handle: device, delta_x: 0, delta_y: 0,
                                });
                            }
                            if (flags & RI_MOUSE_RIGHT_BUTTON_UP) != 0 {
                                events.push(RawInputEvent {
                                    device_type: RawInputDeviceType::Mouse,
                                    usage_id: 2, usage_page: 0x01, pressed: false,
                                    device_handle: device, delta_x: 0, delta_y: 0,
                                });
                            }
                            if mouse.lLastX != 0 || mouse.lLastY != 0 {
                                events.push(RawInputEvent {
                                    device_type: RawInputDeviceType::Mouse,
                                    usage_id: 0, usage_page: 0x01, pressed: false,
                                    device_handle: device,
                                    delta_x: mouse.lLastX, delta_y: mouse.lLastY,
                                });
                            }
                        }
                        if !events.is_empty() {
                            if let Ok(mut cb) = cb_arc.lock() {
                                for event in events {
                                    cb(event);
                                }
                            }
                        }
                    }
                }
            }
            return 0;
        }
        DefWindowProcW(hwnd, msg, w_param, l_param)
    }
}
