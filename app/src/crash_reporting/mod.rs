use std::borrow::Cow;
#[cfg(linux_or_windows)]
use std::path::Path;

use warpui::AppContext;
use warpui::rendering::GPUDeviceInfo;

use crate::auth::UserUid;

#[cfg(linux_or_windows)]
pub fn run_minidump_server(socket_path: &Path) -> anyhow::Result<()> {
    let _ = socket_path;
    log::info!("Minidump reporting is disabled; no crash report will be uploaded");
    Ok(())
}

pub(crate) fn set_tag<'a, 'b>(key: impl Into<Cow<'a, str>>, value: impl Into<Cow<'b, str>>) {
    let key = key.into();
    let _ = value;
    log::debug!("Ignoring crash-reporting tag because reporting is disabled: key={key}");
}

pub(crate) fn set_gpu_device_info(gpu_device_info: GPUDeviceInfo) {
    let _ = gpu_device_info;
    log::debug!("Ignoring GPU crash-reporting metadata because reporting is disabled");
}

pub(crate) fn init(ctx: &mut AppContext) -> bool {
    let _ = ctx;
    log::info!("Crash reporting is disabled; not initializing Sentry");
    false
}

pub(crate) fn is_initialized() -> bool {
    false
}

pub fn uninit_sentry() {}

pub fn init_cocoa_sentry() {}

pub fn uninit_cocoa_sentry() {}

pub fn crash() {
    log::info!("Crash reporting is disabled; ignoring crash request");
}

pub fn set_user_id(user_id: UserUid, email: Option<String>, ctx: &mut AppContext) {
    let _ = (user_id, email, ctx);
}

pub fn set_client_type_tag(client_id: &str) {
    set_tag("warp.client_type", client_id);
}
