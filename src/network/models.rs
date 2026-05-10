use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub ip_address: String,
    pub is_loopback: bool,
    pub is_virtual: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortProxyRule {
    pub id: String,
    pub distro_name: String,
    pub listen_address: String,
    pub listen_port: u16,
    pub target_port: u16,
    pub enable_firewall: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HttpProxyConfig {
    #[serde(default)]
    pub is_enabled: bool,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: String,
    #[serde(default)]
    pub auth_enabled: bool,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub no_proxy: String,
}

pub fn default_no_proxy() -> String {
    "localhost,127.0.0.1,.example.com".to_string()
}

pub fn default_host() -> String {
    "e.g. 192.168.1.10".to_string()
}

pub fn default_port() -> String {
    "e.g. 10808".to_string()
}
