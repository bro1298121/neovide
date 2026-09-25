use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{FALSE, HWND, TRUE},
    Graphics::Dwm::{
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_SYSTEMBACKDROP_TYPE,
        DWMWA_USE_HOSTBACKDROPBRUSH,
    },
    System::Console::{ATTACH_PARENT_PROCESS, AttachConsole},
    UI::Controls::MARGINS,
    UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
    UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    },
};
use windows_registry::{CURRENT_USER, Result};

use crate::error_msg;
use crate::window::WindowBackdrop;

const REGISTRY_PATH_DIRECTORY: &str = "Software\\Classes\\Directory\\Background\\shell\\Neovide";
const REGISTRY_PATH_DIRECTORY_COMMAND: &str =
    "Software\\Classes\\Directory\\Background\\shell\\Neovide\\command";
const REGISTRY_PATH_FOLDER: &str = "Software\\Classes\\*\\shell\\Neovide";
const REGISTRY_PATH_FOLDER_COMMAND: &str = "Software\\Classes\\*\\shell\\Neovide\\command";

fn get_neovide_path() -> String {
    std::env::current_exe().unwrap_or_default().to_str().unwrap_or_default().to_string()
}

fn unregister_rightclick() -> Result<()> {
    let key = CURRENT_USER;
    key.remove_tree(REGISTRY_PATH_DIRECTORY)?;
    key.remove_tree(REGISTRY_PATH_FOLDER)?;

    Ok(())
}

fn register_rightclick_directory() -> Result<()> {
    let neovide_path = get_neovide_path();
    let neovide_description = "Open with Neovide";
    let neovide_command = format!("{neovide_path} \"%V\"");

    let key = CURRENT_USER.create(REGISTRY_PATH_DIRECTORY)?;
    key.set_string("", neovide_description)?;
    key.set_string("Icon", &neovide_path)?;

    let key = CURRENT_USER.create(REGISTRY_PATH_DIRECTORY_COMMAND)?;
    key.set_string("", &neovide_command)?;

    Ok(())
}

fn register_rightclick_file() -> Result<()> {
    let neovide_path = get_neovide_path();
    let neovide_description = "Open with Neovide";
    let neovide_command = format!("{neovide_path} \"%1\"");

    let key = CURRENT_USER.create(REGISTRY_PATH_FOLDER)?;
    key.set_string("", neovide_description)?;
    key.set_string("Icon", &neovide_path)?;

    let key = CURRENT_USER.create(REGISTRY_PATH_FOLDER_COMMAND)?;
    key.set_string("", &neovide_command)?;

    Ok(())
}

pub fn register_right_click() {
    if register_rightclick_directory().is_err() {
        error_msg!("Could not register directory context menu item. Possibly already registered.");
    }
    if register_rightclick_file().is_err() {
        error_msg!("Could not register file context menu item. Possibly already registered.");
    }
}

pub fn unregister_right_click() {
    if unregister_rightclick().is_err() {
        error_msg!("Could not remove context menu items. Possibly already removed.");
    }
}

pub fn windows_fix_dpi() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            .expect("Failed to set DPI awareness!");
    }
}

pub fn windows_attach_to_console() {
    // Attach to parent console tip found here: https://github.com/rust-lang/rust/issues/67159#issuecomment-987882771
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS).ok();
    }
}

/// Applies a window backdrop to the whole window, including the client area.
///
/// Neovide renders through DirectComposition with `WS_EX_NOREDIRECTIONBITMAP`, which makes
/// winit's own `set_system_backdrop` ineffective for the client area (the material only shows
/// behind the title bar). Setting `DWMWA_USE_HOSTBACKDROPBRUSH` lets DWM draw the host
/// backdrop brush behind the DirectComposition content; the translucent background pixels
/// rendered by Neovide then reveal it.
///
/// `backdrop` selects the material: `Mica`/`MicaAlt`/`Acrylic` map to the Windows 11
/// `DWMWA_SYSTEMBACKDROP_TYPE` values 2/4/3 (main window, tabbed window, transient window).
/// `Aero` disables the DWM backdrop and instead relies on the desktop-capture blur painted by
/// the renderer (see `renderer::draw_frame`); the window is excluded from capture via
/// `WDA_EXCLUDEFROMCAPTURE` so the captured desktop shows what is actually behind the window.
/// `None` disables the backdrop and resets the frame.
pub fn set_window_backdrop(window: &winit::window::Window, backdrop: Option<WindowBackdrop>) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut _);

    log::warn!("[backdrop] applying DWM backdrop={backdrop:?}");
    unsafe {
        let (use_backdrop_brush, backdrop_type) = match backdrop {
            Some(WindowBackdrop::Mica) => (TRUE, 2),
            Some(WindowBackdrop::MicaAlt) => (TRUE, 4),
            Some(WindowBackdrop::Acrylic) => (TRUE, 3),
            Some(WindowBackdrop::Aero) | None => (FALSE, 1),
        };

        let result = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            &use_backdrop_brush as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<windows::core::BOOL>() as u32,
        );
        if let Err(error) = result {
            log::warn!("[backdrop] DwmSetWindowAttribute(USE_HOSTBACKDROPBRUSH) failed: {error:?}");
        }

        let result = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop_type as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
        if let Err(error) = result {
            log::warn!("[backdrop] DwmSetWindowAttribute(SYSTEMBACKDROP_TYPE) failed: {error:?}");
        }

        let margins = MARGINS::default();
        if let Err(error) = DwmExtendFrameIntoClientArea(hwnd, &margins) {
            log::warn!("[backdrop] DwmExtendFrameIntoClientArea failed: {error:?}");
        }

        let affinity =
            if matches!(backdrop, Some(WindowBackdrop::Aero)) { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
        if let Err(error) = SetWindowDisplayAffinity(hwnd, affinity) {
            log::warn!("[backdrop] SetWindowDisplayAffinity failed: {error:?}");
        }
    }
}
