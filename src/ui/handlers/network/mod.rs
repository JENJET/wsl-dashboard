use crate::{AppState, AppWindow};
use std::sync::Arc;
use tokio::sync::Mutex;

pub mod utils;
pub use utils::*;
pub mod proxy;
pub mod rules;
pub mod service;

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // 1. Initial Data Refresh
    let ah_refresh = app_handle.clone();
    let as_refresh = app_state.clone();
    tokio::spawn(async move {
        utils::refresh_network_view_data(ah_refresh, as_refresh).await;
    });

    // 2. Setup Sub-handlers
    rules::setup(app, app_handle.clone(), app_state.clone());
    proxy::setup(app, app_handle.clone(), app_state.clone());
    service::setup(app, app_handle, app_state);
}
