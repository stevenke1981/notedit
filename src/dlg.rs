//! Native dialogs parented to the main window, so they stay on top of it and
//! behave modally (Windows/macOS) instead of hiding behind the editor.
use std::cell::RefCell;

thread_local! {
    static MAIN: RefCell<Option<slint::Weak<crate::AppWindow>>> = const { RefCell::new(None) };
}

pub fn set_main(w: slint::Weak<crate::AppWindow>) { MAIN.with(|m| *m.borrow_mut() = Some(w)); }

fn with_parent<T>(d: T, f: impl FnOnce(T, &slint::WindowHandle) -> T) -> T {
    let ui = MAIN.with(|m| m.borrow().as_ref().and_then(|w| w.upgrade()));
    match ui {
        Some(ui) => {
            use slint::ComponentHandle;
            let h = ui.window().window_handle();
            // only parent when the platform handle is actually available
            use raw_window_handle::HasWindowHandle;
            if h.window_handle().is_ok() { f(d, &h) } else { d }
        }
        None => d,
    }
}

pub fn msg() -> rfd::MessageDialog { with_parent(rfd::MessageDialog::new(), |d, h| d.set_parent(h)) }
pub fn file() -> rfd::FileDialog { with_parent(rfd::FileDialog::new(), |d, h| d.set_parent(h)) }
