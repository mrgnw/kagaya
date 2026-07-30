use crate::types::ProcessDef;
use std::collections::HashMap;

/// Topological sort of processes respecting depends_on.
/// `to_start` lists which processes we actually want to start;
/// dependencies of those are pulled in automatically.
pub fn toposort_processes(defs: &[ProcessDef], to_start: &[&str]) -> Result<Vec<String>, String> {
    use std::collections::{HashSet, VecDeque};

    let by_name: HashMap<&str, &ProcessDef> = defs.iter().map(|d| (d.name.as_str(), d)).collect();

    // Collect all processes we need (requested + their transitive deps)
    let mut needed: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = to_start.iter().copied().collect();
    while let Some(name) = queue.pop_front() {
        if needed.insert(name) {
            if let Some(def) = by_name.get(name) {
                for dep in &def.depends_on {
                    queue.push_back(dep.as_str());
                }
            }
        }
    }

    // Kahn's algorithm for topological sort
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for &name in &needed {
        in_degree.entry(name).or_insert(0);
        if let Some(def) = by_name.get(name) {
            for dep in &def.depends_on {
                if needed.contains(dep.as_str()) {
                    *in_degree.entry(name).or_insert(0) += 1;
                }
            }
        }
    }

    // Build reverse adjacency: dep -> vec of dependents
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for &name in &needed {
        if let Some(def) = by_name.get(name) {
            for dep in &def.depends_on {
                if needed.contains(dep.as_str()) {
                    dependents.entry(dep.as_str()).or_default().push(name);
                }
            }
        }
    }

    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&name, _)| name)
        .collect();

    let mut order: Vec<String> = Vec::new();
    while let Some(name) = queue.pop_front() {
        order.push(name.to_string());
        if let Some(deps) = dependents.get(name) {
            for &dependent in deps {
                if let Some(deg) = in_degree.get_mut(dependent) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(dependent);
                    }
                }
            }
        }
    }

    if order.len() < needed.len() {
        let in_cycle: Vec<&str> = needed
            .iter()
            .filter(|&&n| !order.iter().any(|o| o == n))
            .copied()
            .collect();
        return Err(format!(
            "circular dependency detected among: {}",
            in_cycle.join(", ")
        ));
    }

    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ServiceType;

    fn simple_proc(name: &str, command: &str) -> ProcessDef {
        ProcessDef {
            name: name.to_string(),
            command: command.to_string(),
            service_type: ServiceType::Service,
            restart: false,
            env: HashMap::new(),
            autostart: true,
            ports: vec![],
            depends_on: vec![],
            ready: None,
            ready_timeout: 10,
        }
    }

    #[test]
    fn toposort_no_deps() {
        let procs = vec![
            simple_proc("a", "echo a"),
            simple_proc("b", "echo b"),
            simple_proc("c", "echo c"),
        ];
        let order = toposort_processes(&procs, &["a", "b", "c"]).unwrap();
        assert_eq!(order.len(), 3);
        assert!(order.contains(&"a".to_string()));
        assert!(order.contains(&"b".to_string()));
        assert!(order.contains(&"c".to_string()));
    }

    #[test]
    fn toposort_linear_chain() {
        let mut b = simple_proc("b", "echo b");
        b.depends_on = vec!["a".to_string()];
        let mut c = simple_proc("c", "echo c");
        c.depends_on = vec!["b".to_string()];
        let procs = vec![simple_proc("a", "echo a"), b, c];
        let order = toposort_processes(&procs, &["c"]).unwrap();
        assert_eq!(order.len(), 3);
        let pos_a = order.iter().position(|x| x == "a").unwrap();
        let pos_b = order.iter().position(|x| x == "b").unwrap();
        let pos_c = order.iter().position(|x| x == "c").unwrap();
        assert!(pos_a < pos_b);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn toposort_detects_cycle() {
        let mut a = simple_proc("a", "echo a");
        a.depends_on = vec!["b".to_string()];
        let mut b = simple_proc("b", "echo b");
        b.depends_on = vec!["a".to_string()];
        let procs = vec![a, b];
        let result = toposort_processes(&procs, &["a", "b"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("circular dependency"));
    }

    #[test]
    fn toposort_diamond_dependency() {
        let mut b = simple_proc("b", "echo b");
        b.depends_on = vec!["a".to_string()];
        let mut c = simple_proc("c", "echo c");
        c.depends_on = vec!["a".to_string()];
        let mut d = simple_proc("d", "echo d");
        d.depends_on = vec!["b".to_string(), "c".to_string()];
        let procs = vec![simple_proc("a", "echo a"), b, c, d];
        let order = toposort_processes(&procs, &["d"]).unwrap();
        assert_eq!(order.len(), 4);
        let pos_a = order.iter().position(|x| x == "a").unwrap();
        let pos_b = order.iter().position(|x| x == "b").unwrap();
        let pos_c = order.iter().position(|x| x == "c").unwrap();
        let pos_d = order.iter().position(|x| x == "d").unwrap();
        assert!(pos_a < pos_b);
        assert!(pos_a < pos_c);
        assert!(pos_b < pos_d);
        assert!(pos_c < pos_d);
    }
}
