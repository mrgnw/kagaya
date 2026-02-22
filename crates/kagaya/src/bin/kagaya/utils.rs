use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn duration_since_timestamp(ts: u64) -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .saturating_sub(ts)
}

pub fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 {
            format!("{}m", m)
        } else {
            format!("{}m{}s", m, s)
        }
    } else if secs < 86400 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 {
            format!("{}h", h)
        } else {
            format!("{}h{}m", h, m)
        }
    } else {
        let d = secs / 86400;
        let h = (secs % 86400) / 3600;
        if h == 0 {
            format!("{}d", d)
        } else {
            format!("{}d{}h", d, h)
        }
    }
}

#[cfg(target_os = "macos")]
pub fn listening_ports_for_pids(target_pids: &[u32]) -> HashMap<u32, Vec<u16>> {
    use libproc::processes::{pids_by_type, ProcFilter};
    use netstat2::*;

    let af = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let proto = ProtocolFlags::TCP;
    let sockets = match get_sockets_info(af, proto) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };

    let mut all_ports: HashMap<u32, Vec<u16>> = HashMap::new();
    for si in &sockets {
        if let ProtocolSocketInfo::Tcp(ref tcp) = si.protocol_socket_info {
            if tcp.state == TcpState::Listen {
                for pid in &si.associated_pids {
                    let ports = all_ports.entry(*pid).or_default();
                    if !ports.contains(&tcp.local_port) {
                        ports.push(tcp.local_port);
                    }
                }
            }
        }
    }

    let mut result: HashMap<u32, Vec<u16>> = HashMap::new();
    for &pid in target_pids {
        let mut ports: Vec<u16> = Vec::new();

        // Check the pid itself
        if let Some(p) = all_ports.get(&pid) {
            ports.extend(p);
        }

        // Walk all descendants recursively by parent PID
        let mut stack = vec![pid];
        while let Some(parent) = stack.pop() {
            let children =
                pids_by_type(ProcFilter::ByParentProcess { ppid: parent }).unwrap_or_default();
            for child in children {
                if child != 0 && child != pid {
                    if let Some(p) = all_ports.get(&child) {
                        for port in p {
                            if !ports.contains(port) {
                                ports.push(*port);
                            }
                        }
                    }
                    stack.push(child);
                }
            }
        }

        // Also check process group as fallback
        let group_pids =
            pids_by_type(ProcFilter::ByProgramGroup { pgrpid: pid }).unwrap_or_default();
        for gpid in &group_pids {
            if let Some(p) = all_ports.get(gpid) {
                for port in p {
                    if !ports.contains(port) {
                        ports.push(*port);
                    }
                }
            }
        }

        if !ports.is_empty() {
            ports.sort();
            result.insert(pid, ports);
        }
    }
    result
}

#[cfg(not(target_os = "macos"))]
pub fn listening_ports_for_pids(_target_pids: &[u32]) -> HashMap<u32, Vec<u16>> {
    HashMap::new()
}
