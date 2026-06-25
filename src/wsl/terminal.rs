use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use std::sync::Mutex;

use crate::config::TerminalPreset;

// Cache for where.exe path resolution — results don't change at runtime
static EXE_RESOLVE_CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// Cache for validate_preset results
static VALIDATE_CACHE: LazyLock<Mutex<HashMap<String, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn invalidate_caches() {
    EXE_RESOLVE_CACHE.lock().unwrap().clear();
    VALIDATE_CACHE.lock().unwrap().clear();
}

/// Built-in terminal preset identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinTerminal {
    Cmd,
    PowerShell,
    Wt,
    Pwsh,
}

impl BuiltinTerminal {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cmd => "cmd",
            Self::PowerShell => "powershell",
            Self::Wt => "wt",
            Self::Pwsh => "pwsh",
        }
    }

    pub fn priority(&self) -> u32 {
        match self {
            Self::Cmd => 0,
            Self::PowerShell => 1,
            Self::Wt => 2,
            Self::Pwsh => 3,
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "cmd" => Some(Self::Cmd),
            "powershell" => Some(Self::PowerShell),
            "wt" => Some(Self::Wt),
            "pwsh" => Some(Self::Pwsh),
            _ => None,
        }
    }

    /// Returns true for built-in terminals that should appear in the dropdown
    /// even when their executable cannot be validated.
    pub fn is_always_visible(name: &str) -> bool {
        matches!(name, "cmd" | "powershell")
    }
}

impl std::fmt::Display for BuiltinTerminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Simple Windows-style command line argument splitter.
/// Mimics CommandLineToArgvW: splits on whitespace, respects double-quote grouping.
fn split_windows_args(cmd: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            in_quote = !in_quote;
            continue;
        }
        if c == ' ' || c == '\t' {
            if in_quote {
                current.push(c);
            } else if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(c);
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

const BUILTIN_PRESETS: &[(&str, &str, &str)] = &[
    (
        "cmd",
        "cmd.exe",
        " /c start \"WSL: {distro}\" cmd /c wsl -d {distro} --cd {dir}",
    ),
    (
        "powershell",
        "powershell.exe",
        "-NoExit -Command wsl -d {distro} --cd {dir}",
    ),
    ("wt", "wt.exe", "wsl -d {distro} --cd {dir}"),
    (
        "pwsh",
        "pwsh.exe",
        "-NoExit -Command wsl -d {distro} --cd {dir}",
    ),
    (
        "alacritty",
        "alacritty.exe",
        "-e wsl -d {distro} --cd {dir}",
    ),
    ("conemu", "ConEmu.exe", "-run wsl -d {distro} --cd {dir}"),
    ("rio", "rio.exe", "-e wsl -d {distro} --cd {dir}"),
    ("wezterm", "wezterm.exe", "-e wsl -d {distro} --cd {dir}"),
];

pub fn get_builtin_presets_map() -> HashMap<&'static str, TerminalPreset> {
    let mut m = HashMap::new();
    for (id, path, args) in BUILTIN_PRESETS {
        m.insert(
            *id,
            TerminalPreset {
                path: path.to_string(),
                args: args.to_string(),
            },
        );
    }
    m
}

pub fn resolve_presets(
    builtin: HashMap<&'static str, TerminalPreset>,
    terminal_presets: &HashMap<String, TerminalPreset>,
    terminal_user_presets: &HashMap<String, TerminalPreset>,
) -> HashMap<String, TerminalPreset> {
    let mut result: HashMap<String, TerminalPreset> = HashMap::new();
    // builtin first
    for (id, preset) in builtin {
        result.insert(id.to_string(), preset);
    }
    // terminal-presets: override builtin (case-insensitive match)
    for (name, preset) in terminal_presets {
        let key = result
            .keys()
            .find(|k| k.eq_ignore_ascii_case(name))
            .cloned();
        if let Some(k) = key {
            result.insert(k, preset.clone());
        }
    }
    // terminal-user-presets: insert/override (case-insensitive match)
    for (name, preset) in terminal_user_presets {
        let key = result
            .keys()
            .find(|k| k.eq_ignore_ascii_case(name))
            .cloned();
        if let Some(k) = key {
            result.insert(k, preset.clone());
        } else {
            result.insert(name.clone(), preset.clone());
        }
    }
    result
}

pub fn resolve_exe_path(path: &str) -> String {
    let key = path.to_string();

    // Check cache first
    {
        let cache = EXE_RESOLVE_CACHE.lock().unwrap();
        if let Some(cached) = cache.get(&key) {
            return cached.clone().unwrap_or_else(|| key);
        }
    }

    let result = resolve_exe_path_uncached(path);

    // Store in cache
    let mut cache = EXE_RESOLVE_CACHE.lock().unwrap();
    cache.insert(key, Some(result.clone()));
    result
}

fn resolve_exe_path_uncached(path: &str) -> String {
    if Path::new(path).is_absolute() || Path::new(path).exists() {
        return path.to_string();
    }

    {
        use std::os::windows::process::CommandExt;
        if let Ok(out) = std::process::Command::new("where.exe")
            .arg(path)
            .creation_flags(crate::utils::system::CREATE_NO_WINDOW)
            .output()
        {
            if out.status.success() {
                if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        return trimmed.to_string();
                    }
                }
            }
        }
    }

    let exe_name = Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    for dir in get_fallback_dirs(&exe_name) {
        let p = std::path::PathBuf::from(&dir).join(&exe_name);
        if p.exists() {
            return p.to_string_lossy().to_string();
        }
        if let Some(stem) = Path::new(&exe_name).file_stem().and_then(|s| s.to_str()) {
            let alt = format!("{}64.exe", stem);
            let p2 = std::path::PathBuf::from(&dir).join(&alt);
            if p2.exists() {
                return p2.to_string_lossy().to_string();
            }
        }
    }

    path.to_string()
}

/// Build a display string with all placeholders resolved.
/// Uses the same resolution logic as `build_command`.
pub fn format_terminal_command(
    preset: &TerminalPreset,
    distro_name: &str,
    working_dir: &str,
    terminal_proxy: bool,
    proxy_config: &crate::network::models::HttpProxyConfig,
) -> String {
    let resolved_args = replace_placeholders(&preset.args, &preset.path, distro_name, working_dir);

    let mut display = format!("{} {}", preset.path, resolved_args);

    // Append a readable proxy annotation for display purposes
    if terminal_proxy
        && proxy_config.is_enabled
        && !proxy_config.host.is_empty()
        && !proxy_config.port.is_empty()
    {
        let auth_hint = if proxy_config.auth_enabled
            && !proxy_config.username.is_empty()
            && !proxy_config.password.is_empty()
        {
            format!("{}:***@", proxy_config.username)
        } else {
            String::new()
        };
        display.push_str(&format!(
            "  [PROXY: {}{}:{}]",
            auth_hint, proxy_config.host, proxy_config.port
        ));
    }

    display
}

fn replace_placeholders(
    args: &str,
    exe_path: &str,
    distro_name: &str,
    working_dir: &str,
) -> String {
    args.replace("{distro}", distro_name)
        .replace("{dir}", working_dir)
        .replace("{exe}", exe_path)
        // {proxy} is kept for backward compat with user-defined presets;
        // actual proxy is now set via process env vars in build_command().
        .replace("{proxy}", "")
}

/// Apply proxy environment variables to a Command via `.env()`.
/// This works universally across all terminal emulators because the
/// child process inherits the environment, and WSL picks up WSLENV to
/// forward the vars into Linux.
fn apply_proxy_env(
    command: &mut std::process::Command,
    proxy_exports: Option<&[(String, String)]>,
) {
    let Some(exports) = proxy_exports else {
        return;
    };
    if exports.is_empty() {
        return;
    }
    let mut wslenv_parts: Vec<String> = Vec::new();
    for (k, v) in exports {
        command.env(k, v);
        wslenv_parts.push(format!("{}/u", k));
    }
    // Merge with any existing WSLENV from the parent environment
    let existing = std::env::var("WSLENV").unwrap_or_default();
    let wslenv = if existing.is_empty() {
        wslenv_parts.join(":")
    } else {
        format!("{}:{}", existing, wslenv_parts.join(":"))
    };
    command.env("WSLENV", wslenv);
}

pub fn build_command(
    preset: &TerminalPreset,
    distro_name: &str,
    working_dir: &str,
    proxy_exports: Option<&[(String, String)]>,
) -> std::process::Command {
    let cmd_str = replace_placeholders(&preset.args, &preset.path, distro_name, working_dir);

    let exe_path = resolve_exe_path(&preset.path);
    let mut command = std::process::Command::new(&exe_path);
    let args = split_windows_args(&cmd_str);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let exe_name = std::path::Path::new(&preset.path)
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if exe_name == "powershell.exe" || exe_name == "pwsh.exe" {
            command.creation_flags(crate::utils::system::CREATE_NEW_CONSOLE);
        } else {
            command.creation_flags(crate::utils::system::CREATE_NO_WINDOW);
            command.stdout(std::process::Stdio::null());
            command.stderr(std::process::Stdio::null());
        }
    }
    apply_proxy_env(&mut command, proxy_exports);
    command.args(&args);
    command
}

pub fn validate_preset(preset: &TerminalPreset) -> Result<(), String> {
    let key = preset.path.clone();

    // Check cache first
    {
        let cache = VALIDATE_CACHE.lock().unwrap();
        if let Some(&valid) = cache.get(&key) {
            return if valid {
                Ok(())
            } else {
                Err(format!("未找到 {}，请检查是否已安装", key))
            };
        }
    }

    let result = validate_preset_uncached(&key);

    // Store in cache
    let mut cache = VALIDATE_CACHE.lock().unwrap();
    cache.insert(key, result.is_ok());
    result
}

fn validate_preset_uncached(path: &str) -> Result<(), String> {
    // cmd.exe and powershell.exe are system components
    let exe_name = Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if exe_name == "cmd.exe" || exe_name == "powershell.exe" {
        return Ok(());
    }

    // Check file exists as entered
    if Path::new(path).exists() {
        return Ok(());
    }

    // Try where.exe
    {
        use std::os::windows::process::CommandExt;
        if let Ok(out) = std::process::Command::new("where.exe")
            .arg(path)
            .creation_flags(crate::utils::system::CREATE_NO_WINDOW)
            .output()
        {
            if out.status.success() {
                return Ok(());
            }
        }
    }

    // Try common install paths
    let fallback_dirs = get_fallback_dirs(&exe_name);
    for dir in fallback_dirs {
        let p = Path::new(&dir).join(&exe_name);
        if p.exists() {
            return Ok(());
        }
        // Also try x64 variant (e.g. ConEmu.exe -> ConEmu64.exe)
        if let Some(stem) = Path::new(&exe_name).file_stem().and_then(|s| s.to_str()) {
            let alt = format!("{}64.exe", stem);
            let p2 = Path::new(&dir).join(&alt);
            if p2.exists() {
                return Ok(());
            }
        }
    }

    Err(format!("未找到 {}，请检查是否已安装", path))
}

pub(crate) fn get_fallback_dirs(exe: &str) -> Vec<String> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let prog = std::env::var("ProgramFiles").unwrap_or_default();
    let prog_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
    let user = std::env::var("USERPROFILE").unwrap_or_default();
    match exe {
        "pwsh.exe" => vec![
            format!("{}\\Programs\\PowerShell\\7", user),
            format!("{}\\PowerShell\\7", prog),
            format!("{}\\PowerShell\\7", prog_x86),
            format!("{}\\Programs\\PowerShell\\6", user),
            format!("{}\\PowerShell\\6", prog),
            format!("{}\\PowerShell\\6", prog_x86),
        ],
        "wt.exe" => vec![format!("{}\\Microsoft\\WindowsApps", local)],
        "conemu.exe" | "conemu64.exe" => {
            vec![
                format!("{}\\Programs\\ConEmu", user),
                format!("{}\\Programs\\ConEmu", local),
                format!("{}\\ConEmu", prog),
                format!("{}\\ConEmu", prog_x86),
            ]
        }
        "alacritty.exe" => vec![
            format!("{}\\Programs\\Alacritty", user),
            format!("{}\\Programs\\Alacritty", local),
            format!("{}\\Alacritty", prog),
            format!("{}\\Alacritty", prog_x86),
        ],
        "wezterm.exe" => vec![
            format!("{}\\Programs\\WezTerm", user),
            format!("{}\\Programs\\WezTerm", local),
            format!("{}\\WezTerm", prog),
            format!("{}\\WezTerm", prog_x86),
        ],
        "rio.exe" => vec![
            format!("{}\\Programs\\Rio", user),
            format!("{}\\Programs\\Rio", local),
            format!("{}\\Rio", prog),
            format!("{}\\Rio", prog_x86),
        ],
        _ => vec![],
    }
}
