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
            format!("{}m {}s", m, s)
        }
    } else if secs < 86400 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 {
            format!("{}h", h)
        } else {
            format!("{}h {}m", h, m)
        }
    } else {
        let d = secs / 86400;
        let h = (secs % 86400) / 3600;
        if h == 0 {
            format!("{}d", d)
        } else {
            format!("{}d {}h", d, h)
        }
    }
}

pub fn listening_ports_for_pids(target_pids: &[u32]) -> HashMap<u32, Vec<u16>> {
    let all_listeners = match listeners::get_all() {
        Ok(l) => l,
        Err(_) => return HashMap::new(),
    };

    // Build pid -> ports map from all TCP listeners
    let mut all_ports: HashMap<u32, Vec<u16>> = HashMap::new();
    for l in &all_listeners {
        if l.protocol == listeners::Protocol::TCP {
            let port = l.socket.port();
            if port != 0 {
                let ports = all_ports.entry(l.process.pid).or_default();
                if !ports.contains(&port) {
                    ports.push(port);
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
        #[cfg(target_os = "macos")]
        {
            use libproc::processes::{pids_by_type, ProcFilter};
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
        }

        if !ports.is_empty() {
            ports.sort();
            result.insert(pid, ports);
        }
    }
    result
}
