/// --- Netsh Implementation (for batch execution with PortProxy) ---

pub fn get_add_rule_cmd_netsh(rule_name: &str, port: u16, listen_addr: &str) -> String {
    let local_ip_param = if listen_addr == "0.0.0.0" || listen_addr.is_empty() {
        "localip=any"
    } else {
        &format!("localip={}", listen_addr)
    };

    format!(
        "netsh advfirewall firewall add rule name=\"{}\" dir=in action=allow protocol=TCP localport={} {}",
        rule_name, port, local_ip_param
    )
}

pub fn get_delete_rule_cmd_netsh(rule_name: &str) -> String {
    format!(
        "netsh advfirewall firewall delete rule name=\"{}\"",
        rule_name
    )
}
