//! Native window helpers AcmeUI does not expose (gaps G6, G7, G9).
//! Windows: the HWND of our winit window, the title-bar icon, the Ctrl key state
//! for Ctrl+wheel and a raw-window-handle wrapper so rfd dialogs can be parented.

#[cfg(windows)]
mod imp {
    use std::cell::Cell;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetClassNameW, GetSystemMetrics, GetWindowLongPtrW, IsWindowVisible, LoadIconW, LoadImageW, SendMessageW, GWL_STYLE, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, SM_CXSMICON,
        SM_CYSMICON, WM_SETICON, WS_CAPTION,
    };

    thread_local! { static MAIN: Cell<isize> = const { Cell::new(0) }; }

    /// winit's default window class (winit 0.30 `PlatformSpecificWindowAttributes`).
    /// NOTE: depends on the pinned winit / acme-platform rev; re-check when bumping it.
    const WINIT_CLASS: &str = "Window Class";

    /// MAKEINTRESOURCE id of the exe icon (embedded by winresource)
    const ICON_RES_ID: usize = 1;

    unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> i32 {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let mut buf = [0u16; 64];
            let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            // only our winit window: never an rfd / system dialog of the same thread
            if IsWindowVisible(hwnd) != 0 && style & WS_CAPTION == WS_CAPTION && class == WINIT_CLASS {
                *(lp as *mut isize) = hwnd as isize;
                return 0;
            }
        }
        1
    }

    /// Find (once found, cached) the visible captioned winit window of the UI thread.
    pub fn main_hwnd() -> Option<isize> {
        let cur = MAIN.with(|m| m.get());
        if cur != 0 { return Some(cur); }
        let mut found: isize = 0;
        unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(enum_cb), &mut found as *mut isize as LPARAM); }
        if found != 0 { MAIN.with(|m| m.set(found)); Some(found) } else { None }
    }

    /// Use the exe's icon resource (id 1, embedded by winresource) for the title bar / taskbar.
    pub fn set_icon(hwnd: isize) -> bool {
        unsafe {
            let inst = GetModuleHandleW(std::ptr::null());
            let icon = LoadIconW(inst, ICON_RES_ID as *const u16);
            if icon.is_null() { return false; }
            SendMessageW(hwnd as HWND, WM_SETICON, ICON_BIG as usize, icon as isize);
            let small = LoadImageW(inst, ICON_RES_ID as *const u16, IMAGE_ICON, GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_SHARED);
            let small = if small.is_null() { icon } else { small };
            SendMessageW(hwnd as HWND, WM_SETICON, ICON_SMALL as usize, small as isize);
            true
        }
    }

    /// (ctrl, shift, alt) as currently held, from the Win32 key state.
    pub fn mods() -> Option<(bool, bool, bool)> {
        unsafe { Some((GetKeyState(VK_CONTROL as i32) < 0, GetKeyState(VK_SHIFT as i32) < 0, GetKeyState(VK_MENU as i32) < 0)) }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn main_hwnd() -> Option<isize> { None }
    pub fn set_icon(_: isize) -> bool { false }
    pub fn mods() -> Option<(bool, bool, bool)> { None }
}

pub use imp::*;

/// Parent handle for rfd dialogs (Windows only; elsewhere dialogs are unparented).
pub struct ParentWindow(#[allow(dead_code)] isize);

impl ParentWindow {
    pub fn get() -> Option<ParentWindow> { if cfg!(windows) { main_hwnd().map(ParentWindow) } else { None } }
}

impl raw_window_handle::HasWindowHandle for ParentWindow {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        #[cfg(windows)]
        {
            let nz = std::num::NonZeroIsize::new(self.0).ok_or(raw_window_handle::HandleError::Unavailable)?;
            let raw = raw_window_handle::RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(nz));
            // SAFETY: the HWND belongs to the live main window of this thread; the handle is
            // only used for the duration of a modal dialog call on the UI thread.
            Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
        }
        #[cfg(not(windows))]
        { Err(raw_window_handle::HandleError::NotSupported) }
    }
}

impl raw_window_handle::HasDisplayHandle for ParentWindow {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        #[cfg(windows)]
        { Ok(raw_window_handle::DisplayHandle::windows()) }
        #[cfg(not(windows))]
        { Err(raw_window_handle::HandleError::NotSupported) }
    }
}
