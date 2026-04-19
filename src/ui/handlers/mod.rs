pub mod window;
pub mod distro;
pub mod settings;
pub mod update;
pub mod common;
pub mod instance;
pub mod usb;
pub mod network;
pub mod about;

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::{AppWindow, AppState};

pub async fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    common::setup(app, app_handle.clone(), app_state.clone());
    window::setup(app, app_handle.clone());
    distro::setup(app, app_handle.clone(), app_state.clone());
    settings::setup(app, app_handle.clone(), app_state.clone());
    update::setup(app, app_handle.clone(), app_state.clone());
    instance::setup(app, app_handle.clone(), app_state.clone());
    usb::setup(app, app_handle.clone(), app_state.clone());
    network::setup(app, app_handle.clone(), app_state.clone());
    about::setup(app, app_handle.clone(), app_state.clone());
}
