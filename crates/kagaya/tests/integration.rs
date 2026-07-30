use kagaya::types::*;

// --- Types ---

#[test]
fn process_state_is_running() {
    assert!(ProcessState::Running {
        pid: 1,
        uptime_secs: 0
    }
    .is_running());
    assert!(!ProcessState::Stopped.is_running());
    assert!(!ProcessState::Crashed {
        exit_code: 1,
        retries: 0
    }
    .is_running());
    assert!(!ProcessState::Failed { exit_code: 1 }.is_running());
}

#[test]
fn service_status_is_running() {
    let s = ServiceStatus {
        name: "test".into(),
        dir: "/tmp".into(),
        processes: vec![ProcessStatus {
            name: "web".into(),
            state: ProcessState::Running {
                pid: 1,
                uptime_secs: 5,
            },
            pid: Some(1),
            autostart: true,
            service_type: ServiceType::Service,
            ports: vec![],
            ports_expected: vec![],
            state_since: None,
            cpu_percent: None,
            memory_bytes: None,
        }],
    };
    assert!(s.is_running());

    let s2 = ServiceStatus {
        name: "test".into(),
        dir: "/tmp".into(),
        processes: vec![ProcessStatus {
            name: "web".into(),
            state: ProcessState::Stopped,
            pid: None,
            autostart: true,
            service_type: ServiceType::Service,
            ports: vec![],
            ports_expected: vec![],
            state_since: None,
            cpu_percent: None,
            memory_bytes: None,
        }],
    };
    assert!(!s2.is_running());
}
