use about_sevos::AboutApp;
use ui_abi::{HostV1, HostV2, HostV3, HostV4, HostV5, Status, UI_SERVICE_ABI_VERSION};
use ui_app::App;
use ui_core::{Color, Rect};
use ui_platform::{Host, InteractiveHost, InteractiveHostV3, InteractiveHostV4, InteractiveHostV5};
use ui_render::Canvas;
use ui_widgets::Button;
use voyager::VoyagerApp;

use crate::demo;
use crate::runtime;
use crate::storage::{StaticCell, TEXT_SCRATCH};
use crate::surface;
use crate::text;

const UI_SERVICE_API_VERSION: u32 = 1;
const UI_SERVICE_OK: u32 = 1;
const UI_SERVICE_ERROR: u32 = 0;
const ABOUT_PIXELS_LEN: usize = AboutApp::WINDOW.pixel_count();
const VOYAGER_PIXELS_LEN: usize = VoyagerApp::WINDOW.pixel_count();

static ABOUT_PIXELS: StaticCell<[u32; ABOUT_PIXELS_LEN]> =
    StaticCell::new([0; ABOUT_PIXELS_LEN]);
static VOYAGER_PIXELS: StaticCell<[u32; VOYAGER_PIXELS_LEN]> =
    StaticCell::new([0; VOYAGER_PIXELS_LEN]);

#[unsafe(export_name = "UIServiceAPIVersion")]
pub extern "C" fn api_version() -> u32 {
    UI_SERVICE_API_VERSION
}

#[unsafe(export_name = "UIServiceABIVersion")]
pub extern "C" fn abi_version() -> u32 {
    UI_SERVICE_ABI_VERSION
}

#[unsafe(export_name = "UIServiceHasInter")]
pub extern "C" fn has_inter() -> u32 {
    text::has_inter() as u32
}

#[unsafe(export_name = "UIServiceHostV1Size")]
pub extern "C" fn host_v1_size() -> u32 {
    HostV1::expected_size()
}

#[unsafe(export_name = "UIServiceHostV2Size")]
pub extern "C" fn host_v2_size() -> u32 {
    HostV2::expected_size()
}

#[unsafe(export_name = "UIServiceHostV3Size")]
pub extern "C" fn host_v3_size() -> u32 {
    HostV3::expected_size()
}

#[unsafe(export_name = "UIServiceValidateHost")]
pub unsafe extern "C" fn validate_host(host: *const HostV1) -> u32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return Status::InvalidArgument as u32;
    };

    match Host::connect(host) {
        Ok(_) => Status::Ok as u32,
        Err(status) => status as u32,
    }
}

#[unsafe(export_name = "UIServiceValidateHostV2")]
pub unsafe extern "C" fn validate_host_v2(host: *const HostV2) -> u32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return Status::InvalidArgument as u32;
    };

    match InteractiveHost::connect(host) {
        Ok(_) => Status::Ok as u32,
        Err(status) => status as u32,
    }
}

#[unsafe(export_name = "UIServiceValidateHostV3")]
pub unsafe extern "C" fn validate_host_v3(host: *const HostV3) -> u32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return Status::InvalidArgument as u32;
    };

    match InteractiveHostV3::connect(host) {
        Ok(_) => Status::Ok as u32,
        Err(status) => status as u32,
    }
}

#[unsafe(export_name = "UIServiceHostV4Size")]
pub extern "C" fn host_v4_size() -> u32 {
    HostV4::expected_size()
}

#[unsafe(export_name = "UIServiceValidateHostV4")]
pub unsafe extern "C" fn validate_host_v4(host: *const HostV4) -> u32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return Status::InvalidArgument as u32;
    };

    match InteractiveHostV4::connect(host) {
        Ok(_) => Status::Ok as u32,
        Err(status) => status as u32,
    }
}

#[unsafe(export_name = "UIServiceHostV5Size")]
pub extern "C" fn host_v5_size() -> u32 {
    HostV5::expected_size()
}

#[unsafe(export_name = "UIServiceValidateHostV5")]
pub unsafe extern "C" fn validate_host_v5(host: *const HostV5) -> u32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return Status::InvalidArgument as u32;
    };

    match InteractiveHostV5::connect(host) {
        Ok(_) => Status::Ok as u32,
        Err(status) => status as u32,
    }
}

#[unsafe(export_name = "UIServiceHostClear")]
pub unsafe extern "C" fn host_clear(host: *const HostV1, xrgb8888: u32) -> u32 {
    let Some(host_abi) = (unsafe { host.as_ref() }) else {
        return Status::InvalidArgument as u32;
    };

    let mut host = match Host::connect(host_abi) {
        Ok(host) => host,
        Err(status) => return status as u32,
    };

    if let Err(status) = unsafe {
        host.with_surface(|surface| surface.fill(Color::from_xrgb8888(xrgb8888)))
    } {
        return status as u32;
    }

    match host.present(None) {
        Ok(()) => Status::Ok as u32,
        Err(status) => status as u32,
    }
}

#[unsafe(export_name = "UIServiceDrawAbout")]
pub unsafe extern "C" fn draw_about(host: *const HostV1) -> u32 {
    let Some(host_abi) = (unsafe { host.as_ref() }) else {
        return Status::InvalidArgument as u32;
    };

    let mut host = match Host::connect(host_abi) {
        Ok(host) => host,
        Err(status) => return status as u32,
    };
    let scratch = unsafe { TEXT_SCRATCH.get_mut() };
    let mut text = text::backend(scratch);

    if let Err(status) = unsafe {
        host.with_surface(|surface| about_sevos::draw_preview(surface, &mut text))
    } {
        return status as u32;
    }

    match host.present(None) {
        Ok(()) => Status::Ok as u32,
        Err(status) => status as u32,
    }
}

#[unsafe(export_name = "UIServiceRunAbout")]
pub unsafe extern "C" fn run_about(host: *const HostV5) -> u32 {
    let Some(host_abi) = (unsafe { host.as_ref() }) else {
        return Status::InvalidArgument as u32;
    };

    match unsafe { runtime::run(host_abi, || AboutApp, &ABOUT_PIXELS) } {
        Ok(()) => Status::Ok as u32,
        Err(status) => status as u32,
    }
}

#[unsafe(export_name = "UIServiceRunVoyager")]
pub unsafe extern "C" fn run_voyager(host: *const HostV5) -> u32 {
    let Some(host_abi) = (unsafe { host.as_ref() }) else {
        return Status::InvalidArgument as u32;
    };

    match unsafe { runtime::run(host_abi, VoyagerApp::new, &VOYAGER_PIXELS) } {
        Ok(()) => Status::Ok as u32,
        Err(status) => status as u32,
    }
}

#[unsafe(export_name = "UIServiceClear")]
pub unsafe extern "C" fn clear(
    pixels: *mut u32,
    width: u32,
    height: u32,
    stride: u32,
    xrgb8888: u32,
) -> u32 {
    let Some(mut surface) = (unsafe { surface::from_raw(pixels, width, height, stride) }) else {
        return UI_SERVICE_ERROR;
    };

    surface.fill(Color::from_xrgb8888(xrgb8888));
    UI_SERVICE_OK
}

#[unsafe(export_name = "UIServiceDrawButton")]
pub unsafe extern "C" fn draw_button(
    pixels: *mut u32,
    surface_width: u32,
    surface_height: u32,
    stride: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    state: u32,
) -> u32 {
    let Some(mut surface) = (unsafe {
        surface::from_raw(pixels, surface_width, surface_height, stride)
    }) else {
        return UI_SERVICE_ERROR;
    };

    let mut button = Button::new("", Rect::new(x, y, width, height));
    if !demo::apply_button_state(&mut button, state) {
        return UI_SERVICE_ERROR;
    }

    button.draw(&mut surface);
    UI_SERVICE_OK
}

#[unsafe(export_name = "UIServiceDrawDemo")]
pub unsafe extern "C" fn draw_demo(
    pixels: *mut u32,
    width: u32,
    height: u32,
    stride: u32,
) -> u32 {
    let Some(mut surface) = (unsafe { surface::from_raw(pixels, width, height, stride) }) else {
        return UI_SERVICE_ERROR;
    };

    demo::draw_demo(&mut surface);
    UI_SERVICE_OK
}
