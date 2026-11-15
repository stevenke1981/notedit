//! Native dialogs parented to the main window, so they stay on top of it and
//! behave modally (Windows) instead of hiding behind the editor. AcmeUI does
//! not expose the window handle (gap G7): on Windows the HWND is looked up on
//! the UI thread; elsewhere dialogs are unparented.

pub fn msg() -> rfd::MessageDialog {
    let d = rfd::MessageDialog::new();
    match crate::ui::win::ParentWindow::get() { Some(p) => d.set_parent(&p), None => d }
}

pub fn file() -> rfd::FileDialog {
    let d = rfd::FileDialog::new();
    match crate::ui::win::ParentWindow::get() { Some(p) => d.set_parent(&p), None => d }
}
