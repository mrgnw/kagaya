use std::path::Path;

pub struct SuggestedService {
    pub name: String,
    pub command: String,
}

/// Detect project type from directory contents and suggest services.toml entries.
pub fn detect_services(dir: &Path) -> Vec<SuggestedService> {
    let mut suggestions = Vec::new();

    // Procfile (highest priority — explicit intent)
    if let Some(entries) = detect_procfile(dir) {
        return entries;
    }

    // Node/Svelte/JS project
    if let Some(entries) = detect_node(dir) {
        suggestions.extend(entries);
    }

    // Python project
    if let Some(entries) = detect_python(dir) {
        suggestions.extend(entries);
    }

    // Rust project
    if let Some(entries) = detect_rust(dir) {
        suggestions.extend(entries);
    }

    suggestions
}

fn detect_procfile(dir: &Path) -> Option<Vec<SuggestedService>> {
    let path = dir.join("Procfile");
    let content = std::fs::read_to_string(path).ok()?;
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((name, cmd)) = line.split_once(':') {
            entries.push(SuggestedService {
                name: name.trim().to_string(),
                command: cmd.trim().to_string(),
            });
        }
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

fn detect_node(dir: &Path) -> Option<Vec<SuggestedService>> {
    let pkg_path = dir.join("package.json");
    let content = std::fs::read_to_string(pkg_path).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;
    let scripts = pkg.get("scripts")?.as_object()?;

    // Determine package manager
    let pm = if dir.join("pnpm-lock.yaml").exists() || dir.join("pnpm-workspace.yaml").exists() {
        "pnpm"
    } else if dir.join("bun.lock").exists() || dir.join("bun.lockb").exists() {
        "bun"
    } else if dir.join("yarn.lock").exists() {
        "yarn"
    } else {
        "npm"
    };

    let mut entries = Vec::new();

    if scripts.contains_key("dev") {
        entries.push(SuggestedService {
            name: "dev".to_string(),
            command: format!("{pm} run dev"),
        });
    }

    if entries.is_empty() {
        // Fallback: check for start script
        if scripts.contains_key("start") {
            entries.push(SuggestedService {
                name: "web".to_string(),
                command: format!("{pm} run start"),
            });
        }
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

fn detect_python(dir: &Path) -> Option<Vec<SuggestedService>> {
    let has_pyproject = dir.join("pyproject.toml").exists();
    let has_uv_lock = dir.join("uv.lock").exists();
    if !has_pyproject && !has_uv_lock {
        return None;
    }

    let mut entries = Vec::new();

    // Check pyproject.toml for hints
    if let Ok(content) = std::fs::read_to_string(dir.join("pyproject.toml")) {
        // FastAPI / uvicorn
        if content.contains("fastapi") || content.contains("uvicorn") {
            // Look for a main.py or app.py
            let module = if dir.join("main.py").exists() {
                "main:app"
            } else if dir.join("app.py").exists() {
                "app:app"
            } else {
                "main:app"
            };
            let prefix = if has_uv_lock { "uv run " } else { "" };
            entries.push(SuggestedService {
                name: "api".to_string(),
                command: format!("{prefix}uvicorn {module} --reload"),
            });
            return Some(entries);
        }

        // Check [project.scripts] for entry points
        if let Ok(parsed) = content.parse::<toml::Value>() {
            if let Some(scripts) = parsed
                .get("project")
                .and_then(|p| p.get("scripts"))
                .and_then(|s| s.as_table())
            {
                // If there's a single entry point, suggest it
                if scripts.len() == 1 {
                    let (name, _) = scripts.iter().next().unwrap();
                    let prefix = if has_uv_lock { "uv run " } else { "" };
                    entries.push(SuggestedService {
                        name: name.clone(),
                        command: format!("{prefix}{name}"),
                    });
                    return Some(entries);
                }
            }
        }
    }

    // Generic: if there's a main.py
    if dir.join("main.py").exists() {
        let prefix = if has_uv_lock { "uv run " } else { "" };
        entries.push(SuggestedService {
            name: "app".to_string(),
            command: format!("{prefix}python main.py"),
        });
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

fn detect_rust(dir: &Path) -> Option<Vec<SuggestedService>> {
    let cargo_path = dir.join("Cargo.toml");
    let content = std::fs::read_to_string(cargo_path).ok()?;
    let parsed: toml::Value = content.parse().ok()?;

    // Only suggest for binary crates, not libraries
    let has_bin = parsed.get("bin").is_some()
        || parsed
            .get("package")
            .and_then(|p| p.get("default-run"))
            .is_some()
        || dir.join("src/main.rs").exists();

    if !has_bin {
        return None;
    }

    let name = parsed
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("app");

    Some(vec![SuggestedService {
        name: name.to_string(),
        command: "cargo run".to_string(),
    }])
}

/// Format suggestions as services.toml content
pub fn format_services_toml(suggestions: &[SuggestedService]) -> String {
    let mut out = String::new();
    for s in suggestions {
        out.push_str(&format!("{} = {:?}\n", s.name, s.command));
    }
    out
}

/// Describe what was detected (for the prompt message)
pub fn describe_detected(dir: &Path) -> Vec<&'static str> {
    let mut found = Vec::new();
    if dir.join("Procfile").exists() {
        found.push("Procfile");
    }
    if dir.join("package.json").exists() {
        found.push("package.json");
        if dir.join("pnpm-lock.yaml").exists() {
            found.push("pnpm-lock.yaml");
        }
        if dir.join("bun.lock").exists() || dir.join("bun.lockb").exists() {
            found.push("bun.lock");
        }
    }
    if dir.join("pyproject.toml").exists() {
        found.push("pyproject.toml");
    }
    if dir.join("uv.lock").exists() {
        found.push("uv.lock");
    }
    if dir.join("Cargo.toml").exists() {
        found.push("Cargo.toml");
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_detect_node_pnpm() {
        let dir = tmpdir();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "vite dev"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        let suggestions = detect_services(dir.path());
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].name, "dev");
        assert_eq!(suggestions[0].command, "pnpm run dev");
    }

    #[test]
    fn test_detect_node_bun() {
        let dir = tmpdir();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "vite dev"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("bun.lock"), "").unwrap();
        let suggestions = detect_services(dir.path());
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].command, "bun run dev");
    }

    #[test]
    fn test_detect_python_fastapi() {
        let dir = tmpdir();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\ndependencies = [\"fastapi\"]\n",
        )
        .unwrap();
        fs::write(dir.path().join("uv.lock"), "").unwrap();
        fs::write(dir.path().join("main.py"), "").unwrap();
        let suggestions = detect_services(dir.path());
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].name, "api");
        assert_eq!(suggestions[0].command, "uv run uvicorn main:app --reload");
    }

    #[test]
    fn test_detect_rust_binary() {
        let dir = tmpdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        let suggestions = detect_services(dir.path());
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].name, "myapp");
        assert_eq!(suggestions[0].command, "cargo run");
    }

    #[test]
    fn test_detect_procfile_wins() {
        let dir = tmpdir();
        // Both Procfile and package.json exist
        fs::write(
            dir.path().join("Procfile"),
            "web: ./run.sh\nworker: ./worker.sh\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "vite dev"}}"#,
        )
        .unwrap();
        let suggestions = detect_services(dir.path());
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].name, "web");
        assert_eq!(suggestions[0].command, "./run.sh");
    }

    #[test]
    fn test_detect_empty_dir() {
        let dir = tmpdir();
        let suggestions = detect_services(dir.path());
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_detect_node_start_fallback() {
        let dir = tmpdir();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"start": "node server.js", "test": "jest"}}"#,
        )
        .unwrap();
        let suggestions = detect_services(dir.path());
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].name, "web");
        assert_eq!(suggestions[0].command, "npm run start");
    }

    #[test]
    fn test_detect_rust_lib_only() {
        let dir = tmpdir();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn hello() {}").unwrap();
        // No src/main.rs → lib only
        let suggestions = detect_services(dir.path());
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_format_services_toml() {
        let suggestions = vec![
            SuggestedService {
                name: "dev".into(),
                command: "pnpm run dev".into(),
            },
            SuggestedService {
                name: "api".into(),
                command: "uv run uvicorn main:app".into(),
            },
        ];
        let toml = format_services_toml(&suggestions);
        assert!(toml.contains("dev = \"pnpm run dev\""));
        assert!(toml.contains("api = \"uv run uvicorn main:app\""));
    }
}
