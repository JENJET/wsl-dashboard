pub mod about;
pub mod common;
pub mod distro;
pub mod instance;
pub mod network;
pub mod settings;
pub mod update;
pub mod usb;
pub mod window;
pub mod wsl_manage;

use crate::{AppState, AppWindow};
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn setup(
    app: &AppWindow,
    app_handle: slint::Weak<AppWindow>,
    app_state: Arc<Mutex<AppState>>,
) {
    common::setup(app, app_handle.clone(), app_state.clone());
    window::setup(app, app_handle.clone());
    distro::setup(app, app_handle.clone(), app_state.clone());
    settings::setup(app, app_handle.clone(), app_state.clone());
    update::setup(app, app_handle.clone(), app_state.clone());
    instance::setup(app, app_handle.clone(), app_state.clone());
    usb::setup(app, app_handle.clone(), app_state.clone());
    network::setup(app, app_handle.clone(), app_state.clone());
    about::setup(app, app_handle.clone(), app_state.clone());
    wsl_manage::setup(app, app_handle.clone(), app_state.clone());
}
