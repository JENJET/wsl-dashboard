use slint::ComponentHandle;
use tracing::info;
use windows::Foundation::TypedEventHandler;
use windows::UI::ViewManagement::{UIColorType, UISettings};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RegOpenKeyExW, RegQueryValueExW,
};
use windows::core::PCWSTR;

pub struct ThemeWatcher {
    _settings: UISettings,
    // We must keep the handler alive or the event will be un-registered
    _handler: TypedEventHandler<UISettings, windows::core::IInspectable>,
}

// WinRT objects aren't Send by default in Rust, but UISettings is agile
unsafe impl Send for ThemeWatcher {}
unsafe impl Sync for ThemeWatcher {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Theme {
    Light,
    Dark,
}

impl ThemeWatcher {
    pub fn new(handle: slint::Weak<crate::AppWindow>) -> windows::core::Result<Self> {
        let settings = UISettings::new()?;

        let handler = TypedEventHandler::<UISettings, windows::core::IInspectable>::new({
            let handle = handle.clone();
            move |_, _| {
                let theme = Self::get_current_theme();
                info!("Windows theme changed: {:?}", theme);

                let dark_mode = theme == Theme::Dark;
                let handle_clone = handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app_instance) = handle_clone.upgrade() {
                        app_instance
                            .global::<crate::Theme>()
                            .set_dark_mode(dark_mode);
                    }
                });
                Ok(())
            }
        });

        settings.ColorValuesChanged(&handler)?;

        let current = Self::get_current_theme();
        info!(
            "ThemeWatcher initialized. Current Windows theme: {:?}",
            current
        );

        Ok(Self {
            _settings: settings,
            _handler: handler,
        })
    }

    /// Get current theme using WinRT first, fallback to Registry
    pub fn get_current_theme() -> Theme {
        // Priority 1: WinRT UISettings
        if let Ok(settings) = UISettings::new() {
            if let Ok(color) = settings.GetColorValue(UIColorType::Background) {
                // Light mode background is typically white or very bright
                // R:255, G:255, B:255
                let brightness =
                    (color.R as f32 * 0.299 + color.G as f32 * 0.587 + color.B as f32 * 0.114)
                        / 255.0;
                return if brightness < 0.5 {
                    Theme::Dark
                } else {
                    Theme::Light
                };
            }
        }

        // Priority 2: Registry (Specific to "AppsUseLightTheme")
        // HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize
        if let Ok(val) = Self::get_apps_use_light_theme_registry() {
            return if val == 0 { Theme::Dark } else { Theme::Light };
        }

        // Default: Dark
        Theme::Dark
    }

    fn get_apps_use_light_theme_registry() -> windows::core::Result<u32> {
        unsafe {
            let subkey =
                encode_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
            let mut hkey = HKEY::default();

            let status = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                KEY_READ,
                &mut hkey,
            );

            if status.is_err() {
                return Err(windows::core::Error::from_win32());
            }

            let value_name = encode_wide("AppsUseLightTheme");

            let mut data = 0u32;
            let mut data_len = std::mem::size_of::<u32>() as u32;

            let status = RegQueryValueExW(
                hkey,
                PCWSTR(value_name.as_ptr()),
                None,
                None,
                Some(&mut data as *mut u32 as *mut u8),
                Some(&mut data_len),
            );

            use windows::Win32::System::Registry::RegCloseKey;
            let _ = RegCloseKey(hkey);

            if status.is_err() {
                return Err(windows::core::Error::from_win32());
            }

            Ok(data)
        }
    }
}

fn encode_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
