use std::collections::HashMap;
use std::path::Path;

use crate::config::TerminalPreset;

/// Built-in terminal preset identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinTerminal {
    Cmd,
    PowerShell,
    Wt,
    Pwsh,
}

impl BuiltinTerminal {
    #[allow(dead_code)]
    pub const ALL: [Self; 4] = [Self::Cmd, Self::PowerShell, Self::Wt, Self::Pwsh];

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
        " /c start \"WSL: {distro}\" cmd /c {proxy}wsl -d {distro} --cd {dir}",
    ),
    (
        "powershell",
        "powershell.exe",
        "-NoExit -Command {proxy}wsl -d {distro} --cd {dir}",
    ),
    ("wt", "wt.exe", "wsl -d {proxy}{distro} --cd {dir}"),
    (
        "pwsh",
        "pwsh.exe",
        "-NoExit -Command {proxy}wsl -d {distro} --cd {dir}",
    ),
    (
        "alacritty",
        "alacritty.exe",
        "-e {proxy}wsl -d {distro} --cd {dir}",
    ),
    (
        "conemu",
        "ConEmu.exe",
        "-run {proxy}wsl -d {distro} --cd {dir}",
    ),
    ("rio", "rio.exe", "-e {proxy}wsl -d {distro} --cd {dir}"),
    (
        "wezterm",
        "wezterm.exe",
        "-e {proxy}wsl -d {distro} --cd {dir}",
    ),
];

#[allow(dead_code)]
pub fn get_builtin_preset_names() -> Vec<&'static str> {
    BuiltinTerminal::ALL.iter().map(|t| t.as_str()).collect()
}

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

#[allow(dead_code)]
pub fn is_builtin(name: &str) -> bool {
    BuiltinTerminal::from_str(name).is_some()
        || BUILTIN_PRESETS.iter().any(|(id, _, _)| *id == name)
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

pub fn build_proxy_prefix(proxy_exports: Option<&[(String, String)]>) -> String {
    let Some(exports) = proxy_exports else {
        return String::new();
    };
    if exports.is_empty() {
        return String::new();
    }
    let mut prefix = String::new();
    let mut wslenv = String::new();
    for (k, v) in exports {
        prefix.push_str(&format!("set \"{}={}\"& ", k, v));
        wslenv.push_str(&format!("{}/u:", k));
    }
    if !wslenv.is_empty() {
        wslenv.pop();
        prefix.push_str(&format!("set \"WSLENV={}\"& ", wslenv));
    }
    prefix
}

pub fn resolve_exe_path(path: &str) -> String {
    if Path::new(path).is_absolute() || Path::new(path).exists() {
        return path.to_string();
    }

    if let Ok(out) = std::process::Command::new("where.exe").arg(path).output() {
        if out.status.success() {
            if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
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
    let mut exports: Vec<(String, String)> = Vec::new();
    if terminal_proxy
        && proxy_config.is_enabled
        && !proxy_config.host.is_empty()
        && !proxy_config.port.is_empty()
    {
        let auth = if proxy_config.auth_enabled
            && !proxy_config.username.is_empty()
            && !proxy_config.password.is_empty()
        {
            format!("{}:{}@", proxy_config.username, proxy_config.password)
        } else {
            String::new()
        };
        let proxy_url = format!("http://{}{}:{}", auth, proxy_config.host, proxy_config.port);
        exports.push(("HTTP_PROXY".to_string(), proxy_url.clone()));
        exports.push(("HTTPS_PROXY".to_string(), proxy_url.clone()));
        if !proxy_config.no_proxy.is_empty() {
            exports.push(("NO_PROXY".to_string(), proxy_config.no_proxy.clone()));
        }
    }
    let proxy_exports = if exports.is_empty() {
        None
    } else {
        Some(exports)
    };
    let proxy_prefix = build_proxy_prefix(proxy_exports.as_deref());

    let resolved_args = replace_placeholders(
        &preset.args,
        &preset.path,
        distro_name,
        working_dir,
        &proxy_prefix,
    );
    format!("{} {}", preset.path, resolved_args)
}

fn replace_placeholders(
    args: &str,
    exe_path: &str,
    distro_name: &str,
    working_dir: &str,
    proxy_prefix: &str,
) -> String {
    args.replace("{distro}", distro_name)
        .replace("{dir}", working_dir)
        .replace("{exe}", exe_path)
        .replace("{proxy}", proxy_prefix)
}

pub fn build_command(
    preset: &TerminalPreset,
    distro_name: &str,
    working_dir: &str,
    proxy_prefix: &str,
) -> std::process::Command {
    let cmd_str = replace_placeholders(
        &preset.args,
        &preset.path,
        distro_name,
        working_dir,
        proxy_prefix,
    );

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
    command.args(&args);
    command
}

pub fn validate_preset(preset: &TerminalPreset) -> Result<(), String> {
    let path = &preset.path;
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
    if let Ok(out) = std::process::Command::new("where.exe").arg(path).output() {
        if out.status.success() {
            return Ok(());
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
