//! Activate the ChatGPT desktop app before injecting its dictation shortcut.

#[cfg(not(target_os = "windows"))]
pub fn activate_chatgpt_and_focus_input() -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn launch_or_activate_app(_path: &str) -> bool {
    false
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::Duration;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Descendants,
        UIA_EditControlTypeId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindowAsync,
        SwitchToThisWindow, SW_RESTORE, SW_SHOW,
    };

    const CHATGPT_APP_ID: &str = r"shell:AppsFolder\OpenAI.Codex_2p2nqsd0c76g0!App";
    const WINDOW_WAIT_ATTEMPTS: usize = 30;
    const WINDOW_WAIT_STEP: Duration = Duration::from_millis(50);

    pub fn launch_or_activate_app(path: &str) -> bool {
        let executable = Path::new(path);
        if !executable.is_file() || !executable.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("exe")) {
            log::warn!("APP LAUNCH invalid executable path: {path}");
            return false;
        }
        let Some(target) = executable.canonicalize().ok() else { return false };
        let mut state = (None, &target);
        unsafe {
            let _ = EnumWindows(Some(enum_matching_app_window), LPARAM((&mut state as *mut (Option<HWND>, &PathBuf)) as isize));
        }
        if let Some(hwnd) = state.0 {
            return activate_window(hwnd);
        }
        match std::process::Command::new(&target).current_dir(target.parent().unwrap_or(Path::new("."))).spawn() {
            Ok(_) => true,
            Err(error) => {
                log::warn!("APP LAUNCH failed for {}: {error}", target.display());
                false
            }
        }
    }

    unsafe extern "system" fn enum_matching_app_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !unsafe { IsWindowVisible(hwnd).as_bool() } { return BOOL(1); }
        let state = unsafe { &mut *(lparam.0 as *mut (Option<HWND>, &PathBuf)) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)); }
        if pid != 0 && process_image_path(pid).is_some_and(|path| path.to_string_lossy().eq_ignore_ascii_case(&state.1.to_string_lossy())) {
            state.0 = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    pub fn activate_chatgpt_and_focus_input() -> bool {
        let mut launched = false;
        for attempt in 0..WINDOW_WAIT_ATTEMPTS {
            if let Some(hwnd) = find_chatgpt_window() {
                if activate_window(hwnd) && focus_composer(hwnd) {
                    return true;
                }
            } else if !launched {
                launched = launch_chatgpt();
                if !launched {
                    log::warn!("CHATGPT FOCUS launch failed");
                    return false;
                }
            }

            if attempt + 1 < WINDOW_WAIT_ATTEMPTS {
                thread::sleep(WINDOW_WAIT_STEP);
            }
        }

        log::warn!("CHATGPT FOCUS window or composer was not ready");
        false
    }

    fn launch_chatgpt() -> bool {
        std::process::Command::new("explorer.exe")
            .arg(CHATGPT_APP_ID)
            .spawn()
            .is_ok()
    }

    fn find_chatgpt_window() -> Option<HWND> {
        let mut found = None;
        unsafe {
            let _ = EnumWindows(
                Some(enum_chatgpt_window),
                LPARAM((&mut found as *mut Option<HWND>) as isize),
            );
        }
        found
    }

    unsafe extern "system" fn enum_chatgpt_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !unsafe { IsWindowVisible(hwnd).as_bool() } {
            return BOOL(1);
        }

        let found = unsafe { &mut *(lparam.0 as *mut Option<HWND>) };
        if window_process_is_chatgpt(hwnd) {
            *found = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    fn window_process_is_chatgpt(hwnd: HWND) -> bool {
        if unsafe { GetWindowTextLengthW(hwnd) } <= 0 {
            return false;
        }

        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        if pid == 0 {
            return false;
        }

        process_image_path(pid)
            .as_deref()
            .is_some_and(is_chatgpt_executable)
    }

    fn process_image_path(pid: u32) -> Option<PathBuf> {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let result = unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut length,
            )
        };
        let _ = unsafe { CloseHandle(process) };
        result.ok()?;

        let path = OsString::from_wide(&buffer[..length as usize]);
        Some(PathBuf::from(path))
    }

    fn is_chatgpt_executable(path: &Path) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("ChatGPT.exe"))
    }

    fn activate_window(hwnd: HWND) -> bool {
        let current_thread = unsafe { GetCurrentThreadId() };
        let foreground = unsafe { GetForegroundWindow() };
        let foreground_thread = if foreground.0.is_null() {
            0
        } else {
            unsafe { GetWindowThreadProcessId(foreground, None) }
        };
        let target_thread = unsafe { GetWindowThreadProcessId(hwnd, None) };

        let attached_foreground = foreground_thread != 0
            && foreground_thread != current_thread
            && unsafe { AttachThreadInput(current_thread, foreground_thread, true).as_bool() };
        let attached_target = target_thread != 0
            && target_thread != current_thread
            && unsafe { AttachThreadInput(current_thread, target_thread, true).as_bool() };

        let show_command = if unsafe { IsIconic(hwnd).as_bool() } {
            SW_RESTORE
        } else {
            SW_SHOW
        };
        unsafe {
            let _ = ShowWindowAsync(hwnd, show_command);
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            SwitchToThisWindow(hwnd, true);
        }

        if attached_target {
            let _ = unsafe { AttachThreadInput(current_thread, target_thread, false) };
        }
        if attached_foreground {
            let _ = unsafe { AttachThreadInput(current_thread, foreground_thread, false) };
        }

        for _ in 0..10 {
            if unsafe { GetForegroundWindow() } == hwnd {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }

        log::warn!("CHATGPT FOCUS foreground activation failed");
        false
    }

    fn focus_composer(hwnd: HWND) -> bool {
        let _apartment = ComApartment::initialize();
        let automation: IUIAutomation =
            match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
                Ok(automation) => automation,
                Err(error) => {
                    log::warn!("CHATGPT FOCUS UI Automation unavailable: {error}");
                    return false;
                }
            };
        let root = match unsafe { automation.ElementFromHandle(hwnd) } {
            Ok(root) => root,
            Err(error) => {
                log::warn!("CHATGPT FOCUS window element unavailable: {error}");
                return false;
            }
        };
        let condition = match unsafe { automation.CreateTrueCondition() } {
            Ok(condition) => condition,
            Err(error) => {
                log::warn!("CHATGPT FOCUS condition unavailable: {error}");
                return false;
            }
        };
        let elements = match unsafe { root.FindAll(TreeScope_Descendants, &condition) } {
            Ok(elements) => elements,
            Err(error) => {
                log::warn!("CHATGPT FOCUS composer scan failed: {error}");
                return false;
            }
        };
        let count = match unsafe { elements.Length() } {
            Ok(count) => count,
            Err(error) => {
                log::warn!("CHATGPT FOCUS composer count failed: {error}");
                return false;
            }
        };

        let mut best = None;
        for index in 0..count {
            let Ok(element) = (unsafe { elements.GetElement(index) }) else {
                continue;
            };
            let score = composer_score(&element);
            if score > 0 && best.is_none_or(|(_, best_score)| score > best_score) {
                best = Some((index, score));
            }
        }

        let Some((index, _)) = best else {
            log::warn!("CHATGPT FOCUS ProseMirror composer not found");
            return false;
        };
        let Ok(composer) = (unsafe { elements.GetElement(index) }) else {
            return false;
        };
        if let Err(error) = unsafe { composer.SetFocus() } {
            log::warn!("CHATGPT FOCUS composer SetFocus failed: {error}");
            return false;
        }

        thread::sleep(Duration::from_millis(15));
        true
    }

    fn composer_score(element: &IUIAutomationElement) -> u8 {
        let Ok(control_type) = (unsafe { element.CurrentControlType() }) else {
            return 0;
        };
        if control_type != UIA_EditControlTypeId {
            return 0;
        }
        if !unsafe { element.CurrentIsEnabled() }
            .map(|enabled| enabled.as_bool())
            .unwrap_or(false)
        {
            return 0;
        }

        let class_name = unsafe { element.CurrentClassName() }
            .map(|class_name| class_name.to_string())
            .unwrap_or_default();
        if !is_chatgpt_composer_class(&class_name) {
            return 0;
        }
        if class_name.contains("ProseMirror-focused") {
            2
        } else {
            1
        }
    }

    fn is_chatgpt_composer_class(class_name: &str) -> bool {
        class_name.contains("ProseMirror")
    }

    struct ComApartment {
        uninitialize: bool,
    }

    impl ComApartment {
        fn initialize() -> Self {
            let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            Self {
                uninitialize: result.is_ok(),
            }
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            if self.uninitialize {
                unsafe { CoUninitialize() };
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn recognizes_the_packaged_chatgpt_executable() {
            assert!(is_chatgpt_executable(Path::new(
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.903.9818.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe"
            )));
            assert!(!is_chatgpt_executable(Path::new(
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.903.9818.0_x64__2p2nqsd0c76g0\app\codex.exe"
            )));
        }

        #[test]
        fn recognizes_only_prosemirror_as_the_chatgpt_composer() {
            assert!(is_chatgpt_composer_class("ProseMirror-focused"));
            assert!(is_chatgpt_composer_class("ProseMirror"));
            assert!(!is_chatgpt_composer_class("Chrome_WidgetWin_1"));
        }

        #[test]
        #[ignore = "requires the ChatGPT desktop app to be installed"]
        fn live_chatgpt_focus_smoke() {
            assert!(activate_chatgpt_and_focus_input());
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows_impl::activate_chatgpt_and_focus_input;
#[cfg(target_os = "windows")]
pub use windows_impl::launch_or_activate_app;
