//! Register Anchor in ONE project (never global): .claude/settings.json hooks + .mcp.json.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;

const EVENTS: &[(&str, Option<&str>)] = &[
    ("SessionStart", None),
    ("UserPromptSubmit", None),
    ("PreToolUse", Some("*")),
    ("PostToolUse", Some("*")),
    ("Stop", None),
    ("PreCompact", None),
    ("SessionEnd", None),
];

fn read_json(p: &Path) -> Result<Value> {
    if p.exists() {
        let s = std::fs::read_to_string(p)?;
        if s.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&s).with_context(|| format!("parsing {}", p.display()))
    } else {
        Ok(json!({}))
    }
}

fn is_anchor(h: &Value) -> bool {
    h.get("hooks")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().any(|x| x.get("command").and_then(|c| c.as_str()).map(|c| c.contains(" hook ") && c.contains("anchor")).unwrap_or(false)))
        .unwrap_or(false)
}

pub fn install(project: &Path, exe: &Path) -> Result<Vec<String>> {
    let mut changed = Vec::new();
    let claude_dir = project.join(".claude");
    std::fs::create_dir_all(&claude_dir)?;
    let settings_path = claude_dir.join("settings.json");
    let mut settings = read_json(&settings_path)?;
    let exe_s = exe.display().to_string();
    {
        let hooks = settings.as_object_mut().unwrap().entry("hooks").or_insert(json!({}));
        for (ev, matcher) in EVENTS {
            let arr = hooks.as_object_mut().unwrap().entry(*ev).or_insert(json!([]));
            let a = arr.as_array_mut().unwrap();
            a.retain(|h| !is_anchor(h));
            let mut group = json!({"hooks": [{"type": "command", "command": format!("\"{exe_s}\" hook {ev}"), "timeout": 30}]});
            if let Some(m) = matcher {
                group["matcher"] = json!(m);
            }
            a.push(group);
        }
    }
    {
        let obj = settings.as_object_mut().unwrap();
        let en = obj.entry("enabledMcpjsonServers").or_insert(json!([]));
        if let Some(a) = en.as_array_mut()
            && !a.iter().any(|x| x == "anchor") {
                a.push(json!("anchor"));
            }
    }
    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
    changed.push(settings_path.display().to_string());

    let mcp_path = project.join(".mcp.json");
    let mut mcp = read_json(&mcp_path)?;
    mcp.as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert(json!({}))
        .as_object_mut()
        .unwrap()
        .insert("anchor".into(), json!({"command": exe_s, "args": ["mcp"]}));
    std::fs::write(&mcp_path, serde_json::to_string_pretty(&mcp)?)?;
    changed.push(mcp_path.display().to_string());
    Ok(changed)
}

pub fn uninstall(project: &Path) -> Result<()> {
    let settings_path = project.join(".claude").join("settings.json");
    if settings_path.exists() {
        let mut settings = read_json(&settings_path)?;
        if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            for (_, arr) in hooks.iter_mut() {
                if let Some(a) = arr.as_array_mut() {
                    a.retain(|h| !is_anchor(h));
                }
            }
            hooks.retain(|_, v| v.as_array().map(|a| !a.is_empty()).unwrap_or(true));
        }
        if let Some(a) = settings.get_mut("enabledMcpjsonServers").and_then(|v| v.as_array_mut()) {
            a.retain(|x| x != "anchor");
        }
        std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
    }
    let mcp_path = project.join(".mcp.json");
    if mcp_path.exists() {
        let mut mcp = read_json(&mcp_path)?;
        if let Some(s) = mcp.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
            s.remove("anchor");
        }
        std::fs::write(&mcp_path, serde_json::to_string_pretty(&mcp)?)?;
    }
    Ok(())
}

fn user_settings_path() -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(std::path::PathBuf::from(home).join(".claude").join("settings.json"))
}

pub fn add_hooks(settings: &mut Value, exe: &str) {
    let hooks = settings.as_object_mut().unwrap().entry("hooks").or_insert(json!({}));
    for (ev, matcher) in EVENTS {
        let arr = hooks.as_object_mut().unwrap().entry(*ev).or_insert(json!([]));
        let a = arr.as_array_mut().unwrap();
        a.retain(|h| !is_anchor(h));
        let mut group = json!({"hooks": [{"type": "command", "command": format!("\"{exe}\" hook {ev}"), "timeout": 30}]});
        if let Some(m) = matcher {
            group["matcher"] = json!(m);
        }
        a.push(group);
    }
}

/// Global install: hooks in ~/.claude/settings.json (backed up first) + user-scope MCP server.
/// Anchor stays inactive outside git repos, so this never creates stores elsewhere.
pub fn install_global(exe: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let path = user_settings_path()?;
    if path.exists() {
        let backup = path.with_file_name(format!("settings.json.anchor-backup-{}", crate::db::now()));
        std::fs::copy(&path, &backup)?;
        out.push(format!("backup {}", backup.display()));
    }
    let mut settings = read_json(&path)?;
    add_hooks(&mut settings, &exe.display().to_string());
    std::fs::write(&path, serde_json::to_string_pretty(&settings)?)?;
    out.push(format!("updated {}", path.display()));
    let have = std::process::Command::new("claude").args(["mcp", "get", "anchor"]).output();
    let exists = have.map(|o| o.status.success()).unwrap_or(false);
    if !exists {
        let st = std::process::Command::new("claude")
            .args(["mcp", "add", "--scope", "user", "anchor", "--"])
            .arg(exe)
            .arg("mcp")
            .output()
            .context("running `claude mcp add`")?;
        if !st.status.success() {
            anyhow::bail!("claude mcp add failed: {}", String::from_utf8_lossy(&st.stderr));
        }
        out.push("registered MCP server `anchor` (user scope)".into());
    } else {
        out.push("MCP server `anchor` already registered".into());
    }
    Ok(out)
}

pub fn uninstall_global() -> Result<Vec<String>> {
    let mut out = Vec::new();
    let path = user_settings_path()?;
    if path.exists() {
        let mut settings = read_json(&path)?;
        if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            for (_, arr) in hooks.iter_mut() {
                if let Some(a) = arr.as_array_mut() {
                    a.retain(|h| !is_anchor(h));
                }
            }
            hooks.retain(|_, v| v.as_array().map(|a| !a.is_empty()).unwrap_or(true));
        }
        std::fs::write(&path, serde_json::to_string_pretty(&settings)?)?;
        out.push(format!("removed hooks from {}", path.display()));
    }
    let _ = std::process::Command::new("claude").args(["mcp", "remove", "--scope", "user", "anchor"]).output();
    out.push("removed MCP server `anchor` (user scope)".into());
    Ok(out)
}
