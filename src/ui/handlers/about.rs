use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::io::Read;
use tokio::sync::Mutex;
use tracing::{debug, warn};
use crate::{AppWindow, AppState};
use crate::app::updater::OfficialGroupItem;

/// Ensure the BASE_API request is triggered only once during the application's lifecycle
static FETCHED: AtomicBool = AtomicBool::new(false);

pub fn setup(app: &AppWindow, app_handle: slint::Weak<AppWindow>, _app_state: Arc<Mutex<AppState>>) {
    let ah = app_handle.clone();

    // group_clicked: pop up only when the image is ready, otherwise no response
    app.on_group_clicked(move || {
        if let Some(app) = ah.upgrade() {
            if app.get_about_group_pic_ready() {
                app.set_show_group_popup(true);
            }
            // Do not respond if the image is not ready
        }
    });
}

/// Called when the user opens the About page for the first time (from the select_tab hook in common.rs)
pub fn trigger_fetch_if_needed(app_handle: slint::Weak<AppWindow>, app_state: Arc<Mutex<AppState>>) {
    // AtomicBool ensures it triggers only once, preventing duplicate requests even with rapid switching to the About page
    if FETCHED.swap(true, Ordering::SeqCst) {
        debug!("about: BASE_API already fetched, skipping");
        return;
    }
    debug!("about: First visit to About page, triggering BASE_API fetch");

    tokio::spawn(async move {
        // Read user configuration (language + timezone)
        let (timezone, sys_lang) = {
            let state = app_state.lock().await;
            let cfg = state.config_manager.get_config();
            (cfg.system.timezone.clone(), cfg.system.system_language.clone())
        };

        match crate::app::updater::fetch_base_config(&timezone).await {
            Ok(resp) => {
                debug!("about: fetch_base_config success, official-group count={}", resp.official_group.len());
                if let Some(item) = match_group(&resp.official_group, &sys_lang, &timezone) {
                    let pic_url = build_pic_url(&item.pic, &timezone);
                    debug!("about: matched group='{}', pic_url={}", item.name, pic_url);
                    // In the child thread, keep only raw pixel bytes + width/height (all implement Send)
                    // slint::Image holds VRc<OpaqueImageVTable> (contains *mut ()), which does not implement Send,
                    // so it must be reconstructed in the UI thread (inside invoke_from_event_loop).
                    match load_image_pixels(&pic_url) {
                        Ok((pixels, w, h)) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = app_handle.upgrade() {
                                    let buf = slint::SharedPixelBuffer::clone_from_slice(&pixels, w, h);
                                    let img = slint::Image::from_rgba8(buf);
                                    app.set_about_group_pic(img);
                                    app.set_about_group_pic_ready(true);
                                    debug!("about: group pic loaded and ready");
                                }
                            });
                        }
                        Err(e) => {
                            warn!("about: group pic load failed: {}", e);
                        }
                    }
                } else {
                    debug!("about: no matching group item found");
                }
            }
            Err(e) => {
                warn!("about: fetch_base_config failed: {}", e);
                // FETCHED is not reset: do not retry after failure in the current session
            }
        }
    });
}

/// Standardize language code: to lowercase + remove hyphens and underscores
/// Example: zh-CN / zh_CN / zh-cn => zhcn
fn normalize_lang(s: &str) -> String {
    s.to_lowercase()
        .replace('-', "")
        .replace('_', "")
}

/// Match an appropriate entry from the official-group array
///
/// Priority rule: both system-language and timezone are non-empty → AND match (case/separator insensitive)
/// Fallback rule: entry where both system-language and timezone are empty strings (general fallback)
fn match_group<'a>(
    items: &'a [OfficialGroupItem],
    sys_lang: &str,
    timezone: &str,
) -> Option<&'a OfficialGroupItem> {
    let nl = normalize_lang(sys_lang);
    let nt = timezone.to_lowercase();

    // Priority: both language && timezone are non-empty, perform AND match
    for item in items {
        if !item.system_language.is_empty() && !item.timezone.is_empty() {
            if normalize_lang(&item.system_language) == nl
                && item.timezone.to_lowercase() == nt
            {
                return Some(item);
            }
        }
    }

    // Fallback: general entry where both fields are empty
    items
        .iter()
        .find(|i| i.system_language.is_empty() && i.timezone.is_empty())
}

/// Build the complete image URL
/// If the pic field already contains "http", use it directly; otherwise append the domain prefix
fn build_pic_url(pic: &str, timezone: &str) -> String {
    if pic.contains("http") {
        return pic.to_string();
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let base_url = if timezone == crate::app::ZH_TIMEZONE {
        crate::app::STATIC_API
    } else {
        crate::app::STATIC_API_FREE
    };
    format!("{}{}?t={}", base_url, pic, ts)
}

/// Download the image and decode to raw RGBA8 pixel bytes + width/height
/// Returns (rgba_bytes, width, height), all implement Send, can be safely passed across threads
fn load_image_pixels(url: &str) -> Result<(Vec<u8>, u32, u32), String> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?;

    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;

    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((rgba.into_raw(), w, h))
}
