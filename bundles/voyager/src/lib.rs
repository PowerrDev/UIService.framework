#![no_std]

//! Voyager.app's executable: the app, run as a process by `ui-app-nxu`.

/// Called by AppKit.framework's `main` (NXU's frameworks/AppKit.framework).
#[unsafe(no_mangle)]
pub extern "C" fn UIApplicationMain() -> i32 {
    ui_app_nxu::run("/Applications/Voyager.app", voyager::VoyagerApp::new)
}
